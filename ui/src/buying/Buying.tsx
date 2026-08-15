/**
 * **Buying** — P26, scope 4.5. Suppliers, the paper, what the shop owes.
 *
 * # The purchase entry is the screen an owner abandons
 *
 * It is used standing beside a delivery man who wants to leave. So a line is
 * material → quantity → rate → Enter, the running total never moves off the
 * screen, and nothing is modal in the middle of a line. That is P10's billing
 * keyboard applied to a different paper.
 *
 * # Nothing here is arithmetic (R8)
 *
 * The landed cost per line, the invoice totals, the ageing, the overdue
 * sentence and the tax note all arrive as strings composed in Rust. This file
 * has no `*`, no `/` and no money in it — **including the photograph**, whose
 * only computation is the canvas downscale D132 deliberately put on this side.
 */

import { useCallback, useEffect, useRef, useState } from 'react';

import {
  Badge,
  Button,
  Card,
  EmptyState,
  Input,
  Modal,
  Select,
  Table,
  Tabs,
  useToast,
  type Column,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { BuyingView } from '../ipc/generated/BuyingView';
import type { BuyMaterialView } from '../ipc/generated/BuyMaterialView';
import type { PurchaseLineEdit } from '../ipc/generated/PurchaseLineEdit';
import type { PurchaseView } from '../ipc/generated/PurchaseView';
import type { SupplierAccountView } from '../ipc/generated/SupplierAccountView';
import type { SupplierView } from '../ipc/generated/SupplierView';

import './buying.css';

const MODES = [
  { value: 'cash', label: 'Cash' },
  { value: 'bank', label: 'Bank' },
  { value: 'upi', label: 'UPI' },
  { value: 'card', label: 'Card' },
];

const BLANK_LINE: PurchaseLineEdit = {
  materialId: '',
  qty: '',
  unit: '',
  free: '',
  rate: '',
  discount: '',
  taxPercent: '',
};

export function Buying() {
  const [view, setView] = useState<BuyingView | null>(null);
  const [tab, setTab] = useState('deliveries');
  const [entering, setEntering] = useState(false);
  const [account, setAccount] = useState<SupplierAccountView | null>(null);
  const [editingSupplier, setEditingSupplier] = useState<SupplierView | null>(null);
  const [looking, setLooking] = useState<PurchaseView | null>(null);
  /**
   * **Why the screen could not load — and it is on the page, not only in a
   * toast.** Found by running it: an unlicensed counter opened Buying to a
   * completely blank panel with two toasts sliding away in the corner, and
   * thirty seconds later there was nothing at all to read. A screen that cannot
   * load has to say so where the screen is.
   */
  const [refused, setRefused] = useState<string | null>(null);
  const toast = useToast();

  const report = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  const load = useCallback(() => {
    call('buying', { supplier: null })
      .then((fresh) => {
        setView(fresh);
        setRefused(null);
      })
      .catch((cause) => {
        if (isUiError(cause)) setRefused(cause.message);
        report(cause);
      });
  }, [report]);

  useEffect(load, [load]);

  if (refused !== null) {
    return (
      <div className="mb-buying">
        <EmptyState title="Buying is not open on this counter" body={refused} />
      </div>
    );
  }
  if (!view) return <div className="mb-buying" />;

  const supplierColumns: Column<SupplierView>[] = [
    { key: 'name', header: 'Supplier', render: (s) => s.name },
    { key: 'phone', header: 'Phone', render: (s) => s.phone ?? '—' },
    { key: 'terms', header: 'Terms', render: (s) => s.terms },
    {
      key: 'balance',
      header: 'Owed',
      numeric: true,
      render: (s) => <span className="mb-mono">{s.owes ? s.balance.text : '—'}</span>,
    },
    {
      key: 'when',
      header: '',
      render: (s) =>
        s.when === '' ? null : s.isOverdue ? (
          <Badge tone="danger">{s.when}</Badge>
        ) : (
          <Badge tone="neutral">{s.when}</Badge>
        ),
    },
    {
      key: 'do',
      header: '',
      render: (s) => (
        <div className="mb-row mb-row--end">
          <Button
            small
            variant="quiet"
            onClick={() => call('supplier_account', { id: s.id }).then(setAccount).catch(report)}
          >
            Account
          </Button>
          {view.mayManageSuppliers ? (
            <Button small variant="quiet" onClick={() => setEditingSupplier(s)}>
              Edit
            </Button>
          ) : null}
        </div>
      ),
    },
  ];

  const purchaseColumns: Column<PurchaseView>[] = [
    { key: 'date', header: 'Day', render: (p) => p.date },
    { key: 'supplier', header: 'Supplier', render: (p) => p.supplier },
    { key: 'invoice', header: 'Invoice', render: (p) => p.invoiceNo ?? '—' },
    {
      key: 'kind',
      header: '',
      render: (p) =>
        p.cancelled !== '' ? (
          <Badge tone="danger">{p.cancelled}</Badge>
        ) : p.isReturn ? (
          <Badge tone="warn">Sent back</Badge>
        ) : null,
    },
    {
      key: 'total',
      header: 'Total',
      numeric: true,
      render: (p) => <span className="mb-mono">{p.total.text}</span>,
    },
    {
      key: 'outstanding',
      header: 'Still to pay',
      numeric: true,
      render: (p) => <span className="mb-mono">{p.outstanding.text}</span>,
    },
    {
      key: 'do',
      header: '',
      render: (p) => (
        <Button
          small
          variant="quiet"
          onClick={() => call('purchase', { id: p.id }).then(setLooking).catch(report)}
        >
          Open
        </Button>
      ),
    },
  ];

  return (
    <div className="mb-buying">
      <div className="mb-buying__top">
        <div className="mb-buying__figures">
          <Card>
            <div className="mb-buying__figure">
              <span className="mb-buying__label">Owed to suppliers</span>
              <span className="mb-mono mb-buying__value">{view.owed.text}</span>
            </div>
          </Card>
          <Card>
            <div className="mb-buying__figure">
              <span className="mb-buying__label">Overdue</span>
              <span className="mb-mono mb-buying__value">{view.overdue.text}</span>
            </div>
          </Card>
          <Card>
            <div className="mb-buying__figure">
              <span className="mb-buying__label">Bought in 30 days</span>
              <span className="mb-mono mb-buying__value">{view.bought.text}</span>
            </div>
          </Card>
        </div>
        <Button onClick={() => setEntering(true)}>Enter a delivery</Button>
      </div>

      {/* D100 — an unhealthy row carries its own fix, and these are sentences
          written in Rust. */}
      {view.attention.map((line) => (
        <div key={line} className="mb-buying__attention">
          {line}
        </div>
      ))}

      <p className="mb-buying__note">{view.taxNote}</p>

      <Tabs
        active={tab}
        onChange={setTab}
        tabs={[
          { id: 'deliveries', label: 'Deliveries' },
          { id: 'suppliers', label: 'Suppliers' },
          { id: 'orders', label: `Orders${view.orders.length > 0 ? ` (${view.orders.length})` : ''}` },
        ]}
      />

      {tab === 'deliveries' ? (
        view.purchases.length === 0 ? (
          <EmptyState
            title="No deliveries yet"
            body="Enter what arrives and this becomes your food cost, your supplier balance and the money that left the drawer — all from one entry."
          />
        ) : (
          <Table rows={view.purchases} columns={purchaseColumns} rowKey={(p) => p.id} />
        )
      ) : null}

      {tab === 'suppliers' ? (
        <>
          {view.mayManageSuppliers ? (
            <div className="mb-row mb-row--end">
              <Button
                small
                onClick={() =>
                  setEditingSupplier({
                    id: `sup_${Date.now().toString(36)}`,
                    name: '',
                    phone: null,
                    gstin: null,
                    address: null,
                    termsDays: 0,
                    terms: '',
                    balance: { paise: 0n, text: "0.00" },
                    owes: false,
                    when: '',
                    isOverdue: false,
                    isActive: true,
                  })
                }
              >
                Add a supplier
              </Button>
            </div>
          ) : null}
          {view.suppliers.length === 0 ? (
            <EmptyState
              title="No suppliers yet"
              body="A supplier is who you buy from. Even 'Vegetable market' is worth adding once — it is what lets the counter tell you what you owe and how old it is."
            />
          ) : (
            <Table rows={view.suppliers} columns={supplierColumns} rowKey={(s) => s.id} />
          )}
        </>
      ) : null}

      {tab === 'orders' ? (
        view.orders.length === 0 ? (
          <EmptyState
            title="No orders"
            body="A purchase order is optional. You never have to raise one — enter what arrives and nothing here will ever ask about it."
          />
        ) : (
          <Table
            rows={view.orders}
            columns={[
              { key: 'number', header: 'Number', render: (o) => o.number },
              { key: 'supplier', header: 'Supplier', render: (o) => o.supplier },
              { key: 'state', header: 'State', render: (o) => <Badge tone="neutral">{o.state}</Badge> },
              { key: 'expected', header: 'Expected', render: (o) => o.expected || '—' },
              {
                key: 'value',
                header: 'Value',
                numeric: true,
                render: (o) => <span className="mb-mono">{o.value.text}</span>,
              },
              {
                key: 'do',
                header: '',
                render: (o) => (
                  <div className="mb-row mb-row--end">
                    <Button
                      small
                      variant="quiet"
                      onClick={() =>
                        call('set_order_state', { id: o.id, state: 'sent' })
                          .then(setView)
                          .catch(report)
                      }
                    >
                      Mark sent
                    </Button>
                    <Button
                      small
                      variant="quiet"
                      onClick={() =>
                        call('set_order_state', { id: o.id, state: 'closed' })
                          .then(setView)
                          .catch(report)
                      }
                    >
                      Close
                    </Button>
                  </div>
                ),
              },
            ]}
            rowKey={(o) => o.id}
          />
        )
      ) : null}

      {entering ? (
        <PurchaseEntry
          view={view}
          onClose={() => setEntering(false)}
          onSaved={(fresh) => {
            setView(fresh);
            setEntering(false);
          }}
          onError={report}
        />
      ) : null}

      {editingSupplier ? (
        <SupplierEditor
          supplier={editingSupplier}
          onClose={() => setEditingSupplier(null)}
          onSaved={(fresh) => {
            setView(fresh);
            setEditingSupplier(null);
          }}
          onError={report}
        />
      ) : null}

      {account ? (
        <SupplierAccount
          account={account}
          onChanged={setAccount}
          onClose={() => {
            setAccount(null);
            load();
          }}
          onError={report}
        />
      ) : null}

      {looking ? (
        <PurchasePaper
          purchase={looking}
          onClose={() => setLooking(null)}
          onCancelled={(fresh) => {
            setView(fresh);
            setLooking(null);
          }}
          onError={report}
        />
      ) : null}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Entering a delivery.
// ---------------------------------------------------------------------------

function PurchaseEntry({
  view,
  onClose,
  onSaved,
  onError,
}: {
  view: BuyingView;
  onClose: () => void;
  onSaved: (fresh: BuyingView) => void;
  onError: (cause: unknown) => void;
}) {
  const [supplierId, setSupplierId] = useState(view.suppliers[0]?.id ?? '');
  const [invoiceNo, setInvoiceNo] = useState('');
  const [lines, setLines] = useState<PurchaseLineEdit[]>([{ ...BLANK_LINE }]);
  const [charges, setCharges] = useState('');
  const [discount, setDiscount] = useState('');
  const [statedTotal, setStatedTotal] = useState('');
  const [paidNow, setPaidNow] = useState('');
  const [paidMode, setPaidMode] = useState('cash');
  const [photo, setPhoto] = useState<{ id: string; size: string } | null>(null);
  const file = useRef<HTMLInputElement>(null);

  const materials: BuyMaterialView[] = view.materials;

  const setLine = (index: number, patch: Partial<PurchaseLineEdit>) => {
    setLines((all) => all.map((line, n) => (n === index ? { ...line, ...patch } : line)));
  };

  const save = () => {
    call('save_purchase', {
      edit: {
        id: `pur_${Date.now().toString(36)}`,
        supplierId,
        invoiceNo,
        lines: lines.filter((l) => l.materialId !== '' && l.qty.trim() !== ''),
        invoiceDiscount: discount,
        charges,
        statedTotal,
        paidNow,
        paidMode,
        attachmentId: photo?.id ?? '',
        poId: '',
        note: '',
        returnsPurchaseId: '',
      },
    })
      .then(onSaved)
      .catch(onError);
  };

  /**
   * **D132 — the downscale lives here, and that is the whole reason Rust has no
   * image library.** Longest side 1600 px, JPEG 0.7: a 4 MB camera picture
   * becomes about 200 KB, which is what makes keeping it beside the database
   * (and inside the backup) affordable at all.
   */
  const attach = (chosen: File) => {
    const image = new Image();
    const reader = new FileReader();
    reader.onload = () => {
      image.onload = () => {
        const longest = Math.max(image.width, image.height);
        const scale = longest > 1600 ? 1600 / longest : 1;
        const canvas = document.createElement('canvas');
        canvas.width = Math.round(image.width * scale);
        canvas.height = Math.round(image.height * scale);
        const context = canvas.getContext('2d');
        if (!context) return;
        context.drawImage(image, 0, 0, canvas.width, canvas.height);
        call('attach_photo', { dataUrl: canvas.toDataURL('image/jpeg', 0.7) })
          .then((saved) => setPhoto({ id: saved.id, size: saved.size }))
          .catch(onError);
      };
      image.src = String(reader.result);
    };
    reader.readAsDataURL(chosen);
  };

  return (
    <Modal open title="A delivery arrived" onClose={onClose} wide>
      <div className="mb-buying__form">
        <div className="mb-row">
          <Select
            label="Supplier"
            value={supplierId}
            onChange={(e) => setSupplierId(e.target.value)}
            options={view.suppliers.map((s) => ({ value: s.id, label: s.name }))}
          />
          <Input label="Invoice number" value={invoiceNo} onChange={(e) => setInvoiceNo(e.target.value)} />
        </div>

        <table className="mb-buying__lines">
          <thead>
            <tr>
              <th>Material</th>
              <th>Quantity</th>
              <th>Unit</th>
              <th>Free</th>
              <th>Rate</th>
              <th>Discount</th>
              <th>GST %</th>
            </tr>
          </thead>
          <tbody>
            {lines.map((line, index) => {
              const material = materials.find((m) => m.id === line.materialId);
              return (
                <tr key={index}>
                  <td>
                    <Select
                      label=""
                      value={line.materialId}
                      onChange={(event) => {
                        const id = event.target.value;
                        const picked = materials.find((m) => m.id === id);
                        setLine(index, {
                          materialId: id,
                          // The pack the shop BUYS in, which is not the one it
                          // cooks in (D108): rice arrives in bags.
                          unit: picked?.purchaseUnit ?? '',
                          rate: line.rate,
                        });
                        // A blank line appears as soon as the last one is used,
                        // so a delivery of nine things is never nine presses of
                        // "add a line".
                        if (index === lines.length - 1) {
                          setLines((all) => [...all, { ...BLANK_LINE }]);
                        }
                      }}
                      options={[
                        { value: '', label: '—' },
                        ...materials.map((m) => ({ value: m.id, label: m.name })),
                      ]}
                    />
                  </td>
                  <td>
                    <Input label="" value={line.qty} onChange={(e) => setLine(index, { qty: e.target.value })} />
                  </td>
                  <td>
                    <Select
                      label=""
                      value={line.unit}
                      onChange={(e) => setLine(index, { unit: e.target.value })}
                      options={
                        material
                          ? [
                              { value: material.baseUnit, label: material.baseUnit },
                              ...material.packs.map((p) => ({ value: p.name, label: p.name })),
                            ]
                          : [{ value: '', label: '—' }]
                      }
                    />
                  </td>
                  <td>
                    <Input label="" value={line.free} onChange={(e) => setLine(index, { free: e.target.value })} />
                  </td>
                  <td>
                    <Input label="" value={line.rate} onChange={(e) => setLine(index, { rate: e.target.value })} />
                  </td>
                  <td>
                    <Input
                      label=""
                      value={line.discount}
                      onChange={(e) => setLine(index, { discount: e.target.value })}
                    />
                  </td>
                  <td>
                    <Input
                      label=""
                      value={line.taxPercent}
                      onChange={(e) => setLine(index, { taxPercent: e.target.value })}
                    />
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>

        {/* What the last delivery of each material cost, so a rise is visible
            while somebody can still phone about it. Composed in Rust. */}
        {lines
          .map((line) => materials.find((m) => m.id === line.materialId))
          .filter((m): m is BuyMaterialView => m !== undefined && m.cost !== '')
          .map((m) => (
            <p key={m.id} className="mb-buying__hint">
              {m.name} — you were paying {m.cost}
              {m.lastRate === '' ? '' : `, last bought at ${m.lastRate}`}
            </p>
          ))}

        <div className="mb-row">
          <Input label="Transport / loading" value={charges} onChange={(e) => setCharges(e.target.value)} />
          <Input label="Discount on the whole invoice" value={discount} onChange={(e) => setDiscount(e.target.value)} />
          <Input
            label="Total on the paper"
            value={statedTotal}
            onChange={(e) => setStatedTotal(e.target.value)}
          />
        </div>

        <div className="mb-row">
          <Input label="Paid now" value={paidNow} onChange={(e) => setPaidNow(e.target.value)} />
          <Select label="How" value={paidMode} onChange={(e) => setPaidMode(e.target.value)} options={MODES} />
        </div>

        <div className="mb-row">
          <input
            ref={file}
            type="file"
            accept="image/*"
            className="mb-buying__file"
            onChange={(event) => {
              const chosen = event.target.files?.[0];
              if (chosen) attach(chosen);
            }}
          />
          <Button variant="quiet" onClick={() => file.current?.click()}>
            {photo ? `Photograph attached (${photo.size})` : 'Photograph the invoice'}
          </Button>
        </div>

        <div className="mb-row mb-row--end">
          <Button variant="quiet" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={save}>Save the delivery</Button>
        </div>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------------------
// The rest.
// ---------------------------------------------------------------------------

function SupplierEditor({
  supplier,
  onClose,
  onSaved,
  onError,
}: {
  supplier: SupplierView;
  onClose: () => void;
  onSaved: (fresh: BuyingView) => void;
  onError: (cause: unknown) => void;
}) {
  const [name, setName] = useState(supplier.name);
  const [phone, setPhone] = useState(supplier.phone ?? '');
  const [gstin, setGstin] = useState(supplier.gstin ?? '');
  const [address, setAddress] = useState(supplier.address ?? '');
  const [terms, setTerms] = useState(String(supplier.termsDays));

  return (
    <Modal open title={supplier.name === '' ? 'A new supplier' : supplier.name} onClose={onClose}>
      <div className="mb-buying__form">
        <Input label="Name" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
        <Input label="Phone" value={phone} onChange={(e) => setPhone(e.target.value)} />
        <Input label="GST number" value={gstin} onChange={(e) => setGstin(e.target.value)} />
        <Input label="Address" value={address} onChange={(e) => setAddress(e.target.value)} />
        <Input
          label="Payment terms, in days"
          value={terms}
          onChange={(e) => setTerms(e.target.value)}
          hint="0 means you pay at the door. 15 means the invoice is due a fortnight later, and that is what 'overdue' is measured from."
        />
        <div className="mb-row mb-row--end">
          <Button variant="quiet" onClick={onClose}>
            Cancel
          </Button>
          <Button
            onClick={() =>
              call('save_supplier', {
                edit: {
                  id: supplier.id,
                  name,
                  phone,
                  gstin,
                  address,
                  termsDays: terms,
                  note: '',
                  isActive: supplier.isActive,
                },
              })
                .then(onSaved)
                .catch(onError)
            }
          >
            Save
          </Button>
        </div>
      </div>
    </Modal>
  );
}

function SupplierAccount({
  account,
  onChanged,
  onClose,
  onError,
}: {
  account: SupplierAccountView;
  onChanged: (fresh: SupplierAccountView) => void;
  onClose: () => void;
  onError: (cause: unknown) => void;
}) {
  const [amount, setAmount] = useState('');
  const [mode, setMode] = useState('cash');
  const [reference, setReference] = useState('');

  return (
    <Modal open title={account.supplier.name} onClose={onClose} wide>
      <p className="mb-buying__says">{account.says}</p>

      <div className="mb-buying__ageing">
        <span>Not due yet {account.ageing.current.text}</span>
        <span>30 days {account.ageing.days30.text}</span>
        <span>60 days {account.ageing.days60.text}</span>
        <span>90 days + {account.ageing.days90.text}</span>
      </div>

      <div className="mb-row">
        <Input label="Pay them" value={amount} onChange={(e) => setAmount(e.target.value)} />
        <Select label="How" value={mode} onChange={(e) => setMode(e.target.value)} options={MODES} />
        <Input label="Reference" value={reference} onChange={(e) => setReference(e.target.value)} />
        <Button
          onClick={() =>
            call('record_supplier_payment', {
              supplierId: account.supplier.id,
              amount,
              mode,
              reference,
            })
              .then((fresh) => {
                onChanged(fresh);
                setAmount('');
                setReference('');
              })
              .catch(onError)
          }
        >
          Record
        </Button>
      </div>

      <Table
        rows={account.movements}
        columns={[
          { key: 'date', header: 'Day', render: (m) => m.date },
          { key: 'kind', header: 'What', render: (m) => m.kind },
          { key: 'note', header: '', render: (m) => m.note },
          {
            key: 'amount',
            header: 'Amount',
            numeric: true,
            render: (m) => (
              <span className="mb-mono">
                {m.adds ? '' : '− '}
                {m.amount.text}
              </span>
            ),
          },
          {
            key: 'running',
            header: 'Owed after',
            numeric: true,
            render: (m) => <span className="mb-mono">{m.running.text}</span>,
          },
        ]}
        rowKey={(m) => `${m.date}-${m.kind}-${m.amount.text}-${m.running.text}`}
      />
    </Modal>
  );
}

function PurchasePaper({
  purchase,
  onClose,
  onCancelled,
  onError,
}: {
  purchase: PurchaseView;
  onClose: () => void;
  onCancelled: (fresh: BuyingView) => void;
  onError: (cause: unknown) => void;
}) {
  const [reason, setReason] = useState('');
  const [cancelling, setCancelling] = useState(false);
  const [photo, setPhoto] = useState<string | null>(null);

  return (
    <Modal open title={`${purchase.supplier} — ${purchase.date}`} onClose={onClose} wide>
      <Table
        rows={purchase.lines}
        columns={[
          { key: 'material', header: 'Material', render: (l) => l.material },
          { key: 'qty', header: 'Quantity', render: (l) => `${l.qty}${l.free === '' ? '' : ` + ${l.free}`}` },
          {
            key: 'rate',
            header: 'Rate',
            numeric: true,
            render: (l) => <span className="mb-mono">{l.rate.text}</span>,
          },
          { key: 'tax', header: 'GST', render: (l) => l.tax || '—' },
          {
            key: 'value',
            header: 'Cost, all in',
            numeric: true,
            render: (l) => <span className="mb-mono">{l.value.text}</span>,
          },
          /* **D123's number**, and the reason the module exists. The heading
             was "Which is" and said nothing — a person is comparing this
             against the RATE three columns to the left, so it has to say that
             is what it is. Found by looking at it. */
          { key: 'landed', header: 'What it really cost', render: (l) => l.landed },
        ]}
        rowKey={(l) => String(l.seq)}
      />

      <div className="mb-buying__totals">
        <span>Goods {purchase.goods.text}</span>
        <span>Transport {purchase.charges.text}</span>
        <span>GST {purchase.tax.text}</span>
        <span>You can claim {purchase.creditable.text}</span>
        <span className="mb-buying__grand">Total {purchase.total.text}</span>
      </div>

      {purchase.hasPhoto ? (
        <div>
          <Button
            variant="quiet"
            small
            onClick={() =>
              call('purchase_photo', { id: purchase.id })
                .then((found) => setPhoto(found.dataUrl))
                .catch(onError)
            }
          >
            Show the photograph
          </Button>
          {photo ? <img className="mb-buying__photo" src={photo} alt="The paper invoice" /> : null}
        </div>
      ) : null}

      {/* **The cancel is two presses, and that is a finding from looking at
          it.** The first version put a red "Cancel it" button and an empty
          reason box directly under the total — so somebody who opened a
          delivery just to READ it was one stray click from unwinding five
          materials' costs. Now the row a person meets is Close. */}
      {purchase.cancelled === '' ? (
        cancelling ? (
          <div className="mb-buying__form">
            <Input
              label="Cancel this delivery because"
              value={reason}
              autoFocus
              onChange={(e) => setReason(e.target.value)}
              hint="A delivery is never edited. Cancel it with a reason and enter a corrected copy — the ledger then reads as what actually happened."
            />
            <div className="mb-row mb-row--end">
              <Button variant="quiet" onClick={() => setCancelling(false)}>
                Leave it alone
              </Button>
              <Button
                variant="danger"
                disabled={reason.trim() === ''}
                onClick={() =>
                  call('cancel_purchase', { id: purchase.id, reason })
                    .then(onCancelled)
                    .catch(onError)
                }
              >
                Cancel this delivery
              </Button>
            </div>
          </div>
        ) : (
          <div className="mb-row mb-row--end">
            <Button variant="quiet" onClick={() => setCancelling(true)}>
              Cancel this delivery
            </Button>
            <Button onClick={onClose}>Close</Button>
          </div>
        )
      ) : (
        <div className="mb-row mb-row--end">
          <p className="mb-buying__says">{purchase.cancelled}</p>
          <Button onClick={onClose}>Close</Button>
        </div>
      )}
    </Modal>
  );
}
