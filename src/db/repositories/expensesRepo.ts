import { getDb } from "../client";
import type { Expense } from "../../types";

export async function listExpenses(): Promise<Expense[]> {
  return getDb().select<Expense[]>("SELECT * FROM expenses ORDER BY date DESC");
}

export async function listExpensesInRange(startDate: string, endDate: string): Promise<Expense[]> {
  return getDb().select<Expense[]>(
    `SELECT * FROM expenses
     WHERE datetime(date, 'localtime') >= $1 AND datetime(date, 'localtime') <= $2
     ORDER BY date DESC`,
    [`${startDate} 00:00:00`, `${endDate} 23:59:59`]
  );
}

export async function listExpensesWhere(dateFilterSql: string): Promise<Expense[]> {
  return getDb().select<Expense[]>(`SELECT * FROM expenses WHERE ${dateFilterSql}`);
}

export async function addExpense(description: string, amount: number, category: string): Promise<void> {
  await getDb().execute(
    "INSERT INTO expenses (description, amount, category, date) VALUES ($1, $2, $3, $4)",
    [description, amount, category, new Date().toISOString()]
  );
}

export async function deleteExpense(id: number): Promise<void> {
  await getDb().execute("DELETE FROM expenses WHERE id = $1", [id]);
}
