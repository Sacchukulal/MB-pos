import { QrCode, UtensilsCrossed } from "lucide-react";
import { PAPER } from "../../config/constants";
import type { BillDesign, KotDesign, PaperSize, StoreProfile } from "../../types";

/**
 * On-screen approximations of the printed bill and KOT. Always white paper /
 * black ink (the only sanctioned hardcoded colors — real receipts don't theme).
 */

const dashed = { borderTop: "1px dashed #000", margin: "8px 0" } as const;

interface BillPreviewProps {
  bill: BillDesign;
  store: StoreProfile;
  paperSize: PaperSize;
}

export function BillPreview({ bill, store, paperSize }: BillPreviewProps) {
  const width = PAPER[paperSize]?.previewPx ?? 320;
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
        <div style={{ fontSize: bill.addressMeta.size, marginTop: 4, fontWeight: bill.addressMeta.bold ? "bold" : "normal" }}>
          {bill.showAddress && <div>{store.address || "123, Street Name, City"}</div>}
          {bill.showPhone && <div>Tel: {store.phoneNumber || "9876543210"}</div>}
          {bill.showGstin && store.gstNumber && <div>GSTIN: {store.gstNumber}</div>}
          {bill.showFssai && store.fssaiNumber && <div>FSSAI: {store.fssaiNumber}</div>}
        </div>
      </div>

      {bill.separators.header && <div style={dashed} />}

      {/* Meta */}
      <div style={{ fontSize: bill.addressMeta.size, fontWeight: bill.addressMeta.bold ? "bold" : "normal" }}>
        <div style={{ display: "flex", justifyContent: "space-between" }}>
          <div>Bill No: 1234</div>
          <div>Date: 26-Feb-2026</div>
        </div>
        <div style={{ display: "flex", justifyContent: "space-between" }}>
          <div>Time: 12:30 PM</div>
          {bill.showCashier && <div>Cashier: Admin</div>}
        </div>
      </div>

      {bill.separators.meta && <div style={dashed} />}

      {bill.showToken && (
        <>
          <div style={{ textAlign: "center", fontWeight: "bold", margin: "8px 0", fontSize: "1.2em" }}>TOKEN: 105</div>
          {bill.separators.token && <div style={dashed} />}
        </>
      )}

      {/* Items */}
      <table
        style={{
          width: "100%",
          borderCollapse: "collapse",
          margin: "5px 0",
          fontSize: bill.table.size,
          fontWeight: bill.table.bold ? "bold" : "normal",
        }}
      >
        <thead>
          <tr style={{ borderBottom: bill.separators.tableHeader ? "1px dashed #000" : "none" }}>
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

      {bill.separators.tableBody && <div style={dashed} />}

      {/* Totals */}
      <div style={{ fontSize: bill.totals.size, fontWeight: bill.totals.bold ? "bold" : "normal" }}>
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

      {bill.separators.subtotals && <div style={dashed} />}

      <div style={{ display: "flex", justifyContent: "space-between", fontWeight: "bold", fontSize: bill.totals.size }}>
        <span>GRAND TOTAL:</span>
        <span>
          {bill.gst.enabled && exclusive
            ? `Rs. ${(SAMPLE_SUBTOTAL * (1 + gstRate / 100)).toFixed(2)}`
            : `Rs. ${SAMPLE_SUBTOTAL.toFixed(2)}`}
        </span>
      </div>

      {bill.separators.grandTotal && <div style={{ borderBottom: "1px dashed #000", margin: "8px 0" }} />}

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
}

export function KotPreview({ kot, fontFamily, paperSize }: KotPreviewProps) {
  const width = PAPER[paperSize]?.previewPx ?? 320;
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
            <div style={{ margin: "8px 0", fontWeight: "bold", fontSize: "1.4em" }}>TOKEN: 105</div>
            {kot.separators.token && <div style={dashed} />}
          </>
        )}
        {kot.showTitle && (
          <>
            <div style={{ fontWeight: kot.title.bold ? "bold" : "normal", fontSize: kot.title.size }}>--- KOT ---</div>
            {kot.separators.header && <div style={dashed} />}
          </>
        )}
      </div>

      <div style={{ fontSize: kot.meta.size, fontWeight: kot.meta.bold ? "bold" : "normal" }}>
        {meta.length > 0 &&
          (kot.metaTwoColumn
            ? Array.from({ length: Math.ceil(meta.length / 2) }, (_, i) => (
                <div key={i} style={{ display: "flex", justifyContent: "space-between", gap: 12 }}>
                  <span>{meta[i * 2]}</span>
                  <span style={{ textAlign: "right" }}>{meta[i * 2 + 1] || ""}</span>
                </div>
              ))
            : meta.map((m, i) => <div key={i}>{m}</div>))}

        {meta.length > 0 && kot.separators.meta && <div style={dashed} />}

        <table
          style={{
            width: "100%",
            borderCollapse: "collapse",
            margin: "5px 0",
            fontSize: kot.items.size,
            fontWeight: kot.items.bold ? "bold" : "normal",
          }}
        >
          <thead>
            <tr style={{ borderBottom: kot.separators.tableHeader ? "1px dashed #000" : "none" }}>
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

        {kot.separators.tableBody && <div style={dashed} />}
      </div>
    </div>
  );
}
