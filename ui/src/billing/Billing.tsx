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

import { useCallback, useEffect, useId, useMemo, useReducer, useRef, useState } from 'react';

import {
  Button,
  ConfirmDialog,
  EmptyState,
  Icon,
  MoneyInput,
  onlyAmount,
  Page,
  Scroller,
  SearchField,
  Spinner,
  useAction,
  useReport,
  useToast,
} from '../kit';
import { call, inApp, subscribe } from '../ipc/call';
import type { CartView } from '../ipc/generated/CartView';
import type { MenuItemView } from '../ipc/generated/MenuItemView';
import type { EvenSplitView } from '../ipc/generated/EvenSplitView';
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
import { ReasonDialog } from '../corrections/Reason';
import { DiscountDialog } from './Discount';
import { SeparateBill } from './SeparateBill';
import { TableGrid } from './TableGrid';
import { Totals } from './Totals';
import { Before, type Paper } from '../preview/Before';

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
  // **Audit D6 — see it before it prints.** `null` is closed.
  const [preview, setPreview] = useState<Paper | null>(null);
  /**
   * **The line somebody is voiding, once the kitchen has been told.**
   *
   * Before the first ticket a ✕ is a mis-tap being undone and stays silent.
   * After it, food is on a pass and taking it off the bill is a correction the
   * shop has to be able to account for — so it goes through the same reason
   * dialog as every other one (P12), and Rust prints the kitchen its
   * cancellation slip.
   */
  const [voidingLine, setVoidingLine] = useState<{ index: number; name: string } | null>(null);
  /**
   * The line whose quantity is being typed, and what has been typed so far.
   *
   * − and + cover "one more"; this covers "make it seven" and "make it 0.75",
   * which is the same gesture as tapping the number on a phone keypad. It is
   * the only caller of `cart_set_qty`.
   */
  const [typingQty, setTypingQty] = useState<{ index: number; text: string } | null>(null);
  /** The reason for cancelling a parked order (P12). */
  const [cancelReason, setCancelReason] = useState(false);
  /** Moving some of the food onto a second bill — the only part of the old
      "Split" dialog that still needs a screen. */
  const [splitting, setSplitting] = useState(false);
  /** How many are sharing this bill, and what Rust says each one owes. */
  const [ways, setWays] = useState(2);
  const [even, setEven] = useState<EvenSplitView | null>(null);
  /** Scope 1.12 — money off this bill (2026-08-17). */
  const [discounting, setDiscounting] = useState(false);
  /// P15 — the customer picker for a bill going on an account.
  const [onAccount, setOnAccount] = useState(false);
  const [busy, setBusy] = useState(false);
  /** Which way this bill is being paid. Nothing is charged until it is
      completed — the row is a choice, not an action. */
  const [payMode, setPayMode] = useState('Cash');
  /** The cash handed over, as typed. Empty means "the whole bill". */
  const [cashGiven, setCashGiven] = useState('');
  /** Everything under the cart's two main buttons, folded away until asked for. */
  const [moreActions, setMoreActions] = useState(false);
  const moreActionsId = useId();
  /** The orders being cooked right now, folded away until asked for. */
  const [processing, setProcessing] = useState(false);
  const processingId = useId();

  const openOrders = useMemo(
    () => tables.filter((table) => table.orderId !== null),
    [tables],
  );

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

  // One reporter for the whole product — and it obeys the tone the engine set,
  // so "the kitchen already has everything on this bill" no longer arrives in
  // the same red as a printer that has died.
  const report = useReport();
  // One action at a time on this screen, matching the counter in Rust.
  const [act, acting] = useAction();

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
        // Same push, same source: whatever changed the floor changed which
        // orders are open, so the queue follows it instead of the clock.
        void refreshFloor();
      }
    })
      .then((off) => {
        stop = off;
      })
      .catch(() => undefined);
    return () => stop?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
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

  /**
   * **Change a line's quantity** — the thing the cart could not do.
   *
   * Until now the only control on a cart line was ✕: two dosas becoming three
   * meant deleting the line and typing the item again, on the till, mid
   * service. `cart_set_qty` had been in Rust since P09 with nothing calling it.
   *
   * Typing an exact quantity is `cart_set_qty`; − and + are `cart_step_qty`,
   * because a quantity is thousandths and JavaScript has doubles (D2).
   */
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

  /**
   * Commit what was typed into the quantity box.
   *
   * An unchanged value is not sent: blurring the box after looking at it must
   * not write an audit-visible change. **Rust parses the text** — "0.5", "1/2"
   * and "abc" are all its judgement to make, and it already has the sentence
   * for the last one.
   */
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
      // **One less than one is a removal, and a removal after the kitchen has
      // been told is a void.** Sending this to Rust as a step would take the
      // line off silently and the kitchen would keep cooking it.
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

  /**
   * ✕ — and **what ✕ means changes the moment the kitchen has been told.**
   *
   * Before the first ticket it is somebody undoing a mis-tap, and asking them
   * to type a reason for that makes the till slower for nothing. After it,
   * food is being cooked: taking the line off is a correction the shop must be
   * able to account for, so it becomes `void_line` — a reason, an audit row,
   * and a cancellation slip Rust prints for the kitchen (audit B5/B6).
   */
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

  const takeTheBalance = useCallback(
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
      // The ticket is what turns a cart into an open order, so the floor is
      // re-read here rather than on the next tick fifteen seconds later.
      await refreshFloor();
      toast.show('ok', 'Kitchen ticket sent.');
    } catch (cause) {
      report(cause);
    }
  }, [refreshFloor, report, toast]);

  /**
   * **The cook lost the paper** — P32. The whole order again, marked as a
   * reprint, with the ledger untouched: the next delta is still the delta.
   */
  const reprintKitchen = useCallback(async () => {
    try {
      await call('reprint_kitchen_ticket');
      toast.show('ok', 'The whole ticket has been sent again, marked as a reprint.');
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
      // Whatever is still owing goes down in the mode that is lit. On the
      // ordinary bill that is the whole of it and the cashier typed nothing.
      if (cart && cart.balance.paise > 0n) await takeTheBalance(payMode);
      const number = await call('complete_bill');
      setCart(await call('current_cart'));
      setCashGiven('');
      await refreshFloor();
      toast.show('ok', `Bill ${number} settled.`);
    } catch (cause) {
      report(cause);
    }
  }, [cart, payMode, refreshFloor, report, takeTheBalance, toast]);

  /**
   * **The cash box, committed.** It owns the cash line: typing again replaces
   * it, emptying it takes it off. Rust parses the rupees (R8).
   */
  const commitCash = useCallback(
    async (typed: string) => {
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
    [report],
  );

  // A tap goes through the SAME reducer a key does, so touch and keyboard
  // cannot drift apart (scope 1.28, and test T10).
  /**
   * **How many people are sharing it.**
   *
   * One number does both jobs the old dialog asked separately: it is what the
   * bill is divided by, and it is the covers every per-head figure in Reports
   * had nothing to divide by until P31.
   */
  const setPeople = useCallback(async (howMany: number) => {
    setWays(howMany);
    try {
      setEven(await call('even_split', { ways: howMany }));
      await call('set_covers', { covers: howMany });
    } catch {
      // A split nobody can work out is a blank line, not a toast: the cashier
      // asked a question, and the answer is simply not there.
      setEven(null);
    }
  }, []);

  // Asked only while the fold is open, and only ever a question — even_split
  // creates nothing.
  useEffect(() => {
    if (!moreActions || !cart || cart.isEmpty) {
      setEven(null);
      return;
    }
    call('even_split', { ways })
      .then(setEven)
      .catch(() => setEven(null));
  }, [moreActions, ways, cart]);

  const openTable = useCallback(
    (table: TableView) => {
      const index = tables.findIndex((t) => t.id === table.id);
      if (index >= 0) dispatch({ kind: 'tap-tile', index });
    },
    [tables],
  );

  /**
   * **Carry the bill to the table** — the print mark on a tile, 2026-08-17.
   *
   * It does NOT open the table first. That was the tempting version and it is
   * the wrong one: opening a table replaces whatever is in the cart, so a
   * cashier halfway through typing a parcel order who pressed print on table 4
   * would lose the parcel. `print_open_bill` reads the order off disk and
   * touches neither the cart nor the floor, so this is a press with no
   * consequence beyond a piece of paper — which is what the button looks like
   * it does.
   */
  const printTheBill = useCallback(
    async (table: TableView) => {
      // The button only exists on a tile that has one; this is the type
      // narrowing, not a second opinion about whether to print.
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
        // Through the same one-at-a-time gate as the buttons: a held-down
        // shortcut key repeats, and the counter must answer it the same way.
        case 'print-kitchen':
          act(printKitchen);
          return;
        case 'complete-bill':
          act(completeBill);
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
      const editing =
        event.target instanceof HTMLInputElement ||
        event.target instanceof HTMLTextAreaElement;
      // Only the boxes that belong to the keyboard engine feed it. Any other
      // field on this screen owns its own keys.
      if (editing && (event.target as HTMLElement).dataset.keys !== 'engine') return;
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
        {/* **Search first.** The owner's change list of 2026-08-23: the box a
            cashier types in all day was third, behind two controls that are
            touched once a shift, and it was stretched across the whole bar
            while they were squashed into the left corner. The order is the
            frequency now — search, then the order type, then the lock. */}
        <div className="mb-billbar__search">
          {/* P10 owns search behaviour (budget B2). It lives here now so the
              layout is not re-cut next session. */}
          <SearchField
            what="Item or table number"
            value={keys.text}
            ref={searchBox}
            data-keys="engine"
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

        <button
          type="button"
          className="mb-processing__head"
          aria-expanded={processing}
          aria-controls={processingId}
          onClick={() => setProcessing((was) => !was)}
        >
          <Icon name={processing ? 'chevron-up' : 'chevron-down'} size="sm" />
          Processing orders
          <span className="mb-processing__count">{openOrders.length}</span>
        </button>

        <div className="mb-billbar__type">
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

        {/* **The lock is a picture now, not a pill.** Two words and an icon at
            full button size claimed about as much of the bar as all four order
            types together, for "keep this order type for the next order" — a
            thing pressed once a shift. Icon only and tiny. Nothing is lost to a
            reader: the fill says on or off, `aria-pressed` says it out loud,
            and the tooltip still spells the sentence out (§7). */}
        <Button
          className="mb-billbar__lock"
          variant={locked ? 'primary' : 'quiet'}
          onClick={() => setLocked((was) => !was)}
          aria-pressed={locked}
          title={
            locked
              ? 'Type locked — press to let the order type change again'
              : 'Keep this order type for the next order'
          }
        >
          <Icon name="lock" size="sm" label={locked ? 'Type locked' : 'Lock type'} />
        </Button>
        </div>
      </div>

      <div
        className={processing ? 'mb-billing__body mb-billing__body--queue' : 'mb-billing__body'}
      >
        <Scroller inset className="mb-billing__floor">

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
          ) : tables.length === 0 ? (
            /*
              **A shop with a menu and no tables** — and this branch is back
              because of what the owner asked for on 2026-08-17.

              P30.5 deliberately drew NOTHING here, and its reasoning is still
              in `TableGrid`: a tea stall and a parcel counter have no tables
              and never will, so a permanent card explaining tables was
              furniture on the one screen a cashier lives on. That was right
              **while the menu grid filled the pane underneath it.** With the
              menu grid gone this pane is now empty from the search box to the
              bottom of the window — half the counter, blank — which is a
              worse answer than a sentence.

              So it says the one useful thing and gets out of the way. No
              button: a screen is rendered with no props (`SHIPPED_SCREENS`),
              so it cannot navigate, and "Floor" is a word in the bar directly
              above. Billing works completely without ever coming here — type
              the item, press Enter, take the money.
            */
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

          {/* **THE MENU GRID IS NOT ON THIS SCREEN** — the owner, 2026-08-17:
              *"in billing page some changes required, menu section dont show
              here in the billing page, remove it, use the space for tables
              showcasing."*

              It was a second grid of `mb-tile`s under the floor, sharing the
              floor's own layout, so a shop with fifty items had its tables
              pushed up into a strip and everything below the fold was food.
              The screen is the FLOOR now, whole, which is what makes P31's
              bigger tiles affordable.

              Nothing is lost: the search box at the top of this screen already
              searches the menu and P10's keyboard engine adds what it finds —
              that path has been the fast one since P10 and is what a busy
              counter actually uses. `menu` is still loaded here because the
              search needs it and because `tables.length === 0 && menu.length
              === 0` is how this screen knows a shop is brand new. */}
        </Scroller>

        {processing ? (
          <div className="mb-queuepanel" id={processingId}>
            <Scroller inset className="mb-queuepanel__list">
              {openOrders.length === 0 ? (
                <EmptyState small title="Nothing being cooked" body="Sent orders show here." />
              ) : null}
              {openOrders.map((order) => (
                <button
                  type="button"
                  key={order.id}
                  className={
                    order.selected ? 'mb-queueline mb-queueline--on' : 'mb-queueline'
                  }
                  aria-pressed={order.selected}
                  onClick={() => openTable(order)}
                >
                  <span className="mb-queueline__where">{order.label}</span>
                  <span className="mb-queueline__amount">{order.total?.text ?? ''}</span>
                  <span className="mb-queueline__no">{order.billNumber ?? '—'}</span>
                  <span className="mb-queueline__when">
                    {order.minutes === null ? '' : `${order.minutes}m`}
                  </span>
                </button>
              ))}
            </Scroller>
          </div>
        ) : null}

        {/* THE CART IS PERMANENT. It never moves and never hides (§1). */}
        <div className="mb-billing__cart">

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

          <Scroller inset className="mb-cart__lines">
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
                  {/* **− qty + and then ✕**, in that order, because the
                      quantity is what a cashier changes forty times a shift
                      and the removal is what they do once. */}
                  <div className="mb-cartline__qty">
                    <Button
                      small
                      variant="quiet"
                      onClick={() => void step(line, -1)}
                      aria-label={`One less ${line.name}`}
                    >
                      <Icon name="minus" size="sm" />
                    </Button>
                    {typingQty?.index === line.index ? (
                      <input
                        className="mb-cartline__qty-input"
                        autoFocus
                        inputMode="decimal"
                        aria-label={`Quantity of ${line.name}`}
                        value={typingQty.text}
                        onChange={(e) =>
                          setTypingQty({
                            index: line.index,
                            // Digits and one dot. A quantity may be 0.75 kg; it
                            // may never be "2a", which is what this accepted.
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
                        className="mb-cartline__qty-value"
                        aria-label={`Change the quantity of ${line.name}`}
                        onClick={() => setTypingQty({ index: line.index, text: line.qty })}
                      >
                        {line.qty}
                      </button>
                    )}
                    <Button
                      small
                      variant="quiet"
                      onClick={() => void step(line, +1)}
                      aria-label={`One more ${line.name}`}
                    >
                      <Icon name="plus" size="sm" />
                    </Button>
                    <Button
                      small
                      variant="quiet"
                      onClick={() => void takeOffTheBill(line)}
                      aria-label={`Remove ${line.name}`}
                    >
                      <Icon name="x" size="sm" />
                    </Button>
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
          </Scroller>

          <div className="mb-payment">
            <PaymentModes
              mode={payMode}
              onPick={setPayMode}
              onCredit={() => setOnAccount(true)}
            />

            {/* The one box a cashier types money into, and only for the one
                thing that is counted by hand. Card and UPI are always exact. */}
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

          {cart ? <Totals bill={cart.bill} /> : null}

          {/* Two buttons, and a fold. `acting` refuses a second press while
              the first is still running; disabled is so it can be seen. */}
          <div className="mb-actions">
            <Button
              disabled={!cart || cart.isEmpty || acting}
              onClick={() => act(printKitchen)}
            >
              Kitchen ticket
            </Button>
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

          <div
            id={moreActionsId}
            className="mb-actions mb-actions--more"
            hidden={!moreActions}
          >
            {/* "What do we each owe?" — a question, answered in place. It makes
                no bills, and Rust writes the sentence, remainder and all. */}
            <div className="mb-eachpays">
              <span className="mb-eachpays__label">Each pays</span>
              <Button
                small
                variant="quiet"
                disabled={ways <= 2}
                onClick={() => void setPeople(ways - 1)}
                aria-label="One fewer person"
              >
                <Icon name="minus" size="sm" />
              </Button>
              <span className="mb-eachpays__count">{ways}</span>
              <Button
                small
                variant="quiet"
                disabled={ways >= 50}
                onClick={() => void setPeople(ways + 1)}
                aria-label="One more person"
              >
                <Icon name="plus" size="sm" />
              </Button>
              <span className="mb-eachpays__says">{even ? even.note : ''}</span>
            </div>

            <Button
              variant="quiet"
              disabled={!cart || cart.isEmpty}
              onClick={() => setPreview('bill')}
            >
              Preview bill
            </Button>
            <Button
              variant="quiet"
              disabled={!cart || cart.isEmpty}
              onClick={() => setPreview('kitchen')}
            >
              Preview ticket
            </Button>
            {/* Only once a ticket has gone: before that, "Kitchen ticket" is
                the button. */}
            {cart?.orderId ? (
              <Button variant="quiet" onClick={() => act(reprintKitchen)}>
                Send ticket again
              </Button>
            ) : null}
            <Button variant="quiet" onClick={() => void newOrder()}>
              New order
            </Button>
            {/* Only when this shop has a label printer: a button for hardware
                nobody owns is a promise that fails when pressed. */}
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
              onClick={() => setDiscounting(true)}
            >
              {cart && cart.bill.billDiscount.paise > 0n ? 'Change discount' : 'Discount'}
            </Button>
            <Button
              variant="quiet"
              disabled={!cart || cart.isEmpty || !cart.orderId}
              onClick={() => setSplitting(true)}
            >
              Separate bill
            </Button>
            <Button
              variant="quiet"
              disabled={!cart || cart.isEmpty}
              onClick={() => (cart?.orderId ? setCancelReason(true) : setConfirmCancel(true))}
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

      {/* **The paper, before it is paper** — audit D6, built at P32. */}
      <Before
        what={preview ?? 'bill'}
        open={preview !== null}
        onClose={() => setPreview(null)}
        onPrint={
          preview === 'kitchen'
            ? () => act(printKitchen)
            : () => act(completeBill)
        }
      />

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

      {/* **The order is in the books, so cancelling it is a correction.**
          One dialog for all four of P12's corrections, so the reason list, the
          free-text box and the wording cannot drift apart between them. */}
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
                // The cart is cleared LOCALLY afterwards, and only after Rust
                // agreed: clearing first and then failing would take the
                // order off the screen while it stayed open in the books.
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
            // The guest count is saved as it is typed, so the cart on screen
            // is a copy that is now behind. Ask again (D4).
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
              // A void whose kitchen slip failed comes back as an error that
              // still says the line is off — Rust wrote that sentence, and
              // this must show it rather than swallowing it.
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

/**
 * **The four ways to pay, and what each of them knows about the bill.**
 *
 * The owner, 2026-08-22, looking at a settled bill: *"the payment mode
 * selection is also not visible, and it also shows some error notification,
 * what is it?"*
 *
 * The notification was **"That payment could not be taken — a payment has to be
 * more than zero"**, and it was the screen's fault rather than Rust's. Each of
 * these buttons takes *the balance Rust computed*. Once Cash has covered the
 * bill that balance is zero, and the buttons carried on offering themselves —
 * so pressing one sent a zero-rupee payment, which `mb_core::Payment::new`
 * refuses, correctly and always: a zero-rupee row is noise in every report
 * downstream. **The button whose only possible outcome is an error is the bug**,
 * not the refusal. It is the same argument the tile makes about a print mark on
 * an empty table.
 *
 * The other half of the sentence is the answer to it. Nothing on this row ever
 * said which mode had been used — four identical outlines before the money and
 * four identical outlines after it — so there was no way to see that the bill
 * was already paid and therefore no way to guess why the press failed. A mode
 * that has taken money now says so, and keeps saying so while the bill sits
 * there.
 *
 * # Why "taken" is not just "disabled"
 *
 * When the bill is covered every mode is unpressable, so disabling alone would
 * grey out all four equally and lose the one fact worth keeping: **which one
 * the customer actually paid with.** That is what a cashier looks at when the
 * customer asks, and what they need before pressing *Clear payments*.
 *
 * # Exported so it can be tested without a counter
 *
 * The same reason `Totals` and `Tile` are. What went wrong here is a rule
 * about a number Rust sends, and a rule like that should be assertable without
 * standing up a shop, an order and a payment provider first.
 */
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
        {['Cash', 'Card', 'UPI'].map((label) => (
          <Button
            key={label}
            small
            className={
              mode === label ? 'mb-payment__mode mb-payment__mode--on' : 'mb-payment__mode'
            }
            aria-pressed={mode === label}
            onClick={() => onPick(label)}
          >
            {label}
          </Button>
        ))}
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

      {/* A credit sale happens mid-bill with the customer standing there, so
          the picker opens here rather than on another screen. */}
      <div id={creditId} className="mb-payment__credit" hidden={!showCredit}>
        <Button small wide variant="quiet" className="mb-payment__mode" onClick={onCredit}>
          Credit
        </Button>
      </div>
    </>
  );
}
