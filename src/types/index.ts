/** Domain models shared across the app. All persisted shapes mirror the SQLite schema. */

export type OrderType = "Self Service" | "Table" | "Parcel";
export type PaymentMode = "Cash" | "Card" | "UPI" | "Credit";
export type QrMode = "dynamic" | "static" | "none";
export type PaperSize = "2inch" | "3inch" | "4inch";
export type TokenPrintSize = "Normal" | "Large" | "Extra Large";
export type PrinterMode = "Single Printer" | "Multiple Printers";
export type KotStyle = "Single KOT" | "Category-wise KOTs";
export type GstType = "Inclusive" | "Exclusive";
export type SearchMatchMode = "starts" | "contains";
/** Separator line style, shared by the bill and the KOT. */
export type LinePattern = "dashed" | "dotted" | "solid" | "bold" | "double";
/**
 * How receipts are sent to the printer.
 * "graphics" rasterizes the exact preview (font family, size, bold) as a bitmap;
 * "text" uses the printer's built-in font and ESC/POS size multipliers.
 */
export type PrintEngine = "graphics" | "text";

export interface Category {
  id: number;
  name: string;
}

export interface MenuItem {
  id: number;
  category_id: number;
  name: string;
  price: number;
  /** 0 = "86"-ed (hidden on phones, flagged in billing search). Missing = available. */
  is_available?: number;
}

/** Denormalized snapshot stored in order `cart_data` JSON — prices frozen at sale time. */
export interface CartItem extends MenuItem {
  quantity: number;
  /** Per-line note ("no onion"). Line identity for merging = (id, note). */
  note?: string;
}

/** Table master row (Tables & Mobile Ordering settings). Mirrors SQLite restaurant_tables. */
export interface RestaurantTable {
  id: number;
  section: string;
  label: string;
  sort_order: number;
  is_active: number;
}

interface OrderBase {
  id: number;
  cart_data: string;
  customer_name: string;
  customer_phone: string;
  payment_mode: string;
  subtotal: number;
  gst: number;
  total: number;
  order_type: OrderType;
  table_number: string;
  customer_id: number | null;
  token_number: number | null;
  bill_number: string | null;
  created_at: string;
  /* Mobile-orders bridge columns (nullable adds; absent on legacy rows). */
  remote_uuid?: string | null;
  source?: string | null; // 'pos' | 'mobile'
  waiter_name?: string | null;
  updated_at?: string | null;
  cloud_dirty?: number | null;
  /** JSON CartItem[]: what the kitchen has seen. NULL = legacy delta behaviour. */
  printed_items?: string | null;
}

export type ProcessingOrder = OrderBase;
export type FinalizedOrder = OrderBase;

export interface Customer {
  id: number;
  name: string;
  phone: string;
  credit_balance: number;
  created_at: string;
}

export interface CustomerPayment {
  id: number;
  customer_id: number;
  amount: number;
  payment_mode: string;
  date: string;
}

export interface Expense {
  id: number;
  description: string;
  amount: number;
  category: string;
  date: string;
}

export interface StaffMember {
  id: number;
  name: string;
  role: string;
  phone: string;
}

export interface SubscriptionState {
  status: string;
  planId: string;
  /** Human plan name from the Razorpay catalogue ("Yearly Pro"). May be
   *  empty on snapshots cached before the server started sending it. */
  planName: string;
  subscriptionId: string;
  nextBillingDate: string;
  updatedAt: string;
  last_checked_date: string;
}

export interface UserDetails {
  displayName: string;
  email: string;
  mobileNumber: string;
  restaurantName: string;
}

/* ------------------------------------------------------------------ */
/* Settings groups (typed, coerced — components never see raw rows)    */
/* ------------------------------------------------------------------ */

export interface StoreProfile {
  hotelName: string;
  address: string;
  phoneNumber: string;
  gstNumber: string;
  fssaiNumber: string;
  upiId: string;
  merchantName: string;
  paymentReference: string;
}

export interface SectionStyle {
  size: string; // e.g. "12px"
  bold: boolean;
}

export interface BillDesign {
  footerMessage: string;
  /** Header line visibility (GSTIN line only — independent of GST math). */
  showGstin: boolean;
  showFssai: boolean;
  showAddress: boolean;
  showPhone: boolean;
  showCashier: boolean;
  showToken: boolean;
  fontFamily: string;
  linePattern: LinePattern;
  storeName: SectionStyle;
  addressMeta: SectionStyle;
  table: SectionStyle;
  /** Subtotal + GST lines. */
  subtotals: SectionStyle;
  grandTotal: SectionStyle;
  footer: SectionStyle;
  separators: {
    header: boolean;
    meta: boolean;
    token: boolean;
    tableHeader: boolean;
    tableBody: boolean;
    subtotals: boolean;
    grandTotal: boolean;
  };
  gst: {
    enabled: boolean;
    type: GstType;
    percentage: number;
  };
  logo: {
    position: "none" | "top";
    base64: string;
    sizePct: number;
  };
  qrMode: QrMode;
  rowHeight: string;
  searchMatchMode: SearchMatchMode;
}

export interface KotDesign {
  showTitle: boolean;
  showToken: boolean;
  showBillNo: boolean;
  showOrderType: boolean;
  showTable: boolean;
  showDate: boolean;
  /** "Waiter: <name>" in the KOT meta block for mobile orders. */
  showWaiter: boolean;
  metaTwoColumn: boolean;
  title: SectionStyle;
  meta: SectionStyle;
  items: SectionStyle;
  rowHeight: string;
  separators: {
    token: boolean;
    header: boolean;
    meta: boolean;
    tableHeader: boolean;
    tableBody: boolean;
  };
}

export interface PrinterConfig {
  printerMode: PrinterMode;
  defaultPrinter: string;
  kotStyle: KotStyle;
  paperSize: PaperSize;
  printEngine: PrintEngine;
  printBold: boolean;
  kotConfirmation: boolean;
  billConfirmation: boolean;
  disableKot: boolean;
  token: {
    resetDaily: boolean;
    startingNumber: number;
    currentNumber: number;
    printSize: TokenPrintSize;
  };
  bill: {
    resetDaily: boolean;
    prefix: string;
    startingNumber: number;
    currentNumber: number;
  };
  lastResetDate: string; // ISO YYYY-MM-DD
}

export interface AppSettings {
  store: StoreProfile;
  bill: BillDesign;
  kot: KotDesign;
  printer: PrinterConfig;
  categoryPrinters: Record<number, string>;
}
