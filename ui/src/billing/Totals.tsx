/**
 * **The totals block on the till.**
 *
 * Four figures: what it came to, what came off, the tax, and what to say out
 * loud. The rate-by-rate breakdown — taxable value, CGST, SGST, IGST, the
 * liquor line — is still computed for every bill and still goes on the PRINTED
 * bill and into the reports. It is not on this screen, because a cashier
 * reading a total to a customer is not filing a return.
 *
 * The old note, still true of the numbers themselves:
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
      <div className="mb-totals__breakdown">
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

      {isPositive(bill.taxTotal) ? <Row label="Tax" value={bill.taxTotal} /> : null}

      {isNonZero(bill.roundOff) ? (
        <Row muted label="Round off" value={bill.roundOff} />
      ) : null}

      </div>

      {/* **PINNED, and outside the scrolling breakdown.** The number a cashier
          reads out to the customer is the one thing on this panel that may
          never need scrolling to. Found by billing a dosa, a cola and a beer:
          six tax rows pushed TOTAL out of view, which was a worse bug than the
          one the scroll was fixing. */}
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

function isPositive(value: MoneyView): boolean {
  return value.paise > 0n;
}

function isNonZero(value: MoneyView): boolean {
  return value.paise !== 0n;
}
