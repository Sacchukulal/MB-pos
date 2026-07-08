import { useCallback, useEffect, useState } from "react";
import { Calendar as CalendarIcon, CheckCircle2, Edit2, Eye, Printer, PlusCircle, Trash2, X } from "lucide-react";
import StatCard from "../../components/ui/StatCard";
import ConfirmDialog from "../../components/ui/ConfirmDialog";
import { useToast } from "../../hooks/useToast";
import { displayDateTime, formatCurrency } from "../../utils/format";
import * as customersRepo from "../../db/repositories/customersRepo";
import * as ordersRepo from "../../db/repositories/ordersRepo";
import { customerStatementReport } from "../../services/printing/templates/reports";
import { printReport } from "../../services/printing/printService";
import { reprintBill } from "./reprint";
import type { AppSettings, Customer } from "../../types";

interface Transaction {
  type: "bill" | "payment";
  id: number;
  date: string;
  amount: number;
  mode: string;
  details: string;
}

interface CreditCustomersProps {
  settings: AppSettings | null;
  customers: Customer[];
  onCustomersChanged: () => void;
  selectedCustomer: Customer | null;
  onSelectCustomer: (customer: Customer | null) => void;
  dateRange: { start: string; end: string };
  onDateRangeChange: (range: { start: string; end: string }) => void;
  /** Exposes the current transactions so the parent can export them to CSV. */
  onTransactionsChanged: (transactions: Transaction[]) => void;
}

