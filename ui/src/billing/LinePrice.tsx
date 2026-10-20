import { useRef, useState } from 'react';
import { Button, MoneyInput, useReport } from '../kit';
import { call } from '../ipc/call';
import type { CartLineView } from '../ipc/generated/CartLineView';
import type { CartView } from '../ipc/generated/CartView';

export function LinePrice({ line, overridden, onSaved }: {
  line: CartLineView; overridden: boolean; onSaved: (cart: CartView) => void;
}) {
  const [edit, setEdit] = useState<{ line: CartLineView; price: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const saving = useRef(false);
  const report = useReport();
  const save = async () => {
    if (saving.current || !edit) return;
    if (!edit.price.trim() || edit.price === edit.line.unitPrice.text) {
      setEdit(null);
      return;
    }
    saving.current = true;
    setBusy(true);
    try {
      onSaved(await call('cart_set_price', {
        index: edit.line.index, price: edit.price, expectedLine: edit.line.editToken,
      }));
    } catch (cause) { report(cause); }
    finally { saving.current = false; setBusy(false); setEdit(null); }
  };
  return <div className="mb-cartline__amount" onClick={(event) => event.stopPropagation()}>
    {edit ? <MoneyInput autoFocus prefix={undefined} className="mb-cartline__price-input"
      aria-label={`Unit price for ${line.name}`} value={edit.price} disabled={busy}
      onFocus={(event) => event.target.select()}
      onChange={(price) => setEdit({ ...edit, price })}
      onBlur={() => void save()}
      onKeyDown={(event) => {
        event.stopPropagation();
        if (event.key === 'Enter' || event.key === 'Escape') {
          event.preventDefault();
          if (event.key === 'Enter') void save();
          else if (!saving.current) setEdit(null);
        }
      }} />
      : <Button variant="quiet" size="sm" aria-label={`Edit unit price for ${line.name}`}
        title={`${line.qty} × ${(line.displayUnitPrice ?? line.unitPrice).text} — edit item price for this bill`}
        onClick={() => setEdit({ line, price: line.unitPrice.text })}>
        {line.amount.text}{overridden ? ' *' : ''}
      </Button>}
  </div>;
}
