/** The totals block on the till. */

import { Scroller } from '../kit';
import type { BillView } from '../ipc/generated/BillView';
import type { MoneyView } from '../ipc/generated/MoneyView';

export function Totals({ bill }: { bill: BillView }) {
  return (
    <div className="mb-totals">
      <Scroller inset className="mb-totals__breakdown">
      <Row label="Subtotal" value={bill.subtotal} />

      {isPositive(bill.lineDiscount) ? (
        <Row muted label="Line discounts" value={bill.lineDiscount} negative />
      ) : null}
      {isPositive(bill.billDiscount) ? (
        <Row muted label="Bill discount" value={bill.billDiscount} negative />
      ) : null}

      {/*
        "a discount that had to be capped says so; the flag reaches the bill." It reaches
        `Bill`; dropping it here would be the flag dying on its last hop after travelling three
        phases.
      */}
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

      </Scroller>

      {/* PINNED, and outside the scrolling breakdown. */}
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
      {/*
        The minus sign is a LABEL, not arithmetic: Rust sends the magnitude of a discount and
        the screen says which way it goes.
      */}
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
