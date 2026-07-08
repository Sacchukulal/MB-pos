import { getDb } from "../client";
import type { Customer, CustomerPayment } from "../../types";

export async function listCustomers(): Promise<Customer[]> {
  return getDb().select<Customer[]>("SELECT * FROM customers ORDER BY name");
}

export async function getCustomer(id: number): Promise<Customer | null> {
  const rows = await getDb().select<Customer[]>("SELECT * FROM customers WHERE id = $1", [id]);
  return rows[0] ?? null;
}

export async function addCustomer(name: string, phone: string): Promise<number | null> {
  const result = await getDb().execute(
    "INSERT INTO customers (name, phone, created_at) VALUES ($1, $2, $3)",
    [name, phone, new Date().toISOString()]
  );
  return result.lastInsertId ?? null;
}

export async function renameCustomer(id: number, name: string): Promise<void> {
  await getDb().execute("UPDATE customers SET name = $1 WHERE id = $2", [name, id]);
}

/** Deletes a customer, their payments, and detaches their bills (matches existing behavior). */
export async function deleteCustomer(id: number): Promise<void> {
  const db = getDb();
  await db.execute("UPDATE finalized_orders SET customer_id = NULL WHERE customer_id = $1", [id]);
  await db.execute("DELETE FROM customer_payments WHERE customer_id = $1", [id]);
  await db.execute("DELETE FROM customers WHERE id = $1", [id]);
}

export async function addToCreditBalance(id: number, amount: number): Promise<void> {
  await getDb().execute("UPDATE customers SET credit_balance = credit_balance + $1 WHERE id = $2", [amount, id]);
}

/** Records a repayment and reduces the balance. */
export async function recordPayment(customerId: number, amount: number, mode: string): Promise<void> {
  const db = getDb();
  await db.execute("UPDATE customers SET credit_balance = credit_balance - $1 WHERE id = $2", [amount, customerId]);
  await db.execute(
    "INSERT INTO customer_payments (customer_id, amount, payment_mode, date) VALUES ($1, $2, $3, $4)",
    [customerId, amount, mode, new Date().toISOString()]
  );
}

/** Clears the full outstanding balance, recording it as a settlement payment. */
export async function settleAllDue(customerId: number, amount: number): Promise<void> {
  const db = getDb();
  await db.execute("UPDATE customers SET credit_balance = 0 WHERE id = $1", [customerId]);
  await db.execute(
    "INSERT INTO customer_payments (customer_id, amount, payment_mode, date) VALUES ($1, $2, $3, $4)",
    [customerId, amount, "Full Settlement", new Date().toISOString()]
  );
}

export async function listPayments(customerId: number, startDate: string, endDate: string): Promise<CustomerPayment[]> {
  return getDb().select<CustomerPayment[]>(
    `SELECT * FROM customer_payments
     WHERE customer_id = $1
       AND datetime(date, 'localtime') >= $2 AND datetime(date, 'localtime') <= $3`,
    [customerId, `${startDate} 00:00:00`, `${endDate} 23:59:59`]
  );
}
