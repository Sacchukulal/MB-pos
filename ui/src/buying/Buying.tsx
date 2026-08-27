import { useCallback, useEffect, useRef, useState } from 'react';

import { Badge, Button, Card, EmptyState, freshId, Input, Modal, MoneyInput, onlyAmount, PhoneInput, Select, Table, Tabs, useToast, type Column, InfoTip } from '../kit';
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
  /** Raising a purchase order, which nothing could do. */
  const [ordering, setOrdering] = useState(false);
  const [account, setAccount] = useState<SupplierAccountView | null>(null);
  const [editingSupplier, setEditingSupplier] = useState<SupplierView | null>(null);
  const [looking, setLooking] = useState<PurchaseView | null>(null);
  /** Why the screen could not load — and it is on the page, not only in a toast. */
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
              <span className="mb-buying__label">
                Bought in 30 days
                <InfoTip label="About the food cost">{view.taxNote}</InfoTip>
              </span>
              <span className="mb-mono mb-buying__value">{view.bought.text}</span>
            </div>
          </Card>
        </div>
        <div className="mb-row">
          {/*
            A purchase order could be advanced and closed and never CREATED —
            `save_purchase_order` had no caller.
          */}
          <Button variant="secondary" onClick={() => setOrdering(true)}>
            Raise an order
          </Button>
          <Button onClick={() => setEntering(true)}>Enter a delivery</Button>
        </div>
      </div>

      {/* An unhealthy row carries its own fix, and these are sentences written in Rust. */}
      {view.attention.map((line) => (
        <div key={line} className="mb-buying__attention">
          {line}
        </div>
      ))}

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
            body="Enter what arrives: it becomes your food cost, your supplier balance and the money that left the drawer."
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
                    id: freshId('sup'),
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
              body="Add who you buy from — even 'Vegetable market' — and the counter can say what you owe."
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
            body="Optional. Enter what arrives and nothing here will ask about it."
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

      {ordering ? (
        <RaiseOrder
          view={view}
          onClose={() => setOrdering(false)}
          onSaved={(fresh) => {
            setView(fresh);
            setOrdering(false);
            setTab('orders');
            toast.show('ok', 'The order is raised. Mark it sent when you have.');
          }}
          onError={report}
        />
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

// Entering a delivery.

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
        id: freshId('pur'),
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

  /** The downscale lives here, and that is the whole reason Rust has no image library. */
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
                          // The pack the shop BUYS in, which is not the one it cooks in: rice
                          // arrives in bags.
                          unit: picked?.purchaseUnit ?? '',
                          rate: line.rate,
                        });
                        // A blank line appears as soon as the last one is used, so a delivery
                        // of nine things is never nine presses of "add a line".
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

        {/*
          What the last delivery of each material cost, so a rise is visible while somebody can
          still phone about it.
        */}
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
          <MoneyInput label="Transport / loading" value={charges} onChange={setCharges} />
          <MoneyInput label="Discount on the whole invoice" value={discount} onChange={setDiscount} />
          <MoneyInput
            label="Total on the paper"
            value={statedTotal}
            onChange={setStatedTotal}
          />
        </div>

        <div className="mb-row">
          <MoneyInput label="Paid now" value={paidNow} onChange={setPaidNow} />
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

