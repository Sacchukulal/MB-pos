import type { BillDesign, CartItem } from "../types";

export interface CartTotals {
  subtotal: number;
  gst: number;
  total: number;
}

/**
 * The single GST computation used by the cart, checkout, and previews.
 * Exclusive: GST is added on top. Inclusive: GST is carved out of the price.
 */
export function computeCartTotals(cart: CartItem[], gst: BillDesign["gst"]): CartTotals {
  const subtotal = cart.reduce((sum, item) => sum + item.price * item.quantity, 0);
  if (!gst.enabled) return { subtotal, gst: 0, total: subtotal };

  const rate = gst.percentage / 100;
  if (gst.type === "Exclusive") {
    const tax = subtotal * rate;
    return { subtotal, gst: tax, total: subtotal + tax };
  }
  return { subtotal, gst: subtotal - subtotal / (1 + rate), total: subtotal };
}

/**
 * Taxable value of a stored order — correct for both GST types.
 * With inclusive GST the stored subtotal already contains the tax.
 */
export function taxableValue(order: { subtotal: number; gst: number; total: number }): number {
  // total == subtotal → inclusive (tax inside); total > subtotal → exclusive.
  return order.total > order.subtotal ? order.subtotal : order.subtotal - order.gst;
}
