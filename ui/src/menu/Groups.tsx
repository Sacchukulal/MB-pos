/** The menu's groups — and where each group's food is cooked. */

import { useCallback, useEffect, useState } from 'react';

import { Badge, Button, freshId, Icon, Input, Modal, plural, Select } from '../kit';
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
  /** The group being renamed, and the name so far. */
  const [renaming, setRenaming] = useState<{ id: string; name: string } | null>(null);
  const [busy, setBusy] = useState(false);
  /** The printers, and the route each group already has. */
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
    // The id is ours to make and never shown.
    await save(freshId('cat'), name, true);
    setAdding('');
  };

  return (
    <Modal open title="Categories" onClose={onClose} wide>
      <p className="mb-muted">
        Categories are how your menu is arranged — Tiffin, Drinks, Tandoor.
        Each category can also send its kitchen tickets to its own printer.
      </p>

      <div className="mb-groups__add">
        <Input
          label="Add a category"
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
          No categories yet. A shop with a short menu does not need any — every
          item simply shows on one screen.
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
                          : plural(category.itemCount, 'item')}
                      </span>
                      {category.isActive ? null : <Badge tone="warn">Off the menu</Badge>}
                    </>
                  )}
                </div>

                {/*
                  Where this group's food is cooked — `route_category`, which existed and lived
                  three screens away from here.
                */}
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
                      // Short, because the box is narrow and a label that ellipsises is a label
                      // nobody can read.
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