// The rest.

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
        <PhoneInput label="Phone" value={phone} onChange={setPhone} />
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
  /** Correcting the ledger, which had no button. */
  const [fixing, setFixing] = useState(false);
  const [adjust, setAdjust] = useState('');
  const [increases, setIncreases] = useState(false);
  const [why, setWhy] = useState('');

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
        <MoneyInput label="Pay them" value={amount} onChange={setAmount} />
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
        <Button variant="quiet" onClick={() => setFixing(!fixing)}>
          {fixing ? 'Never mind' : 'Correct the balance'}
        </Button>
      </div>

      {/* A correction is a LINE, not an edit. */}
      {fixing ? (
        <div className="mb-row">
          <Input
            label="By how much"
            value={adjust}
            inputMode="decimal"
            className="mb-input--number"
            onChange={(e) => setAdjust(onlyAmount(e.target.value))}
          />
          <Select
            label="Which way"
            value={increases ? 'up' : 'down'}
            onChange={(e) => setIncreases(e.target.value === 'up')}
            options={[
              { value: 'down', label: 'We owe them less' },
              { value: 'up', label: 'We owe them more' },
            ]}
          />
          <Input
            label="Why"
            value={why}
            placeholder="Credit note for the returned oil"
            onChange={(e) => setWhy(e.target.value)}
          />
          <Button
            disabled={adjust.trim() === '' || why.trim() === ''}
            onClick={() =>
              call('save_supplier_adjustment', {
                supplierId: account.supplier.id,
                amount: adjust.trim(),
                increases,
                reason: why.trim(),
              })
                .then((fresh) => {
                  onChanged(fresh);
                  setFixing(false);
                  setAdjust('');
                  setWhy('');
                })
                .catch(onError)
            }
          >
            Correct it
          </Button>
        </div>
      ) : null}

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

      {/* The cancel is two presses, and that is a finding from looking at it. */}
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

/** Raising a purchase order. */
function RaiseOrder({
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
  const [number, setNumber] = useState('');
  const [expected, setExpected] = useState('');
  const [note, setNote] = useState('');
  const [lines, setLines] = useState<PurchaseLineEdit[]>([{ ...BLANK_LINE }]);
  const [busy, setBusy] = useState(false);

  const setLine = (index: number, patch: Partial<PurchaseLineEdit>) => {
    setLines((all) => all.map((line, n) => (n === index ? { ...line, ...patch } : line)));
  };

  const ready = lines.filter((l) => l.materialId !== '' && l.qty.trim() !== '');

  return (
    <Modal open title="Raise a purchase order" onClose={onClose} wide>
      <p className="mb-buying__says">
        What you are asking a supplier for. Nothing moves on the shelf and
        nothing is owed until the delivery arrives and you enter it.
      </p>

      <div className="mb-row">
        <Select
          label="Supplier"
          value={supplierId}
          onChange={(e) => setSupplierId(e.target.value)}
          options={view.suppliers.map((s) => ({ value: s.id, label: s.name }))}
        />
        <Input
          label="Your number for it"
          hint="Leave it blank and we will give it one."
          value={number}
          onChange={(e) => setNumber(e.target.value)}
        />
        <Input
          label="Expected"
          placeholder="2026-08-20"
          value={expected}
          onChange={(e) => setExpected(e.target.value)}
        />
      </div>

      {lines.map((line, index) => (
        <div className="mb-row" key={index}>
          <Select
            label="What"
            value={line.materialId}
            onChange={(e) => setLine(index, { materialId: e.target.value })}
            options={[
              { value: '', label: 'Pick a material' },
              ...view.materials.map((m: BuyMaterialView) => ({ value: m.id, label: m.name })),
            ]}
          />
          <Input
            label="How much"
            value={line.qty}
            onChange={(e) => setLine(index, { qty: e.target.value })}
          />
          <Input
            label="In"
            hint="kg, litre, packet"
            value={line.unit}
            onChange={(e) => setLine(index, { unit: e.target.value })}
          />
          <MoneyInput
            label="Rate you expect"
            value={line.rate}
            onChange={(next) => setLine(index, { rate: next })}
          />
        </div>
      ))}

      <div className="mb-row">
        <Button
          small
          variant="quiet"
          onClick={() => setLines([...lines, { ...BLANK_LINE }])}
        >
          Another line
        </Button>
      </div>

      <Input label="Anything to tell them" value={note} onChange={(e) => setNote(e.target.value)} />

      <div className="mb-row mb-row--end">
        <Button variant="quiet" onClick={onClose}>
          Cancel
        </Button>
        <Button
          variant="primary"
          disabled={busy || supplierId === '' || ready.length === 0}
          onClick={() => {
            setBusy(true);
            call('save_purchase_order', {
              edit: {
                id: freshId('po'),
                supplierId,
                number,
                expected,
                note,
                lines: ready,
              },
            })
              .then(onSaved)
              .catch(onError)
              .finally(() => setBusy(false));
          }}
        >
          Raise it
        </Button>
      </div>
    </Modal>
  );
}
