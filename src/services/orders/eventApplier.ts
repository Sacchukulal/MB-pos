import * as ordersRepo from "../../db/repositories/ordersRepo";
import * as customersRepo from "../../db/repositories/customersRepo";
import { hasAppliedEvent, recordAppliedEvent } from "../../db/repositories/appliedEventsRepo";
import { claimOrderNumbers } from "../../db/repositories/settingsRepo";
import { computeCartTotals } from "../../utils/gst";
import { printBill, printKot } from "../../services/printing/printService";
import { requestBillSync } from "../sync/billSync";
import { isSlotOccupied } from "../../features/billing/tableUtils";
import {
  finalizedRowToWire,
  mergeCartLines,
  parseCartJson,
  subtractCart,
  wireToCart,
  type WireItem,
  type WireOrder,
} from "./wire";
import type { AppSettings, CartItem, Category, OrderType, ProcessingOrder } from "../../types";

/**
 * Applies ONE phone intent (order_event) to SQLite + the printers. The POS
 * is the authority: numbers are claimed here, totals recomputed here (never
 * trusted from the phone), and every printer setting is honoured — except
 * the KOT/bill confirmation popups, which are bypassed for mobile-originated
 * actions only (a mobile order must never wait for a click).
 *
 * Idempotent per event id via the applied_order_events ledger.
 */

export interface WireEvent {
  eventId: string;
  clientEventId: string;
  kind: string;
  orderId: string | null;
  orderClientUuid: string | null;
  payload: Record<string, unknown>;
  actorKind: string;
  actorId: string | null;
  actorName: string;
  createdAt: string;
}

export interface ApplyContext {
  settings: AppSettings;
  categories: Category[];
}

export interface ApplyResult {
  status: "applied" | "rejected";
  reason?: string;
  /** Non-empty when the order saved but a printer failed (phone shows it). */
  printError?: string;
  /** Final cloud truth for orders that no longer have a local row (billed/cancelled). */
  cloudOrder?: WireOrder;
  /** New-order alert info for the POS UI. */
  alert?: { tableNumber: string; orderType: string; waiterName: string; total: number };
}

const rejected = (reason: string): ApplyResult => ({ status: "rejected", reason });

const SUB_LETTERS = ["B", "C", "D", "E", "F", "G", "H"];

/** Resolve the table slot for a new mobile order — never silently merge. */
function resolveTableSlot(requested: string, open: ProcessingOrder[]): string | null {
  const base = requested.trim();
  if (!base) return "";
  if (!isSlotOccupied(open, base)) return base;
  for (const letter of SUB_LETTERS) {
    if (!isSlotOccupied(open, `${base}${letter}`)) return `${base}${letter}`;
  }
  return null; // B..H exhausted
}

function draftFromRow(row: ProcessingOrder, cart: CartItem[], totals: { subtotal: number; gst: number; total: number }): ordersRepo.OrderDraft {
  return {
    cart,
    customerName: row.customer_name ?? "",
    customerPhone: row.customer_phone ?? "",
    paymentMode: row.payment_mode || "Cash",
    subtotal: totals.subtotal,
    gst: totals.gst,
    total: totals.total,
    orderType: row.order_type,
    tableNumber: row.table_number ?? "",
    customerId: row.customer_id,
  };
}

async function findOrderByUuid(uuid: string | null): Promise<ProcessingOrder | null> {
  if (!uuid) return null;
  const orders = await ordersRepo.listProcessingOrders();
  return orders.find((o) => o.remote_uuid === uuid) ?? null;
}

/** printed_items with the legacy fallback: NULL means "kitchen has seen the full cart". */
const printedItemsOf = (row: ProcessingOrder, cart: CartItem[]): CartItem[] =>
  row.printed_items != null ? parseCartJson(row.printed_items) : cart;

