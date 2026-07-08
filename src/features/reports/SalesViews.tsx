import { Award, Edit2, Printer, ShoppingBag, Wallet } from "lucide-react";
import StatCard from "../../components/ui/StatCard";
import { displayDate, displayDateTime, formatCurrency, formatPercent } from "../../utils/format";
import { taxableValue } from "../../utils/gst";
import { PAYMENT_MODES } from "../../config/constants";
import type { Expense, FinalizedOrder } from "../../types";
import type { ExpenseStats, ItemSalesFilter, ItemSalesRow, SalesStats } from "./salesStats";

/* --------------------------- Sales Summary ------------------------- */

export function SalesSummaryView({ stats, expenseStats }: { stats: SalesStats; expenseStats: ExpenseStats }) {
  return (
    <>
      <div className="stat-grid">
        <StatCard label="Total Revenue" value={formatCurrency(stats.totalRevenue)} sub={`${stats.totalOrders} bills`} />
        <StatCard label="Gross Sales" value={formatCurrency(stats.grossSales)} sub="Before tax" />
        <StatCard label="GST Collected" value={formatCurrency(stats.totalGst)} sub="Output tax" />
        <StatCard label="Avg. Bill Value" value={formatCurrency(stats.avgBill)} sub="Per order" />
        <StatCard label="Total Orders" value={stats.totalOrders.toLocaleString("en-IN")} sub={`${stats.totalItemsSold} items`} />
        <StatCard label="Total Expenses" value={formatCurrency(stats.totalExpenses)} sub={`${expenseStats.count} entries`} />
        <StatCard label="Net Profit" value={formatCurrency(stats.netProfit)} sub="Revenue − expenses" />
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "var(--space-4)" }}>
        <div className="section">
          <div className="section-head">
            <Wallet size={15} /> Payment Methods
          </div>
          <table className="table">
            <thead>
              <tr>
                <th>Mode</th>
                <th className="num">Bills</th>
                <th className="num">Share</th>
                <th className="num">Amount</th>
              </tr>
            </thead>
            <tbody>
              {Object.entries(stats.paymentBreakdown).map(([mode, data]) => (
                <tr key={mode}>
                  <td className="strong">{mode}</td>
                  <td className="num">{data.count}</td>
                  <td className="num">{formatPercent(data.amount, stats.totalRevenue)}</td>
                  <td className="num strong">{formatCurrency(data.amount)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        <div className="section">
          <div className="section-head">
            <ShoppingBag size={15} /> Order Types
          </div>
          <table className="table">
            <thead>
              <tr>
                <th>Type</th>
                <th className="num">Orders</th>
                <th className="num">Share</th>
                <th className="num">Amount</th>
              </tr>
            </thead>
            <tbody>
              {Object.entries(stats.typeBreakdown).map(([type, data]) => (
                <tr key={type}>
                  <td className="strong">{type}</td>
                  <td className="num">{data.count}</td>
                  <td className="num">{formatPercent(data.amount, stats.totalRevenue)}</td>
                  <td className="num strong">{formatCurrency(data.amount)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      <div className="section">
        <div className="section-head">
          <Award size={15} /> Top Selling Items
        </div>
        <table className="table">
          <thead>
            <tr>
              <th style={{ width: 48 }}>#</th>
              <th>Item</th>
              <th className="num">Qty Sold</th>
              <th className="num">Revenue</th>
              <th className="num">% of Sales</th>
            </tr>
          </thead>
          <tbody>
            {stats.topItems.length === 0 ? (
              <tr className="empty-row">
                <td colSpan={5}>No sales recorded for this period.</td>
              </tr>
            ) : (
              stats.topItems.map((item, idx) => (
                <tr key={item.name}>
                  <td style={{ color: "var(--text-tertiary)" }}>{idx + 1}</td>
                  <td className="strong">{item.name}</td>
                  <td className="num">{item.qty}</td>
                  <td className="num strong">{formatCurrency(item.total)}</td>
                  <td className="num">{formatPercent(item.total, stats.grossSales)}</td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>
    </>
  );
}

/* ---------------------------- Item Sales --------------------------- */

interface ItemSalesViewProps {
  rows: ItemSalesRow[];
  filter: ItemSalesFilter;
  onFilterChange: (filter: ItemSalesFilter) => void;
  itemNames: string[];
  categoryNames: string[];
}

export function ItemSalesView({ rows, filter, onFilterChange, itemNames, categoryNames }: ItemSalesViewProps) {
  const totalQty = rows.reduce((s, r) => s + r.qty, 0);
  const totalRev = rows.reduce((s, r) => s + r.total, 0);
  return (
    <>
      <div className="rep-filters">
        <div className="field">
          <label>Item</label>
          <select className="select" value={filter.item} onChange={(e) => onFilterChange({ ...filter, item: e.target.value })}>
            <option>All Items</option>
            {itemNames.map((name) => (
              <option key={name}>{name}</option>
            ))}
          </select>
        </div>
        <div className="field">
          <label>Category</label>
          <select className="select" value={filter.category} onChange={(e) => onFilterChange({ ...filter, category: e.target.value })}>
            <option>All Categories</option>
            {categoryNames.map((name) => (
              <option key={name}>{name}</option>
            ))}
          </select>
        </div>
        <div className="field" style={{ flex: 2 }}>
          <label>Search</label>
          <input
            type="text"
            className="input"
            placeholder="Search items…"
            value={filter.search}
            onChange={(e) => onFilterChange({ ...filter, search: e.target.value })}
          />
        </div>
        <button className="btn btn--ghost" onClick={() => onFilterChange({ item: "All Items", category: "All Categories", search: "" })}>
          Reset
        </button>
      </div>

      <div className="table-wrap">
        <table className="table">
          <thead>
            <tr>
              <th style={{ width: 48 }}>#</th>
              <th>Item Name</th>
              <th>Category</th>
              <th className="num">Qty Sold</th>
              <th className="num">Total Revenue</th>
              <th className="num">% Share</th>
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 ? (
              <tr className="empty-row">
                <td colSpan={6}>No items found matching the filters.</td>
              </tr>
            ) : (
              rows.map((row, idx) => (
                <tr key={row.name}>
                  <td style={{ color: "var(--text-tertiary)" }}>{idx + 1}</td>
                  <td className="strong">{row.name}</td>
                  <td className="text-secondary">{row.category}</td>
                  <td className="num">{row.qty}</td>
                  <td className="num strong">{formatCurrency(row.total)}</td>
                  <td className="num">{formatPercent(row.total, totalRev)}</td>
                </tr>
              ))
            )}
          </tbody>
          {rows.length > 0 && (
            <tfoot>
              <tr>
                <td colSpan={3} className="strong">Total ({rows.length} items)</td>
                <td className="num strong">{totalQty}</td>
                <td className="num strong">{formatCurrency(totalRev)}</td>
                <td className="num">100%</td>
              </tr>
            </tfoot>
          )}
        </table>
      </div>
    </>
  );
}

/* --------------------------- Recent Bills -------------------------- */

interface RecentBillsViewProps {
  orders: FinalizedOrder[];
  editingPaymentModeId: number | null;
  newPaymentMode: string;
  onStartEditPayment: (order: FinalizedOrder) => void;
  onChangePaymentMode: (mode: string) => void;
  onSavePaymentMode: (orderId: number) => void;
  onCancelEditPayment: () => void;
  onReprint: (orderId: number) => void;
}

export function RecentBillsView({
  orders,
  editingPaymentModeId,
  newPaymentMode,
  onStartEditPayment,
  onChangePaymentMode,
  onSavePaymentMode,
  onCancelEditPayment,
  onReprint,
}: RecentBillsViewProps) {
  return (
    <div className="table-wrap">
      <table className="table">
        <thead>
          <tr>
            <th>Bill No</th>
            <th>Date / Time</th>
            <th>Customer</th>
            <th>Type</th>
            <th>Payment Mode</th>
            <th className="num">Total</th>
            <th className="center">Action</th>
          </tr>
        </thead>
        <tbody>
          {orders.length === 0 ? (
            <tr className="empty-row">
              <td colSpan={7}>No bills found for this period.</td>
            </tr>
          ) : (
            orders.map((o) => (
              <tr key={o.id}>
                <td className="strong" style={{ color: "var(--accent)" }}>{o.bill_number || `#${o.id}`}</td>
                <td style={{ whiteSpace: "nowrap" }}>{displayDateTime(o.created_at)}</td>
                <td className="strong">{o.customer_name || "Guest"}</td>
                <td className="text-secondary">{o.order_type}</td>
                <td>
                  {editingPaymentModeId === o.id ? (
                    <div className="inline-edit">
                      <select className="select" style={{ width: "auto", padding: "0.35rem 0.5rem" }} value={newPaymentMode} onChange={(e) => onChangePaymentMode(e.target.value)}>
                        {PAYMENT_MODES.map((m) => (
                          <option key={m} value={m}>
                            {m}
                          </option>
                        ))}
                      </select>
                      <button className="btn btn--primary btn--sm" onClick={() => onSavePaymentMode(o.id)}>
                        Save
                      </button>
                      <button className="btn btn--ghost btn--sm" onClick={onCancelEditPayment}>
                        Cancel
                      </button>
                    </div>
                  ) : (
                    <span className="inline-edit">
                      <span className="badge badge--info">{o.payment_mode || "Cash"}</span>
                      <button className="row-action-btn" title="Edit Payment Mode" onClick={() => onStartEditPayment(o)}>
                        <Edit2 size={13} />
                      </button>
                    </span>
                  )}
                </td>
                <td className="num strong">{formatCurrency(o.total)}</td>
                <td className="center">
                  <button className="btn btn--ghost btn--sm" onClick={() => onReprint(o.id)}>
                    <Printer size={14} /> Reprint
                  </button>
                </td>
              </tr>
            ))
          )}
        </tbody>
      </table>
    </div>
  );
}

/* --------------------------- Day-wise Sales ------------------------ */

export function DayWiseView({ stats }: { stats: SalesStats }) {
  return (
    <>
      <div className="stat-grid">
        <StatCard label="Active Days" value={stats.activeDays.toLocaleString("en-IN")} sub="With sales" />
        <StatCard label="Avg / Day" value={formatCurrency(stats.activeDays ? stats.totalRevenue / stats.activeDays : 0)} sub="Revenue" />
        <StatCard
          label="Best Day"
          value={stats.bestDay ? formatCurrency(stats.bestDay.total) : "—"}
          sub={stats.bestDay ? new Date(stats.bestDay.date).toLocaleDateString("en-GB", { day: "2-digit", month: "short" }) : "No data"}
        />
        <StatCard label="Total Revenue" value={formatCurrency(stats.totalRevenue)} sub={`${stats.totalOrders} bills`} />
      </div>
      <div className="table-wrap">
        <table className="table">
          <thead>
            <tr>
              <th>Date</th>
              <th className="num">Bills</th>
              <th className="num">Gross Sales</th>
              <th className="num">GST</th>
              <th className="num">Net Revenue</th>
              <th className="num">Avg Bill</th>
            </tr>
          </thead>
          <tbody>
            {stats.daySeries.length === 0 ? (
              <tr className="empty-row">
                <td colSpan={6}>No sales recorded for this period.</td>
              </tr>
            ) : (
              stats.daySeries.map((d) => (
                <tr key={d.date}>
                  <td className="strong" style={{ whiteSpace: "nowrap" }}>
                    {new Date(d.date).toLocaleDateString("en-GB", { weekday: "short", day: "2-digit", month: "short", year: "numeric" })}
                  </td>
                  <td className="num">{d.orders}</td>
                  <td className="num">{formatCurrency(d.gross)}</td>
                  <td className="num">{formatCurrency(d.gst)}</td>
                  <td className="num strong">{formatCurrency(d.total)}</td>
                  <td className="num">{formatCurrency(d.orders ? d.total / d.orders : 0)}</td>
                </tr>
              ))
            )}
          </tbody>
          {stats.daySeries.length > 0 && (
            <tfoot>
              <tr>
                <td className="strong">Total</td>
                <td className="num strong">{stats.totalOrders}</td>
                <td className="num strong">{formatCurrency(stats.grossSales)}</td>
                <td className="num strong">{formatCurrency(stats.totalGst)}</td>
                <td className="num strong">{formatCurrency(stats.totalRevenue)}</td>
                <td className="num">{formatCurrency(stats.avgBill)}</td>
              </tr>
            </tfoot>
          )}
        </table>
      </div>
    </>
  );
}

/* ---------------------------- Tax Report --------------------------- */

export function TaxReportView({ stats, orders }: { stats: SalesStats; orders: FinalizedOrder[] }) {
  return (
    <>
      <div className="stat-grid">
        <StatCard label="Taxable Value" value={formatCurrency(stats.taxableTotal)} sub="Net of tax" />
        <StatCard label="CGST" value={formatCurrency(stats.totalGst / 2)} sub="Central GST" />
        <StatCard label="SGST" value={formatCurrency(stats.totalGst / 2)} sub="State GST" />
        <StatCard label="Total GST" value={formatCurrency(stats.totalGst)} sub="Output tax" />
        <StatCard label="Gross Total" value={formatCurrency(stats.totalRevenue)} sub="Incl. tax" />
      </div>
      <div className="table-wrap">
        <table className="table">
          <thead>
            <tr>
              <th>Bill No</th>
              <th>Date</th>
              <th>Payment</th>
              <th className="num">Taxable Value</th>
              <th className="num">CGST</th>
              <th className="num">SGST</th>
              <th className="num">Total</th>
            </tr>
          </thead>
          <tbody>
            {orders.length === 0 ? (
              <tr className="empty-row">
                <td colSpan={7}>No taxable bills for this period.</td>
              </tr>
            ) : (
              orders.map((o) => (
                <tr key={o.id}>
                  <td className="strong" style={{ color: "var(--accent)" }}>{o.bill_number || `#${o.id}`}</td>
                  <td style={{ whiteSpace: "nowrap" }}>{displayDate(o.created_at)}</td>
                  <td className="text-secondary">{o.payment_mode || "Cash"}</td>
                  <td className="num">{formatCurrency(taxableValue(o))}</td>
                  <td className="num">{formatCurrency(o.gst / 2)}</td>
                  <td className="num">{formatCurrency(o.gst / 2)}</td>
                  <td className="num strong">{formatCurrency(o.total)}</td>
                </tr>
              ))
            )}
          </tbody>
          {orders.length > 0 && (
            <tfoot>
              <tr>
                <td colSpan={3} className="strong">Total</td>
                <td className="num strong">{formatCurrency(stats.taxableTotal)}</td>
                <td className="num strong">{formatCurrency(stats.totalGst / 2)}</td>
                <td className="num strong">{formatCurrency(stats.totalGst / 2)}</td>
                <td className="num strong">{formatCurrency(stats.totalRevenue)}</td>
              </tr>
            </tfoot>
          )}
        </table>
      </div>
    </>
  );
}

/* --------------------------- Expenses view ------------------------- */

export function ExpensesView({ stats, expenseStats, expenses }: { stats: SalesStats; expenseStats: ExpenseStats; expenses: Expense[] }) {
  return (
    <>
      <div className="stat-grid">
        <StatCard label="Total Expenses" value={formatCurrency(expenseStats.total)} sub={`${expenseStats.count} entries`} />
        <StatCard label="Categories" value={expenseStats.cats.length.toLocaleString("en-IN")} sub="Heads" />
        <StatCard label="Avg / Entry" value={formatCurrency(expenseStats.avg)} sub="Per expense" />
        <StatCard label="Net Profit" value={formatCurrency(stats.netProfit)} sub="After expenses" />
      </div>

      {expenseStats.cats.length > 0 && (
        <div className="section">
          <div className="section-head">
            <Wallet size={15} /> Expenses by Category
          </div>
          <table className="table">
            <thead>
              <tr>
                <th>Category</th>
                <th className="num">Entries</th>
                <th className="num">Share</th>
                <th className="num">Amount</th>
              </tr>
            </thead>
            <tbody>
              {expenseStats.cats.map((c) => (
                <tr key={c.name}>
                  <td className="strong">{c.name}</td>
                  <td className="num">{c.count}</td>
                  <td className="num">{formatPercent(c.amount, expenseStats.total)}</td>
                  <td className="num strong">{formatCurrency(c.amount)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      <div className="table-wrap">
        <table className="table">
          <thead>
            <tr>
              <th>Date</th>
              <th>Description</th>
              <th>Category</th>
              <th className="num">Amount</th>
            </tr>
          </thead>
          <tbody>
            {expenses.length === 0 ? (
              <tr className="empty-row">
                <td colSpan={4}>No expenses recorded for this period.</td>
              </tr>
            ) : (
              expenses.map((e) => (
                <tr key={e.id}>
                  <td style={{ whiteSpace: "nowrap" }}>{displayDate(e.date)}</td>
                  <td className="strong">{e.description || "—"}</td>
                  <td>
                    <span className="badge badge--warning">{e.category || "Uncategorized"}</span>
                  </td>
                  <td className="num strong">{formatCurrency(e.amount)}</td>
                </tr>
              ))
            )}
          </tbody>
          {expenses.length > 0 && (
            <tfoot>
              <tr>
                <td colSpan={3} className="strong">Total ({expenseStats.count})</td>
                <td className="num strong">{formatCurrency(expenseStats.total)}</td>
              </tr>
            </tfoot>
          )}
        </table>
      </div>
    </>
  );
}
