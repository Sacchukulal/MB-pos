/**
 * The keyboard, on screen: the suggestion list, the quantity popup, the busy-table chooser and
 * the help sheet.
 */

import { useEffect, useRef } from 'react';

import { Button, Icon, Modal } from '../kit';
import type { MenuItemView } from '../ipc/generated/MenuItemView';
import { SHORTCUTS, SUB_TABLE_LETTERS, type Mode } from './keyboard';
import type { TableView } from '../ipc/generated/TableView';

export function Suggestions({
  items,
  highlighted,
  onPick,
}: {
  items: readonly MenuItemView[];
  highlighted: number;
  onPick: (index: number) => void;
}) {
  if (items.length === 0) return null;
  return (
    <ul className="mb-suggestions" role="listbox" aria-label="Menu suggestions">
      {items.map((item, index) => (
        <li key={item.id}>
          <button
            type="button"
            role="option"
            aria-selected={index === highlighted}
            className={[
              'mb-suggestion',
              index === highlighted ? 'mb-suggestion--on' : '',
            ]
              .filter(Boolean)
              .join(' ')}
            // Touch reaches the same place Enter does.
            onClick={() => onPick(index)}
          >
            <span className="mb-cartline__name">{item.name}</span>
            <span className="mb-cartline__rate">{item.rateLabel}</span>
            {/* Its own class, not the table tile's. */}
            <span className="mb-suggestion__price">{item.price.text}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}

/** The quantity popup. */
export function QuantityPopup({
  mode,
  onType,
  onConfirm,
  onCancel,
  onWeigh,
}: {
  mode: Extract<Mode, { kind: 'quantity' }>;
  onType: (text: string) => void;
  onConfirm: () => void;
  onCancel: () => void;
  onWeigh?: () => void;
}) {
  const box = useRef<HTMLInputElement>(null);

  // The quantity field takes focus, and it has to win a fight to get it.
  useEffect(() => {
    const id = requestAnimationFrame(() => box.current?.focus());
    return () => cancelAnimationFrame(id);
  }, []);

  const shown = mode.typed === '' ? '1' : mode.typed;
  return (
    <Modal
      open
      title={mode.item.name}
      onClose={onCancel}
      actions={
        <>
          <Button onClick={onCancel}>Cancel</Button>
          <Button variant="primary" onClick={onConfirm}>
            Add
          </Button>
        </>
      }
    >
      <div className="mb-quantity">
        <Button
          onClick={() => onType(step(shown, -1))}
          aria-label="One fewer"
        >
          −
        </Button>
        <input
          className="mb-input mb-input--number mb-quantity__value"
          ref={box}
          value={mode.typed}
          placeholder="1"
          inputMode="decimal"
          aria-label="Quantity"
          data-keys="engine"
          onChange={(event) => onType(event.target.value)}
        />
        <Button onClick={() => onType(step(shown, 1))} aria-label="One more">
          +
        </Button>
        {onWeigh ? (
          <Button onClick={onWeigh} aria-label="Take the weight from the scale">
            <Icon name="scale" size="sm" />
            Weigh
          </Button>
        ) : null}
      </div>
      <span className="mb-field__hint">
        Type a quantity — 2, or 0.5 for half a kilo. Blank means one.
      </span>
    </Modal>
  );
}

/** Plus and minus, on a quantity — which is a count, not money. */
function step(current: string, by: number): string {
  const asNumber = Number(current);
  if (!Number.isFinite(asNumber)) return '1';
  const next = Math.max(0, asNumber + by);
  return next === 0 ? '' : String(Number(next.toFixed(3)));
}

/** A busy table: merge, or take a sub-table letter. */
export function BusyTable({
  mode,
  taken,
  onChoose,
  onCancel,
}: {
  mode: Extract<Mode, { kind: 'table-busy' }>;
  taken: readonly string[];
  onChoose: (choice: number) => void;
  onCancel: () => void;
}) {
  return (
    <Modal
      open
      title={`Table ${mode.table.label} is busy`}
      onClose={onCancel}
      wide
    >
      <div className="mb-stack">
        <span>
          Add these items to the order already on that table, or start a second
          one beside it.
        </span>
        <div className="mb-busy__choices">
          <Button
            variant={mode.choice === 0 ? 'primary' : 'secondary'}
            onClick={() => onChoose(0)}
          >
            Merge into {mode.table.label}
          </Button>
          {SUB_TABLE_LETTERS.map((letter, index) => (
            <Button
              key={letter}
              variant={mode.choice === index + 1 ? 'primary' : 'secondary'}
              // A letter already in use is refused rather than hidden, so the floor's shape
              // stays legible.
              disabled={taken.includes(letter)}
              onClick={() => onChoose(index + 1)}
            >
              {mode.table.label}
              {letter}
            </Button>
          ))}
        </div>
        <span className="mb-field__hint">
          Arrows move, Enter chooses, Esc goes back. Only the new items go to
          the kitchen.
        </span>
      </div>
    </Modal>
  );
}

/** The shortcut sheet. */
export function HelpSheet({ onClose }: { onClose: () => void }) {
  const groups = [...new Set(SHORTCUTS.map((s) => s.group))];
  return (
    <Modal
      open
      title="Keyboard shortcuts"
      onClose={onClose}
      wide
      actions={
        <>
          <Button onClick={() => window.print()}>Print this sheet</Button>
          <Button variant="primary" onClick={onClose}>
            Close
          </Button>
        </>
      }
    >
      <div className="mb-help">
        {groups.map((group) => (
          <div className="mb-help__group" key={group}>
            <span className="mb-floor__heading">{group}</span>
            {SHORTCUTS.filter((s) => s.group === group).map((shortcut) => (
              <div className="mb-help__row" key={`${group}-${shortcut.keys}-${shortcut.what}`}>
                <kbd className="mb-help__keys">{shortcut.keys}</kbd>
                <span>{shortcut.what}</span>
              </div>
            ))}
          </div>
        ))}
      </div>
    </Modal>
  );
}

/** Which sub-table letters are already in use on a table. */
export function takenLetters(
  tables: readonly TableView[],
  label: string,
): string[] {
  return tables
    .filter((t) => t.label.startsWith(label) && t.label.length === label.length + 1)
    .map((t) => t.label.slice(label.length));
}
