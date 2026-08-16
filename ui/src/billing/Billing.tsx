/**
 * The billing screen — **the heart of the product** (audit 2.3).
 *
 * Four regions that never move (UI_GUIDELINES §4): a top bar with the order
 * type and search, the table grid, and a **permanent** cart with the totals,
 * the payment panel and the actions under it.
 *
 * # This session is the screen; P10 is the keyboard
 *
 * Every action below is a **named function**, not a closure buried in an
 * `onClick`, so P10 binds keys to the same things a mouse presses rather than
 * re-implementing them. Crown jewel 1 — *"the billing keyboard flow… is why
 * your counter is fast"* — gets a session of its own.
 *
 * # There is no cart in this file
 *
 * The cart lives in Rust (`src-tauri/src/billing.rs`). Every change is a
 * command that returns the whole new `CartView`, and this renders it. There is
 * no money in TypeScript to do arithmetic on, which is R8 made structural
 * rather than remembered.
 */

import { useCallback, useEffect, useReducer, useRef, useState } from 'react';

import {
  Badge,
  Button,
  ConfirmDialog,
  EmptyState,
  Icon,
  Page,
  SearchField,
  SectionHeader,
  Spinner,
  useToast,
} from '../kit';
import { call, inApp, isUiError, subscribe } from '../ipc/call';
import type { CartView } from '../ipc/generated/CartView';
import type { MenuItemView } from '../ipc/generated/MenuItemView';
import type { TableView } from '../ipc/generated/TableView';
import { useTick } from '../clock';
import { mark } from '../perf';
import { BusyTable, HelpSheet, QuantityPopup, Suggestions, takenLetters } from './Keys';
import {
  initial as initialKeys,
  reduce as reduceKeys,
  type Command as KeyCommand,
  type Event as KeyEvent,
  type State as KeyboardState,
} from './keyboard';

/** The reducer's state, plus the commands it last asked for. */
type KeyState = KeyboardState & { outbox: KeyCommand[]; seq: number };
import { PutOnAccount } from '../credit/Credit';
import { TableGrid } from './TableGrid';
import { Totals } from './Totals';

import './billing.css';

const ORDER_TYPES = ['Dine in', 'Parcel', 'Self service', 'Delivery'] as const;

