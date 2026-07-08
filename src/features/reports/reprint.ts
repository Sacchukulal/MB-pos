import { getFinalizedOrder, parseCart } from "../../db/repositories/ordersRepo";
import { printBill, type PrintResult } from "../../services/printing/printService";
import type { AppSettings } from "../../types";

/**
 * Reprints a finalized bill through the SAME template as the original checkout,
 * so reprints honor every bill-design setting (sections, separators, QR, logo).
 */
export async function reprintBill(settings: AppSettings, orderId: number): Promise<PrintResult> {
  const order = await getFinalizedOrder(orderId);
  if (!order) return { ok: false, error: "Order not found" };

  return printBill(settings, {
    cart: parseCart(order.cart_data),
    subtotal: order.subtotal,
    gst: order.gst,
    total: order.total,
    billNumber: order.bill_number || String(order.id),
    tokenNumber: order.token_number,
    orderType: order.order_type,
    tableNumber: order.table_number,
    customerName: order.customer_name || undefined,
    date: new Date(order.created_at),
    // Stored amounts tell us the GST mode: totals above subtotal mean exclusive.
    gstInclusive: order.gst > 0 && order.total <= order.subtotal,
  });
}
