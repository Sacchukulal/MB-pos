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

export interface Category {
  id: number;
  name: string;
}

export interface MenuItem {
  id: number;
  category_id: number;
  name: string;
  price: number;
}

/** Denormalized snapshot stored in order `cart_data` JSON — prices frozen at sale time. */
export interface CartItem extends MenuItem {
  quantity: number;
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
  storeName: SectionStyle;
  addressMeta: SectionStyle;
  table: SectionStyle;
  totals: SectionStyle;
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
