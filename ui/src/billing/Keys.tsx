/** The keyboard, on screen: the suggestion list, the how-many box, the table box, the help sheet. */

import { useEffect, useRef, useState } from 'react';

import { Button, Icon, Input, Modal, NumberInput, Pick, cx, onlyAmount } from '../kit';
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
  const list = useRef<HTMLUListElement>(null);
  // The arrows never leave the chosen row out of sight — the list is taller than its box.
  useEffect(() => {
    if (highlighted < 0) return;
    const row = list.current?.children[highlighted];
    // A test's DOM has no scrolling to do.
    if (row instanceof HTMLElement && typeof row.scrollIntoView === 'function') {
      row.scrollIntoView({ block: 'nearest' });
    }
  }, [highlighted]);

  if (items.length === 0) return null;
  return (
    <ul ref={list} className="mb-suggestions mb-sheet" role="listbox" aria-label="Menu suggestions">
      {items.map((item, index) => (
        <li key={item.id}>
          <Pick
            role="option"
            aria-selected={index === highlighted}
            className={cx('mb-sheet__item', 'mb-suggestion')}
            // Touch reaches the same place Enter does.
            onClick={() => onPick(index)}
          >
            <span className="mb-suggestion__name">{item.name}</span>
            <span className="mb-suggestion__price">{item.price.text}</span>
          </Pick>
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
      small
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
        <NumberInput
          ref={box}
          className="mb-ask__box"
          data-keys="engine"
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
  busy = false,
  onOpen,
  onClose,
}: {
  tables: readonly TableView[];
  busy?: boolean;
  onOpen: (table: TableView, seat?: string) => void;
  onClose: () => void;
}) {
  const [typed, setTyped] = useState('');
  const [problem, setProblem] = useState<string | undefined>();
  const [occupied, setOccupied] = useState<TableView | null>(null);
  const submit = () => {
    if (busy) return;
    const wanted = typed.trim().toLowerCase();
    const table = tables.find((t) => !t.seat && t.label.toLowerCase() === wanted);
    if (table?.orderId || (table && table.state !== 'free')) setOccupied(table);
    else if (table) onOpen(table);
    else setProblem(wanted === '' ? 'Type the table number.' : `There is no table ${typed.trim()}.`);
  };

  if (occupied) {
    return <OccupiedTableBox table={occupied} tables={tables} busy={busy} onOpen={onOpen} onClose={onClose} />;
  }

  return (
    <Modal
      open
      small
      title="Which table?"
      onClose={() => { if (!busy) onClose(); }}
      actions={
        <>
          <Button disabled={busy} onClick={onClose}>Cancel</Button>
          <Button disabled={busy} variant="primary" onClick={submit}>
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
        disabled={busy}
        error={problem}
        onChange={(event) => {
          setTyped(event.target.value);
          setProblem(undefined);
        }}
        onKeyDown={(event) => {
          if (event.key === 'Enter') {
            event.preventDefault();
            // React can mount the next dialog before this native event reaches document.
            // Keep this press from also confirming that dialog's default choice.
            event.stopPropagation();
            if (event.repeat) return;
            submit();
          }
        }}
      />
    </Modal>
  );
}

/** Keep the existing party, or carry the typed items to a separate bill on this table. */
function OccupiedTableBox({ table, tables, busy, onOpen, onClose }: {
  table: TableView;
  tables: readonly TableView[];
  busy: boolean;
  onOpen: (table: TableView, seat?: string) => void;
  onClose: () => void;
}) {
  const [seat, setSeat] = useState<string | undefined>();
  const picker = useRef<HTMLDivElement>(null);
  // A is the original party. Saved and unsaved parties both reserve their letters.
  // Saved subtable tiles carry the ORDER id, so match their printed seat in the same room.
  const taken = new Set(tables.filter((t) => t.seat && t.section === table.section
    && t.label === `${table.label}${t.seat}`).map((t) => t.seat));
  const choices: (string | undefined)[] = [
    undefined,
    ...Array.from('BCDEFGHIJKLMNOPQRSTUVWXYZ').filter((letter) => !taken.has(letter)),
  ];
  const available = seat === undefined || !taken.has(seat);
  const label = `${table.label}${seat ?? ''}`;
  const choose = () => {
    if (!busy && available) onOpen(table, seat);
  };
  const step = (direction: number) => {
    if (busy) return;
    const index = Math.max(0, choices.indexOf(seat));
    setSeat(choices[Math.max(0, Math.min(choices.length - 1, index + direction))]);
    picker.current?.focus();
  };
  useEffect(() => { picker.current?.focus(); }, []);

  return (
    <div onKeyDown={(event) => {
      if (event.key === 'Enter' && event.repeat) {
        event.preventDefault();
        event.stopPropagation();
        return;
      }
      if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
        event.preventDefault();
        event.stopPropagation();
        step(event.key === 'ArrowLeft' ? -1 : 1);
      }
    }}>
      <Modal
        open
        small
        title={`Table ${table.label} already has an order`}
        onClose={() => { if (!busy) onClose(); }}
        onEnter={choose}
        actions={<>
          <Button disabled={busy} onClick={onClose}>Cancel</Button>
          <Button disabled={busy || !available} variant="primary" onClick={choose}>Continue</Button>
        </>}
      >
        <div className="mb-ask">
          <div ref={picker} className="mb-table-choice" tabIndex={0} role="group" aria-label="Order table">
            <Button disabled={busy || seat === undefined} aria-label="Previous table option" onClick={() => step(-1)}>
              <Icon name="chevron-left" />
            </Button>
            <span className="mb-table-choice__label" aria-live="polite">{label}</span>
            <Button disabled={busy || seat === choices[choices.length - 1]} aria-label="Next table option" onClick={() => step(1)}>
              <Icon name="chevron-right" />
            </Button>
          </div>
          <span>{seat === undefined ? 'Add items to the existing order.' : `Create a separate order on table ${label}.`}</span>
          {!available ? <span role="alert">This subtable was just taken. Choose another letter.</span> : null}
          <span className="mb-ask__keys">
            <kbd className="mb-kbd">{'\u2190 \u2192'}</kbd> choose <kbd className="mb-kbd">Enter</kbd> continue
          </span>
        </div>
      </Modal>
    </div>
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
