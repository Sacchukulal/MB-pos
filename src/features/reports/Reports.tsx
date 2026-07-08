import { useCallback, useEffect, useMemo, useState } from "react";
import {
  BarChart3,
  Calendar as CalendarIcon,
  Download,
  Package,
  Printer,
  Receipt,
  TrendingUp,
  Users,
  Wallet,
} from "lucide-react";
import Spinner from "../../components/ui/Spinner";
import PlanGate from "../../components/ui/PlanGate";
import { usePlanGate } from "../../hooks/usePlanGate";
import { useSettings } from "../../hooks/useSettings";
import { useToast } from "../../hooks/useToast";
import { todayISO } from "../../utils/format";
import * as menuRepo from "../../db/repositories/menuRepo";
import * as ordersRepo from "../../db/repositories/ordersRepo";
import * as expensesRepo from "../../db/repositories/expensesRepo";
import * as customersRepo from "../../db/repositories/customersRepo";
import { printReport } from "../../services/printing/printService";
import * as reportTemplates from "../../services/printing/templates/reports";
import { computeExpenseStats, computeFilteredItemSales, computeSalesStats, uniqueItemNames, type ItemSalesFilter } from "./salesStats";
import { DayWiseView, ExpensesView, ItemSalesView, RecentBillsView, SalesSummaryView, TaxReportView } from "./SalesViews";
import CreditCustomers from "./CreditCustomers";
import { reprintBill } from "./reprint";
import type { Customer, Expense, FinalizedOrder } from "../../types";

type MainTab = "Sales Overview" | "Credit Customers";
type SalesReport = "Sales Summary" | "Day-wise Sales" | "Item Sales" | "Tax Report" | "Expenses" | "Recent Bills";

const SALES_SUBTABS: { id: SalesReport; icon: typeof TrendingUp }[] = [
  { id: "Sales Summary", icon: TrendingUp },
  { id: "Day-wise Sales", icon: CalendarIcon },
  { id: "Item Sales", icon: Package },
  { id: "Tax Report", icon: BarChart3 },
  { id: "Expenses", icon: Wallet },
  { id: "Recent Bills", icon: Receipt },
];

interface ReportsProps {
  dbReady: boolean;
}

