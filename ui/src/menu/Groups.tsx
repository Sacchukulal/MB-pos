/**
 * **The menu's groups — and where each group's food is cooked.**
 *
 * # What was wrong
 *
 * `save_menu_category` has been in Rust since P13 and nothing ever called it.
 * A shop could see its groups down the left of the Menu screen and could not
 * add one, rename one or retire one — ever, by any route. The owner found it on
 * a real install; it is the first item on their list.
 *
 * `route_category` was reachable, but only from Settings > Printers, three
 * screens away from the place a person is standing when they think *"the
 * tandoor should get the kebabs"*. Both live here now, in one dialog, because
 * they are one thought: **what are my groups, and where does each one's food
 * get cooked?**
 *
 * # Two commands, one screen, and no third copy of anything
 *
 * This file owns no rule. `save_menu_category` decides what a group is,
 * `route_category` decides what a route is, and each returns the whole new list
 * so nothing here has to keep a second one in step (D4).
 *
 * # Why retiring is not deleting
 *
 * There is no `delete_menu_category` in Rust and this does not ask for one. A
 * group with a year of bills behind it cannot be removed without those bills
 * losing what they were — so "Remove" turns it OFF: it stops appearing on the
 * billing screen, its items keep working, and last April's report still adds
 * up. That is `isActive`, and the button says what it does.
 */

import { useCallback, useEffect, useState } from 'react';

import { Badge, Button, Icon, Input, Modal, Select } from '../kit';
import { call } from '../ipc/call';
import type { CategoryView } from '../ipc/generated/CategoryView';
import type { PrintersView } from '../ipc/generated/PrintersView';

export function Groups({
  categories,
  onChanged,
  onClose,
  onFailed,
}: {
  categories: readonly CategoryView[];
  /** The whole new list, straight from Rust. */
  onChanged: (fresh: readonly CategoryView[]) => void;
  onClose: () => void;
  onFailed: (cause: unknown) => void;
}) {
  const [adding, setAdding] = useState('');
  /** The group being renamed, and the name so far. `null` when nobody is. */
  const [renaming, setRenaming] = useState<{ id: string; name: string } | null>(null);
  const [busy, setBusy] = useState(false);
  /**
   * The printers, and the route each group already has.
   *
   * **Absent is a state, not a failure.** Routing needs `settings.printer`,
   * and a manager who may edit the menu but not the hardware still has every
   * right to be in this dialog. So a refusal leaves this `null` and the column
   * simply is not drawn — rather than a toast about a permission they did not
   * ask to use.
   */
  const [printers, setPrinters] = useState<PrintersView | null>(null);

  useEffect(() => {
    call('printer_setup')
      .then(setPrinters)
      .catch(() => setPrinters(null));
  }, []);

  const save = useCallback(
    async (id: string, name: string, isActive: boolean) => {
      setBusy(true);
      try {
        onChanged(await call('save_menu_category', { id, name, isActive }));
      } catch (cause) {
        onFailed(cause);
      } finally {
        setBusy(false);
      }
    },
    [onChanged, onFailed],
  );

  const add = async () => {
    const name = adding.trim();
    if (name === '') return;
    // The id is ours to make and never shown. `save_menu_category` upserts on
    // it, so a fresh one is what makes this an ADD rather than a rename of
    // whatever happened to be there.
    await save(`cat_${Date.now().toString(36)}`, name, true);
    setAdding('');
  };

  return (
    <Modal open title="Groups" onClose={onClose} wide>
      <p className="mb-muted">
        Groups are how your items are arranged on the billing screen — Tiffin,
        Drinks, Tandoor. Each group can also send its kitchen tickets to its own
        printer.
      </p>

      <div className="mb-groups__add">
        <Input
          label="Add a group"
          value={adding}
          autoFocus
          placeholder="Tandoor"
          onChange={(event) => setAdding(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') void add();
          }}
        />
        <Button variant="primary" disabled={busy || adding.trim() === ''} onClick={() => void add()}>
          <Icon name="plus" size="sm" />
          Add
        </Button>
      </div>

      {categories.length === 0 ? (
        <p className="mb-muted">
          No groups yet. A shop with a short menu does not need any — every item
          simply shows on one screen.
        </p>
      ) : (
        <ul className="mb-groups">
          {categories.map((category) => {
            const route = printers?.routes.find((r) => r.categoryId === category.id);
            return (
              <li key={category.id} className="mb-groups__row">
                <div className="mb-groups__what">
                  {renaming?.id === category.id ? (
                    <Input
                      label="Name"
                      value={renaming.name}
                      autoFocus
                      onChange={(event) =>
                        setRenaming({ id: category.id, name: event.target.value })
                      }
                      onKeyDown={(event) => {
                        if (event.key === 'Escape') setRenaming(null);
                        if (event.key !== 'Enter') return;
                        const name = renaming.name.trim();
                        setRenaming(null);
                        if (name !== '' && name !== category.name) {
                          void save(category.id, name, category.isActive);
                        }
                      }}
                    />
                  ) : (
                    <>
                      <strong>{category.name}</strong>
                      <span className="mb-muted">
                        {category.itemCount === 0n
                          ? 'nothing in it yet'
                          : `${category.itemCount} item(s)`}
                      </span>
                      {category.isActive ? null : <Badge tone="warn">Off the menu</Badge>}
                    </>
                  )}
                </div>

                {/* **Where this group's food is cooked** — `route_category`,
                    which existed and lived three screens away from here. */}
                {printers ? (
                  <div className="mb-groups__route">
                    <Select
                      label="Kitchen printer"
                      value={route?.printerId ?? ''}
                    onChange={(event) => {
                      const printerId = event.currentTarget.value;
                      call('route_category', { categoryId: category.id, printerId })
                        .then(setPrinters)
                        .catch(onFailed);
                    }}
                    options={[
                      // Short, because the box is narrow and a label that ellipsises is a
                      // label nobody can read. The heading above it says "Kitchen printer".
                      { value: '', label: 'The usual one' },
                      ...printers.printers
                        .filter((p) => p.role === 'kitchen' || p.role === 'both')
                        .map((p) => ({ value: p.id, label: p.name })),
                    ]}
                  />
                  </div>
                ) : null}

                <div className="mb-row">
                  <Button
                    small
                    disabled={busy}
                    onClick={() => setRenaming({ id: category.id, name: category.name })}
                  >
                    Rename
                  </Button>
                  <Button
                    small
                    variant="quiet"
                    disabled={busy}
                    onClick={() => void save(category.id, category.name, !category.isActive)}
                  >
                    {category.isActive ? 'Remove' : 'Put back'}
                  </Button>
                </div>
              </li>
            );
          })}
        </ul>
      )}

      <div className="mb-row mb-row--end">
        <Button variant="primary" onClick={onClose}>
          Done
        </Button>
      </div>
    </Modal>
  );
}
