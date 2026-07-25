import { QrCode, UtensilsCrossed } from "lucide-react";
import { PAPER } from "../../config/constants";
import { clampSize } from "../../services/printing/fit";
import { TOKEN_SCALE } from "../../services/printing/templates/billCanvas";
import type { BillDesign, KotDesign, LinePattern, PaperSize, StoreProfile, TokenPrintSize } from "../../types";

/**
 * On-screen mirror of the printed bill and KOT. With the graphics print engine
 * these are drawn to paper from the same measurements, so the preview is the
 * contract — keep any layout change here in sync with templates/*Canvas.ts.
 * Always white paper / black ink (the only sanctioned hardcoded colors).
 */

/** CSS border for each separator style. */
const LINE_CSS: Record<LinePattern, string> = {
  dashed: "1px dashed #000",
  dotted: "2px dotted #000",
  solid: "1px solid #000",
  bold: "2px solid #000",
  double: "3px double #000",
};

function Sep({ pattern }: { pattern: LinePattern }) {
  return <div style={{ borderTop: LINE_CSS[pattern] ?? LINE_CSS.dashed, margin: "8px 0" }} />;
}

/** Token line size, matching TOKEN_SCALE used by the printer. */
function tokenFontSize(baseSize: string, printSize: TokenPrintSize): string {
  const base = parseInt(baseSize, 10) || 12;
  return `${Math.round(base * (TOKEN_SCALE[printSize] ?? TOKEN_SCALE.Large))}px`;
}

interface BillPreviewProps {
  bill: BillDesign;
  store: StoreProfile;
  paperSize: PaperSize;
  tokenPrintSize: TokenPrintSize;
}

