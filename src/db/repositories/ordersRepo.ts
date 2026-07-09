import { getDb } from "../client";
import type { CartItem, FinalizedOrder, OrderType, ProcessingOrder } from "../../types";

export interface OrderDraft {
  cart: CartItem[];
  customerName: string;
  customerPhone: string;
  paymentMode: string;
  subtotal: number;
  gst: number;
  total: number;
  orderType: OrderType;
  tableNumber: string;
  customerId: number | null;
}

/* ------------------------ processing orders ------------------------ */

export async function listProcessingOrders(): Promise<ProcessingOrder[]> {
  return getDb().select<ProcessingOrder[]>("SELECT * FROM processing_orders ORDER BY created_at DESC");
}

export async function getProcessingOrder(id: number): Promise<ProcessingOrder | null> {
  const rows = await getDb().select<ProcessingOrder[]>("SELECT * FROM processing_orders WHERE id = $1", [id]);
  return rows[0] ?? null;
}

export async function insertProcessingOrder(
  draft: OrderDraft,
  tokenNumber: number,
  billNumber: string
): Promise<number | null> {
  const result = await getDb().execute(
    `INSERT INTO processing_orders
       (cart_data, customer_name, customer_phone, payment_mode, subtotal, gst, total,
        order_type, table_number, customer_id, token_number, bill_number, created_at)
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)`,
    [
      JSON.stringify(draft.cart), draft.customerName, draft.customerPhone, draft.paymentMode,
      draft.subtotal, draft.gst, draft.total, draft.orderType, draft.tableNumber,
      draft.customerId, tokenNumber, billNumber, new Date().toISOString(),
    ]
  );
  return result.lastInsertId ?? null;
}

export async function updateProcessingOrder(id: number, draft: OrderDraft): Promise<void> {
  await getDb().execute(
    `UPDATE processing_orders SET
       cart_data=$1, customer_name=$2, customer_phone=$3, payment_mode=$4,
       subtotal=$5, gst=$6, total=$7, order_type=$8, table_number=$9, customer_id=$10
     WHERE id=$11`,
    [
      JSON.stringify(draft.cart), draft.customerName, draft.customerPhone, draft.paymentMode,
      draft.subtotal, draft.gst, draft.total, draft.orderType, draft.tableNumber,
      draft.customerId, id,
    ]
  );
}

export async function deleteProcessingOrder(id: number): Promise<void> {
  await getDb().execute("DELETE FROM processing_orders WHERE id = $1", [id]);
}

/* ------------------------ finalized orders ------------------------- */

export async function insertFinalizedOrder(
  draft: OrderDraft,
  billNumber: string,
  tokenNumber: number | null,
  createdAt: string
): Promise<void> {
  await getDb().execute(
    `INSERT INTO finalized_orders
       (cart_data, customer_name, customer_phone, payment_mode, subtotal, gst, total,
        order_type, table_number, customer_id, bill_number, token_number, created_at)
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)`,
    [
      JSON.stringify(draft.cart), draft.customerName, draft.customerPhone, draft.paymentMode,
      draft.subtotal, draft.gst, draft.total, draft.orderType, draft.tableNumber,
      draft.customerId, billNumber, tokenNumber, createdAt,
    ]
  );
}

export async function getFinalizedOrder(id: number): Promise<FinalizedOrder | null> {
  const rows = await getDb().select<FinalizedOrder[]>("SELECT * FROM finalized_orders WHERE id = $1", [id]);
  return rows[0] ?? null;
}

/** Orders in an inclusive local-date range (YYYY-MM-DD strings). */
export async function listFinalizedOrders(startDate: string, endDate: string): Promise<FinalizedOrder[]> {
  return getDb().select<FinalizedOrder[]>(
    `SELECT * FROM finalized_orders
     WHERE datetime(created_at, 'localtime') >= $1 AND datetime(created_at, 'localtime') <= $2
     ORDER BY created_at DESC`,
    [`${startDate} 00:00:00`, `${endDate} 23:59:59`]
  );
}

export async function listFinalizedOrdersByCustomer(
  customerId: number,
  startDate: string,
  endDate: string
): Promise<FinalizedOrder[]> {
  return getDb().select<FinalizedOrder[]>(
    `SELECT * FROM finalized_orders
     WHERE customer_id = $1
       AND datetime(created_at, 'localtime') >= $2 AND datetime(created_at, 'localtime') <= $3`,
    [customerId, `${startDate} 00:00:00`, `${endDate} 23:59:59`]
  );
}

export async function listRecentFinalizedOrders(dateFilterSql: string): Promise<FinalizedOrder[]> {
  return getDb().select<FinalizedOrder[]>(
    `SELECT * FROM finalized_orders WHERE ${dateFilterSql} ORDER BY created_at DESC`
  );
}

export async function updateOrderPaymentMode(id: number, mode: string): Promise<void> {
  // synced=0 re-queues the bill so the cloud copy picks up the edit (upsert-safe).
  await getDb().execute("UPDATE finalized_orders SET payment_mode = $1, synced = 0 WHERE id = $2", [mode, id]);
}

/** Parse an order's cart JSON defensively (older rows may be corrupted). */
export function parseCart(cartData: string): CartItem[] {
  try {
    const parsed = JSON.parse(cartData);
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}
