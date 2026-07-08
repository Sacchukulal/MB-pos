import type { BillDesign, KotDesign, PrinterConfig, StoreProfile } from "../types";

export const DEFAULT_STORE_PROFILE: StoreProfile = {
  hotelName: "",
  address: "",
  phoneNumber: "",
  gstNumber: "",
  fssaiNumber: "",
  upiId: "",
  merchantName: "",
  paymentReference: "",
};

export const DEFAULT_BILL_DESIGN: BillDesign = {
  footerMessage: "Thank you! Visit again.",
  showGstin: true,
  showFssai: true,
  showAddress: true,
  showPhone: true,
  showCashier: true,
  showToken: true,
  fontFamily: "monospace",
  storeName: { size: "16px", bold: true },
  addressMeta: { size: "12px", bold: false },
  table: { size: "12px", bold: false },
  totals: { size: "12px", bold: true },
  footer: { size: "12px", bold: false },
  separators: {
    header: true,
    meta: true,
    token: true,
    tableHeader: true,
    tableBody: true,
    subtotals: true,
    grandTotal: true,
  },
  gst: { enabled: true, type: "Exclusive", percentage: 5 },
  logo: { position: "none", base64: "", sizePct: 50 },
  qrMode: "none",
  rowHeight: "4px 0",
  searchMatchMode: "starts",
};

export const DEFAULT_KOT_DESIGN: KotDesign = {
  showTitle: true,
  showToken: true,
  showBillNo: true,
  showOrderType: true,
  showTable: true,
  showDate: true,
  metaTwoColumn: true,
  title: { size: "16px", bold: true },
  meta: { size: "12px", bold: false },
  items: { size: "12px", bold: true },
  rowHeight: "4px 0",
  separators: {
    token: true,
    header: true,
    meta: true,
    tableHeader: true,
    tableBody: true,
  },
};

export const DEFAULT_PRINTER_CONFIG: PrinterConfig = {
  printerMode: "Single Printer",
  defaultPrinter: "",
  kotStyle: "Category-wise KOTs",
  paperSize: "3inch",
  printBold: false,
  kotConfirmation: false,
  billConfirmation: false,
  disableKot: false,
  token: { resetDaily: true, startingNumber: 100, currentNumber: 100, printSize: "Large" },
  bill: { resetDaily: false, prefix: "", startingNumber: 0, currentNumber: 0 },
  lastResetDate: "",
};
