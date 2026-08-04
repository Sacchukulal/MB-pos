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

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  ConfirmDialog,
  EmptyState,
  SearchField,
  SectionHeader,
  Spinner,
  useToast,
} from '../kit';
import { call, inApp, isUiError } from '../ipc/call';
import type { CartView } from '../ipc/generated/CartView';
import type { MenuItemView } from '../ipc/generated/MenuItemView';
import type { TableView } from '../ipc/generated/TableView';
import { useTick } from '../clock';
import { TableGrid } from './TableGrid';
import { Totals } from './Totals';

import './billing.css';

const ORDER_TYPES = ['Dine in', 'Parcel', 'Self service', 'Delivery'] as const;

export function Billing() {
  const toast = useToast();
  const [cart, setCart] = useState<CartView | null>(null);
  const [tables, setTables] = useState<readonly TableView[]>([]);
  const [menu, setMenu] = useState<readonly MenuItemView[]>([]);
  const [filter, setFilter] = useState('');
  const [locked, setLocked] = useState(false);
  const [confirmCancel, setConfirmCancel] = useState(false);
  const [busy, setBusy] = useState(false);

  // ONE shared clock (§5 rule 10). The tiles do not each own a timer; they
  // re-read the elapsed minutes the order already carries when this ticks.
  const tick = useTick();

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
  }, [report]);

  // The floor re-reads on every tick, which is how a timer that lives on the
  // ORDER reaches the screen without the screen counting anything itself.
  useEffect(() => {
    void refreshFloor();
  }, [refreshFloor, tick]);

  // --- the actions. Named, so P10 can bind keys to them. -------------------

  const addItem = useCallback(
    async (itemId: string) => {
      try {
        setCart(await call('cart_add', { itemId, qty: null, note: null }));
      } catch (cause) {
        report(cause);
      }
    },
    [report],
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
            amountPaise: cart.balance.paise,
          }),
        );
      } catch (cause) {
        report(cause);
      }
    },
    [cart, report],
  );

  const clearPayments = useCallback(async () => {
    try {
      setCart(await call('cart_clear_payments'));
    } catch (cause) {
      report(cause);
    }
  }, [report]);

  const openTable = useCallback(
    (table: TableView) => {
      if (table.state === 'free') {
        toast.show('info', `Table ${table.label} is free. Add items to start an order.`);
        return;
      }
      // P10 opens the order into the cart (budget B7). The tile is wired now so
      // the interaction exists and the next session is behaviour, not layout.
      toast.show('info', `Opening table ${table.label} arrives with the keyboard (P10).`);
    },
    [toast],
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

  if (!inApp()) {
    return (
      <EmptyState
        title="The billing screen needs the app"
        body="A browser has no engine behind it. Run Magic Bill itself."
      />
    );
  }

  return (
    <div className="mb-billing">
      <div className="mb-topbar">
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
          {locked ? '🔒 Type locked' : '🔓 Lock type'}
        </Button>

        <div className="mb-topbar__search">
          {/* P10 owns search behaviour (budget B2). It lives here now so the
              layout is not re-cut next session. */}
          <SearchField
            what="Filter tables"
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
          />
        </div>
      </div>

      <div className="mb-billing__body">
        <div className="mb-billing__floor">
          {tables.length === 0 && menu.length === 0 ? (
            <EmptyState
              title="This shop has no menu or tables yet"
              body="P13 builds the menu screens and P14 the floor. Until then, put a demo shop in to see the screen work."
              action={
                <Button variant="primary" onClick={() => void seedDemo()} disabled={busy}>
                  {busy ? <Spinner /> : 'Add a demo shop'}
                </Button>
              }
            />
          ) : (
            <TableGrid tables={tables} filter={filter} onOpen={openTable} />
          )}

          {menu.length > 0 ? (
            <div className="mb-floor__section">
              <SectionHeader
                title="Menu"
                note="P10 makes this a keyboard search. For now, press one."
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
            note={cart?.isEmpty ? 'Empty' : `${cart?.lines.length ?? 0} lines`}
          />

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
                      ✕
                    </Button>
                    <span className="mb-cartline__qty-value">{line.qty}</span>
                  </div>
                  <span className="mb-cartline__amount">{line.amount.text}</span>
                </div>
              ))
            ) : (
              <EmptyState
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
              {/* Present and disabled, saying why — a control that is silently
                  absent is a feature nobody remembers to build. */}
              <Button small disabled title="Khata needs customers, which arrive at P15">
                Khata
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
            <Button disabled={!cart || cart.isEmpty}>Kitchen ticket</Button>
            <Button variant="primary" disabled={!cart || cart.isEmpty}>
              Complete bill
            </Button>
            <Button variant="quiet" onClick={() => void newOrder()}>
              New order
            </Button>
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
    </div>
  );
}