export default function Reports({ dbReady }: ReportsProps) {
  const gate = usePlanGate(dbReady);
  const { settings } = useSettings();
  const { toast } = useToast();

  const [mainTab, setMainTab] = useState<MainTab>("Sales Overview");
  const [salesReport, setSalesReport] = useState<SalesReport>("Sales Summary");
  const [dateRange, setDateRange] = useState(() => ({ start: todayISO(), end: todayISO() }));
  const [customerDateRange, setCustomerDateRange] = useState(() => {
    const start = new Date();
    start.setDate(start.getDate() - 30);
    return { start: start.toISOString().split("T")[0], end: todayISO() };
  });

  const [loading, setLoading] = useState(false);
  const [orders, setOrders] = useState<FinalizedOrder[]>([]);
  const [expenses, setExpenses] = useState<Expense[]>([]);
  const [categories, setCategories] = useState<Record<number, string>>({});
  const [customers, setCustomers] = useState<Customer[]>([]);
  const [selectedCustomer, setSelectedCustomer] = useState<Customer | null>(null);
  const [customerTransactions, setCustomerTransactions] = useState<
    { type: "bill" | "payment"; date: string; amount: number; mode: string; details: string }[]
  >([]);

  const [itemFilter, setItemFilter] = useState<ItemSalesFilter>({ item: "All Items", category: "All Categories", search: "" });
  const [editingPaymentModeId, setEditingPaymentModeId] = useState<number | null>(null);
  const [newPaymentMode, setNewPaymentMode] = useState("Cash");

  /* --------------------------- data loading -------------------------- */

  const fetchData = useCallback(async () => {
    if (!dbReady || !gate.plan.usable) return;
    setLoading(true);
    try {
      if (mainTab === "Sales Overview") {
        const cats = await menuRepo.listCategories();
        const catMap: Record<number, string> = {};
        cats.forEach((c) => (catMap[c.id] = c.name));
        setCategories(catMap);
        setOrders(await ordersRepo.listFinalizedOrders(dateRange.start, dateRange.end));
        setExpenses(await expensesRepo.listExpensesInRange(dateRange.start, dateRange.end));
      } else {
        setCustomers(await customersRepo.listCustomers());
      }
    } catch (error) {
      console.error("Failed to fetch report data:", error);
    } finally {
      setLoading(false);
    }
  }, [dbReady, gate.plan.usable, mainTab, dateRange]);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  const salesStats = useMemo(() => computeSalesStats(orders, expenses), [orders, expenses]);
  const expenseStats = useMemo(() => computeExpenseStats(expenses), [expenses]);
  const itemRows = useMemo(() => computeFilteredItemSales(orders, categories, itemFilter), [orders, categories, itemFilter]);
  const itemNames = useMemo(() => uniqueItemNames(orders), [orders]);
  const period = `${dateRange.start} to ${dateRange.end}`;

  /* ------------------------------ actions ---------------------------- */

  const handleUpdatePaymentMode = async (orderId: number) => {
    try {
      await ordersRepo.updateOrderPaymentMode(orderId, newPaymentMode);
      setOrders((prev) => prev.map((o) => (o.id === orderId ? { ...o, payment_mode: newPaymentMode } : o)));
      setEditingPaymentModeId(null);
      toast("Payment mode updated successfully.", "success");
    } catch (error) {
      console.error("Failed to update payment mode:", error);
      toast("Failed to update payment mode.", "danger");
    }
  };

  const handleReprint = async (orderId: number) => {
    if (!settings) return;
    const result = await reprintBill(settings, orderId);
    if (result.ok) toast("Print successful!", "success");
    else if (result.error === "NO_PRINTER") toast("No default printer set!", "warning");
    else toast(`Print failed: ${result.error}`, "danger");
  };

  const handlePrintReport = async () => {
    if (!settings) return;
    let text = "";
    if (mainTab === "Sales Overview") {
      switch (salesReport) {
        case "Sales Summary":
          text = reportTemplates.salesSummaryReport(settings, period, salesStats);
          break;
        case "Item Sales":
          text = reportTemplates.itemSalesReport(
            settings,
            period,
            {
              category: itemFilter.category !== "All Categories" ? itemFilter.category : undefined,
              item: itemFilter.item !== "All Items" ? itemFilter.item : undefined,
            },
            itemRows
          );
          break;
        case "Recent Bills":
          text = reportTemplates.recentBillsReport(
            settings,
            period,
            orders.map((o) => ({ billNo: o.bill_number || `#${o.id}`, total: o.total }))
          );
          break;
        case "Expenses":
          text = reportTemplates.expensesReport(settings, period, expenseStats.cats, expenseStats.total);
          break;
        case "Tax Report":
          text = reportTemplates.taxReport(settings, period, {
            taxableValue: salesStats.taxableTotal,
            totalGst: salesStats.totalGst,
            grossTotal: salesStats.totalRevenue,
          });
          break;
        case "Day-wise Sales":
          text = reportTemplates.dayWiseReport(settings, period, salesStats.daySeries, salesStats.totalRevenue);
          break;
      }
    } else {
      text = reportTemplates.creditBalancesReport(
        settings,
        customers.map((c) => ({ name: c.name, balance: c.credit_balance }))
      );
    }

    const result = await printReport(settings, text);
    if (result.ok) toast("Report printed successfully!", "success");
    else if (result.error === "NO_PRINTER") toast("No default printer set!", "warning");
    else toast(`Print failed: ${result.error}`, "danger");
  };

  const handleExportCSV = () => {
    let csv = "";
    if (mainTab === "Sales Overview") {
      switch (salesReport) {
        case "Sales Summary":
          csv += "Order ID,Date,Customer,Type,Payment Mode,Subtotal,GST,Total\n";
          orders.forEach((o) => {
            csv += `${o.id},${o.created_at},${o.customer_name || "Guest"},${o.order_type},${o.payment_mode},${o.subtotal},${o.gst},${o.total}\n`;
          });
          break;
        case "Item Sales":
          csv += "Item Name,Category,Quantity Sold,Total Revenue\n";
          itemRows.forEach((row) => {
            csv += `${row.name},${row.category},${row.qty},${row.total}\n`;
          });
          break;
        case "Recent Bills":
          csv += "Order ID,Date,Customer,Type,Payment Mode,Total\n";
          orders.forEach((o) => {
            csv += `${o.id},${o.created_at},${o.customer_name || "Guest"},${o.order_type},${o.payment_mode},${o.total}\n`;
          });
          break;
        case "Expenses":
          csv += "Date,Description,Category,Amount\n";
          expenses.forEach((e) => {
            csv += `${e.date},${e.description},${e.category || "Uncategorized"},${e.amount}\n`;
          });
          csv += `\nCategory,Entries,Total\n`;
          expenseStats.cats.forEach((c) => {
            csv += `${c.name},${c.count},${c.amount}\n`;
          });
          csv += `TOTAL,,${expenseStats.total}\n`;
          break;
        case "Tax Report":
          csv += "Bill No,Date,Taxable Value,GST,Total\n";
          orders.forEach((o) => {
            csv += `${o.bill_number || o.id},${o.created_at},${o.subtotal},${o.gst},${o.total}\n`;
          });
          csv += `\nTaxable Value,GST Collected,Total\n`;
          csv += `${salesStats.taxableTotal},${salesStats.totalGst},${salesStats.totalRevenue}\n`;
          break;
        case "Day-wise Sales":
          csv += "Date,Orders,Gross Sales,GST,Net Revenue\n";
          salesStats.daySeries.forEach((d) => {
            csv += `${d.date},${d.orders},${d.gross},${d.gst},${d.total}\n`;
          });
          break;
      }
    } else if (selectedCustomer) {
      csv += `Statement for ${selectedCustomer.name} (${selectedCustomer.phone || "No Phone"})\n`;
      csv += `Date Range: ${customerDateRange.start} to ${customerDateRange.end}\n`;
      csv += `Current Credit Balance: ₹${selectedCustomer.credit_balance.toFixed(2)}\n\n`;
      csv += "Type,ID/Details,Date,Amount,Mode\n";
      customerTransactions.forEach((t) => {
        csv += `${t.type.toUpperCase()},${t.details},${t.date},${t.amount},${t.mode}\n`;
      });
    } else {
      csv += "Customer ID,Name,Phone,Credit Balance\n";
      customers.forEach((c) => {
        csv += `${c.id},${c.name},${c.phone},${c.credit_balance}\n`;
      });
    }

    const blob = new Blob([csv], { type: "text/csv;charset=utf-8;" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = `${mainTab.replace(/ /g, "_")}_${selectedCustomer ? selectedCustomer.name : ""}_${dateRange.start}_to_${dateRange.end}.csv`;
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
    URL.revokeObjectURL(url);
    toast("Report exported to CSV.", "success");
  };

  /* ------------------------------ render ------------------------------ */

  return (
    <PlanGate checking={gate.checking} plan={gate.plan}>
      <div className="rep-page">
        <div className="rep-topbar">
          <div className="page-head">
            <h1>
              <BarChart3 size={24} /> Reports &amp; Analytics
            </h1>
            <p>Sales performance, item insights and customer credit — all in one place</p>
          </div>
          <div className="tabs">
            {([
              { id: "Sales Overview" as const, icon: TrendingUp },
              { id: "Credit Customers" as const, icon: Users },
            ]).map((tab) => (
              <button
                key={tab.id}
                className={`tab ${mainTab === tab.id ? "active" : ""}`}
                onClick={() => {
                  setMainTab(tab.id);
                  setSelectedCustomer(null);
                }}
              >
                <tab.icon size={16} /> {tab.id}
              </button>
            ))}
          </div>
        </div>

        <div className="rep-toolbar">
          <div className="rep-subtabs">
            {mainTab === "Sales Overview" ? (
              SALES_SUBTABS.map((tab) => (
                <button key={tab.id} className={`tab ${salesReport === tab.id ? "active" : ""}`} onClick={() => setSalesReport(tab.id)}>
                  <tab.icon size={14} /> {tab.id}
                </button>
              ))
            ) : (
              <h3 style={{ fontSize: "var(--text-lg)", fontWeight: "var(--font-bold)" }}>Credit Customers</h3>
            )}
          </div>

          <div className="rep-toolbar-right">
            {mainTab === "Sales Overview" && (
              <div className="date-range">
                <CalendarIcon size={16} style={{ color: "var(--text-tertiary)" }} />
                <input type="date" className="input" value={dateRange.start} onChange={(e) => setDateRange((p) => ({ ...p, start: e.target.value }))} />
                <span className="date-range-sep">to</span>
                <input type="date" className="input" value={dateRange.end} onChange={(e) => setDateRange((p) => ({ ...p, end: e.target.value }))} />
              </div>
            )}
            <button className="btn btn--ghost" onClick={handlePrintReport}>
              <Printer size={16} /> Print Report
            </button>
            <button className="btn btn--ghost" onClick={handleExportCSV}>
              <Download size={16} /> Export CSV
            </button>
          </div>
        </div>

        <div className="rep-content">
          {loading ? (
            <div className="loading-center" style={{ minHeight: 200, gap: "var(--space-3)" }}>
              <Spinner size={24} /> Loading Data…
            </div>
          ) : mainTab === "Sales Overview" ? (
            <>
              {salesReport === "Sales Summary" && <SalesSummaryView stats={salesStats} expenseStats={expenseStats} />}
              {salesReport === "Item Sales" && (
                <ItemSalesView
                  rows={itemRows}
                  filter={itemFilter}
                  onFilterChange={setItemFilter}
                  itemNames={itemNames}
                  categoryNames={Object.values(categories)}
                />
              )}
              {salesReport === "Recent Bills" && (
                <RecentBillsView
                  orders={orders}
                  editingPaymentModeId={editingPaymentModeId}
                  newPaymentMode={newPaymentMode}
                  onStartEditPayment={(o) => {
                    setEditingPaymentModeId(o.id);
                    setNewPaymentMode(o.payment_mode || "Cash");
                  }}
                  onChangePaymentMode={setNewPaymentMode}
                  onSavePaymentMode={handleUpdatePaymentMode}
                  onCancelEditPayment={() => setEditingPaymentModeId(null)}
                  onReprint={handleReprint}
                />
              )}
              {salesReport === "Day-wise Sales" && <DayWiseView stats={salesStats} />}
              {salesReport === "Tax Report" && <TaxReportView stats={salesStats} orders={orders} />}
              {salesReport === "Expenses" && <ExpensesView stats={salesStats} expenseStats={expenseStats} expenses={expenses} />}
            </>
          ) : (
            <CreditCustomers
              settings={settings}
              customers={customers}
              onCustomersChanged={fetchData}
              selectedCustomer={selectedCustomer}
              onSelectCustomer={setSelectedCustomer}
              dateRange={customerDateRange}
              onDateRangeChange={setCustomerDateRange}
              onTransactionsChanged={setCustomerTransactions}
            />
          )}
        </div>
      </div>
    </PlanGate>
  );
}
