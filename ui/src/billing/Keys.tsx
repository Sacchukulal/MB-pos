/** The keyboard, on screen: the suggestion list, the how-many box and the help sheet. */

import { useEffect, useRef } from 'react';

import { Button, Modal, onlyAmount } from '../kit';
import type { MenuItemView } from '../ipc/generated/MenuItemView';
import { SHORTCUTS, type Mode } from './keyboard';

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
            <span className="mb-suggestion__name">{item.name}</span>
            <span className="mb-suggestion__rate">{item.rateLabel}</span>
            {/* Its own class, not the table tile's. */}
            <span className="mb-suggestion__price">{item.price.text}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}

/**
 * How many of the chosen item. A box under the search box, not a dialog: the keyboard engine
 * owns every key in it (Enter adds, arrows step, Esc leaves), so nothing here decides.
 */
export function HowMany({
  mode,
  onChange,
  onAdd,
}: {
  mode: Extract<Mode, { kind: 'quantity' }>;
  onChange: (text: string) => void;
  /** The button, for a hand on the screen — the same as Enter. */
  onAdd: () => void;
}) {
  const box = useRef<HTMLInputElement>(null);
  // The "1" is selected, so typing a number replaces it and Enter alone keeps it.
  useEffect(() => {
    box.current?.focus();
    box.current?.select();
  }, []);
  return (
    <div className="mb-ask" role="dialog" aria-label={`How many ${mode.item.name}`}>
      <div className="mb-ask__what">
        <span className="mb-ask__name">{mode.item.name}</span>
        <span className="mb-ask__price">{mode.item.price.text}</span>
      </div>
      <div className="mb-ask__row">
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
        <Button variant="primary" onMouseDown={(event) => event.preventDefault()} onClick={onAdd}>
          Add
        </Button>
      </div>
      <span className="mb-ask__keys">
        <kbd className="mb-kbd">↑ ↓</kbd> more or fewer <kbd className="mb-kbd">Enter</kbd> add{' '}
        <kbd className="mb-kbd">Esc</kbd> leave it
      </span>
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
