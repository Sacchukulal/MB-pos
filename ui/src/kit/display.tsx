/** The things that show rather than collect: surfaces, tables, badges, states. */

import type { ReactNode } from 'react';

import type { MoneyView } from '../ipc/generated/MoneyView';
import { cx } from './cx';
import { Button } from './controls';
import { InfoTip } from './InfoTip';

export function Card({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={cx('mb-card', className)}>
      {children}
    </div>
  );
}

/** A heading for a block within a screen. */
export function SectionHeader({
  title,
  note,
  action,
  sticky = false,
}: {
  title: string;
  note?: ReactNode;
  action?: ReactNode;
  /** Stays put while the section under it scrolls. */
  sticky?: boolean;
}) {
  return (
    <div className={cx('mb-section', sticky && 'mb-section--sticky')}>
      <h3 className="mb-section__title">{title}</h3>
      {note ? <InfoTip label={`About ${title}`}>{note}</InfoTip> : null}
      {action ? <div className="mb-section__action">{action}</div> : null}
    </div>
  );
}

/** An amount, exactly as Rust formatted it. */
export function Money({
  value,
  symbol = false,
}: {
  value: MoneyView;
  /** Show the ₹ beside the figure. */
  symbol?: boolean;
}) {
  return (
    <span className="mb-numeric" data-paise={value.paise}>
      {symbol ? '₹' : ''}
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
  /** The sentence under the figure — "What the till expects, before counting.". */
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

/** A state, in a shape and a colour and a word. */
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
  /** The column disappears when no row has one. */
  optional?: boolean;
  /** Never wraps — a time, a number that is not money. */
  nowrap?: boolean;
  render: (row: Row) => ReactNode;
}

/** The classes a cell wears for its column. */
function cellClass<Row>(column: Column<Row>): string | undefined {
  const classes = [column.numeric ? 'mb-numeric' : '', column.nowrap ? 'mb-nowrap' : '']
    .filter(Boolean)
    .join(' ');
  return classes === '' ? undefined : classes;
}

/** What `optional` counts as nothing. */
function blank(cell: ReactNode): boolean {
  return cell === null || cell === undefined || cell === '' || cell === false;
}

export function Table<Row>({
  columns,
  rows,
  rowKey,
  empty,
  footer,
  dense = false,
}: {
  columns: readonly Column<Row>[];
  rows: readonly Row[];
  rowKey: (row: Row) => string;
  empty?: ReactNode;
  /** The totals row, and it belongs to the table. */
  footer?: readonly ReactNode[];
  /** Tighter rows, for a list that runs to hundreds — a menu. */
  dense?: boolean;
}) {
  if (rows.length === 0 && empty) return <>{empty}</>;

  // Render once, here: an `optional` column has to be looked at to know whether it survives,
  // and calling `render` twice for that would be a second pass over every row.
  const cells = rows.map((row) => columns.map((column) => column.render(row)));
  const shown = columns.filter(
    (column, index) =>
      !column.optional || cells.some((cellsOfRow) => !blank(cellsOfRow[index])),
  );
  const keep = columns.map((column) => shown.includes(column));

  return (
    // The wrapper is what stops a squeezed table shredding: below the floor width it scrolls
    // sideways in its own box instead of wrapping a cell to one word per line.
    <div className="mb-table__wrap">
    <table className={cx('mb-table', dense && 'mb-table--dense')}>
      <thead>
        <tr>
          {shown.map((column) => (
            <th
              key={column.key}
              className={cellClass(column)}
            >
              {column.header}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((row, rowIndex) => (
          <tr key={rowKey(row)}>
            {columns.map((column, index) => {
              if (!keep[index]) return null;
              const cell = cells[rowIndex]?.[index];
              return (
                <td key={column.key} className={cellClass(column)}>
                  {column.optional && blank(cell) ? '—' : cell}
                </td>
              );
            })}
          </tr>
        ))}
      </tbody>
      {footer ? (
        <tfoot>
          <tr>
            {columns.map((column, index) =>
              !keep[index] ? null : (
                <td
                  key={column.key}
                  className={column.numeric ? 'mb-numeric' : undefined}
                >
                  {footer[index] ?? null}
                </td>
              ),
            )}
          </tr>
        </tfoot>
      ) : null}
    </table>
    </div>
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

/** Nothing here yet — and it says what to do about it. */
export function EmptyState({
  title,
  body,
  action,
  small = false,
}: {
  title: string;
  body?: string;
  action?: ReactNode;
  /** True inside a panel rather than a page — the cart, a drawer, a column. */
  small?: boolean;
}) {
  return (
    <div className={cx('mb-empty', small && 'mb-empty--small')}>
      <span className="mb-empty__title">{title}</span>
      {body ? <span>{body}</span> : null}
      {action}
    </div>
  );
}

/** A screen the licence does not open, said on the screen itself. */
export function Locked({
  says,
  onOpenAccount,
}: {
  says: string;
  onOpenAccount?: () => void;
}) {
  return (
    <EmptyState
      title="This part needs a licence"
      body={says}
      action={
        onOpenAccount ? (
          <Button variant="primary" onClick={onOpenAccount}>
            Open Account
          </Button>
        ) : undefined
      }
    />
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

/** The unsaved-changes guard. */
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

// Kept separate so `display.tsx` does not import `controls.tsx` at module scope, which would
// make the two files a cycle the moment a control wants a badge.
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
