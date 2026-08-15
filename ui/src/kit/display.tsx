/**
 * The things that show rather than collect: surfaces, tables, badges, states.
 *
 * Two rules run through all of them.
 *
 * **Digits align wherever they are compared** (§3). `Money` and `Numeric` exist
 * so that never depends on somebody remembering a class name — *"a column of
 * rupees that doesn't line up looks broken to a shopkeeper."*
 *
 * **Colour is never the only signal** (§2 rule 2). A `Badge` has a border and a
 * label as well as a fill; grey-scale the screen and every state is still
 * legible.
 */

import type { ReactNode } from 'react';

import type { MoneyView } from '../ipc/generated/MoneyView';

export function Card({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={['mb-card', className ?? ''].filter(Boolean).join(' ')}>
      {children}
    </div>
  );
}

export function SectionHeader({
  title,
  note,
  action,
}: {
  title: string;
  note?: string;
  action?: ReactNode;
}) {
  return (
    <div className="mb-section">
      <h3 className="mb-section__title">{title}</h3>
      {note ? <span className="mb-section__note">{note}</span> : null}
      {action}
    </div>
  );
}

/**
 * An amount, exactly as Rust formatted it.
 *
 * **This component cannot compute anything, and that is the point.** It takes a
 * `MoneyView` — integer paise plus the string `Money::to_plain_string`
 * produced — and renders the string. R8, and D2: JavaScript has no integers,
 * so a rupee that TypeScript touched is a rupee that might be wrong.
 */
export function Money({ value }: { value: MoneyView }) {
  return (
    <span className="mb-numeric" data-paise={value.paise}>
      {value.text}
    </span>
  );
}

/** Any other figure that sits in a column — a count, a quantity, a percentage. */
export function Numeric({ children }: { children: ReactNode }) {
  return <span className="mb-numeric">{children}</span>;
}

export function StatCard({
  label,
  value,
  note,
}: {
  label: string;
  value: ReactNode;
  /**
   * The sentence under the figure — *"What the till expects, before
   * counting."*
   *
   * **A parameter of its own, and P27.5 is why.** The dashboard used to pass
   * its note inside `value`, which is the element that carries
   * `font-family: var(--font-mono)` so that a column of rupees lines up (§3).
   * So every explanatory sentence on the reports screen rendered in a
   * monospace face — prose set like a terminal, on the screen an owner reads
   * most carefully. Found by opening the screen and looking at it (D55).
   */
  note?: ReactNode;
}) {
  return (
    <Card>
      <div className="mb-stat">
        <span className="mb-stat__label">{label}</span>
        <span className="mb-stat__value">{value}</span>
        {note ? <span className="mb-stat__note">{note}</span> : null}
      </div>
    </Card>
  );
}

export type BadgeTone =
  | 'neutral'
  | 'ok'
  | 'warn'
  | 'danger'
  | 'info'
  | 'accent';

/**
 * A state, in a shape and a colour and a word.
 *
 * Note there is an `accent` tone and it is **never** used for a semantic state
 * (§2 rule 1): changing a shop's accent must not make "paid" and "void" look
 * alike, so "paid" is `ok` and stays `ok` whatever the owner picks.
 */
export function Badge({
  tone = 'neutral',
  children,
}: {
  tone?: BadgeTone;
  children: ReactNode;
}) {
  return <span className={`mb-badge mb-badge--${tone}`}>{children}</span>;
}

export interface Column<Row> {
  key: string;
  header: string;
  /** Right-aligned and tabular. Money and counts, always. */
  numeric?: boolean;
  render: (row: Row) => ReactNode;
}