export async function applyOrderEvent(ev: WireEvent, ctx: ApplyContext): Promise<ApplyResult> {
  // Ledger guard: already applied (ack was lost) -> ack again, change nothing.
  if (await hasAppliedEvent(ev.eventId)) return { status: "applied" };

  const { settings, categories } = ctx;
  const gstConfig = settings.bill.gst;
  const disableKot = Boolean(settings.printer.disableKot);
  const p = ev.payload ?? {};

  const printDelta = async (
    items: CartItem[],
    row: { token_number: number | null; bill_number: string | null; order_type: string; table_number: string | null },
    variant?: "cancel" | "reprint",
    cancelInfo?: { reason: string; by: string }
  ): Promise<string> => {
    if (disableKot || items.length === 0) return "";
    const result = await printKot(settings, items, categories, {
      tokenNumber: row.token_number ?? "?",
      billNumber: row.bill_number ?? "?",
      orderType: row.order_type,
      tableNumber: row.table_number ?? "",
      waiterName: ev.actorName,
      variant,
      cancelReason: cancelInfo?.reason,
      cancelledBy: cancelInfo?.by,
    });
    if (result.ok) return "";
    console.error("[orders] KOT print failed:", result.error);
    return result.error === "NO_PRINTER" ? "No printer configured at the counter" : "Printer problem at the counter";
  };

  switch (ev.kind) {
    /* ------------------------------ create ------------------------------ */
    case "create": {
      const uuid = ev.orderClientUuid ?? "";
      if (!uuid) return rejected("server");
      // A retried create whose ledger entry was lost: the row is the guard.
      if (await findOrderByUuid(uuid)) {
        await recordAppliedEvent(ev.eventId, ev.kind);
        return { status: "applied" };
      }

      const items = wireToCart((p.items as WireItem[]) ?? []);
      if (items.length === 0) return rejected("empty-cart");

      const orderType = (String(p.orderType ?? "Table") as OrderType) || "Table";
      let tableNumber = "";
      if (orderType === "Table") {
        const open = await ordersRepo.listProcessingOrders();
        const slot = resolveTableSlot(String(p.tableNumber ?? ""), open);
        if (slot === null) return rejected("table-full");
        tableNumber = slot;
      }

      const totals = computeCartTotals(items, gstConfig);
      const claimed = await claimOrderNumbers({ token: true, bill: true });
      const draft: ordersRepo.OrderDraft = {
        cart: items,
        customerName: String(p.customerName ?? ""),
        customerPhone: String(p.customerPhone ?? ""),
        paymentMode: "Cash",
        subtotal: totals.subtotal,
        gst: totals.gst,
        total: totals.total,
        orderType,
        tableNumber,
        customerId: null,
      };
      const newId = await ordersRepo.insertProcessingOrder(draft, claimed.tokenNumber, claimed.billNumber, {
        remoteUuid: uuid,
        source: "mobile",
        waiterName: ev.actorName,
      });
      if (newId == null) return rejected("server");

      const printError = await printDelta(items, {
        token_number: claimed.tokenNumber,
        bill_number: claimed.billNumber,
        order_type: orderType,
        table_number: tableNumber,
      });

      // With KOT printing disabled the kitchen never sees tickets; mark the
      // items "seen" so enabling the setting later doesn't dump old orders.
      await ordersRepo.setOrderBridgeFields(newId, {
        printedItems: printError ? [] : items,
        cloudDirty: true,
      });

      await recordAppliedEvent(ev.eventId, ev.kind);
      return {
        status: "applied",
        printError,
        alert: { tableNumber, orderType, waiterName: ev.actorName, total: totals.total },
      };
    }

    /* ---------------------------- add_items ---------------------------- */
    case "add_items": {
      const order = await findOrderByUuid(ev.orderClientUuid);
      if (!order) return rejected("order-gone");

      const deltas = wireToCart((p.items as WireItem[]) ?? []);
      if (deltas.length === 0) return rejected("empty-cart");

      const cart = parseCartJson(order.cart_data);
      const printed = printedItemsOf(order, cart);
      const merged = mergeCartLines(cart, deltas);
      const totals = computeCartTotals(merged, gstConfig);

      await ordersRepo.updateProcessingOrder(order.id, draftFromRow(order, merged, totals));

      const kotDelta = subtractCart(merged, printed);
      const printError = await printDelta(kotDelta, order);

      await ordersRepo.setOrderBridgeFields(order.id, {
        printedItems: printError ? printed : merged,
        cloudDirty: true,
      });

      await recordAppliedEvent(ev.eventId, ev.kind);
      return { status: "applied", printError };
    }

    /* ---------------------------- void_items ---------------------------- */
    case "void_items": {
      const order = await findOrderByUuid(ev.orderClientUuid);
      if (!order) return rejected("order-gone");

      const toRemove = ((p.items as { localId: number; quantity: number }[]) ?? []).map((i) => ({
        id: Number(i.localId),
        quantity: Number(i.quantity),
      }));
      const reason = String(p.reason ?? "").trim();
      if (toRemove.length === 0 || !reason) return rejected("server");

      const cart = parseCartJson(order.cart_data).map((i) => ({ ...i }));
      const printed = printedItemsOf(order, cart).map((i) => ({ ...i }));

      // Validate: every requested quantity must still be in the cart.
      for (const r of toRemove) {
        const have = cart.filter((c) => c.id === r.id).reduce((s, c) => s + c.quantity, 0);
        if (r.quantity <= 0 || have < r.quantity) return rejected("already-removed");
      }

      // Reduce cart + printed lines (plain line first, noted lines after),
      // and collect what was actually removed for the cancellation ticket.
      const removedLines: CartItem[] = [];
      const reduce = (list: CartItem[], id: number, qty: number, collect: boolean) => {
        let remaining = qty;
        const lines = list
          .filter((l) => l.id === id)
          .sort((a, b) => (a.note ? 1 : 0) - (b.note ? 1 : 0));
        for (const line of lines) {
          if (remaining <= 0) break;
          const take = Math.min(line.quantity, remaining);
          line.quantity -= take;
          remaining -= take;
          if (collect && take > 0) removedLines.push({ ...line, quantity: take });
        }
      };
      toRemove.forEach((r) => {
        reduce(cart, r.id, r.quantity, true);
        reduce(printed, r.id, r.quantity, false);
      });
      const newCart = cart.filter((i) => i.quantity > 0);
      const newPrinted = printed.filter((i) => i.quantity > 0);

      const totals = computeCartTotals(newCart, gstConfig);
      await ordersRepo.updateProcessingOrder(order.id, draftFromRow(order, newCart, totals));

      const printError = await printDelta(removedLines, order, "cancel", {
        reason,
        by: ev.actorName,
      });

      await ordersRepo.setOrderBridgeFields(order.id, {
        printedItems: newPrinted,
        cloudDirty: true,
      });

      await recordAppliedEvent(ev.eventId, ev.kind);
      return { status: "applied", printError };
    }

    /* ------------------------ set_payment / set_customer ------------------------ */
    case "set_payment":
    case "set_customer": {
      const order = await findOrderByUuid(ev.orderClientUuid);
      if (!order) return rejected("order-gone");

      const cart = parseCartJson(order.cart_data);
      const totals = computeCartTotals(cart, gstConfig);
      const draft = draftFromRow(order, cart, totals);

      if (ev.kind === "set_payment") {
        draft.paymentMode = String(p.paymentMode ?? draft.paymentMode);
        if (Number.isInteger(p.customerLocalId)) {
          const customer = await customersRepo.getCustomer(Number(p.customerLocalId));
          if (customer) {
            draft.customerId = customer.id;
            draft.customerName = customer.name;
            draft.customerPhone = customer.phone ?? "";
          }
        }
      } else {
        draft.customerName = String(p.customerName ?? "");
        draft.customerPhone = String(p.customerPhone ?? "");
        if (Number.isInteger(p.customerLocalId)) draft.customerId = Number(p.customerLocalId);
      }

      await ordersRepo.updateProcessingOrder(order.id, draft);
      await recordAppliedEvent(ev.eventId, ev.kind);
      return { status: "applied" };
    }

    /* ----------------------------- finalize ----------------------------- */
    case "finalize": {
      const order = await findOrderByUuid(ev.orderClientUuid);
      if (!order) return rejected("order-gone");

      const cart = parseCartJson(order.cart_data);
      if (cart.length === 0) return rejected("empty-cart");
      const printed = printedItemsOf(order, cart);

      // Any unprinted items reach the kitchen BEFORE the bill prints.
      const kotDelta = subtractCart(cart, printed);
      let printError = await printDelta(kotDelta, order);

      const totals = computeCartTotals(cart, gstConfig);
      const paymentMode = String(p.paymentMode ?? "Cash");
      const draft = draftFromRow(order, cart, totals);
      draft.paymentMode = paymentMode;

      if (paymentMode === "Credit") {
        const customerId = Number(p.customerLocalId);
        const customer = Number.isInteger(customerId) ? await customersRepo.getCustomer(customerId) : null;
        if (!customer) return rejected("server");
        draft.customerId = customer.id;
        draft.customerName = customer.name;
        draft.customerPhone = customer.phone ?? "";
      }

      // Mirrors the billing screen's checkout exactly: reuse the order's
      // numbers + created_at, settle credit, delete the open row, sync.
      const waiterName = order.waiter_name || ev.actorName;
      await ordersRepo.insertFinalizedOrder(
        draft,
        order.bill_number ?? "",
        order.token_number,
        order.created_at,
        { remoteUuid: order.remote_uuid ?? undefined, source: order.source ?? "mobile", waiterName }
      );
      if (draft.paymentMode === "Credit" && draft.customerId) {
        await customersRepo.addToCreditBalance(draft.customerId, totals.total);
      }
      await ordersRepo.deleteProcessingOrder(order.id);
      requestBillSync();

      const billResult = await printBill(settings, {
        cart,
        subtotal: totals.subtotal,
        gst: totals.gst,
        total: totals.total,
        billNumber: order.bill_number ?? "",
        tokenNumber: order.token_number,
        orderType: order.order_type,
        tableNumber: order.table_number ?? "",
        customerName: draft.customerName || undefined,
        cashierName: waiterName || undefined,
        date: new Date(),
        gstPercentage: gstConfig.enabled ? gstConfig.percentage : undefined,
        gstInclusive: gstConfig.enabled && gstConfig.type === "Inclusive",
      });
      if (!billResult.ok) {
        console.error("[orders] bill print failed:", billResult.error);
        printError =
          billResult.error === "NO_PRINTER"
            ? "No printer configured at the counter"
            : "Printer problem at the counter";
      }

      await recordAppliedEvent(ev.eventId, ev.kind);
      return { status: "applied", printError };
    }

    /* --------------------------- cancel_order --------------------------- */
    case "cancel_order": {
      const order = await findOrderByUuid(ev.orderClientUuid);
      if (!order) return rejected("order-gone");

      const reason = String(p.reason ?? "").trim();
      if (!reason) return rejected("server");

      const cart = parseCartJson(order.cart_data);
      const printed = printedItemsOf(order, cart);
      const printError = await printDelta(printed, order, "cancel", {
        reason,
        by: ev.actorName,
      });

      // Build the final cloud truth BEFORE the local row disappears.
      const cloudOrder: WireOrder = {
        ...finalizedRowToWire({ ...order }),
        status: "cancelled",
        paymentMode: order.payment_mode ?? "",
      };
      await ordersRepo.deleteProcessingOrder(order.id);

      await recordAppliedEvent(ev.eventId, ev.kind);
      return { status: "applied", printError, cloudOrder };
    }

    /* ---------------------------- reprint_kot ---------------------------- */
    case "reprint_kot": {
      const order = await findOrderByUuid(ev.orderClientUuid);
      if (!order) return rejected("order-gone");

      const cart = parseCartJson(order.cart_data);
      const printed = printedItemsOf(order, cart);
      if (printed.length === 0) return rejected("empty-cart");
      const printError = await printDelta(printed, order, "reprint");
      if (printError) return rejected("no-printer");

      await recordAppliedEvent(ev.eventId, ev.kind);
      return { status: "applied" };
    }

    default:
      return rejected("server");
  }
}