export function Billing() {
  const toast = useToast();
  const [cart, setCart] = useState<CartView | null>(null);
  const [tables, setTables] = useState<readonly TableView[]>([]);
  const [menu, setMenu] = useState<readonly MenuItemView[]>([]);
  // The grid is unfiltered: the search box is for the menu, and a table is
  // reached by typing its number and pressing Enter (audit F5, one keystroke
  // shorter than filtering). P14 may add a filter of its own for the floor
  // plan; this session deliberately does not.
  const filter = '';
  const [locked, setLocked] = useState(false);
  const [confirmCancel, setConfirmCancel] = useState(false);
  /// P15 — the customer picker for a bill going on an account.
  const [onAccount, setOnAccount] = useState(false);
  const [busy, setBusy] = useState(false);

  // ONE shared clock (§5 rule 10). The tiles do not each own a timer; they
  // re-read the elapsed minutes the order already carries when this ticks.
  const tick = useTick();

  // **The keyboard.** The reducer decides; this component performs. Every
  // command it returns is one of the named functions below — which P09 wrote
  // that way on purpose so this session binds keys rather than re-implementing
  // behaviour.
  //
  const searchBox = useRef<HTMLInputElement>(null);
  /**
   * **P29 — whether this shop has a scale and a label printer.**
   *
   * Asked once, when the screen opens. A button for hardware a shop does not
   * own is worse than no button: it is a promise that fails when pressed.
   */
  const [hasScale, setHasScale] = useState(false);
  const [hasLabels, setHasLabels] = useState(false);

  // **The reducer is PURE, and the commands ride in the state.**
  //
  // The first version pushed them into a ref from inside the reducer, and that
  // is a side effect — which React's StrictMode double-invokes reducers
  // specifically to catch. It caught it: every command ran twice, so a blank
  // quantity added TWO of everything and `Cart::add` dutifully merged them
  // into a line of quantity 2. One beer came out as 440.00.
  //
  // Carrying the commands in the state makes the reducer a function of its
  // inputs again: invoking it twice produces the same state twice, and the
  // effect below performs each batch exactly once, keyed on `seq`.
  const [keys, dispatch] = useReducer(
    (state: KeyState, event: KeyEvent): KeyState => {
      const [next, commands] = reduceKeys(state, event);
      return { ...next, outbox: commands, seq: state.seq + 1 };
    },
    { ...initialKeys(), outbox: [] as KeyCommand[], seq: 0 },
  );

  /**
   * **P29, scope 7.6 — the scanner, which is a keyboard.**
   *
   * A scanner types the code into the search box and presses Enter, so the box
   * gets exactly what a fast cashier typing "dosa" gets. The only thing that
   * tells them apart is the TIMING, so the timing is collected here — the
   * characters and the gap before each one — and the decision is made in Rust
   * by a pure function with its own tests (R8, and `mb_core::devices`).
   *
   * **The dangerous mistake is the other one.** Missing a scan costs a
   * re-scan; reading a fast typist as a scan throws away what they typed. So
   * when Rust says "typing", this does nothing at all and Enter behaves
   * exactly as it did before P29.
   */
  const strokes = useRef<{ text: string; gaps: number[]; at: number }>({
    text: '',
    gaps: [],
    at: 0,
  });

  const noteKeystroke = useCallback((text: string) => {
    const now = Date.now();
    const before = strokes.current;
    const grewByOne = text.length === before.text.length + 1 && text.startsWith(before.text);
    if (grewByOne && before.text !== '') {
      strokes.current = {
        text,
        gaps: [...before.gaps, now - before.at],
        at: now,
      };
      return;
    }
    // Anything else — a paste, a backspace, a fresh box — starts again. A
    // half-remembered burst is worse than no burst.
    strokes.current = { text, gaps: [], at: now };
  }, []);

  const report = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) {
        toast.show('danger', cause.message, cause.detail ?? undefined);
      } else {
        toast.show('danger', String(cause));
      }
    },
    [toast],
  );

  // **Silent on failure, and deliberately.** This runs on every tick of the
  // shared clock, so a shop that is not open yet would otherwise raise a toast
  // every fifteen seconds for ever. The empty state below already says what is
  // wrong, and it says it once, in the place the eye is already looking.
  const refreshFloor = useCallback(async () => {
    if (!inApp()) return;
    try {
      setTables(await call('open_orders'));
    } catch {
      // A floor that will not load is visible as an empty floor.
      setTables([]);
    }
  }, []);

  useEffect(() => {
    if (!inApp()) return;
    call('current_cart').then(setCart).catch(report);
    call('menu_items').then(setMenu).catch(report);
    // Silent on failure: a cashier who may not open the device screen still
    // bills, and the two buttons below simply do not appear.
    call('device_manager')
      .then((devices) => {
        setHasScale(devices.devices.some((d) => d.kind === 'scale' && d.setUp));
        setHasLabels(devices.devices.some((d) => d.kind === 'label' && d.setUp));
      })
      .catch(() => {
        setHasScale(false);
        setHasLabels(false);
      });
  }, [report]);

  // **The floor changed the order this cart has open** (P20, D83).
  //
  // Pushed, and subscribed once — the empty dependency list is deliberate, the
  // same lesson P19's panel learned: a listener that depends on anything that
  // changes identity is a listener that is torn down and re-attached, and the
  // push lands in the gap. Without this the cashier finds out when they next
  // press something, and the thing they are most likely to press is Complete
  // bill.
  useEffect(() => {
    if (!inApp()) return undefined;
    let stop: (() => void) | undefined;
    subscribe((message) => {
      if (message.kind === 'floorChanged') {
        call('current_cart')
          .then(setCart)
          .catch(() => undefined);
      }
    })
      .then((off) => {
        stop = off;
      })
      .catch(() => undefined);
    return () => stop?.();
  }, []);

  // The floor re-reads on every tick, which is how a timer that lives on the
  // ORDER reaches the screen without the screen counting anything itself.
  useEffect(() => {
    void refreshFloor();
  }, [refreshFloor, tick]);

  // --- the actions. Named, so P10 can bind keys to them. -------------------

  const addItem = useCallback(
    async (itemId: string, qty: string | null = null) => {
      try {
        setCart(await call('cart_add', { itemId, qty, note: null }));
      } catch (cause) {
        report(cause);
      }
    },
    [report],
  );

  /** Budget B7 — an existing table's order, into the cart. */
  const openTableById = useCallback(
    async (tableId: string) => {
      try {
        setCart(await call('open_table', { tableId }));
        await refreshFloor();
      } catch (cause) {
        report(cause);
      }
    },
    [refreshFloor, report],
  );

  const removeLine = useCallback(
    async (index: number) => {
      try {
        setCart(await call('cart_remove', { index }));
      } catch (cause) {
        report(cause);
      }
    },
    [report],
  );

  const newOrder = useCallback(async () => {
    try {
      // The order-type LOCK (crown jewel 1): a parcel counter should not be
      // re-selecting the type forty times an hour.
      setCart(await call('cart_clear', { keepType: locked }));
      await refreshFloor();
    } catch (cause) {
      report(cause);
    }
  }, [locked, refreshFloor, report]);

  /**
   * Enter, with a burst of characters in the box: ask Rust whether that was a
   * machine. Returns true when it handled it, so the ordinary Enter does not
   * also run.
   */
  const handledAsScan = useCallback(async (): Promise<boolean> => {
    const burst = strokes.current;
    if (burst.text.trim() === '') return false;
    try {
      const outcome = await call('scanned', { text: burst.text, gapsMs: burst.gaps });
      if (outcome.what === 'typing') return false;

      strokes.current = { text: '', gaps: [], at: 0 };
      dispatch({ kind: 'typed', text: '' });

      if (outcome.what === 'item' || outcome.what === 'weighed') {
        await addItem(outcome.itemId, outcome.qty);
        if (outcome.says) toast.show('ok', outcome.says);
        return true;
      }
      if (outcome.what === 'bill') {
        // A printed bill, scanned back onto the screen. The Bills screen is
        // where a settled bill is worked on, so this says what it found
        // rather than pretending the billing screen can reopen it.
        toast.show('info', `${outcome.says} — open it under Bills.`);
        return true;
      }
      // Unknown: the code is real, nothing on this counter has it. Offering
      // to attach it to an item is the Menu screen's job (D102), so this
      // sends them there rather than growing a second editor here.
      toast.show(
        'warn',
        outcome.says,
        'Add it as the item’s code on the Menu screen, then scan again.',
      );
      return true;
    } catch (cause) {
      // **A scanner that cannot be asked about must not eat a keystroke.**
      report(cause);
      return false;
    }
  }, [addItem, report, toast]);

  const setOrderType = useCallback(
    async (orderType: string) => {
      try {
        setCart(await call('cart_set_order_type', { orderType }));
      } catch (cause) {
        report(cause);
      }
    },
    [report],
  );

  const takePayment = useCallback(
    async (mode: string) => {
      if (!cart) return;
      try {
        // The amount is the balance Rust computed. TypeScript passes it back;
        // it does not work it out.
        setCart(
          await call('cart_add_payment', {
            mode,
            // The wire carries a JSON number; MoneyView types the field as the
            // i64 it is in Rust. `Number` reconciles the two and computes
            // nothing (R8).
            amountPaise: Number(cart.balance.paise),
          }),
        );
      } catch (cause) {
        report(cause);
      }
    },
    [cart, report],
  );

  /** The delta only — never the whole order (crown jewel 2). */
  const printKitchen = useCallback(async () => {
    try {
      await call('print_kitchen_ticket');
      setCart(await call('current_cart'));
      toast.show('ok', 'Kitchen ticket sent.');
    } catch (cause) {
      report(cause);
    }
  }, [report, toast]);

  /**
   * Settle, then print. **In that order** — the money is on disk before the
   * paper is attempted, so a printer that is off cannot lose a bill (D4).
   */
  const completeBill = useCallback(async () => {
    try {
      const number = await call('complete_bill');
      setCart(await call('current_cart'));
      await refreshFloor();
      toast.show('ok', `Bill ${number} settled.`);
    } catch (cause) {
      report(cause);
    }
  }, [refreshFloor, report, toast]);

  const clearPayments = useCallback(async () => {
    try {
      setCart(await call('cart_clear_payments'));
    } catch (cause) {
      report(cause);
    }
  }, [report]);

  // A tap goes through the SAME reducer a key does, so touch and keyboard
  // cannot drift apart (scope 1.28, and test T10).
  const openTable = useCallback(
    (table: TableView) => {
      const index = tables.findIndex((t) => t.id === table.id);
      if (index >= 0) dispatch({ kind: 'tap-tile', index });
    },
    [tables],
  );

  const seedDemo = useCallback(async () => {
    setBusy(true);
    try {
      await call('seed_demo_shop');
      setMenu(await call('menu_items'));
      await refreshFloor();
      toast.show('ok', 'A demo shop is in place.');
    } catch (cause) {
      report(cause);
    } finally {
      setBusy(false);
    }
  }, [refreshFloor, report, toast]);

  // --- performing what the reducer asked for -----------------------------
  //
  // One place, so a command is a name rather than a closure, and so P11's
  // permissions have exactly one gate to sit in front of later.
  const perform = useCallback(
    async (command: KeyCommand) => {
      switch (command.do) {
        case 'search': {
          if (command.text.trim() === '') {
            dispatch({ kind: 'suggestions', items: [] });
            return;
          }
          try {
            const items = await call('search_items', { text: command.text, mode: null });
            dispatch({ kind: 'suggestions', items });
          } catch {
            dispatch({ kind: 'suggestions', items: [] });
          }
          return;
        }
        case 'add-item':
          await addItem(command.itemId, command.qty);
          return;
        case 'open-table':
          await openTableById(command.tableId);
          return;
        case 'set-order-type':
          await setOrderType(command.value);
          return;
        case 'new-order':
          await newOrder();
          return;
        case 'confirm-new-order':
          setConfirmCancel(true);
          return;
        case 'focus-search':
          searchBox.current?.focus();
          return;
        case 'print-kitchen':
          await printKitchen();
          return;
        case 'complete-bill':
          await completeBill();
          return;
        case 'merge-into':
          await openTableById(command.tableId);
          toast.show('info', 'Merged. Only the new items will go to the kitchen.');
          return;
        case 'sub-table':
          toast.show('info', `Sub-table ${command.letter} needs the settle flow — still to come.`);
          return;
      }
    },
    [addItem, completeBill, newOrder, openTableById, printKitchen, setOrderType, toast],
  );

  // Perform one batch per committed dispatch. Keyed on `seq` rather than on
  // the array, so a batch that happens to be identical to the last one still
  // runs — pressing Enter twice really is two commands.
  useEffect(() => {
    for (const command of keys.outbox) void perform(command);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [keys.seq]);

  // Keep the reducer's picture of the world in step. It never reads state
  // itself — it is told, which is what keeps it testable with no browser.
  useEffect(() => {
    dispatch({ kind: 'tables', tables });
  }, [tables]);

  useEffect(() => {
    dispatch({
      kind: 'cart',
      hasItems: cart ? !cart.isEmpty : false,
      // From the ORDER'S OWN LEDGER (crown jewel 2), so Enter on an empty box
      // picks the right branch after a merge and after a restart — not from
      // anything this screen is remembering.
      kitchenUpToDate: cart?.kitchenUpToDate ?? true,
    });
  }, [cart]);

  useEffect(() => {
    if (cart?.orderType) dispatch({ kind: 'order-type', value: cart.orderType });
  }, [cart?.orderType]);

  // **The search box has focus from the moment the screen opens.**
  // v1's did, and a cashier who has to click before typing has already lost
  // the advantage this whole session exists to keep. Found by running it: the
  // first attempt typed into nothing at all.
  useEffect(() => {
    searchBox.current?.focus();
  }, []);

  // **Whatever took focus gives it back.** The quantity popup focuses its own
  // panel; when it closes, the caret must return to the search box or the next
  // thing a cashier types goes nowhere. Found by running it: the first item
  // was added by keyboard and the second one silently was not.
  useEffect(() => {
    if (keys.mode.kind === 'searching') searchBox.current?.focus();
  }, [keys.mode.kind]);

  // Every key, in one listener, on the window — so a cashier never has to
  // click into anything first.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      // Let the browser have the ordinary editing keys inside a field.
      const editing =
        event.target instanceof HTMLInputElement ||
        event.target instanceof HTMLTextAreaElement;
      const interesting = [
        'Enter', 'Escape', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', '?',
      ];
      if (!interesting.includes(event.key)) return;
      if (editing && (event.key === 'ArrowLeft' || event.key === 'ArrowRight')) {
        // Inside a box with text in it, left/right move the caret. With an
        // empty box they cycle the order type, which is v1's behaviour.
        const box = event.target as HTMLInputElement;
        if (box.value !== '') return;
      }
      event.preventDefault();
      // **P29: was that a scanner?** Only ever on Enter, only ever when there
      // is a burst in the box, and only when Rust says so — otherwise the
      // ordinary Enter runs, exactly as it did before.
      if (event.key === 'Enter') {
        void handledAsScan().then((handled) => {
          if (handled) return;
          const done = mark('keystroke');
          dispatch({ kind: 'key', key: 'Enter' });
          done();
        });
        return;
      }
      // B1: mark the input, and let the hook report when the pixels changed.
      const done = mark('keystroke');
      dispatch({ kind: 'key', key: event.key });
      done();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [handledAsScan, mark]);

  if (!inApp()) {
    return (
      <EmptyState
        title="The billing screen needs the app"
        body="A browser has no engine behind it. Run Magic Bill itself."
      />
    );
  }

  return (
    /* `scroll={false}`: the billing screen manages its own two columns and
       must NEVER scroll as a whole — the cart is permanent (§1) and a cart you
       can scroll away from is not permanent. `Page` is here for one thing, the
       page margin, which after P27.5 no screen sets for itself. */
    <Page scroll={false} className="mb-billing">
      <div className="mb-billbar">
        <div className="mb-segment" role="group" aria-label="Order type">
          {ORDER_TYPES.map((kind) => (
            <button
              key={kind}
              type="button"
              className="mb-segment__option"
              aria-pressed={cart?.orderType === kind}
              onClick={() => void setOrderType(kind)}
            >
              {kind}
            </button>
          ))}
        </div>

        <Button
          variant={locked ? 'primary' : 'secondary'}
          onClick={() => setLocked((was) => !was)}
          aria-pressed={locked}
          title="Keep this order type for the next order"
        >
          <Icon name="lock" size="sm" />
          {locked ? 'Type locked' : 'Lock type'}
        </Button>

        <div className="mb-billbar__search">
          {/* P10 owns search behaviour (budget B2). It lives here now so the
              layout is not re-cut next session. */}
          <SearchField
            what="Search the menu, or type a table number"
            value={keys.text}
            ref={searchBox}
            // The box searches the MENU and accepts a table number. It does
            // NOT filter the floor: typing "dos" was emptying the grid, which
            // is the opposite of useful — a cashier searching for a dosa still
            // wants to see their tables. Found by running it.
            //
            // Audit F5 asked for a way through twenty open tables; typing the
            // table number and pressing Enter is that way, and it is one
            // keystroke shorter than filtering.
            onChange={(event) => {
              // P29 — the timing, for the scan-or-person question. Recording
              // it costs nothing and asking is only ever done on Enter.
              noteKeystroke(event.target.value);
              dispatch({ kind: 'typed', text: event.target.value });
            }}
          />
          <Suggestions
            items={keys.suggestions}
            highlighted={keys.highlighted}
            onPick={(index) => dispatch({ kind: 'tap-suggestion', index })}
          />
        </div>
      </div>

      <div className="mb-billing__body">
        <div className="mb-billing__floor">
          {/* **The set-up list is not on this screen any more** — P30.6.
              D102 put it beside the till and was right that it must never be
              a gate; it was wrong that the till is where it belongs. The
              owner installed the counter and found six rows with a "Do it"
              button each, on the page a cashier looks at all day. Every step
              is an alert behind the bell now, with the same button on it. */}
          {tables.length === 0 && menu.length === 0 ? (
            <EmptyState
              title="This shop has no menu or tables yet"
              // UI_GUIDELINES §6, and audit F8: never a system message. A
              // shopkeeper does not know what "P13" is, and saying it in the
              // one place a new shop starts is the worst possible moment.
              body="Add your items and tables in Settings — or put a demo shop in to see how the counter works."
              action={
                <Button variant="primary" onClick={() => void seedDemo()} disabled={busy}>
                  {busy ? <Spinner /> : 'Add a demo shop'}
                </Button>
              }
            />
          ) : (
            /* An empty floor draws NOTHING here — see `TableGrid`, P30.5. */
            <TableGrid tables={tables} filter={filter} onOpen={openTable} />
          )}

          {menu.length > 0 ? (
            <div className="mb-floor__section">
              <SectionHeader
                title="Menu"
                note="Type to search, or press one."
              />
              <div className="mb-floor__grid">
                {menu.map((item) => (
                  <button
                    key={item.id}
                    type="button"
                    className="mb-tile mb-tile--occupied"
                    onClick={() => void addItem(item.id)}
                    aria-label={`Add ${item.name}, ${item.price.text}`}
                  >
                    <span className="mb-cartline__name">{item.name}</span>
                    <span className="mb-tile__amount">{item.price.text}</span>
                    <span className="mb-cartline__rate">{item.rateLabel}</span>
                  </button>
                ))}
              </div>
            </div>
          ) : null}
        </div>

        {/* THE CART IS PERMANENT. It never moves and never hides (§1). */}
        <div className="mb-billing__cart">
          <SectionHeader
            title={cart?.table ? `Table ${cart.table}` : (cart?.orderType ?? 'Order')}
            note={
              cart?.isEmpty
                ? 'Empty'
                : `${cart?.lines.length ?? 0} ${cart?.lines.length === 1 ? 'line' : 'lines'}`
            }
          />

          {/* **Audit I6 — a very long bill says so** (P30). Not a refusal:
              a wedding party really does order sixty dishes, and a counter
              that stopped selling would be a worse product than a long
              ticket. The whole sentence is Rust's. */}
          {cart && cart.lengthSays ? (
            <p className="mb-cart__long">{cart.lengthSays}</p>
          ) : null}

          {/* **What the floor did while this was being typed** (P20, D83).
              The counter already took the change — it is the authority — and
              this offers to bring the lines into the bill on screen. Nothing
              here touches the payment somebody may be halfway through
              counting out. */}
          {cart && cart.fromTheFloor.length > 0 ? (
            <div className="mb-cart__floor">
              {cart.fromTheFloor.map((change) => (
                // The whole sentence, written in Rust.
                <p className="mb-cart__floorsays" key={`${change.itemId}-${change.qty}`}>
                  {change.says}
                </p>
              ))}
              <div className="mb-row--end">
                <Button
                  small
                  variant="quiet"
                  onClick={() => {
                    call('dismiss_the_floors_items').then(setCart).catch(report);
                  }}
                >
                  Not now
                </Button>
                <Button
                  small
                  variant="primary"
                  onClick={() => {
                    call('take_the_floors_items').then(setCart).catch(report);
                  }}
                >
                  Add them to this bill
                </Button>
              </div>
            </div>
          ) : null}

          <div className="mb-cart__lines">
            {cart && cart.lines.length > 0 ? (
              cart.lines.map((line) => (
                <div className="mb-cartline" key={line.index}>
                  <div>
                    <div className="mb-cartline__name">{line.name}</div>
                    {line.note ? (
                      <div className="mb-cartline__note">{line.note}</div>
                    ) : null}
                    <div className="mb-cartline__rate">{line.rateLabel}</div>
                  </div>
                  <div className="mb-cartline__qty">
                    <Button
                      small
                      variant="quiet"
                      onClick={() => void removeLine(line.index)}
                      aria-label={`Remove ${line.name}`}
                    >
                      <Icon name="x" size="sm" />
                    </Button>
                    <span className="mb-cartline__qty-value">{line.qty}</span>
                  </div>
                  <span className="mb-cartline__amount">{line.amount.text}</span>
                </div>
              ))
            ) : (
              <EmptyState
                small
                title="Nothing on this bill yet"
                body="Press an item to add it."
              />
            )}
          </div>

          {cart ? <Totals bill={cart.bill} /> : null}

          <div className="mb-payment">
            <div className="mb-payment__modes">
              {['Cash', 'Card', 'UPI'].map((mode) => (
                <Button
                  key={mode}
                  small
                  onClick={() => void takePayment(mode)}
                  disabled={!cart || cart.isEmpty}
                >
                  {mode}
                </Button>
              ))}
              {/* P15 made this real. A credit sale happens mid-bill with the
                  customer standing there, so the picker opens here rather
                  than sending a cashier to another screen. */}
              <Button
                small
                disabled={!cart || cart.isEmpty}
                onClick={() => setOnAccount(true)}
              >
                Credit
              </Button>
            </div>

            {cart && cart.payments.length > 0 ? (
              <div className="mb-payment__taken">
                {cart.payments.map((payment) => (
                  <div className="mb-totals__row" key={payment.index}>
                    <span>{payment.mode}</span>
                    <span className="mb-totals__value">{payment.amount.text}</span>
                  </div>
                ))}
                <div className="mb-totals__row">
                  <span>Balance</span>
                  <span className="mb-totals__value">{cart.balance.text}</span>
                </div>
                {cart.change.paise > 0n ? (
                  <div className="mb-totals__row">
                    <span>
                      <Badge tone="ok">Change due</Badge>
                    </span>
                    <span className="mb-totals__value">{cart.change.text}</span>
                  </div>
                ) : null}
                <Button small variant="quiet" onClick={() => void clearPayments()}>
                  Clear payments
                </Button>
              </div>
            ) : null}
          </div>

          <div className="mb-actions">
            <Button
              disabled={!cart || cart.isEmpty}
              onClick={() => void printKitchen()}
            >
              Kitchen ticket
            </Button>
            <Button
              variant="primary"
              disabled={!cart || cart.isEmpty}
              onClick={() => void completeBill()}
            >
              Complete bill
            </Button>
            <Button variant="quiet" onClick={() => void newOrder()}>
              New order
            </Button>
            {/* **P29, scope 7.9 — a parcel label.** Only when this shop has a
                label printer set up: a button for hardware nobody owns is a
                promise that fails when pressed. */}
            {hasLabels ? (
              <Button
                variant="quiet"
                disabled={!cart || cart.isEmpty}
                onClick={() => {
                  const first = cart?.lines[0];
                  if (!first) return;
                  call('print_label', {
                    line: `${first.qty} x ${first.name}`,
                    token: cart?.table ?? 'Parcel',
                  })
                    .then(() => toast.show('ok', 'The label is printing.'))
                    .catch(report);
                }}
              >
                Label
              </Button>
            ) : null}
            <Button
              variant="quiet"
              disabled={!cart || cart.isEmpty}
              onClick={() => setConfirmCancel(true)}
            >
              Cancel order
            </Button>
          </div>
        </div>
      </div>

      {keys.mode.kind === 'quantity' ? (
        <QuantityPopup
          mode={keys.mode}
          onType={(text) => dispatch({ kind: 'typed', text })}
          onConfirm={() => dispatch({ kind: 'key', key: 'Enter' })}
          onCancel={() => dispatch({ kind: 'key', key: 'Escape' })}
          // **P29, scope 7.7.** Only offered when this shop has a scale — and
          // a scale that does not answer says so in a toast and leaves the
          // typed quantity exactly where it was. Weighing can fail; billing
          // cannot.
          onWeigh={
            hasScale
              ? () => {
                  call('read_scale_once')
                    .then((answer) => {
                      if (!answer.answered) {
                        toast.show('warn', answer.says);
                        return;
                      }
                      // "1.234 kg" — the number is the quantity, the unit is
                      // the scale's own word for it.
                      const [amount] = answer.says.split(' ');
                      if (amount) dispatch({ kind: 'typed', text: amount });
                      toast.show('ok', answer.says);
                    })
                    .catch(report);
                }
              : undefined
          }
        />
      ) : null}

      {keys.mode.kind === 'table-busy' ? (
        <BusyTable
          mode={keys.mode}
          taken={takenLetters(tables, keys.mode.table.label)}
          onChoose={(choice) => {
            // Tap and key take the same road: move the highlight, then Enter.
            const steps = choice - (keys.mode.kind === 'table-busy' ? keys.mode.choice : 0);
            for (let n = 0; n < Math.abs(steps); n += 1) {
              dispatch({ kind: 'key', key: steps > 0 ? 'ArrowRight' : 'ArrowLeft' });
            }
            dispatch({ kind: 'key', key: 'Enter' });
          }}
          onCancel={() => dispatch({ kind: 'key', key: 'Escape' })}
        />
      ) : null}

      {keys.mode.kind === 'help' ? (
        <HelpSheet onClose={() => dispatch({ kind: 'key', key: 'Escape' })} />
      ) : null}

      <ConfirmDialog
        open={confirmCancel}
        title="Cancel this order?"
        body="Everything on this bill will be cleared. This cannot be undone."
        confirmLabel="Cancel the order"
        cancelLabel="Keep it"
        destructive
        onConfirm={() => {
          setConfirmCancel(false);
          void newOrder();
        }}
        onCancel={() => setConfirmCancel(false)}
      />

      {onAccount ? (
        <PutOnAccount
          onClose={() => setOnAccount(false)}
          onDone={(said) => {
            setOnAccount(false);
            toast.show('ok', said);
            // The cart came back from Rust with the credit payment on it; ask
            // for it again rather than trusting a second copy (D4).
            call('current_cart').then(setCart).catch(report);
          }}
          onFailed={report}
        />
      ) : null}
    </Page>
  );
}
