/** The billing screen — the heart of the product. */

import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useReducer,
  useRef,
  useState,
} from 'react';

import {
  Button,
  cx,
  EmptyState,
  Icon,
  MoneyInput,
  onlyAmount,
  Page,
  Scroller,
  SearchField,
  Spinner,
  Stepper,
  useAction,
  useReport,
  useToast,
} from '../kit';
import { call, inApp, subscribe } from '../ipc/call';
import type { CartView } from '../ipc/generated/CartView';
import type { MenuItemView } from '../ipc/generated/MenuItemView';
import type { TableView } from '../ipc/generated/TableView';
import { useTick } from '../clock';
import { mark } from '../perf';
import { BusyTable, HelpSheet, Suggestions, takenLetters } from './Keys';
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
import { ReasonDialog } from '../corrections/Reason';
import { DiscountDialog } from './Discount';
import { Processing, ProcessingHead, processingOrders } from './Processing';
import { SeparateBill } from './SeparateBill';
import { TableGrid } from './TableGrid';
import { Totals } from './Totals';
import { Before } from '../preview/Before';
import { keep, remember } from '../remember';

import './billing.css';

const ORDER_TYPES = ['Dine in', 'Parcel', 'Self service', 'Delivery'] as const;

/** Whether the processing panel is open — a look preference, kept on this computer. */
const PROCESSING_KEY = 'mb.billing.processing';

