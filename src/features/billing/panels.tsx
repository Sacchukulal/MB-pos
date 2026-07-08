import { Banknote, ChevronDown, ChevronUp, CreditCard, ListTodo, Plus, Smartphone, Trash2 } from "lucide-react";
import { ORDER_TYPES } from "../../config/constants";
import { formatCurrency } from "../../utils/format";
import type { CartItem, Customer, OrderType, ProcessingOrder } from "../../types";

/* --------------------- Processing orders panel --------------------- */

interface ProcessingOrdersPanelProps {
  orders: ProcessingOrder[];
  activeOrderId: number | null;
  orderType: OrderType;
  orderTypeLocked: boolean;
  onSelectOrder: (order: ProcessingOrder) => void;
  onNewOrder: () => void;
  onToggleLock: () => void;
  onSelectOrderType: (type: OrderType) => void;
  lockedOrderType: OrderType;
}

export function ProcessingOrdersPanel({
  orders,
  activeOrderId,
  orderType,
  orderTypeLocked,
  onSelectOrder,
  onNewOrder,
  onToggleLock,
  onSelectOrderType,
  lockedOrderType,
}: ProcessingOrdersPanelProps) {
  return (
    <div className="po-panel">
      <div className="po-head">
        <div className="po-title-row">
          <span className="po-title">
            <ListTodo size={15} /> Processing Orders
          </span>
          <div className="po-title-actions">
            <button
              type="button"
              className={`ot-toggle ${orderTypeLocked ? "on" : ""}`}
              onClick={onToggleLock}
              title={
                orderTypeLocked
                  ? `Lock: ON — pinned to "${lockedOrderType}". New orders start here; arrow keys ← → disabled. Click to turn off.`
                  : "Lock: OFF — arrow keys ← → switch order type. Click to lock the current tab for new orders."
              }
              aria-pressed={orderTypeLocked}
              aria-label="Lock order type"
            />
            <button className="btn btn--primary btn--sm btn--pill" onClick={onNewOrder} title="Press Esc to start a new order">
              <Plus size={14} /> New <span style={{ opacity: 0.75, fontWeight: 500 }}>(Esc)</span>
            </button>
          </div>
        </div>

        <div className="seg" role="tablist" aria-label="Order type">
          {ORDER_TYPES.map((type) => (
            <button
              key={type}
              role="tab"
              aria-selected={orderType === type}
              className={`seg-btn ${orderType === type ? "active" : ""}`}
              onClick={() => onSelectOrderType(type)}
            >
              {type}
            </button>
          ))}
        </div>
      </div>

      <div className="po-list">
        {orders.length === 0 ? (
          <div className="po-empty">No active orders.</div>
        ) : (
          orders.map((order) => (
            <div
              key={order.id}
              className={`po-card ${activeOrderId === order.id ? "active" : ""}`}
              onClick={() => onSelectOrder(order)}
            >
              <div className="po-card-title">
                <span>{order.bill_number ? `Order ${order.bill_number}` : `Order #${order.id}`}</span>
                {order.order_type === "Table" && order.table_number ? (
                  <span className="po-table-badge">
                    <span className="text-blink">{String(order.table_number).replace(/([A-Za-z]+)/g, "-$1")}</span>
                  </span>
                ) : (
                  <span>{formatCurrency(order.total)}</span>
                )}
              </div>
              <div className="po-card-sub">
                <span>{order.order_type}</span>
                {order.order_type === "Table" && order.table_number && (
                  <span style={{ fontWeight: "var(--font-semibold)" }}>{formatCurrency(order.total)}</span>
                )}
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

/* ----------------------------- Cart panel -------------------------- */

interface CartPanelProps {
  cart: CartItem[];
  onQuantityChange: (id: number, quantity: number) => void;
  onQuantityBlur: (id: number) => void;
  onRemove: (id: number) => void;
}

export function CartPanel({ cart, onQuantityChange, onQuantityBlur, onRemove }: CartPanelProps) {
  if (cart.length === 0) {
    return (
      <div className="cart-panel">
        <div className="empty-cart">No items added yet.</div>
      </div>
    );
  }
  return (
    <div className="cart-panel">
      <table className="cart-table">
        <thead>
          <tr>
            <th>Item</th>
            <th className="text-right">Price</th>
            <th className="text-center">Qty</th>
            <th className="text-right">Total</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {cart.map((item) => (
            <tr key={item.id}>
              <td>
                <div className="cart-item-name" title={item.name}>
                  {item.name}
                </div>
              </td>
              <td className="text-right text-secondary">{formatCurrency(item.price)}</td>
              <td className="text-center">
                <input
                  type="number"
                  min={1}
                  className="cart-qty-input"
                  value={item.quantity || ""}
                  onChange={(e) => {
                    const val = e.target.value;
                    if (val === "") {
                      onQuantityChange(item.id, 0);
                    } else {
                      const qty = parseInt(val);
                      if (!isNaN(qty)) onQuantityChange(item.id, qty);
                    }
                  }}
                  onBlur={() => onQuantityBlur(item.id)}
                  onFocus={(e) => e.target.select()}
                />
              </td>
              <td className="text-right" style={{ fontWeight: "var(--font-semibold)" }}>
                {formatCurrency(item.price * item.quantity)}
              </td>
              <td className="text-center">
                <button className="row-action-btn danger" onClick={() => onRemove(item.id)}>
                  <Trash2 size={14} />
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/* --------------------------- Payment panel ------------------------- */

interface PaymentPanelProps {
  open: boolean;
  onToggleOpen: () => void;
  paymentMode: string;
  billingType: "Cash" | "Credit";
  onSelectPayment: (mode: "Cash" | "Card" | "UPI") => void;
  onSelectCredit: () => void;
  creditCustomers: Customer[];
  selectedCustomerId: number | null;
  onSelectCustomer: (id: number) => void;
  onAddCustomer: () => void;
}

const PAYMENT_BUTTONS = [
  { mode: "Cash" as const, icon: Banknote },
  { mode: "Card" as const, icon: CreditCard },
  { mode: "UPI" as const, icon: Smartphone },
];

export function PaymentPanel({
  open,
  onToggleOpen,
  paymentMode,
  billingType,
  onSelectPayment,
  onSelectCredit,
  creditCustomers,
  selectedCustomerId,
  onSelectCustomer,
  onAddCustomer,
}: PaymentPanelProps) {
  return (
    <div className="payment-panel">
      <button className="payment-panel-toggle" onClick={onToggleOpen}>
        <span>Payment Mode: {billingType === "Credit" ? "Credit" : paymentMode}</span>
        {open ? <ChevronUp size={18} /> : <ChevronDown size={18} />}
      </button>

      {open && (
        <>
          <div className="payment-grid">
            {PAYMENT_BUTTONS.map(({ mode, icon: Icon }) => (
              <button
                key={mode}
                className={`pay-btn ${paymentMode === mode && billingType !== "Credit" ? "active" : ""}`}
                onClick={() => onSelectPayment(mode)}
              >
                <Icon size={16} />
                <span>{mode}</span>
              </button>
            ))}
          </div>
          <div className="credit-row">
            <button
              className={`pay-btn ${billingType === "Credit" ? "active" : ""}`}
              style={{ flex: billingType === "Credit" ? "0 0 auto" : 1 }}
              onClick={onSelectCredit}
            >
              Credit Billing
            </button>
            {billingType === "Credit" && (
              <>
                <select
                  className="select"
                  style={{ flex: 1 }}
                  value={selectedCustomerId ?? ""}
                  onChange={(e) => onSelectCustomer(Number(e.target.value))}
                >
                  <option value="" disabled>
                    Select Customer
                  </option>
                  {creditCustomers.map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.name} {c.phone ? `(${c.phone})` : ""}
                    </option>
                  ))}
                </select>
                <button className="btn btn--primary" style={{ padding: "0.5rem" }} onClick={onAddCustomer} title="Add Customer">
                  <Plus size={18} />
                </button>
              </>
            )}
          </div>
        </>
      )}
    </div>
  );
}