export default function CreditCustomers({
  settings,
  customers,
  onCustomersChanged,
  selectedCustomer,
  onSelectCustomer,
  dateRange,
  onDateRangeChange,
  onTransactionsChanged,
}: CreditCustomersProps) {
  const { toast } = useToast();
  const [transactions, setTransactions] = useState<Transaction[]>([]);
  const [editingCustomer, setEditingCustomer] = useState<{ id: number; name: string } | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Customer | null>(null);
  const [paymentAmount, setPaymentAmount] = useState("");
  const [paymentMode, setPaymentMode] = useState("Cash");

  const fetchTransactions = useCallback(
    async (customerId: number) => {
      try {
        const [bills, payments] = await Promise.all([
          ordersRepo.listFinalizedOrdersByCustomer(customerId, dateRange.start, dateRange.end),
          customersRepo.listPayments(customerId, dateRange.start, dateRange.end),
        ]);
        const merged: Transaction[] = [
          ...bills.map((b) => ({
            type: "bill" as const,
            id: b.id,
            date: b.created_at,
            amount: b.total,
            mode: b.payment_mode,
            details: `Bill #${b.id}`,
          })),
          ...payments.map((p) => ({
            type: "payment" as const,
            id: p.id,
            date: p.date,
            amount: p.amount,
            mode: p.payment_mode,
            details: "Payment Recorded",
          })),
        ];
        merged.sort((a, b) => new Date(b.date).getTime() - new Date(a.date).getTime());
        setTransactions(merged);
        onTransactionsChanged(merged);
      } catch (error) {
        console.error("Failed to fetch customer transactions:", error);
      }
    },
    [dateRange, onTransactionsChanged]
  );

  useEffect(() => {
    if (selectedCustomer && dateRange.start && dateRange.end) {
      fetchTransactions(selectedCustomer.id);
    }
  }, [selectedCustomer, dateRange, fetchTransactions]);

  /* ------------------------------ actions ---------------------------- */

  const handleRecordPayment = async () => {
    if (!selectedCustomer || !paymentAmount) return;
    const amount = parseFloat(paymentAmount);
    if (isNaN(amount) || amount <= 0) {
      toast("Invalid payment amount.", "warning");
      return;
    }
    try {
      await customersRepo.recordPayment(selectedCustomer.id, amount, paymentMode);
      setPaymentAmount("");
      toast("Payment recorded successfully.", "success");
      const updated = await customersRepo.getCustomer(selectedCustomer.id);
      if (updated) onSelectCustomer(updated);
      onCustomersChanged();
      fetchTransactions(selectedCustomer.id);
    } catch (error) {
      console.error("Failed to record payment:", error);
      toast("Failed to record payment.", "danger");
    }
  };

  const handleSettleAll = async () => {
    if (!selectedCustomer || selectedCustomer.credit_balance <= 0) return;
    try {
      await customersRepo.settleAllDue(selectedCustomer.id, selectedCustomer.credit_balance);
      toast("Customer due settled successfully.", "success");
      onSelectCustomer({ ...selectedCustomer, credit_balance: 0 });
      onCustomersChanged();
      fetchTransactions(selectedCustomer.id);
    } catch (error) {
      console.error("Failed to settle due:", error);
      toast("Failed to settle due.", "danger");
    }
  };

  const handleRename = async () => {
    if (!editingCustomer) return;
    try {
      await customersRepo.renameCustomer(editingCustomer.id, editingCustomer.name);
      toast("Customer name updated.", "success");
      if (selectedCustomer?.id === editingCustomer.id) {
        onSelectCustomer({ ...selectedCustomer, name: editingCustomer.name });
      }
      setEditingCustomer(null);
      onCustomersChanged();
    } catch (error) {
      console.error("Failed to update name:", error);
      toast("Failed to update name.", "danger");
    }
  };

  const handleDelete = async () => {
    if (!deleteTarget) return;
    const target = deleteTarget;
    setDeleteTarget(null);
    try {
      await customersRepo.deleteCustomer(target.id);
      toast("Customer deleted.");
      if (selectedCustomer?.id === target.id) onSelectCustomer(null);
      onCustomersChanged();
    } catch (error) {
      console.error("Failed to delete customer:", error);
      toast("Failed to delete customer.", "danger");
    }
  };

  const handlePrintStatement = async () => {
    if (!selectedCustomer || !settings) return;
    const text = customerStatementReport(
      settings,
      { name: selectedCustomer.name, phone: selectedCustomer.phone, balance: selectedCustomer.credit_balance },
      `${dateRange.start} to ${dateRange.end}`,
      transactions
    );
    const result = await printReport(settings, text);
    toast(result.ok ? "Statement printed!" : result.error === "NO_PRINTER" ? "No default printer set!" : "Print failed.", result.ok ? "success" : "danger");
  };

  const handleReprint = async (orderId: number) => {
    if (!settings) return;
    const result = await reprintBill(settings, orderId);
    toast(result.ok ? "Print successful!" : result.error === "NO_PRINTER" ? "No default printer set!" : "Print failed.", result.ok ? "success" : "danger");
  };

  /* ------------------------------ render ------------------------------ */

  const totalDue = customers.reduce((s, c) => s + (c.credit_balance > 0 ? c.credit_balance : 0), 0);
  const withDue = customers.filter((c) => c.credit_balance > 0).length;

  return (
    <>
      <div className="stat-grid">
        <StatCard label="Total Customers" value={customers.length.toLocaleString("en-IN")} sub="On record" />
        <StatCard label="Customers With Due" value={withDue.toLocaleString("en-IN")} sub="Pending credit" />
        <StatCard label="Total Outstanding" value={formatCurrency(totalDue)} sub="Receivable" />
      </div>

      <div className="table-wrap">
        <table className="table">
          <thead>
            <tr>
              <th>Customer Name</th>
              <th>Phone</th>
              <th className="num">Balance</th>
              <th className="num">Actions</th>
            </tr>
          </thead>
          <tbody>
            {customers.length === 0 ? (
              <tr className="empty-row">
                <td colSpan={4}>No customers found.</td>
              </tr>
            ) : (
              customers.map((c) => (
                <tr key={c.id}>
                  <td>
                    {editingCustomer?.id === c.id ? (
                      <div className="inline-edit">
                        <input
                          type="text"
                          className="input"
                          style={{ padding: "0.35rem 0.5rem", width: "auto" }}
                          value={editingCustomer.name}
                          onChange={(e) => setEditingCustomer({ ...editingCustomer, name: e.target.value })}
                        />
                        <button className="btn btn--primary btn--sm" onClick={handleRename}>
                          Save
                        </button>
                        <button className="btn btn--ghost btn--sm" onClick={() => setEditingCustomer(null)}>
                          Cancel
                        </button>
                      </div>
                    ) : (
                      <button className="rep-link" onClick={() => onSelectCustomer(c)}>
                        {c.name}
                      </button>
                    )}
                  </td>
                  <td className="text-secondary">{c.phone || "—"}</td>
                  <td className={`num ${c.credit_balance > 0 ? "amt-due" : "amt-clear"}`}>{formatCurrency(c.credit_balance)}</td>
                  <td>
                    <div className="data-row-actions">
                      <button className="btn btn--primary btn--sm" onClick={() => onSelectCustomer(c)}>
                        <Eye size={14} /> View
                      </button>
                      <button className="btn btn--ghost btn--sm" onClick={() => setEditingCustomer({ id: c.id, name: c.name })}>
                        <Edit2 size={14} /> Edit
                      </button>
                      <button className="btn btn--danger btn--sm" onClick={() => setDeleteTarget(c)} title="Delete customer">
                        <Trash2 size={14} />
                      </button>
                    </div>
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>

      {/* Customer detail modal */}
      {selectedCustomer && (
        <div className="modal-overlay modal-overlay--heavy" onClick={() => onSelectCustomer(null)}>
          <div className="rep-modal-card" onClick={(e) => e.stopPropagation()}>
            <div className="rep-modal-head">
              <div>
                <h2 className="rep-modal-title">{selectedCustomer.name}</h2>
                <p className="rep-modal-sub">{selectedCustomer.phone || "No phone"}</p>
              </div>
              <button className="icon-btn" onClick={() => onSelectCustomer(null)}>
                <X size={20} />
              </button>
            </div>

            <div className="rep-modal-body">
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "var(--space-5)", alignItems: "center" }}>
                <StatCard label="Pending Balance" value={formatCurrency(selectedCustomer.credit_balance)} />
                <div style={{ display: "flex", gap: "var(--space-2)", justifyContent: "flex-end", flexWrap: "wrap" }}>
                  <button className="btn btn--ghost" onClick={handlePrintStatement}>
                    <Printer size={16} /> Print Statement
                  </button>
                  <button className="btn btn--primary" onClick={handleSettleAll} disabled={selectedCustomer.credit_balance <= 0}>
                    <CheckCircle2 size={16} /> Settle All Due
                  </button>
                </div>
              </div>

              <div className="rep-filters">
                <div className="field" style={{ flex: 2 }}>
                  <label>Record Payment</label>
                  <input type="number" className="input" placeholder="Amount" value={paymentAmount} onChange={(e) => setPaymentAmount(e.target.value)} />
                </div>
                <div className="field">
                  <label>Mode</label>
                  <select className="select" value={paymentMode} onChange={(e) => setPaymentMode(e.target.value)}>
                    <option>Cash</option>
                    <option>UPI</option>
                    <option>Card</option>
                  </select>
                </div>
                <button className="btn btn--primary" onClick={handleRecordPayment}>
                  <PlusCircle size={18} /> Record
                </button>
              </div>

              <div>
                <div className="rep-section-head">
                  <h4>Transaction History</h4>
                  <div className="date-range">
                    <CalendarIcon size={14} style={{ color: "var(--text-tertiary)" }} />
                    <input
                      type="date"
                      className="input"
                      value={dateRange.start}
                      onChange={(e) => onDateRangeChange({ ...dateRange, start: e.target.value })}
                    />
                    <span className="date-range-sep">to</span>
                    <input
                      type="date"
                      className="input"
                      value={dateRange.end}
                      onChange={(e) => onDateRangeChange({ ...dateRange, end: e.target.value })}
                    />
                  </div>
                </div>
                <div className="table-wrap">
                  <table className="table">
                    <thead>
                      <tr>
                        <th>Date</th>
                        <th>Details</th>
                        <th className="num">Amount</th>
                        <th>Mode</th>
                        <th className="center">Action</th>
                      </tr>
                    </thead>
                    <tbody>
                      {transactions.length === 0 ? (
                        <tr className="empty-row">
                          <td colSpan={5}>No history found.</td>
                        </tr>
                      ) : (
                        transactions.map((t) => (
                          <tr key={`${t.type}-${t.id}`}>
                            <td style={{ whiteSpace: "nowrap" }}>{displayDateTime(t.date)}</td>
                            <td>
                              <span className="inline-edit">
                                <span className={`badge ${t.type === "bill" ? "badge--danger" : "badge--success"}`} style={{ textTransform: "uppercase" }}>
                                  {t.type}
                                </span>
                                {t.details}
                              </span>
                            </td>
                            <td className="num strong">{formatCurrency(t.amount)}</td>
                            <td>{t.mode}</td>
                            <td className="center">
                              {t.type === "bill" && (
                                <button className="row-action-btn" title="Reprint Bill" onClick={() => handleReprint(t.id)}>
                                  <Printer size={14} />
                                </button>
                              )}
                            </td>
                          </tr>
                        ))
                      )}
                    </tbody>
                  </table>
                </div>
              </div>
            </div>
          </div>
        </div>
      )}

      {deleteTarget && (
        <ConfirmDialog
          title="Delete Customer"
          danger
          message={
            <>
              Are you sure you want to delete <strong>{deleteTarget.name}</strong>? This will also remove their
              association from all bills and payments.
            </>
          }
          confirmLabel="Delete"
          onConfirm={handleDelete}
          onCancel={() => setDeleteTarget(null)}
        />
      )}
    </>
  );
}
