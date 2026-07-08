import { useEffect, useMemo, useState } from "react";
import {
  Activity,
  Clock,
  CreditCard,
  IndianRupee,
  ReceiptText,
  ShoppingCart,
  Tag,
  TrendingUp,
  UtensilsCrossed,
} from "lucide-react";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  Legend,
  Pie,
  PieChart,
  ResponsiveContainer,
  Tooltip as RechartsTooltip,
  XAxis,
  YAxis,
} from "recharts";
import { STORAGE_KEYS } from "../../config/constants";
import { displayDateTime, formatCurrency } from "../../utils/format";
import { usePlanGate } from "../../hooks/usePlanGate";
import PlanGate from "../../components/ui/PlanGate";
import Spinner from "../../components/ui/Spinner";
import * as menuRepo from "../../db/repositories/menuRepo";
import * as ordersRepo from "../../db/repositories/ordersRepo";
import * as expensesRepo from "../../db/repositories/expensesRepo";
import type { Expense, FinalizedOrder } from "../../types";

type TimeRange = "today" | "7d" | "30d" | "all";

interface DashboardProps {
  dbReady: boolean;
}

/** Chart palette read from the active theme's tokens. */
function getThemeColors(): string[] {
  const style = getComputedStyle(document.documentElement);
  const fallbacks = ["#34d399", "#60a5fa", "#fbbf24", "#a78bfa", "#f87171", "#f472b6", "#22d3ee", "#2dd4bf"];
  return fallbacks.map((fb, i) => style.getPropertyValue(`--chart-${i + 1}`).trim() || fb);
}

function dateFilterSql(range: TimeRange, column: string): string {
  switch (range) {
    case "today":
      return `date(${column}, 'localtime') = date('now', 'localtime')`;
    case "7d":
      return `date(${column}, 'localtime') >= date('now', '-7 days', 'localtime')`;
    case "30d":
      return `date(${column}, 'localtime') >= date('now', '-30 days', 'localtime')`;
    default:
      return "1=1";
  }
}

/** Bucket key + label: hourly for "today", daily otherwise. */
function bucketOf(iso: string | undefined, range: TimeRange): { key: string; label: string } {
  if (range === "today") {
    const d = new Date(iso ?? "");
    if (isNaN(d.getTime())) return { key: "Unknown", label: "Unknown" };
    const hour = d.getHours();
    const hour12 = hour % 12 || 12;
    return { key: String(hour).padStart(2, "0"), label: `${hour12} ${hour >= 12 ? "PM" : "AM"}` };
  }
  const key = iso ? iso.substring(0, 10) : "Unknown";
  const d = new Date(key);
  return { key, label: isNaN(d.getTime()) ? key : d.toLocaleDateString("en-US", { month: "short", day: "numeric" }) };
}