export function BillPreview({ bill, store, paperSize, tokenPrintSize }: BillPreviewProps) {
  const width = PAPER[paperSize]?.previewPx ?? 320;
  const line = bill.linePattern;
  const metaSize = clampSize(bill.addressMeta.size, "billMeta", bill.fontFamily, paperSize);
  const tableSize = clampSize(bill.table.size, "billTable", bill.fontFamily, paperSize);
  const subSize = clampSize(bill.subtotals.size, "billTotals", bill.fontFamily, paperSize);
  const grandSize = clampSize(bill.grandTotal.size, "billTotals", bill.fontFamily, paperSize);
  const gstRate = bill.gst.percentage;
  const exclusive = bill.gst.type === "Exclusive";
  const SAMPLE_SUBTOTAL = 510;

  return (
    <div className="paper-preview" style={{ width, fontFamily: bill.fontFamily }}>
      {bill.logo.position === "top" && (
        <div style={{ display: "flex", justifyContent: "center", marginBottom: 10 }}>
          <div style={{ width: `${bill.logo.sizePct || 50}%`, display: "flex", justifyContent: "center" }}>
            {bill.logo.base64 ? (
              <img src={bill.logo.base64} alt="" style={{ width: "100%", height: "auto" }} />
            ) : (
              <UtensilsCrossed size={48} color="#000" />
            )}
          </div>
        </div>
      )}

      {/* Header */}
      <div style={{ textAlign: "center", marginBottom: 10 }}>
        <div style={{ fontWeight: bill.storeName.bold ? "bold" : "normal", fontSize: bill.storeName.size }}>
          {store.hotelName || "YOUR HOTEL NAME"}
        </div>
        <div style={{ fontSize: metaSize, marginTop: 4, fontWeight: bill.addressMeta.bold ? "bold" : "normal" }}>
          {bill.showAddress && <div>{store.address || "123, Street Name, City"}</div>}
          {bill.showPhone && <div>Tel: {store.phoneNumber || "9876543210"}</div>}
          {bill.showGstin && store.gstNumber && <div>GSTIN: {store.gstNumber}</div>}
          {bill.showFssai && store.fssaiNumber && <div>FSSAI: {store.fssaiNumber}</div>}
        </div>
      </div>

      {bill.separators.header && <Sep pattern={line} />}

      {/* Meta */}
      <div style={{ fontSize: metaSize, fontWeight: bill.addressMeta.bold ? "bold" : "normal" }}>
        <div style={{ display: "flex", justifyContent: "space-between" }}>
          <div>Bill No: 1234</div>
          <div>Date: 26-Feb-2026</div>
        </div>
        <div style={{ display: "flex", justifyContent: "space-between" }}>
          <div>Time: 12:30 PM</div>
          {bill.showCashier && <div>Cashier: Admin</div>}
        </div>
      </div>

      {bill.separators.meta && <Sep pattern={line} />}

      {bill.showToken && (
        <>
          <div
            style={{
              textAlign: "center",
              fontWeight: "bold",
              margin: "8px 0",
              fontSize: tokenFontSize(metaSize, tokenPrintSize),
            }}
          >
            TOKEN: 105
          </div>
          {bill.separators.token && <Sep pattern={line} />}
        </>
      )}

      {/* Items */}
      <table
        style={{
          width: "100%",
          borderCollapse: "collapse",
          margin: "5px 0",
          fontSize: tableSize,
          fontWeight: bill.table.bold ? "bold" : "normal",
        }}
      >
        <thead>
          <tr style={{ borderBottom: bill.separators.tableHeader ? LINE_CSS[line] : "none" }}>
            <th style={{ textAlign: "left", padding: bill.rowHeight, fontWeight: "inherit" }}>Item</th>
            <th style={{ textAlign: "right", padding: bill.rowHeight, fontWeight: "inherit" }}>Qty</th>
            <th style={{ textAlign: "right", padding: bill.rowHeight, fontWeight: "inherit" }}>Price</th>
            <th style={{ textAlign: "right", padding: bill.rowHeight, fontWeight: "inherit" }}>Amt</th>
          </tr>
        </thead>
        <tbody>
          {[
            ["Paneer Tikka", 1, "250.00", "250.00"],
            ["Butter Naan", 2, "40.00", "80.00"],
            ["Dal Makhani", 1, "180.00", "180.00"],
          ].map(([name, qty, price, amt]) => (
            <tr key={String(name)}>
              <td style={{ padding: bill.rowHeight }}>{name}</td>
              <td style={{ textAlign: "right", padding: bill.rowHeight }}>{qty}</td>
              <td style={{ textAlign: "right", padding: bill.rowHeight }}>{price}</td>
              <td style={{ textAlign: "right", padding: bill.rowHeight }}>{amt}</td>
            </tr>
          ))}
        </tbody>
      </table>

      {bill.separators.tableBody && <Sep pattern={line} />}

      {/* Totals */}
      <div style={{ fontSize: subSize, fontWeight: bill.subtotals.bold ? "bold" : "normal" }}>
        <div style={{ display: "flex", justifyContent: "space-between" }}>
          <span>Subtotal:</span>
          <span>{SAMPLE_SUBTOTAL.toFixed(2)}</span>
        </div>
        {bill.gst.enabled &&
          (exclusive ? (
            <>
              <div style={{ display: "flex", justifyContent: "space-between" }}>
                <span>CGST ({gstRate / 2}%):</span>
                <span>{((SAMPLE_SUBTOTAL * (gstRate / 100)) / 2).toFixed(2)}</span>
              </div>
              <div style={{ display: "flex", justifyContent: "space-between" }}>
                <span>SGST ({gstRate / 2}%):</span>
                <span>{((SAMPLE_SUBTOTAL * (gstRate / 100)) / 2).toFixed(2)}</span>
              </div>
            </>
          ) : (
            <div style={{ fontSize: "0.9em" }}>
              (Includes Rs. {(SAMPLE_SUBTOTAL - SAMPLE_SUBTOTAL / (1 + gstRate / 100)).toFixed(2)} GST)
            </div>
          ))}
      </div>

      {bill.separators.subtotals && <Sep pattern={line} />}

      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          fontWeight: bill.grandTotal.bold ? "bold" : "normal",
          fontSize: grandSize,
        }}
      >
        <span>GRAND TOTAL:</span>
        <span>
          {bill.gst.enabled && exclusive
            ? `Rs. ${(SAMPLE_SUBTOTAL * (1 + gstRate / 100)).toFixed(2)}`
            : `Rs. ${SAMPLE_SUBTOTAL.toFixed(2)}`}
        </span>
      </div>

      {bill.separators.grandTotal && <Sep pattern={line} />}

      {/* Footer */}
      <div
        style={{
          textAlign: "center",
          marginTop: 10,
          fontSize: bill.footer.size,
          fontWeight: bill.footer.bold ? "bold" : "normal",
        }}
      >
        {bill.footerMessage || "Thank you! Visit again."}
      </div>

      {bill.qrMode !== "none" && (
        <div style={{ display: "flex", flexDirection: "column", alignItems: "center", marginTop: 15, paddingTop: 15 }}>
          <div style={{ fontSize: "0.85em", marginBottom: 5 }}>Scan to Pay via UPI</div>
          <div
            style={{
              width: 100,
              height: 100,
              border: "1px solid #000",
              display: "flex",
              justifyContent: "center",
              alignItems: "center",
              background: "#fff",
            }}
          >
            <QrCode size={64} color="#000" />
          </div>
        </div>
      )}
    </div>
  );
}

