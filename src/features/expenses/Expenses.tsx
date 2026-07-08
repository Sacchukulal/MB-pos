import { useEffect, useState } from "react";
import { Plus, Trash2, Wallet } from "lucide-react";
import { EXPENSE_CATEGORIES } from "../../config/constants";
import { formatCurrency } from "../../utils/format";
import { useToast } from "../../hooks/useToast";
import ConfirmDialog from "../../components/ui/ConfirmDialog";
import * as expensesRepo from "../../db/repositories/expensesRepo";
import type { Expense } from "../../types";

interface ExpensesProps {
  dbReady: boolean;
}

export default function Expenses({ dbReady }: ExpensesProps) {
  const { toast } = useToast();
  const [expenses, setExpenses] = useState<Expense[]>([]);
  const [description, setDescription] = useState("");
  const [amount, setAmount] = useState("");
  const [category, setCategory] = useState(EXPENSE_CATEGORIES[0]);
  const [saving, setSaving] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Expense | null>(null);

  const fetchExpenses = async () => {
    try {
      setExpenses(await expensesRepo.listExpenses());
    } catch (error) {
      console.error("Failed to fetch expenses:", error);
    }
  };

  useEffect(() => {
    if (dbReady) fetchExpenses();
  }, [dbReady]);

  const handleAdd = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!description.trim() || !amount) return;
    try {
      setSaving(true);
      await expensesRepo.addExpense(description.trim(), parseFloat(amount), category);
      setDescription("");
      setAmount("");
      await fetchExpenses();
      toast("Expense added.", "success");
    } catch (error) {
      console.error("Failed to add expense:", error);
      toast("Failed to add expense.", "danger");
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async () => {
    if (!deleteTarget) return;
    const target = deleteTarget;
    setDeleteTarget(null);
    try {
      await expensesRepo.deleteExpense(target.id);
      await fetchExpenses();
      toast("Expense deleted.");
    } catch (error) {
      console.error("Failed to delete expense:", error);
      toast("Failed to delete expense.", "danger");
    }
  };

  const totalAmount = expenses.reduce((sum, exp) => sum + exp.amount, 0);

  return (
    <div className="page settings-page">
      <div className="page-head">
        <h1>Expense Tracker</h1>
        <p>Manage and track your daily business expenses</p>
      </div>

      <div className="section">
        <div className="section-head">
          <Plus size={14} /> Add New Expense
        </div>
        <form onSubmit={handleAdd} className="inline-add-bar">
          <input
            type="text"
            className="input"
            style={{ flex: 2, minWidth: 200 }}
            placeholder="Description (e.g. Milk, Electricity)"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            required
          />
          <input
            type="number"
            className="input"
            style={{ flex: 1, minWidth: 120 }}
            placeholder="Amount"
            value={amount}
            onChange={(e) => setAmount(e.target.value)}
            step="0.01"
            min="0"
            required
          />
          <select className="select" style={{ flex: 1, minWidth: 140 }} value={category} onChange={(e) => setCategory(e.target.value)}>
            {EXPENSE_CATEGORIES.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
          <button type="submit" className="btn btn--primary" disabled={saving}>
            {saving ? "Adding..." : "Add Expense"}
          </button>
        </form>
      </div>

      <div className="table-wrap" style={{ flex: 1, minHeight: 0 }}>
        <table className="table">
          <thead>
            <tr>
              <th>Date</th>
              <th>Description</th>
              <th>Category</th>
              <th className="num">Amount</th>
              <th style={{ width: 50 }} />
            </tr>
          </thead>
          <tbody>
            {expenses.length === 0 ? (
              <tr className="empty-row">
                <td colSpan={5}>
                  <Wallet size={40} style={{ opacity: 0.25, marginBottom: 8 }} />
                  <p>No expenses recorded yet.</p>
                </td>
              </tr>
            ) : (
              expenses.map((expense) => (
                <tr key={expense.id}>
                  <td style={{ whiteSpace: "nowrap" }}>{new Date(expense.date).toLocaleDateString()}</td>
                  <td className="strong">{expense.description}</td>
                  <td>
                    <span className="badge">{expense.category}</span>
                  </td>
                  <td className="num strong">{formatCurrency(expense.amount)}</td>
                  <td className="center">
                    <button className="row-action-btn danger" onClick={() => setDeleteTarget(expense)} title="Delete expense">
                      <Trash2 size={16} />
                    </button>
                  </td>
                </tr>
              ))
            )}
          </tbody>
          {expenses.length > 0 && (
            <tfoot>
              <tr>
                <td colSpan={3} className="text-right strong">
                  Total:
                </td>
                <td className="num text-danger strong">{formatCurrency(totalAmount)}</td>
                <td />
              </tr>
            </tfoot>
          )}
        </table>
      </div>

      {deleteTarget && (
        <ConfirmDialog
          title="Delete Expense"
          message={
            <>
              Delete the expense <strong>"{deleteTarget.description}"</strong> of {formatCurrency(deleteTarget.amount)}?
            </>
          }
          confirmLabel="Delete"
          danger
          onConfirm={handleDelete}
          onCancel={() => setDeleteTarget(null)}
        />
      )}
    </div>
  );
}
