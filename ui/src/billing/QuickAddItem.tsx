import { useEffect, useRef, useState } from 'react';
import { Button, Input, Modal, MoneyInput, Select, freshId, useReport } from '../kit';
import { call } from '../ipc/call';
import type { CartView } from '../ipc/generated/CartView';
import type { CategoryView } from '../ipc/generated/CategoryView';
import type { MenuItemView } from '../ipc/generated/MenuItemView';
import type { MenuRowView } from '../ipc/generated/MenuRowView';
import type { TaxPageView } from '../ipc/generated/TaxPageView';

const normalise = (name: string) => name.trim().replace(/\s+/g, ' ').toLowerCase();

/** Another entry point to the Menu save command, followed by a normal cart add. */
export function QuickAddItem({ initialName, onClose, onAdded, onMenu }: {
  initialName: string;
  onClose: () => void;
  onAdded: (cart: CartView) => void;
  onMenu: (items: MenuItemView[]) => void;
}) {
  const [name, setName] = useState(initialName);
  const [price, setPrice] = useState('');
  const [category, setCategory] = useState('');
  const [categories, setCategories] = useState<CategoryView[]>([]);
  const [tax, setTax] = useState<TaxPageView | null>(null);
  const [existing, setExisting] = useState<MenuRowView | null>(null);
  const [savedId, setSavedId] = useState<string | null>(null);
  const id = useRef(freshId('itm'));
  const saving = useRef(false);
  const priceBox = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);
  const report = useReport();
  useEffect(() => {
    priceBox.current?.focus();
    Promise.all([call('menu_categories'), call('tax_page')]).then(([rows, defaults]) => {
      const live = rows.filter((row) => row.isActive);
      setCategories(live);
      setCategory(live[0]?.id ?? '');
      setTax(defaults);
    }).catch(report);
  }, [report]);
  const slabId = categories.find((row) => row.id === category)?.defaultSlabId ?? tax?.shopSlabId;
  const slab = tax?.slabs.find((row) => row.id === slabId);
  const save = async (useExisting = false) => {
    if (saving.current || !name.trim() || !price.trim() || !tax || (useExisting && existing && !existing.isAvailable)) return;
    saving.current = true;
    setBusy(true);
    try {
      let itemId = savedId;
      if (!itemId) {
        const rows = await call('menu_rows');
        const duplicate = rows.find((row) => normalise(row.name) === normalise(name));
        if (duplicate) {
          if (!useExisting || existing?.id !== duplicate.id) {
            setExisting(duplicate);
            return;
          }
          itemId = duplicate.id;
        } else {
          await call('save_menu_item', { edit: {
            id: id.current, name: name.trim().replace(/\s+/g, ' '), price,
            categoryId: category || null, taxClassId: null, priceBasis: null,
            hsn: null, shortCode: null, cost: null, isOpenPrice: false,
            isAvailable: true, course: null, prepMinutes: null,
          } });
          itemId = id.current;
        }
        // A retry after a failed cart add must not create the item again.
        setSavedId(itemId);
      }
      onMenu(await call('menu_items'));
      onAdded(await call('cart_add', { itemId, qty: '1', note: null }));
    } catch (cause) { report(cause); }
    finally { saving.current = false; setBusy(false); }
  };
  const ready = !busy && Boolean(tax && name.trim() && price.trim());
  return <Modal open title="Add new menu item" onClose={() => { if (!saving.current) onClose(); }}
    onEnter={() => { if (ready) void save(Boolean(existing)); }}
    actions={<>
      <Button disabled={busy} onClick={onClose}>Cancel</Button>
      <Button variant="primary" disabled={!ready || Boolean(existing && !existing.isAvailable)} onClick={() => void save(Boolean(existing))}>
        {savedId ? 'Add to bill' : existing ? 'Add existing item to bill' : 'Save & add to bill'}
      </Button>
    </>}>
    <form onSubmit={(event) => { event.preventDefault(); if (ready && (!existing || existing.isAvailable)) void save(Boolean(existing)); }}>
    <Input label="Item name" value={name} disabled={busy || Boolean(savedId)} onChange={(event) => { setName(event.target.value); setExisting(null); }} />
    <MoneyInput ref={priceBox} label="Price" autoFocus value={price} disabled={busy || Boolean(savedId)} onChange={setPrice} />
    <Select label="Category" value={category} disabled={busy || Boolean(savedId)} onChange={(event) => setCategory(event.target.value)}
      options={[{ value: '', label: 'No category' }, ...categories.map((row) => ({ value: row.id, label: row.name }))]} />
    <p>{tax ? !tax.chargesGst ? 'Default tax: no GST charged' : `Default tax: ${slab?.rate ?? tax.shopRate + '%'} · ${slab?.priceWords ?? tax.shopBasis}` : 'Loading default tax…'}</p>
    {existing ? <p role="status">{existing.name} already exists at {existing.price.text}. {existing.isAvailable ? 'Add the existing item at its saved price?' : 'It is unavailable. Update it on the Menu page.'}</p> : null}
    {savedId ? <p role="status">The menu item is saved. Retry adding it to this bill.</p> : null}
    <button type="submit" hidden disabled={!ready || Boolean(existing && !existing.isAvailable)} />
    </form>
  </Modal>;
}
