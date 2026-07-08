import { CMD, lineWidth, padLeft, padRight } from "../escpos";
import { formatAmount } from "../../../utils/format";
import type { AppSettings } from "../../../types";

/**
 * Thermal print layouts for the Reports screen. Each builder returns the
 * document text; printService handles bytes, fallback, and the paper cut.
 */

interface ReportContext {
  width: number;
  sep: string;
  header: (title: string, period?: string) => string;
}

function context(settings: AppSettings, storeName: string): ReportContext {
  const width = lineWidth(settings.printer.paperSize);
  const sep = "-".repeat(width);
  const header = (title: string, period?: string) => {
    let text = CMD.ALIGN_CENTER + CMD.SIZE_DOUBLE_HEIGHT;
    if (storeName) text += `${storeName.toUpperCase()}\n`;
    text += CMD.SIZE_NORMAL;
    text += `${title}\n`;
    if (period) text += `Period: ${period}\n`;
    text += `${sep}\n` + CMD.ALIGN_LEFT;
    return text;
  };
  return { width, sep, header };
}

const bold = (line: string) => `${CMD.BOLD_ON}${line}${CMD.BOLD_OFF}`;

function totalLine(label: string, value: string, width: number): string {
  return bold(`${padRight(label, width - 15)}${padLeft(value, 15)}`) + "\n";
}

/* ------------------------------ builders --------------------------- */

export function salesSummaryReport(
  settings: AppSettings,
  period: string,
  data: { totalRevenue: number; totalGst: number; totalExpenses: number }
): string {
  const { width, sep, header } = context(settings, settings.store.hotelName);
  let text = header("SALES SUMMARY", period);
  text += `${padRight("Total Revenue:", width - 15)}${padLeft(formatAmount(data.totalRevenue), 15)}\n`;
  text += `${padRight("GST Collected:", width - 15)}${padLeft(formatAmount(data.totalGst), 15)}\n`;
  text += `${padRight("Total Expenses:", width - 15)}${padLeft(formatAmount(data.totalExpenses), 15)}\n`;
  text += `${sep}\n`;
  text += totalLine("NET PROFIT:", formatAmount(data.totalRevenue - data.totalExpenses), width);
  text += `${sep}\n\n\n\n`;
  return text;
}

export function itemSalesReport(
  settings: AppSettings,
  period: string,
  filters: { category?: string; item?: string },
  rows: { name: string; qty: number; total: number }[]
): string {
  const { width, sep } = context(settings, settings.store.hotelName);
  let text = CMD.ALIGN_CENTER + CMD.SIZE_DOUBLE_HEIGHT;
  if (settings.store.hotelName) text += `${settings.store.hotelName.toUpperCase()}\n`;
  text += CMD.SIZE_NORMAL + `ITEM SALES REPORT\nPeriod: ${period}\n`;
  if (filters.category) text += `Category: ${filters.category}\n`;
  if (filters.item) text += `Item: ${filters.item}\n`;
  text += `${sep}\n` + CMD.ALIGN_LEFT;

  const nameW = Math.floor(width * 0.55);
  const qtyW = Math.floor(width * 0.15);
  const totW = Math.floor(width * 0.3) - 2;
  text += `${padRight("Item", nameW)} ${padLeft("Qty", qtyW)} ${padLeft("Total", totW)}\n${sep}\n`;

  let grandTotal = 0;
  rows.forEach((row) => {
    grandTotal += row.total;
    text += `${padRight(row.name, nameW)} ${padLeft(row.qty, qtyW)} ${padLeft(formatAmount(row.total), totW)}\n`;
  });
  text += `${sep}\n`;
  text += totalLine("TOTAL REVENUE:", formatAmount(grandTotal), width);
  text += "\n\n\n\n";
  return text;
}

export function recentBillsReport(
  settings: AppSettings,
  period: string,
  rows: { billNo: string; total: number }[]
): string {
  const { width, sep, header } = context(settings, settings.store.hotelName);
  let text = header("RECENT BILLS", period);
  text += `${padRight("Bill No", 10)} ${padLeft("Total", width - 11)}\n${sep}\n`;
  let grandTotal = 0;
  rows.forEach((row) => {
    grandTotal += row.total;
    text += `${padRight(row.billNo, 10)} ${padLeft(formatAmount(row.total), width - 11)}\n`;
  });
  text += `${sep}\n`;
  text += totalLine("TOTAL:", formatAmount(grandTotal), width);
  text += "\n\n\n\n";
  return text;
}