export function Billing() {
  const toast = useToast();
  const [cart, setCart] = useState<CartView | null>(null);
  const [tables, setTables] = useState<readonly TableView[]>([]);
  const [menu, setMenu] = useState<readonly MenuItemView[]>([]);
  // The grid is unfiltered: the search box is for the menu, and a table is reached by typing
  // its number and pressing Enter.
  const filter = '';
  // See it before it prints.
  const [preview, setPreview] = useState(false);
  /** The line somebody is voiding, once the kitchen has been told. */
  const [voidingLine, setVoidingLine] = useState<{ index: number; name: string } | null>(null);
  /** The line whose quantity is being typed, and what has been typed so far. */
  const [typingQty, setTypingQty] = useState<{ index: number; text: string } | null>(null);
  /** The reason for cancelling a parked order. */
  const [cancelReason, setCancelReason] = useState(false);
  /**
   * Moving some of the food onto a second bill — the only part of the old "Split" dialog that
   * still needs a screen.
   */
  const [splitting, setSplitting] = useState(false);
  /** Money off this bill. */
  const [discounting, setDiscounting] = useState(false);
  // The customer picker for a bill going on an account.
  const [onAccount, setOnAccount] = useState(false);
  const [busy, setBusy] = useState(false);
  /** Which way this bill is being paid. */
  const [payMode, setPayMode] = useState('Cash');
  /** The cash handed over, as typed. */
  const [cashGiven, setCashGiven] = useState('');
  /** Everything under the cart's two main buttons, folded away until asked for. */
  const [moreActions, setMoreActions] = useState(false);
  const moreActionsId = useId();
  /** The processing panel beside the floor, as it was last left. */
  const [processingOpen, setProcessingOpen] = useState(
    () => remember(PROCESSING_KEY, 'open') !== 'folded',
  );
  const processingId = useId();
  useEffect(() => {
    keep(PROCESSING_KEY, processingOpen ? 'open' : 'folded');
  }, [processingOpen]);

  // ONE shared clock (§5 rule 10).
  const tick = useTick();

  // The keyboard. The reducer decides; this component performs.
  const searchBox = useRef<HTMLInputElement>(null);
  /** Whether this shop has a scale and a label printer. */
  const [hasScale, setHasScale] = useState(false);
  const [hasLabels, setHasLabels] = useState(false);

  // The reducer is PURE, and the commands ride in the state.
  const [keys, dispatch] = useReducer(
    (state: KeyState, event: KeyEvent): KeyState => {
      const [next, commands] = reduceKeys(state, event);
      return { ...next, outbox: commands, seq: state.seq + 1 };
    },
    { ...initialKeys(), outbox: [] as KeyCommand[], seq: 0 },
  );

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
    // Anything else — a paste, a backspace, a fresh box — starts again.
    strokes.current = { text, gaps: [], at: now };
  }, []);

  // One reporter for the whole product — and it obeys the tone the engine set, so "the kitchen
  // already has everything on this bill" no longer arrives in the same red as a printer that
  // has died.
  const report = useReport();
  // One action at a time on this screen, matching the counter in Rust.
  const [act, acting] = useAction();

  // Silent on failure, and deliberately.
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
    // Silent on failure: a cashier who may not open the device screen still bills, and the two
    // buttons below simply do not appear.
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

  // The floor changed the order this cart has open.
  useEffect(() => {
    if (!inApp()) return undefined;
    let stop: (() => void) | undefined;
    subscribe((message) => {
      if (message.kind === 'floorChanged') {
        call('current_cart')
          .then(setCart)
          .catch(() => undefined);
      }
      // The data says an order, a table or a payment changed — whoever changed it.
      if (message.kind === 'floorChanged' || message.kind === 'floor') void refreshFloor();
    })
      .then((off) => {
        stop = off;
      })
      .catch(() => undefined);
    return () => stop?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The tick is for the timers only: a timer lives on the ORDER, so the screen re-reads rather
  // than counting. Changes arrive by push, above.
  useEffect(() => {
    void refreshFloor();
  }, [refreshFloor, tick]);

  // The same list the grid draws, narrowed to what the kitchen has. A settled order leaves it
  // the moment the floor is re-read.
  const processing = useMemo(
    () => processingOrders(tables, cart?.kitchenTicketOff ?? false),
    [tables, cart?.kitchenTicketOff],
  );

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

  const openOrderById = useCallback(
    async (orderId: string) => {
      try {
        setCart(await call('open_order', { orderId }));
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

  /** Change a line's quantity — the thing the cart could not do. */
  const setQty = useCallback(
    async (index: number, qty: string) => {
      try {
        setCart(await call('cart_set_qty', { index, qty }));
      } catch (cause) {
        report(cause);
      }
    },
    [report],
  );

  /** Commit what was typed into the quantity box. */
  const commitQty = useCallback(async () => {
    const typed = typingQty;
    setTypingQty(null);
    if (!typed) return;
    const was = cart?.lines.find((l) => l.index === typed.index)?.qty;
    if (typed.text.trim() === '' || typed.text.trim() === was) return;
    await setQty(typed.index, typed.text.trim());
  }, [cart?.lines, setQty, typingQty]);

  const step = useCallback(
    async (line: { index: number; qty: string; name: string }, by: number) => {
      // One less than one is a removal, and a removal after the kitchen has been told is a
      // void.
      if (by < 0 && cart?.kitchenTold && line.qty === '1') {
        setVoidingLine({ index: line.index, name: line.name });
        return;
      }
      try {
        setCart(await call('cart_step_qty', { index: line.index, by }));
      } catch (cause) {
        report(cause);
      }
    },
    [cart?.kitchenTold, report],
  );

  /** ✕ — and what ✕ means changes the moment the kitchen has been told. */
  const takeOffTheBill = useCallback(
    async (line: { index: number; name: string }) => {
      if (cart?.kitchenTold) {
        setVoidingLine({ index: line.index, name: line.name });
        return;
      }
      await removeLine(line.index);
    },
    [cart?.kitchenTold, removeLine],
  );

  const newOrder = useCallback(async () => {
    try {
      // The type stays: a parcel counter should not re-pick it forty times an hour. The shop's
      // lock, when it is on, is applied in Rust.
      setCart(await call('cart_clear', { keepType: true }));
      await refreshFloor();
    } catch (cause) {
      report(cause);
    }
  }, [refreshFloor, report]);

  /** Enter, with a burst of characters in the box: ask Rust whether that was a machine. */
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
        // A printed bill, scanned back onto the screen.
        toast.show('info', `${outcome.says} — open it under Bills.`);
        return true;
      }
      // Unknown: the code is real, nothing on this counter has it.
      toast.show(
        'warn',
        outcome.says,
        'Add it as the item’s code on the Menu screen, then scan again.',
      );
      return true;
    } catch (cause) {
      // A scanner that cannot be asked about must not eat a keystroke.
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

  /** The delta only — never the whole order. */
  const printKitchen = useCallback(async () => {
    try {
      await call('print_kitchen_ticket');
      setCart(await call('current_cart'));
      // The ticket is what turns a cart into an open order, so the floor is re-read here rather
      // than on the next tick fifteen seconds later.
      await refreshFloor();
      toast.show('ok', 'Kitchen ticket sent.');
    } catch (cause) {
      report(cause);
    }
  }, [refreshFloor, report, toast]);

  /** The cook lost the paper. */
  const reprintKitchen = useCallback(async () => {
    try {
      await call('reprint_kitchen_ticket');
      toast.show('ok', 'The whole ticket has been sent again, marked as a reprint.');
    } catch (cause) {
      report(cause);
    }
  }, [report, toast]);

  /**
   * Settle, then print. In that order — the money is on disk before the paper is attempted, so
   * a printer that is off cannot lose a bill.
   */
  const completeBill = useCallback(async () => {
    try {
      // Whatever is still owing goes down in the mode that is lit — Rust takes it and settles
      // in one command.
      const number = await call('complete_bill', { mode: payMode });
      setCart(await call('current_cart'));
      setCashGiven('');
      await refreshFloor();
      toast.show('ok', `Bill ${number} settled.`);
    } catch (cause) {
      report(cause);
    }
  }, [payMode, refreshFloor, report, toast]);

  /** The cash box, committed — and never onto an empty cart, which is what a blur after settling would do. */
  const commitCash = useCallback(
    async (typed: string) => {
      if (!cart || cart.isEmpty) return;
      try {
        setCart(
          typed.trim() === ''
            ? await call('cart_clear_payments')
            : await call('cart_cash_given', { amount: typed.trim() }),
        );
      } catch (cause) {
        report(cause);
      }
    },
    [cart, report],
  );

  // A tap goes through the SAME reducer a key does, so touch and keyboard cannot drift apart.
  const openTable = useCallback(
    (table: TableView) => {
      const index = tables.findIndex((t) => t.id === table.id);
      if (index >= 0) dispatch({ kind: 'tap-tile', index });
    },
    [tables],
  );

  /** Carry the bill to the table. */
  const printTheBill = useCallback(
    async (table: TableView) => {
      // The button only exists on a tile that has one; this is the type narrowing, not a second
      // opinion about whether to print.
      if (!table.orderId) return;
      try {
        toast.show('ok', await call('print_open_bill', { orderId: table.orderId }));
      } catch (cause) {
        report(cause);
      }
    },
    [report, toast],
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

  // Performing what the reducer asked for.
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
        case 'open-order':
          await openOrderById(command.orderId);
          return;
        case 'set-order-type':
          await setOrderType(command.value);
          return;
        case 'new-order':
          await newOrder();
          return;
        case 'focus-search':
          searchBox.current?.focus();
          return;
        // Through the same one-at-a-time gate as the buttons: a held-down shortcut key repeats,
        // and the counter must answer it the same way.
        case 'print-kitchen':
          act(printKitchen);
          return;
        case 'complete-bill':
          act(completeBill);
          return;
        case 'merge-into':
          try {
            setCart(await call('join_table', { tableId: command.tableId, seat: null }));
            await refreshFloor();
            toast.show('ok', 'On that table’s bill. Only the new items go to the kitchen.');
          } catch (cause) {
            report(cause);
          }
          return;
        case 'sub-table':
          try {
            setCart(await call('join_table', { tableId: command.tableId, seat: command.letter }));
          } catch (cause) {
            report(cause);
          }
          return;
      }
    },
    [
      addItem,
      completeBill,
      newOrder,
      openOrderById,
      openTableById,
      printKitchen,
      refreshFloor,
      report,
      setOrderType,
      toast,
    ],
  );

  // Perform one batch per committed dispatch.
  useEffect(() => {
    for (const command of keys.outbox) void perform(command);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [keys.seq]);

  // Keep the reducer's picture of the world in step.
  useEffect(() => {
    dispatch({ kind: 'tables', tables });
  }, [tables]);

  useEffect(() => {
    dispatch({
      kind: 'cart',
      hasItems: cart ? !cart.isEmpty : false,
      // From the ORDER'S OWN LEDGER, so Enter on an empty box picks the right branch after a
      // merge and after a restart — not from anything this screen is remembering.
      kitchenUpToDate: cart?.kitchenUpToDate ?? true,
    });
  }, [cart]);

  useEffect(() => {
    if (cart?.orderType) {
      dispatch({ kind: 'order-type', value: cart.orderType, locked: cart.orderTypeLocked });
    }
  }, [cart?.orderType, cart?.orderTypeLocked]);

  // The search box has focus from the moment the screen opens.
  useEffect(() => {
    searchBox.current?.focus();
  }, []);

  // Whatever took focus gives it back.
  useEffect(() => {
    if (keys.mode.kind === 'searching') searchBox.current?.focus();
  }, [keys.mode.kind]);

  // Every key, in one listener, on the window — so a cashier never has to click into anything
  // first.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const editing =
        event.target instanceof HTMLInputElement ||
        event.target instanceof HTMLTextAreaElement;
      // Only the boxes that belong to the keyboard engine feed it.
      if (editing && (event.target as HTMLElement).dataset.keys !== 'engine') return;
      const interesting = [
        'Enter', 'Escape', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', '?',
      ];
      if (!interesting.includes(event.key)) return;
      if (editing && (event.key === 'ArrowLeft' || event.key === 'ArrowRight')) {
        // Inside a box with text in it, left/right move the caret.
        const box = event.target as HTMLInputElement;
        if (box.value !== '') return;
      }
      event.preventDefault();
      // Was that a scanner?
      if (event.key === 'Enter') {
        void handledAsScan().then((handled) => {
          if (handled) return;
          const done = mark('keystroke');
          dispatch({ kind: 'key', key: 'Enter' });
          done();
        });
        return;
      }
      // Mark the input, and let the hook report when the pixels changed.
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
    /*
     * `scroll={false}`: the billing screen manages its own two columns and must NEVER scroll as
     * a whole — the cart is permanent (§1) and a cart you can scroll away from is not
     * permanent.
     */
    <Page scroll={false} className="mb-billing">
      {/*
        The left column: the top row holds the search box, New order and the processing
        panel's head; under it the floor, with the processing list folding down beside it. The
        floor's tiles reflow into whatever the list leaves.
      */}
      <div className="mb-billing__side">
        <div className="mb-billbar">
          <div className="mb-billbar__search">
            <SearchField
              what="Item or table number"
              value={keys.text}
              ref={searchBox}
              data-keys="engine"
              // The box searches the MENU and accepts a table number.
              onChange={(event) => {
                // The timing, for the scan-or-person question.
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

          {/* Esc from an empty box does the same — see the keyboard engine. */}
          <Button onClick={() => void newOrder()}>
            New order
            <kbd className="mb-kbd">Esc</kbd>
          </Button>
        </div>

        {/* Over its own column, as wide as the list under it. */}
        <ProcessingHead
          count={processing.length}
          open={processingOpen}
          controls={processingId}
          onToggle={() => setProcessingOpen((was) => !was)}
        />

        <Scroller inset className="mb-billing__floor">
          {/* The set-up list is not on this screen any more. */}
          {tables.length === 0 && menu.length === 0 ? (
            <EmptyState
              title="This shop has no menu or tables yet"
              body="Add your items and tables in Settings — or put a demo shop in to see how the counter works."
              action={
                <Button variant="primary" onClick={() => void seedDemo()} disabled={busy}>
                  {busy ? <Spinner /> : 'Add a demo shop'}
                </Button>
              }
            />
          ) : tables.length === 0 ? (
            /* A shop with a menu and no tables. */
            <EmptyState
              title="No tables yet"
              body="Add your tables on the Floor screen and they will show here. A counter with no tables does not need any — search for an item above and start the bill."
            />
          ) : (
            <TableGrid
              tables={tables}
              filter={filter}
              onOpen={openTable}
              onPrintBill={printTheBill}
            />
          )}

          {/* THE MENU GRID IS NOT ON THIS SCREEN. */}
        </Scroller>

        {/* Folds up under its head. Folded: no height, and nothing in it can take focus. */}
        <div
          id={processingId}
          className={cx('mb-processing__fold', processingOpen && 'mb-processing__fold--open')}
          inert={!processingOpen}
        >
          <div className="mb-processing__panel">
            <Scroller inset className="mb-processing__body">
              <Processing orders={processing} onOpen={openTable} />
            </Scroller>
          </div>
        </div>
      </div>

      <div className="mb-billbar__type">
        {cart?.orderTypeLocked ? null : (
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
        )}
      </div>

      {/* THE CART IS PERMANENT. */}
      <div className="mb-billing__cart">

        {/* A very long bill says so. */}
        {cart && cart.lengthSays ? (
          <p className="mb-cart__long">{cart.lengthSays}</p>
        ) : null}

        {/* What the floor did while this was being typed. */}
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

        {/* Where this bill is, and its number once it has one. */}
        {cart && (cart.table || cart.billNumber) ? (
          <div className="mb-cart__head">
            <span className="mb-cart__where">
              {cart.table ? `Table ${cart.table}` : cart.orderType}
            </span>
            {cart.billNumber ? <span className="mb-cart__no">{cart.billNumber}</span> : null}
          </div>
        ) : null}

        <Scroller inset className="mb-cart__lines">
          {cart && cart.lines.length > 0 ? (
            cart.lines.map((line) => (
              <div className="mb-cartline" key={line.index}>
                <div className="mb-cartline__what">
                  <span className="mb-cartline__name">{line.name}</span>
                  {/* The price of one, its tax rate, the extras on it, and any money off. */}
                  <span className="mb-cartline__detail">
                    <span className="mb-cartline__price">{line.unitPrice.text}</span>
                    <span>{line.rateLabel}</span>
                    {line.modifiers.length > 0 ? (
                      <span className="mb-cartline__extras">{line.modifiers.join(', ')}</span>
                    ) : null}
                    {line.discount.paise > 0n ? (
                      <span className="mb-cartline__price">−{line.discount.text}</span>
                    ) : null}
                  </span>
                  {line.note ? <span className="mb-cartline__note">{line.note}</span> : null}
                </div>
                {/*
                  − qty + and then ✕, in that order, because the quantity is what a cashier
                  changes forty times a shift and the removal is what they do once.
                */}
                <Stepper
                  label={`Quantity of ${line.name}`}
                  what={line.name}
                  onLess={() => void step(line, -1)}
                  onMore={() => void step(line, +1)}
                >
                  {typingQty?.index === line.index ? (
                    <input
                      className="mb-stepper__value"
                      autoFocus
                      inputMode="decimal"
                      aria-label={`Quantity of ${line.name}`}
                      value={typingQty.text}
                      onChange={(e) =>
                        setTypingQty({
                          index: line.index,
                          // Digits and one dot.
                          text: onlyAmount(e.target.value),
                        })
                      }
                      onBlur={() => void commitQty()}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') void commitQty();
                        if (e.key === 'Escape') setTypingQty(null);
                      }}
                    />
                  ) : (
                    <button
                      type="button"
                      className="mb-stepper__value"
                      aria-label={`Change the quantity of ${line.name}`}
                      onClick={() => setTypingQty({ index: line.index, text: line.qty })}
                    >
                      {line.qty}
                    </button>
                  )}
                </Stepper>
                {typingQty?.index === line.index && hasScale ? (
                  <button
                    type="button"
                    className="mb-cartline__tool"
                    title="Take the weight from the scale"
                    aria-label="Take the weight from the scale"
                    onMouseDown={(e) => e.preventDefault()}
                    onClick={() => {
                      call('read_scale_once')
                        .then((answer) => {
                          if (!answer.answered) {
                            toast.show('warn', answer.says);
                            return;
                          }
                          // "1.234 kg" — the number is the quantity.
                          const [amount] = answer.says.split(' ');
                          if (amount) setTypingQty({ index: line.index, text: amount });
                        })
                        .catch(report);
                    }}
                  >
                    <Icon name="scale" size="sm" />
                  </button>
                ) : null}
                <span className="mb-cartline__amount">{line.amount.text}</span>
                <button
                  type="button"
                  className="mb-cartline__tool mb-cartline__tool--remove"
                  title={`Remove ${line.name}`}
                  aria-label={`Remove ${line.name}`}
                  onClick={() => void takeOffTheBill(line)}
                >
                  <Icon name="x" size="sm" />
                </button>
              </div>
            ))
          ) : (
            <EmptyState
              small
              title="Nothing on this bill yet"
              body="Press an item to add it."
            />
          )}
        </Scroller>

        <div className="mb-payment">
          <PaymentModes
            mode={payMode}
            onPick={setPayMode}
            onCredit={() => setOnAccount(true)}
          />

          {/*
            The one box a cashier types money into, and only for the one thing that is counted
            by hand.
          */}
          {payMode === 'Cash' ? (
            <div className="mb-payment__cash">
              <MoneyInput
                label="Cash given"
                value={cashGiven}
                onChange={setCashGiven}
                onBlur={() => void commitCash(cashGiven)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') void commitCash(cashGiven);
                }}
              />
              {cart && cart.change.paise > 0n ? (
                <span className="mb-payment__answer">
                  Change <strong>{cart.change.text}</strong>
                </span>
              ) : cart && cart.payments.length > 0 && cart.balance.paise > 0n ? (
                <span className="mb-payment__answer">
                  Still owing <strong>{cart.balance.text}</strong>
                </span>
              ) : null}
            </div>
          ) : null}
        </div>

        {cart && !cart.isEmpty ? <Totals bill={cart.bill} /> : null}

        {/* Two buttons, and a fold. */}
        <div className="mb-actions">
          {cart?.kitchenTicketOff ? null : (
          <Button
            disabled={!cart || cart.isEmpty || acting}
            onClick={() => act(printKitchen)}
          >
            Kitchen ticket
          </Button>
          )}
          <Button
            variant="primary"
            disabled={!cart || cart.isEmpty || acting}
            onClick={() => act(completeBill)}
          >
            Complete bill
          </Button>

          <button
            type="button"
            className="mb-actions__toggle"
            aria-expanded={moreActions}
            aria-controls={moreActionsId}
            title={moreActions ? 'Hide the rest' : 'The rest of the actions'}
            onClick={() => setMoreActions((was) => !was)}
          >
            <Icon
              name={moreActions ? 'chevron-up' : 'chevron-down'}
              size="sm"
              label={moreActions ? 'Hide the rest' : 'The rest of the actions'}
            />
          </button>
        </div>

        {/* The rest, in two lines: the paper first, then what changes the bill. */}
        <div id={moreActionsId} className="mb-actions--more" hidden={!moreActions}>
          <Button small disabled={!cart || cart.isEmpty} onClick={() => setPreview(true)}>
            Preview bill
          </Button>
          {/* Only once a ticket has gone: before that, "Kitchen ticket" is the button. */}
          {cart?.orderId && !cart.kitchenTicketOff ? (
            <Button small onClick={() => act(reprintKitchen)}>
              Reprint ticket
            </Button>
          ) : null}
          {/* Only when this shop has a label printer. */}
          {hasLabels ? (
            <Button
              small
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
          <Button small disabled={!cart || cart.isEmpty} onClick={() => setDiscounting(true)}>
            {cart && cart.bill.billDiscount.paise > 0n ? 'Change discount' : 'Discount'}
          </Button>
          <Button
            small
            disabled={!cart || cart.isEmpty || !cart.orderId}
            onClick={() => setSplitting(true)}
          >
            Separate bill
          </Button>
          {/* A parked order is cancelled with a reason; a typed one simply goes. */}
          <Button
            small
            disabled={!cart || cart.isEmpty}
            onClick={() => (cart?.orderId ? setCancelReason(true) : void newOrder())}
          >
            Cancel order
          </Button>
        </div>
      </div>

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

      {/* The paper, before it is paper. */}
      <Before open={preview} onClose={() => setPreview(false)} onPrint={() => act(completeBill)} />

      {/* The order is in the books, so cancelling it is a correction. */}
      {cancelReason && cart?.orderId ? (
        <ReasonDialog
          kind="cancel"
          what={`Cancel order ${cart.table ?? cart.orderType} — ${cart.bill.grandTotal.text}`}
          confirmLabel="Cancel the order"
          onCancel={() => setCancelReason(false)}
          onConfirm={(reason) => {
            setCancelReason(false);
            const id = cart.orderId;
            if (!id) return;
            call('cancel_order', { orderId: id, reason })
              .then(async () => {
                toast.show('ok', 'The order is cancelled, and the kitchen has been told.');
                // The cart is cleared LOCALLY afterwards, and only after Rust agreed: clearing
                // first and then failing would take the order off the screen while it stayed
                // open in the books.
                await newOrder();
              })
              .catch(report);
          }}
        />
      ) : null}

      {discounting && cart ? (
        <DiscountDialog
          cart={cart}
          onClose={() => setDiscounting(false)}
          onChanged={setCart}
        />
      ) : null}

      {splitting && cart ? (
        <SeparateBill
          cart={cart}
          onClose={() => {
            setSplitting(false);
            // The guest count is saved as it is typed, so the cart on screen is a copy that is
            // now behind.
            call('current_cart').then(setCart).catch(report);
          }}
          onSplit={(said) => {
            setSplitting(false);
            toast.show('ok', said);
            call('current_cart').then(setCart).catch(report);
            void refreshFloor();
          }}
          onFailed={report}
        />
      ) : null}

      {/* Taking a line off a bill the kitchen is already cooking. */}
      {voidingLine ? (
        <ReasonDialog
          kind="item_void"
          what={`Take ${voidingLine.name} off this bill`}
          confirmLabel="Take it off"
          onCancel={() => setVoidingLine(null)}
          onConfirm={(reason) => {
            const line = voidingLine;
            setVoidingLine(null);
            call('void_line', { index: line.index, reason })
              .then((fresh) => {
                setCart(fresh);
                toast.show('ok', `${line.name} is off the bill.`);
              })
              // A void whose kitchen slip failed comes back as an error that still says the
              // line is off — Rust wrote that sentence, and this must show it rather than
              // swallowing it.
              .catch(report);
          }}
        />
      ) : null}

      {onAccount ? (
        <PutOnAccount
          onClose={() => setOnAccount(false)}
          onDone={(said) => {
            setOnAccount(false);
            toast.show('ok', said);
            // The cart came back from Rust with the credit payment on it; ask for it again
            // rather than trusting a second copy.
            call('current_cart').then(setCart).catch(report);
          }}
          onFailed={report}
        />
      ) : null}
    </Page>
  );
}

/** The four ways to pay, and what each of them knows about the bill. */
export function PaymentModes({
  mode,
  onPick,
  onCredit,
}: {
  /** Which mode is lit. */
  mode: string;
  onPick: (mode: string) => void;
  onCredit: () => void;
}) {
  const [showCredit, setShowCredit] = useState(false);
  const creditId = useId();

  return (
    <>
      <div className="mb-payment__modes">
        {/* Exactly one of these, so it is the kit's segmented control. */}
        <div className="mb-segment mb-segment--fill" role="group" aria-label="Paid by">
          {['Cash', 'Card', 'UPI'].map((label) => (
            <button
              key={label}
              type="button"
              className="mb-segment__option"
              aria-pressed={mode === label}
              onClick={() => onPick(label)}
            >
              {label}
            </button>
          ))}
        </div>
        <button
          type="button"
          className="mb-payment__reveal"
          aria-expanded={showCredit}
          aria-controls={creditId}
          title={showCredit ? 'Hide credit billing' : 'Show credit billing'}
          onClick={() => setShowCredit((was) => !was)}
        >
          <Icon
            name={showCredit ? 'chevron-up' : 'chevron-down'}
            size="sm"
            label={showCredit ? 'Hide credit billing' : 'Show credit billing'}
          />
        </button>
      </div>

      {/*
        A credit sale happens mid-bill with the customer standing there, so the picker opens
        here rather than on another screen.
      */}
      <div id={creditId} className="mb-payment__credit" hidden={!showCredit}>
        <Button small wide onClick={onCredit}>
          Credit
        </Button>
      </div>
    </>
  );
}