export default function Dashboard({ dbReady }: DashboardProps) {
  const gate = usePlanGate(dbReady);
  const [loading, setLoading] = useState(true);
  const [timeRange, setTimeRange] = useState<TimeRange>(
    () => (localStorage.getItem(STORAGE_KEYS.dashboardTimeRange) as TimeRange) || "today"
  );
  const [orders, setOrders] = useState<FinalizedOrder[]>([]);
  const [expenses, setExpenses] = useState<Expense[]>([]);
  const [categories, setCategories] = useState<Record<number, string>>({});

  useEffect(() => {
    localStorage.setItem(STORAGE_KEYS.dashboardTimeRange, timeRange);
  }, [timeRange]);

  useEffect(() => {
    if (!dbReady || gate.checking || !gate.plan.usable) return;
    let cancelled = false;
    (async () => {
      setLoading(true);
      try {
        const cats = await menuRepo.listCategories();
        const catMap: Record<number, string> = {};
        cats.forEach((c) => (catMap[c.id] = c.name));

        const [ordersRes, expensesRes] = await Promise.all([
          ordersRepo.listRecentFinalizedOrders(dateFilterSql(timeRange, "created_at")),
          expensesRepo.listExpensesWhere(dateFilterSql(timeRange, "date")),
        ]);
        if (cancelled) return;
        setCategories(catMap);
        setOrders(ordersRes);
        setExpenses(expensesRes);
      } catch (err) {
        console.error("Dashboard fetch error:", err);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [dbReady, gate.checking, gate.plan.usable, timeRange]);

  const data = useMemo(() => {
    let totalRevenue = 0;
    let totalExpenses = 0;
    let totalItemsSold = 0;

    const buckets: Record<string, { Revenue: number; Expenses: number; label: string }> = {};
    const paymentMap: Record<string, number> = {};
    const orderTypeMap: Record<string, number> = {};
    const itemSales: Record<number, { name: string; qty: number; revenue: number; category_id: number }> = {};

    orders.forEach((o) => {
      totalRevenue += o.total || 0;
      const pMode = o.payment_mode || "Other";
      paymentMap[pMode] = (paymentMap[pMode] || 0) + (o.total || 0);
      const oType = o.order_type || "Unknown";
      orderTypeMap[oType] = (orderTypeMap[oType] || 0) + 1;

      const { key, label } = bucketOf(o.created_at, timeRange);
      if (!buckets[key]) buckets[key] = { Revenue: 0, Expenses: 0, label };
      buckets[key].Revenue += o.total || 0;

      ordersRepo.parseCart(o.cart_data).forEach((item) => {
        if (!itemSales[item.id]) {
          itemSales[item.id] = { name: item.name, qty: 0, revenue: 0, category_id: item.category_id };
        }
        const qty = item.quantity || 1;
        itemSales[item.id].qty += qty;
        itemSales[item.id].revenue += (item.price || 0) * qty;
        totalItemsSold += qty;
      });
    });

    expenses.forEach((e) => {
      totalExpenses += e.amount || 0;
      const { key, label } = bucketOf(e.date, timeRange);
      if (!buckets[key]) buckets[key] = { Revenue: 0, Expenses: 0, label };
      buckets[key].Expenses += e.amount || 0;
    });

    let trendData = Object.keys(buckets)
      .sort()
      .map((key) => ({ name: buckets[key].label, Revenue: buckets[key].Revenue, Expenses: buckets[key].Expenses }));
    // AreaChart needs ≥2 points to draw a line — duplicate a lone point.
    if (trendData.length === 1) {
      trendData = [
        { ...trendData[0], name: `${trendData[0].name} (Start)` },
        { ...trendData[0], name: `${trendData[0].name} (End)` },
      ];
    }

    const topItems = Object.values(itemSales).sort((a, b) => b.qty - a.qty).slice(0, 5);

    const catSales: Record<string, number> = {};
    Object.values(itemSales).forEach((item) => {
      const catName = categories[item.category_id] || "Uncategorized";
      catSales[catName] = (catSales[catName] || 0) + item.revenue;
    });
    const categoryData = Object.keys(catSales)
      .map((k) => ({ name: k, value: catSales[k] }))
      .sort((a, b) => b.value - a.value)
      .slice(0, 5);

    return {
      totalRevenue,
      totalOrders: orders.length,
      aov: orders.length > 0 ? totalRevenue / orders.length : 0,
      totalItemsSold,
      netProfit: totalRevenue - totalExpenses,
      trendData,
      paymentData: Object.keys(paymentMap).map((k) => ({ name: k, value: paymentMap[k] })),
      topItems,
      categoryData,
      recentOrders: orders.slice(0, 6),
    };
  }, [orders, expenses, categories, timeRange]);

  const COLORS = getThemeColors();
  const tooltipStyle = {
    backgroundColor: "var(--bg-elevated)",
    border: "1px solid var(--border-color)",
    borderRadius: "var(--radius-md)",
    boxShadow: "var(--shadow-md)",
  };

  return (
    <PlanGate checking={gate.checking} plan={gate.plan}>
      <div className="dash-page">
        <div className="dash-header">
          <div className="dash-title">
            <Activity size={26} color="var(--accent)" />
            Business Overview
          </div>
          <select className="select" style={{ width: "auto" }} value={timeRange} onChange={(e) => setTimeRange(e.target.value as TimeRange)}>
            <option value="today">Today</option>
            <option value="7d">Last 7 Days</option>
            <option value="30d">Last 30 Days</option>
            <option value="all">All Time</option>
          </select>
        </div>

        {/* KPIs */}
        <div className="kpi-grid">
          {[
            { label: "Gross Revenue", value: formatCurrency(data.totalRevenue), icon: IndianRupee, color: "var(--success)" },
            { label: "Total Orders", value: String(data.totalOrders), icon: ReceiptText, color: "var(--info)" },
            { label: "Avg. Order Value", value: formatCurrency(data.aov), icon: ShoppingCart, color: "var(--chart-4)" },
            { label: "Net Profit", value: formatCurrency(data.netProfit), icon: TrendingUp, color: "var(--warning)", danger: data.netProfit < 0 },
          ].map((kpi) => (
            <div key={kpi.label} className="kpi-box" style={{ "--kpi-color": kpi.color } as React.CSSProperties}>
              <div className="kpi-header">
                <span>{kpi.label}</span>
                <div className="kpi-icon-wrap">
                  <kpi.icon size={22} />
                </div>
              </div>
              <div className="kpi-value" style={kpi.danger ? { color: "var(--danger)" } : undefined}>
                {kpi.value}
              </div>
            </div>
          ))}
        </div>

        {/* Trend + category/payment analytics */}
        <div className="chart-grid">
          <div className="dash-panel" style={{ minHeight: 350 }}>
            {loading && (
              <div className="loader-overlay">
                <Spinner size={28} />
              </div>
            )}
            <div className="panel-title">
              <TrendingUp size={20} /> Revenue vs Expenses Trend
            </div>
            <div style={{ flex: 1, width: "100%", minHeight: 300 }}>
              <ResponsiveContainer width="100%" height="100%">
                <AreaChart data={data.trendData} margin={{ top: 10, right: 10, left: -20, bottom: 0 }}>
                  <defs>
                    <linearGradient id="colorRev" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="5%" stopColor="var(--success)" stopOpacity={0.4} />
                      <stop offset="95%" stopColor="var(--success)" stopOpacity={0} />
                    </linearGradient>
                    <linearGradient id="colorExp" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="5%" stopColor="var(--danger)" stopOpacity={0.4} />
                      <stop offset="95%" stopColor="var(--danger)" stopOpacity={0} />
                    </linearGradient>
                  </defs>
                  <CartesianGrid strokeDasharray="3 3" stroke="var(--border-subtle)" vertical={false} />
                  <XAxis dataKey="name" stroke="var(--text-secondary)" tick={{ fontSize: 12 }} axisLine={false} tickLine={false} />
                  <YAxis stroke="var(--text-secondary)" tick={{ fontSize: 12 }} axisLine={false} tickLine={false} tickFormatter={(val) => `₹${val}`} />
                  <RechartsTooltip contentStyle={tooltipStyle} />
                  <Legend wrapperStyle={{ paddingTop: 10 }} />
                  <Area type="monotone" name="Revenue" dataKey="Revenue" stroke="var(--success)" strokeWidth={3} fillOpacity={1} fill="url(#colorRev)" />
                  <Area type="monotone" name="Expenses" dataKey="Expenses" stroke="var(--danger)" strokeWidth={3} fillOpacity={1} fill="url(#colorExp)" />
                </AreaChart>
              </ResponsiveContainer>
            </div>
          </div>

          <div className="dash-panel" style={{ minHeight: 400 }}>
            {loading && (
              <div className="loader-overlay">
                <Spinner size={28} />
              </div>
            )}
            <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: "var(--space-4)" }}>
              <div style={{ flex: 1.5, minHeight: 240, display: "flex", flexDirection: "column" }}>
                <div className="panel-title" style={{ marginBottom: "0.25rem" }}>
                  <Tag size={20} /> Sales by Category
                </div>
                <div style={{ flex: 1, width: "100%", position: "relative" }}>
                  <ResponsiveContainer width="100%" height="100%">
                    <PieChart>
                      <Pie data={data.categoryData} cx="50%" cy="45%" innerRadius={45} outerRadius={70} paddingAngle={3} dataKey="value" stroke="none">
                        {data.categoryData.map((_entry, index) => (
                          <Cell key={index} fill={COLORS[index % COLORS.length]} />
                        ))}
                      </Pie>
                      <RechartsTooltip formatter={(value: any) => formatCurrency(Number(value))} contentStyle={tooltipStyle} />
                      <Legend verticalAlign="bottom" align="center" iconType="circle" wrapperStyle={{ fontSize: 12, paddingTop: 10 }} />
                    </PieChart>
                  </ResponsiveContainer>
                </div>
              </div>

              <div className="section-divider" style={{ margin: 0 }} />

              <div style={{ flex: 1, minHeight: 120, display: "flex", flexDirection: "column" }}>
                <div className="panel-title" style={{ marginBottom: "0.5rem" }}>
                  <CreditCard size={20} /> Payment Modes
                </div>
                <div style={{ flex: 1 }}>
                  <ResponsiveContainer width="100%" height="100%">
                    <BarChart data={data.paymentData} layout="vertical" margin={{ top: 0, right: 30, left: 0, bottom: 5 }}>
                      <CartesianGrid strokeDasharray="3 3" horizontal vertical={false} stroke="var(--border-subtle)" />
                      <XAxis type="number" hide />
                      <YAxis dataKey="name" type="category" axisLine={false} tickLine={false} tick={{ fontSize: 11, fill: "var(--text-primary)" }} width={65} />
                      <RechartsTooltip formatter={(value: any) => formatCurrency(Number(value))} cursor={{ fill: "var(--bg-hover)" }} contentStyle={tooltipStyle} />
                      <Bar dataKey="value" radius={[0, 4, 4, 0]} barSize={18}>
                        {data.paymentData.map((_entry, index) => (
                          <Cell key={index} fill={COLORS[(index + 4) % COLORS.length]} />
                        ))}
                      </Bar>
                    </BarChart>
                  </ResponsiveContainer>
                </div>
              </div>
            </div>
          </div>
        </div>

        {/* Top items + recent orders */}
        <div className="bottom-grid">
          <div className="dash-panel">
            {loading && (
              <div className="loader-overlay">
                <Spinner size={28} />
              </div>
            )}
            <div className="panel-title">
              <UtensilsCrossed size={20} /> Top Selling Items
            </div>
            <div style={{ flex: 1, display: "flex", flexDirection: "column" }}>
              {data.topItems.length > 0 ? (
                data.topItems.map((item, idx) => (
                  <div key={idx} className="list-item">
                    <div className="item-info">
                      <span className="item-name">{item.name}</span>
                      <span className="item-meta">{item.qty} units sold</span>
                    </div>
                    <span className="item-value text-success">{formatCurrency(item.revenue)}</span>
                  </div>
                ))
              ) : (
                <div className="loading-center" style={{ minHeight: 120 }}>No sales data found.</div>
              )}
            </div>
          </div>

          <div className="dash-panel" style={{ overflowX: "auto" }}>
            {loading && (
              <div className="loader-overlay">
                <Spinner size={28} />
              </div>
            )}
            <div className="panel-title">
              <Clock size={20} /> Recent Orders
            </div>
            {data.recentOrders.length > 0 ? (
              <table className="table">
                <thead>
                  <tr>
                    <th>Bill No</th>
                    <th>Date &amp; Time</th>
                    <th>Type</th>
                    <th>Mode</th>
                    <th className="num">Total</th>
                  </tr>
                </thead>
                <tbody>
                  {data.recentOrders.map((order, idx) => (
                    <tr key={idx}>
                      <td className="strong">{order.bill_number || `#${order.id}`}</td>
                      <td className="text-secondary" style={{ whiteSpace: "nowrap" }}>
                        {order.created_at ? displayDateTime(order.created_at) : "Unknown"}
                      </td>
                      <td>
                        <span className="badge">{order.order_type || "Unknown"}</span>
                      </td>
                      <td>{order.payment_mode || "Cash"}</td>
                      <td className="num strong text-success">{formatCurrency(order.total || 0)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            ) : (
              <div className="loading-center" style={{ minHeight: 120 }}>No recent orders found in this period.</div>
            )}
          </div>
        </div>
      </div>
    </PlanGate>
  );
}
