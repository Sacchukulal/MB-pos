import { parseCart } from "../../db/repositories/ordersRepo";
import { taxableValue } from "../../utils/gst";
import type { Expense, FinalizedOrder } from "../../types";

export interface Breakdown {
  amount: number;
  count: number;
}

export interface DayRow {
  date: string;
  orders: number;
  gross: number;
  gst: number;
  total: number;
}

export interface SalesStats {
  totalRevenue: number;
  grossSales: number;
  /** GST-correct taxable value (net of tax for inclusive-GST bills). */
  taxableTotal: number;
  totalGst: number;
  totalExpenses: number;
  totalOrders: number;
  avgBill: number;
  netProfit: number;
  totalItemsSold: number;
  paymentBreakdown: Record<string, Breakdown>;
  typeBreakdown: Record<string, Breakdown>;
  topItems: { name: string; qty: number; total: number }[];
  daySeries: DayRow[];
  bestDay: { date: string; total: number } | null;
  activeDays: number;
}

export function computeSalesStats(orders: FinalizedOrder[], expenses: Expense[]): SalesStats {
  let totalRevenue = 0;
  let grossSales = 0;
  let taxableTotal = 0;
  let totalGst = 0;
  let totalItemsSold = 0;

  const paymentBreakdown: Record<string, Breakdown> = {
    Cash: { amount: 0, count: 0 },
    Card: { amount: 0, count: 0 },
    UPI: { amount: 0, count: 0 },
    Credit: { amount: 0, count: 0 },
  };
  const typeBreakdown: Record<string, Breakdown> = {
    "Self Service": { amount: 0, count: 0 },
    Table: { amount: 0, count: 0 },
    Parcel: { amount: 0, count: 0 },
  };
  const itemAgg: Record<string, { qty: number; total: number }> = {};
  const dayAgg: Record<string, DayRow> = {};

  orders.forEach((order) => {
    totalRevenue += order.total;
    grossSales += order.subtotal;
    taxableTotal += taxableValue(order);
    totalGst += order.gst;

    const pm = order.payment_mode || "Cash";
    if (!paymentBreakdown[pm]) paymentBreakdown[pm] = { amount: 0, count: 0 };
    paymentBreakdown[pm].amount += order.total;
    paymentBreakdown[pm].count += 1;

    const ot = order.order_type || "Self Service";
    if (!typeBreakdown[ot]) typeBreakdown[ot] = { amount: 0, count: 0 };
    typeBreakdown[ot].amount += order.total;
    typeBreakdown[ot].count += 1;

    const dayKey = (order.created_at || "").split("T")[0] || order.created_at;
    if (!dayAgg[dayKey]) dayAgg[dayKey] = { date: dayKey, orders: 0, gross: 0, gst: 0, total: 0 };
    dayAgg[dayKey].orders += 1;
    dayAgg[dayKey].gross += order.subtotal;
    dayAgg[dayKey].gst += order.gst;
    dayAgg[dayKey].total += order.total;

    parseCart(order.cart_data).forEach((item) => {
      totalItemsSold += item.quantity;
      if (!itemAgg[item.name]) itemAgg[item.name] = { qty: 0, total: 0 };
      itemAgg[item.name].qty += item.quantity;
      itemAgg[item.name].total += item.price * item.quantity;
    });
  });

  const totalExpenses = expenses.reduce((sum, exp) => sum + exp.amount, 0);
  const totalOrders = orders.length;

  const topItems = Object.entries(itemAgg)
    .map(([name, d]) => ({ name, ...d }))
    .sort((a, b) => b.total - a.total)
    .slice(0, 5);

  const daySeries = Object.values(dayAgg).sort((a, b) => b.date.localeCompare(a.date));
  const bestDay = daySeries.reduce<{ date: string; total: number } | null>(
    (best, d) => (!best || d.total > best.total ? { date: d.date, total: d.total } : best),
    null
  );

  return {
    totalRevenue,
    grossSales,
    taxableTotal,
    totalGst,
    totalExpenses,
    totalOrders,
    avgBill: totalOrders > 0 ? totalRevenue / totalOrders : 0,
    netProfit: totalRevenue - totalExpenses,
    totalItemsSold,
    paymentBreakdown,
    typeBreakdown,
    topItems,
    daySeries,
    bestDay,
    activeDays: daySeries.length,
  };
}

export interface ExpenseStats {
  total: number;
  count: number;
  avg: number;
  cats: { name: string; amount: number; count: number }[];
}

export function computeExpenseStats(expenses: Expense[]): ExpenseStats {
  const byCat: Record<string, Breakdown> = {};
  let total = 0;
  expenses.forEach((e) => {
    const cat = e.category || "Uncategorized";
    if (!byCat[cat]) byCat[cat] = { amount: 0, count: 0 };
    byCat[cat].amount += e.amount;
    byCat[cat].count += 1;
    total += e.amount;
  });
  const cats = Object.entries(byCat)
    .map(([name, d]) => ({ name, ...d }))
    .sort((a, b) => b.amount - a.amount);
  return { total, count: expenses.length, avg: expenses.length ? total / expenses.length : 0, cats };
}

export interface ItemSalesFilter {
  item: string;
  category: string;
  search: string;
}

export interface ItemSalesRow {
  name: string;
  category: string;
  qty: number;
  total: number;
}

export function computeFilteredItemSales(
  orders: FinalizedOrder[],
  categories: Record<number, string>,
  filter: ItemSalesFilter
): ItemSalesRow[] {
  const itemSales: Record<string, { qty: number; total: number; category: string }> = {};
  orders.forEach((order) => {
    parseCart(order.cart_data).forEach((item) => {
      const catName = categories[item.category_id] || "Uncategorized";
      if (filter.category !== "All Categories" && catName !== filter.category) return;
      if (filter.item !== "All Items" && item.name !== filter.item) return;
      if (filter.search && !item.name.toLowerCase().includes(filter.search.toLowerCase())) return;
      if (!itemSales[item.name]) itemSales[item.name] = { qty: 0, total: 0, category: catName };
      itemSales[item.name].qty += item.quantity;
      itemSales[item.name].total += item.price * item.quantity;
    });
  });
  return Object.entries(itemSales)
    .map(([name, data]) => ({ name, ...data }))
    .sort((a, b) => b.total - a.total);
}

export function uniqueItemNames(orders: FinalizedOrder[]): string[] {
  const items = new Set<string>();
  orders.forEach((order) => parseCart(order.cart_data).forEach((item) => items.add(item.name)));
  return Array.from(items).sort();
}