export function Table<Row>({
  columns,
  rows,
  rowKey,
  empty,
  footer,
}: {
  columns: readonly Column<Row>[];
  rows: readonly Row[];
  rowKey: (row: Row) => string;
  empty?: ReactNode;
  /**
   * **The totals row, and it belongs to the table.**
   *
   * P18 first drew it as a second `<table>` underneath, and the columns did
   * not line up — two tables cannot agree on a column width. §3: *"a column of
   * rupees that doesn't line up looks broken to a shopkeeper."* One cell per
   * column, or nothing.
   */
  footer?: readonly ReactNode[];
}) {
  if (rows.length === 0 && empty) return <>{empty}</>;
  return (
    <table className="mb-table">
      <thead>
        <tr>
          {columns.map((column) => (
            <th
              key={column.key}
              className={column.numeric ? 'mb-numeric' : undefined}
            >
              {column.header}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((row) => (
          <tr key={rowKey(row)}>
            {columns.map((column) => (
              <td
                key={column.key}
                className={column.numeric ? 'mb-numeric' : undefined}
              >
                {column.render(row)}
              </td>
            ))}
          </tr>
        ))}
      </tbody>
      {footer ? (
        <tfoot>
          <tr>
            {columns.map((column, index) => (
              <td
                key={column.key}
                className={column.numeric ? 'mb-numeric' : undefined}
              >
                {footer[index] ?? null}
              </td>
            ))}
          </tr>
        </tfoot>
      ) : null}
    </table>
  );
}

export function Tabs({
  tabs,
  active,
  onChange,
}: {
  tabs: readonly { id: string; label: string }[];
  active: string;
  onChange: (id: string) => void;
}) {
  return (
    <div className="mb-tabs" role="tablist">
      {tabs.map((tab) => (
        <button
          key={tab.id}
          type="button"
          role="tab"
          className="mb-tab"
          aria-selected={tab.id === active}
          onClick={() => onChange(tab.id)}
        >
          {tab.label}
        </button>
      ))}
    </div>
  );
}

/**
 * Nothing here yet — and it says what to do about it.
 *
 * UI_GUIDELINES §6: a message written from the cashier's side of the screen. An
 * empty state that only says "No data" has wasted the one moment somebody was
 * looking for guidance.
 */
export function EmptyState({
  title,
  body,
  action,
}: {
  title: string;
  body?: string;
  action?: ReactNode;
}) {
  return (
    <div className="mb-empty">
      <span className="mb-empty__title">{title}</span>
      {body ? <span>{body}</span> : null}
      {action}
    </div>
  );
}

export function Spinner({ label = 'Working' }: { label?: string }) {
  return (
    <span className="mb-row">
      <span className="mb-spinner" aria-hidden="true" />
      <span className="mb-visually-hidden">{label}</span>
    </span>
  );
}

/**
 * The unsaved-changes guard — Save / Discard / Cancel, v1's behaviour with new
 * code.
 *
 * It is a component rather than a pattern because the alternative is what audit
 * F10 describes happening to confirmations: every screen inventing its own, and
 * one of them forgetting.
 */
export function SaveBar({
  dirty,
  onSave,
  onDiscard,
  saving,
  note,
}: {
  dirty: boolean;
  onSave: () => void;
  onDiscard: () => void;
  saving?: boolean;
  note?: string;
}) {
  if (!dirty) return null;
  return (
    <div className="mb-savebar">
      <span className="mb-savebar__note">{note ?? 'You have changes that are not saved.'}</span>
      <div className="mb-savebar__actions">
        <ButtonImport onDiscard={onDiscard} onSave={onSave} saving={saving} />
      </div>
    </div>
  );
}

// Kept separate so `display.tsx` does not import `controls.tsx` at module
// scope, which would make the two files a cycle the moment a control wants a
// badge.
function ButtonImport({
  onDiscard,
  onSave,
  saving,
}: {
  onDiscard: () => void;
  onSave: () => void;
  saving?: boolean;
}) {
  return (
    <>
      <button type="button" className="mb-button mb-button--quiet" onClick={onDiscard}>
        Discard
      </button>
      <button
        type="button"
        className="mb-button mb-button--primary"
        onClick={onSave}
        disabled={saving}
      >
        {saving ? 'Saving…' : 'Save'}
      </button>
    </>
  );
}

/**
 * A date range. P18's reports live on this; here it is the control, not the
 * calendar — the native picker is keyboard-reachable, touch-friendly and
 * already translated by the operating system, which a hand-built calendar
 * would not be until P23.
 */
export function DateRangePicker({
  from,
  to,
  onChange,
}: {
  from: string;
  to: string;
  onChange: (from: string, to: string) => void;
}) {
  return (
    <div className="mb-daterange">
      <div className="mb-field">
        <label className="mb-field__label" htmlFor="mb-range-from">
          From
        </label>
        <input
          id="mb-range-from"
          type="date"
          className="mb-input"
          value={from}
          onChange={(event) => onChange(event.target.value, to)}
        />
      </div>
      <div className="mb-field">
        <label className="mb-field__label" htmlFor="mb-range-to">
          To
        </label>
        <input
          id="mb-range-to"
          type="date"
          className="mb-input"
          value={to}
          onChange={(event) => onChange(from, event.target.value)}
        />
      </div>
    </div>
  );
}
