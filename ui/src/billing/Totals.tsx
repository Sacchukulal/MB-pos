/**
 * **The totals block — a feature, not a footer.**
 *
 * > Audit **B11**: *"the tax report splits GST 50/50 into CGST/SGST always. No
 * > IGST, no inter-state, no HSN summary, and nothing that can be filed
 * > directly."*
 *
 * > Audit **B10**: *"one GST rate for the entire bill. A shop selling
 * > AC-restaurant food (5%) plus packaged water or cigarettes (12%/18%) cannot
 * > bill correctly."*
 *
 * This is where a chartered accountant first sees whether the product can be
 * filed from, so **it never collapses into one "GST" line**. Two rates means
 * two rows. Alcohol is listed on its own, outside every GST total, which is
 * scope 2.3 and the reason a bar can use this at all.
 *
 * Every figure is a `MoneyView` computed in Rust. There is not one arithmetic
 * operation in this file, and `check-no-money.mjs` fails the build if one
 * appears.
 */

import type { BillView } from '../ipc/generated/BillView';
import type { MoneyView } from '../ipc/generated/MoneyView';

export function Totals({ bill }: { bill: BillView }) {
  return (
    <div className="mb-totals">
      <Row label="Subtotal" value={bill.subtotal} />

      {isPositive(bill.lineDiscount) ? (
        <Row muted label="Line discounts" value={bill.lineDiscount} negative />
      ) : null}
      {isPositive(bill.billDiscount) ? (
        <Row muted label="Bill discount" value={bill.billDiscount} negative />
      ) : null}

      {/* D15: "a discount that had to be capped says so; the flag reaches the
          bill." It reaches `Bill`; dropping it here would be the flag dying on
          its last hop after travelling three phases. */}
      {bill.discountCapped ? (
        <span className="mb-totals__capped">
          The discount was larger than the bill allowed, so it was reduced.
        </span>
      ) : null}

      {bill.charges.map((charge) => (
        <Row
          key={charge.name}
          muted
          label={`${charge.name} (${charge.rateLabel})`}
          value={charge.amount}
        />
      ))}

      {/* Tax, BROKEN OUT BY RATE — scope 2.7. */}
      {bill.taxRows.length > 0 ? (
        <div className="mb-totals__tax">
          {bill.taxRows.map((row) => (
            <div key={row.rateLabel}>
              <Row muted label={`Taxable @ ${row.rateLabel}`} value={row.taxable} />
              {row.isInterstate ? (
                <Row label={`IGST ${row.rateLabel}`} value={row.igst} />
              ) : (
                <>
                  <Row label={`CGST ${halfOf(row.rateLabel)}`} value={row.cgst} />
                  <Row label={`SGST ${halfOf(row.rateLabel)}`} value={row.sgst} />
                </>
              )}
            </div>
          ))}
        </div>
      ) : null}

      {/* Scope 2.3 — the liquor line. NEVER inside a GST total. */}
      {isPositive(bill.nonGstValue) ? (
        <Row label="Non-GST value" value={bill.nonGstValue} />
      ) : null}
      {isPositive(bill.exemptValue) ? (
        <Row label="Exempt value" value={bill.exemptValue} />
      ) : null}

      {isNonZero(bill.roundOff) ? (
        <Row muted label="Round off" value={bill.roundOff} />
      ) : null}

      <Row grand label="TOTAL" value={bill.grandTotal} />
    </div>
  );
}

function Row({
  label,
  value,
  muted,
  grand,
  negative,
}: {
  label: string;
  value: MoneyView;
  muted?: boolean;
  grand?: boolean;
  negative?: boolean;
}) {
  const classes = [
    'mb-totals__row',
    muted ? 'mb-totals__row--muted' : '',
    grand ? 'mb-totals__row--grand' : '',
  ]
    .filter(Boolean)
    .join(' ');
  return (
    <div className={classes}>
      <span className="mb-totals__label">{label}</span>
      {/* The minus sign is a LABEL, not arithmetic: Rust sends the magnitude
          of a discount and the screen says which way it goes. */}
      <span className="mb-totals__value">
        {negative ? '−' : ''}
        {value.text}
      </span>
    </div>
  );
}

/**
 * "5%" → "2.5%".
 *
 * String work on a label, not arithmetic on money: CGST and SGST are each half
 * the rate by law, and the rate is a name here. The *amounts* were split in
 * mb-core, and this never touches them.
 */
function halfOf(rateLabel: string): string {
  const digits = rateLabel.replace('%', '');
  const asNumber = Number(digits);
  if (!Number.isFinite(asNumber)) return rateLabel;
  const half = asNumber / 2;
  return `${half % 1 === 0 ? half : half.toFixed(2).replace(/0$/, '')}%`;
}

function isPositive(value: MoneyView): boolean {
  return value.paise > 0n;
}

function isNonZero(value: MoneyView): boolean {
  return value.paise !== 0n;
}