interface KotPreviewProps {
  kot: KotDesign;
  fontFamily: string;
  paperSize: PaperSize;
  linePattern: LinePattern;
  tokenPrintSize: TokenPrintSize;
}

export function KotPreview({ kot, fontFamily, paperSize, linePattern, tokenPrintSize }: KotPreviewProps) {
  const width = PAPER[paperSize]?.previewPx ?? 320;
  const metaSize = clampSize(kot.meta.size, "kotMeta", fontFamily, paperSize);
  const itemSize = clampSize(kot.items.size, "kotItems", fontFamily, paperSize);
  const meta: string[] = [];
  if (kot.showBillNo) meta.push("Bill No: 1234");
  if (kot.showOrderType) meta.push("Order: Dining");
  if (kot.showTable) meta.push("Table: T2");
  if (kot.showDate) meta.push("Date: 26/02 12:30 pm");

  return (
    <div className="paper-preview" style={{ width, fontFamily }}>
      <div style={{ textAlign: "center", marginBottom: 10 }}>
        {kot.showToken && (
          <>
            <div style={{ margin: "8px 0", fontWeight: "bold", fontSize: tokenFontSize(metaSize, tokenPrintSize) }}>
              TOKEN: 105
            </div>
            {kot.separators.token && <Sep pattern={linePattern} />}
          </>
        )}
        {kot.showTitle && (
          <>
            <div style={{ fontWeight: kot.title.bold ? "bold" : "normal", fontSize: kot.title.size }}>--- KOT ---</div>
            {kot.separators.header && <Sep pattern={linePattern} />}
          </>
        )}
      </div>

      <div style={{ fontSize: metaSize, fontWeight: kot.meta.bold ? "bold" : "normal" }}>
        {meta.length > 0 &&
          (kot.metaTwoColumn
            ? Array.from({ length: Math.ceil(meta.length / 2) }, (_, i) => (
                <div key={i} style={{ display: "flex", justifyContent: "space-between", gap: 12 }}>
                  <span>{meta[i * 2]}</span>
                  <span style={{ textAlign: "right" }}>{meta[i * 2 + 1] || ""}</span>
                </div>
              ))
            : meta.map((m, i) => <div key={i}>{m}</div>))}

        {meta.length > 0 && kot.separators.meta && <Sep pattern={linePattern} />}

        <table
          style={{
            width: "100%",
            borderCollapse: "collapse",
            margin: "5px 0",
            fontSize: itemSize,
            fontWeight: kot.items.bold ? "bold" : "normal",
          }}
        >
          <thead>
            <tr style={{ borderBottom: kot.separators.tableHeader ? LINE_CSS[linePattern] : "none" }}>
              <th style={{ textAlign: "left", padding: kot.rowHeight, fontWeight: "inherit" }}>Item</th>
              <th style={{ textAlign: "right", padding: kot.rowHeight, fontWeight: "inherit" }}>Qty</th>
            </tr>
          </thead>
          <tbody>
            {[
              ["Paneer Tikka", 1],
              ["Butter Naan", 2],
              ["Dal Makhani", 1],
            ].map(([name, qty]) => (
              <tr key={String(name)}>
                <td style={{ padding: kot.rowHeight }}>{name}</td>
                <td style={{ textAlign: "right", padding: kot.rowHeight }}>{qty}</td>
              </tr>
            ))}
          </tbody>
        </table>

        {kot.separators.tableBody && <Sep pattern={linePattern} />}
      </div>
    </div>
  );
}