export function expensesReport(
  settings: AppSettings,
  period: string,
  categories: { name: string; count: number; amount: number }[],
  total: number
): string {
  const { width, sep, header } = context(settings, settings.store.hotelName);
  let text = header("EXPENSE REPORT", period);
  const nameW = Math.floor(width * 0.5);
  const cntW = Math.floor(width * 0.2);
  const amtW = Math.floor(width * 0.3) - 2;
  text += `${padRight("Category", nameW)} ${padLeft("Entries", cntW)} ${padLeft("Amount", amtW)}\n${sep}\n`;
  categories.forEach((c) => {
    text += `${padRight(c.name, nameW)} ${padLeft(c.count, cntW)} ${padLeft(formatAmount(c.amount), amtW)}\n`;
  });
  text += `${sep}\n`;
  text += totalLine("TOTAL EXPENSES:", formatAmount(total), width);
  text += "\n\n\n\n";
  return text;
}

export function taxReport(
  settings: AppSettings,
  period: string,
  data: { taxableValue: number; totalGst: number; grossTotal: number }
): string {
  const { width, sep, header } = context(settings, settings.store.hotelName);
  let text = header("TAX (GST) REPORT", period);
  text += `${padRight("Taxable Value:", width - 15)}${padLeft(formatAmount(data.taxableValue), 15)}\n`;
  text += `${padRight("CGST:", width - 15)}${padLeft(formatAmount(data.totalGst / 2), 15)}\n`;
  text += `${padRight("SGST:", width - 15)}${padLeft(formatAmount(data.totalGst / 2), 15)}\n`;
  text += `${sep}\n`;
  text += totalLine("TOTAL GST:", formatAmount(data.totalGst), width);
  text += totalLine("GROSS TOTAL:", formatAmount(data.grossTotal), width);
  text += "\n\n\n\n";
  return text;
}

export function dayWiseReport(
  settings: AppSettings,
  period: string,
  rows: { date: string; orders: number; total: number }[],
  totalRevenue: number
): string {
  const { width, sep, header } = context(settings, settings.store.hotelName);
  let text = header("DAY-WISE SALES", period);
  text += `${padRight("Date", 12)} ${padLeft("Bills", 6)} ${padLeft("Revenue", width - 20)}\n${sep}\n`;
  rows.forEach((d) => {
    text += `${padRight(d.date, 12)} ${padLeft(d.orders, 6)} ${padLeft(formatAmount(d.total), width - 20)}\n`;
  });
  text += `${sep}\n`;
  text += totalLine("TOTAL:", formatAmount(totalRevenue), width);
  text += "\n\n\n\n";
  return text;
}

export function creditBalancesReport(
  settings: AppSettings,
  customers: { name: string; balance: number }[]
): string {
  const { width, sep, header } = context(settings, settings.store.hotelName);
  let text = header("CREDIT CUSTOMERS BALANCE");
  const nameW = Math.floor(width * 0.6);
  text += `${padRight("Customer", nameW)} ${padLeft("Balance", width - nameW - 1)}\n${sep}\n`;
  let totalDue = 0;
  customers.forEach((c) => {
    totalDue += c.balance;
    text += `${padRight(c.name, nameW)} ${padLeft(formatAmount(c.balance), width - nameW - 1)}\n`;
  });
  text += `${sep}\n`;
  text += totalLine("TOTAL DUE:", formatAmount(totalDue), width);
  text += "\n\n\n\n";
  return text;
}

export function customerStatementReport(
  settings: AppSettings,
  customer: { name: string; phone?: string; balance: number },
  period: string,
  transactions: { date: string; type: "bill" | "payment"; amount: number; mode: string }[]
): string {
  const { width, sep } = context(settings, settings.store.hotelName);
  let text = CMD.ALIGN_CENTER + CMD.SIZE_DOUBLE_HEIGHT;
  if (settings.store.hotelName) text += `${settings.store.hotelName.toUpperCase()}\n`;
  text += CMD.SIZE_NORMAL + `CUSTOMER STATEMENT\n${sep}\n` + CMD.ALIGN_LEFT;
  text += `Customer: ${customer.name}\n`;
  if (customer.phone) text += `Phone: ${customer.phone}\n`;
  text += `Period: ${period}\n${sep}\n`;
  text += `${padRight("Date", 10)} ${padRight("Type", 8)} ${padLeft("Amount", 10)} ${padLeft("Mode", 8)}\n${sep}\n`;
  transactions.forEach((t) => {
    const dateStr = new Date(t.date).toLocaleDateString("en-GB", { day: "2-digit", month: "2-digit" });
    text += `${padRight(dateStr, 10)} ${padRight(t.type === "bill" ? "Bill" : "Pay", 8)} ${padLeft(formatAmount(t.amount), 10)} ${padLeft(t.mode, 8)}\n`;
  });
  text += `${sep}\n`;
  text += bold(`${padRight("PENDING BALANCE:", width - 12)}${padLeft(formatAmount(customer.balance), 12)}`) + "\n";
  text += `${sep}\n\n\n\n`;
  return text;
}
