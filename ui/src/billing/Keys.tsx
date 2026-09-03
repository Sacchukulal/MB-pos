/** The keyboard, on screen: the suggestion list, the how-many box, the table box, the help sheet. */

import { useEffect, useRef, useState } from 'react';

import { Button, Input, Modal, cx, onlyAmount } from '../kit';
import type { MenuItemView } from '../ipc/generated/MenuItemView';
import type { TableView } from '../ipc/generated/TableView';
import { SHORTCUTS, type Mode } from './keyboard';

/** The menu items that match what was typed, under the search box. */
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
    <ul className="mb-suggestions mb-sheet" role="listbox" aria-label="Menu suggestions">
      {items.map((item, index) => (
        <li key={item.id}>
          <button
            type="button"
            role="option"
            aria-selected={index === highlighted}
            className={cx('mb-sheet__item', 'mb-suggestion')}
            // Touch reaches the same place Enter does.
            onClick={() => onPick(index)}
          >
            <span className="mb-suggestion__name">{item.name}</span>
            <span className="mb-suggestion__rate">{item.rateLabel}</span>
            <span className="mb-suggestion__price">{item.price.text}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}

/**
 * How many of the chosen item, in a dialog in the middle of the screen. The keyboard engine
 * owns every key in it (Enter adds, arrows step, Esc leaves), so nothing here decides.
 */
export function HowMany({
  mode,
  onChange,
  onAdd,
  onLeave,
}: {
  mode: Extract<Mode, { kind: 'quantity' }>;
  onChange: (text: string) => void;
  /** The button, for a hand on the screen — the same as Enter. */
  onAdd: () => void;
  /** The dialog's own close: the same as Esc. */
  onLeave: () => void;
}) {
  const box = useRef<HTMLInputElement>(null);
  // The "1" is selected, so typing a number replaces it and Enter alone keeps it.
  useEffect(() => {
    box.current?.focus();
    box.current?.select();
  }, []);
  return (
    <Modal
      open
      title={`${mode.item.name} · ${mode.item.price.text}`}
      onClose={onLeave}
      actions={
        <>
          <Button onMouseDown={(event) => event.preventDefault()} onClick={onLeave}>
            Leave it
          </Button>
          <Button variant="primary" onMouseDown={(event) => event.preventDefault()} onClick={onAdd}>
            Add
          </Button>
        </>
      }
    >
      <div className="mb-ask">
        <input
          ref={box}
          className="mb-input mb-input--number mb-ask__box"
          data-keys="engine"
          inputMode="decimal"
          autoComplete="off"
          aria-label="How many"
          value={mode.text}
          onChange={(event) => onChange(onlyAmount(event.target.value))}
        />
        <span className="mb-ask__keys">
          <kbd className="mb-kbd">↑ ↓</kbd> more or fewer <kbd className="mb-kbd">Enter</kbd> add{' '}
          <kbd className="mb-kbd">Esc</kbd> leave it
        </span>
      </div>
    </Modal>
  );
}

/**
 * Which table, asked in the middle of the screen when Enter lands on a dine-in cart with no
 * table: one box for the number, and Enter opens that table.
 */
export function TableBox({
  tables,
  onOpen,
  onClose,
}: {
  tables: readonly TableView[];
  onOpen: (table: TableView) => void;
  onClose: () => void;
}) {
  const [typed, setTyped] = useState('');
  const [problem, setProblem] = useState<string | undefined>();
  const submit = () => {
    const wanted = typed.trim().toLowerCase();
    const table = tables.find((t) => t.label.toLowerCase() === wanted);
    if (table) onOpen(table);
    else setProblem(wanted === '' ? 'Type the table number.' : `There is no table ${typed.trim()}.`);
  };

  return (
    <Modal
      open
      title="Which table?"
      onClose={onClose}
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={submit}>
            Open
          </Button>
        </>
      }
    >
      <Input
        label="Table number"
        value={typed}
        autoFocus
        autoComplete="off"
        error={problem}
        onChange={(event) => {
          setTyped(event.target.value);
          setProblem(undefined);
        }}
        onKeyDown={(event) => {
          if (event.key === 'Enter') {
            event.preventDefault();
            submit();
          }
        }}
      />
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
