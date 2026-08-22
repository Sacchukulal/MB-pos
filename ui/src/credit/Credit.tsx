/**
 * **Who owes me money** — scope 5.1, 5.2, 5.3, 5.4.
 *
 * The owner renamed this from "khata" on 2026-08-08.
 *
 * # The default view is the one an owner opens
 *
 * Not an alphabetical customer list. A shop opens this screen to answer one
 * question — *who owes me money, and for how long* — so that is what it shows
 * first, oldest debt at the top. The alphabetical list is one click away and
 * nobody clicks it.
 *
 * # Nothing here is arithmetic
 *
 * The balance, the ageing buckets, the running column in the statement and the
 * "74 days" are all computed in Rust and arrive formatted (R8, D39). A screen
 * that divides by thirty is a screen with a second answer to what a customer
 * owes.
 */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  Checkbox,
  EmptyState,
  freshId,
  Icon,
  Input,
  Modal,
  MoneyInput,
  Page,
  PageHeader,
  Panel,
  PhoneInput,
  SearchField,
  SectionHeader,
  Select,
  Table,
  Toolbar,
  useToast,
  type Column,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { AccountView } from '../ipc/generated/AccountView';
import type { CustomerView } from '../ipc/generated/CustomerView';

import './credit.css';

export function Credit() {
  const [rows, setRows] = useState<readonly CustomerView[]>([]);
  const [everybody, setEverybody] = useState(false);
  const [find, setFind] = useState('');
  const [open, setOpen] = useState<AccountView | null>(null);
  const [editing, setEditing] = useState<CustomerView | null>(null);
  const toast = useToast();

  const report = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  const load = useCallback(() => {
    call(everybody ? 'customers' : 'who_owes').then(setRows).catch(report);
  }, [everybody, report]);

  useEffect(load, [load]);

  const shown = rows.filter((row) => {
    const needle = find.trim().toLowerCase();
    if (needle === '') return true;
    return (
      row.name.toLowerCase().includes(needle) || (row.phone ?? '').includes(needle)
    );
  });

  const columns: Column<CustomerView>[] = [
    { key: 'name', header: 'Customer', render: (c) => c.name },
    { key: 'phone', header: 'Phone', render: (c) => c.phone ?? '—' },
    {
      key: 'balance',
      header: 'Owes',
      numeric: true,
      render: (c) => <span className="mb-mono">{c.balance.text}</span>,
    },
    {
      key: 'oldest',
      header: 'Oldest',
      // The number an owner acts on: not "he owes me money" but "he has owed
      // me this for 74 days".
      render: (c) => c.oldest,
    },
    {
      key: 'limit',
      header: 'Limit',
      numeric: true,
      render: (c) =>
        c.creditLimit ? (
          <span className="mb-mono">{c.creditLimit.text}</span>
        ) : (
          <span className="mb-credit__nolimit">No limit</span>
        ),
    },
    {
      key: 'do',
      header: '',
      render: (c) => (
        <div className="mb-row">
          <Button
            small
            onClick={() => {
              call('customer_account', { customerId: c.id }).then(setOpen).catch(report);
            }}
          >
            Open
          </Button>
          <Button small variant="quiet" onClick={() => setEditing(c)}>
            Edit
          </Button>
        </div>
      ),
    },
  ];

  const addCustomer = () =>
    setEditing({
      id: freshId('cus'),
      name: '',
      phone: null,
      gstin: null,
      address: null,
      creditLimit: null,
      isActive: true,
      balance: { paise: 0n, text: '0.00' },
      oldest: '—',
    });

  return (
    <Page className="mb-credit">
      {/*
        **A title, an action, and a filter — and each in its own place** (P27.5).

        Before this the screen opened with one row holding a search box, two
        view buttons and "Add a customer", all `Button`s, two of them filled
        accent. So the thing that CHANGES THE SHOP (adding a customer) and the
        thing that changes what you are LOOKING AT (who owes me / everybody)
        were the same shape, the same colour, and eight pixels apart. The
        screen also had no title at all.
      */}
      <PageHeader
        title="Credit"
        subtitle="Who owes this shop money, and how long they have owed it."
        count={shown.length}
        actions={
          <Button variant="primary" onClick={addCustomer}>
            <Icon name="plus" size="sm" />
            Add a customer
          </Button>
        }
      />

      <Toolbar
        end={
          <div className="mb-tabs" role="tablist" aria-label="Which customers">
            <button
              type="button"
              role="tab"
              className="mb-tab"
              aria-selected={!everybody}
              onClick={() => setEverybody(false)}
            >
              Who owes me
            </button>
            <button
              type="button"
              role="tab"
              className="mb-tab"
              aria-selected={everybody}
              onClick={() => setEverybody(true)}
            >
              Everybody
            </button>
          </div>
        }
      >
        <div className="mb-credit__find">
          <SearchField
            value={find}
            placeholder="Find a customer, or a number"
            onChange={(event) => setFind(event.target.value)}
          />
        </div>
      </Toolbar>

      {shown.length === 0 ? (
        <EmptyState
          title={everybody ? 'No customers yet' : 'Nobody owes you anything'}
          body={
            everybody
              ? 'Add a regular, and their bills can go on the account.'
              : 'That is the good state. Switch to Everybody to see the whole list.'
          }
        />
      ) : (
        <Panel flush>
          <Table rows={[...shown]} columns={columns} rowKey={(c) => c.id} />
        </Panel>
      )}

      {open ? (
        <Account
          account={open}
          onClose={() => {
            setOpen(null);
            load();
          }}
          onChanged={setOpen}
          onFailed={report}
        />
      ) : null}

      {editing ? (
        <EditCustomer
          customer={editing}
          onClose={() => setEditing(null)}
          onSaved={(fresh) => {
            setRows(fresh);
            setEditing(null);
            load();
          }}
          onFailed={report}
        />
      ) : null}
    </Page>
  );
}

/** One account: the ageing, the ledger, a repayment, and the statement. */
function Account({
  account,
  onClose,
  onChanged,
  onFailed,
}: {
  account: AccountView;
  onClose: () => void;
  onChanged: (account: AccountView) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [amount, setAmount] = useState('');
  const [mode, setMode] = useState('cash');
  const [reference, setReference] = useState('');
  const [adjusting, setAdjusting] = useState(false);
  const [adjustment, setAdjustment] = useState('');
  const [increases, setIncreases] = useState(false);
  const [reason, setReason] = useState('');
  const toast = useToast();

  return (
    <Modal open title={account.customer.name} onClose={onClose} wide>
      <div className="mb-credit__ageing">
        <span>
          Owes <strong className="mb-mono">{account.customer.balance.text}</strong>
        </span>
        <span>Oldest {account.ageing.oldest}</span>
        <span>Up to 30 days {account.ageing.current.text}</span>
        <span>30–60 {account.ageing.days30.text}</span>
        <span>60–90 {account.ageing.days60.text}</span>
        <span className="mb-credit__old">Over 90 {account.ageing.days90.text}</span>
      </div>

      <SectionHeader
        title="Take a repayment"
        note={
          <>
            In a real payment mode — cash, card or UPI. Money arriving is money
            arriving, and it has to show in the day&rsquo;s takings as what it
            was.
          </>
        }
      />
      <div className="mb-comp__choice">
        <MoneyInput label="Amount" value={amount} onChange={setAmount} />
        <Select
          label="How"
          value={mode}
          onChange={(e) => setMode(e.target.value)}
          options={[
            { value: 'cash', label: 'Cash' },
            { value: 'card', label: 'Card' },
            { value: 'upi', label: 'UPI' },
          ]}
        />
        <Input
          label="Reference"
          value={reference}
          onChange={(e) => setReference(e.target.value)}
        />
        <Button
          variant="primary"
          onClick={() => {
            call('record_repayment', {
              customerId: account.customer.id,
              amount,
              mode,
              reference,
            })
              .then((fresh) => {
                onChanged(fresh);
                setAmount('');
                setReference('');
                toast.show('ok', 'Repayment recorded.');
              })
              .catch(onFailed);
          }}
        >
          Take it
        </Button>
      </div>

      <h3 className="mb-credit__heading">The account</h3>
      {account.movements.length === 0 ? (
        <EmptyState title="Nothing yet" body="Bills on the account and repayments both appear here." />
      ) : (
        <table className="mb-ledger">
          <thead>
            <tr>
              <th>Date</th>
              <th>What</th>
              <th>Note</th>
              <th className="mb-numeric">Amount</th>
              <th className="mb-numeric">Balance</th>
            </tr>
          </thead>
          <tbody>
            {account.movements.map((row, index) => (
              <tr key={`${row.date}-${index}`}>
                <td>{row.date}</td>
                <td>{row.kind}</td>
                <td>{row.note}</td>
                <td className="mb-numeric mb-mono">
                  {row.adds ? row.amount.text : `-${row.amount.text}`}
                </td>
                <td className="mb-numeric mb-mono">{row.running.text}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <div className="mb-row mb-row--end">
        <Button
          variant="quiet"
          onClick={() => {
            void navigator.clipboard.writeText(account.statement);
            toast.show('ok', 'The statement is on the clipboard.');
          }}
        >
          Copy the statement
        </Button>
        <Button variant="quiet" onClick={() => setAdjusting(true)}>
          Adjust
        </Button>
        <Button variant="primary" onClick={onClose}>
          Done
        </Button>
      </div>

      {adjusting ? (
        <Modal
          open
          title="Adjust the account"
          note="An opening balance from the notebook, or money written off. It needs a reason — this is the one place money can leave an account without anybody paying."
          onClose={() => setAdjusting(false)}
        >
          <MoneyInput
            label="Amount"
            value={adjustment}
            autoFocus
            onChange={setAdjustment}
          />
          <Checkbox
            label="They owe MORE (an opening balance, or a bill somebody missed)"
            checked={increases}
            onChange={(e) => setIncreases(e.target.checked)}
          />
          <Input label="Why" value={reason} onChange={(e) => setReason(e.target.value)} />
          <div className="mb-row mb-row--end">
            <Button variant="quiet" onClick={() => setAdjusting(false)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                call('save_credit_adjustment', {
                  customerId: account.customer.id,
                  amount: adjustment,
                  increases,
                  reason,
                })
                  .then((fresh) => {
                    onChanged(fresh);
                    setAdjusting(false);
                    setAdjustment('');
                    setReason('');
                  })
                  .catch(onFailed);
              }}
            >
              Save
            </Button>
          </div>
        </Modal>
      ) : null}
    </Modal>
  );
}

function EditCustomer({
  customer,
  onClose,
  onSaved,
  onFailed,
}: {
  customer: CustomerView;
  onClose: () => void;
  onSaved: (rows: readonly CustomerView[]) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [name, setName] = useState(customer.name);
  const [phone, setPhone] = useState(customer.phone ?? '');
  const [gstin, setGstin] = useState(customer.gstin ?? '');
  const [address, setAddress] = useState(customer.address ?? '');
  const [limit, setLimit] = useState(customer.creditLimit?.text ?? '');
  const [active, setActive] = useState(customer.isActive);

  return (
    <Modal open title={customer.name === '' ? 'Add a customer' : customer.name} onClose={onClose}>
      <Input label="Name" value={name} autoFocus onChange={(e) => setName(e.target.value)} />
      <PhoneInput
        label="Phone"
        hint="The number IS the customer here — one number, one account."
        value={phone}
        onChange={setPhone}
      />
      <MoneyInput
        label="Credit limit"
        hint="Leave it blank for no limit. Blank is not a limit of zero."
        value={limit}
        onChange={setLimit}
      />
      <Input label="GSTIN" value={gstin} onChange={(e) => setGstin(e.target.value)} />
      <Input label="Address" value={address} onChange={(e) => setAddress(e.target.value)} />
      <Checkbox
        label="A customer here"
        checked={active}
        onChange={(e) => setActive(e.target.checked)}
      />
      <div className="mb-row mb-row--end">
        <Button variant="quiet" onClick={onClose}>
          Cancel
        </Button>
        <Button
          variant="primary"
          onClick={() => {
            call('save_customer', {
              edit: {
                id: customer.id,
                name,
                phone,
                gstin,
                address,
                creditLimit: limit,
                isActive: active,
              },
            })
              .then(onSaved)
              .catch(onFailed);
          }}
        >
          Save
        </Button>
      </div>
    </Modal>
  );
}

/**
 * **Putting a bill on somebody's account**, from the billing screen.
 *
 * Exported because that is where a credit sale happens — mid-bill, with the
 * customer standing there — and a screen that made a cashier leave the bill to
 * find a customer would be a screen nobody used.
 */
export function PutOnAccount({
  onClose,
  onDone,
  onFailed,
}: {
  onClose: () => void;
  onDone: (said: string) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [people, setPeople] = useState<readonly CustomerView[]>([]);
  const [find, setFind] = useState('');
  const [chosen, setChosen] = useState<CustomerView | null>(null);
  const [says, setSays] = useState<string | null>(null);
  const [over, setOver] = useState(false);

  useEffect(() => {
    call('customers').then(setPeople).catch(onFailed);
  }, [onFailed]);

  const pick = (customer: CustomerView) => {
    setChosen(customer);
    // What this bill would do to the account, said BEFORE it happens.
    call('credit_headroom', { customerId: customer.id })
      .then((room) => {
        setSays(room.says);
        setOver(room.verdict === 'over');
      })
      .catch(onFailed);
  };

  const shown = people.filter((p) => {
    const needle = find.trim().toLowerCase();
    if (needle === '') return true;
    return p.name.toLowerCase().includes(needle) || (p.phone ?? '').includes(needle);
  });

  return (
    <Modal open title="On the account" onClose={onClose} wide>
      {chosen === null ? (
        <>
          <SearchField
            value={find}
            placeholder="Name or number"
            onChange={(event) => setFind(event.target.value)}
          />
          {shown.length === 0 ? (
            <EmptyState
              title="No customers yet"
              body="Add one in Credit, and their bills can go on the account."
            />
          ) : (
            <ul className="mb-comp__list">
              {shown.map((person) => (
                <li key={person.id} className="mb-comp__row">
                  <Button variant="quiet" onClick={() => pick(person)}>
                    {person.name}
                  </Button>
                  <span className="mb-comp__rule">{person.phone ?? ''}</span>
                  <span className="mb-mono">{person.balance.text}</span>
                  <span className="mb-comp__rule">{person.oldest}</span>
                </li>
              ))}
            </ul>
          )}
        </>
      ) : (
        <>
          <p>{says}</p>
          {over ? (
            <Badge tone="danger">Past the limit — this needs approval</Badge>
          ) : null}
          <div className="mb-row mb-row--end">
            <Button variant="quiet" onClick={() => setChosen(null)}>
              Somebody else
            </Button>
            <Button
              variant={over ? 'danger' : 'primary'}
              onClick={() => {
                call('put_on_account', { customerId: chosen.id, overrideLimit: over })
                  .then(() => onDone(`On ${chosen.name}'s account.`))
                  .catch(onFailed);
              }}
            >
              {over ? 'Approve and put it on' : 'Put it on the account'}
            </Button>
          </div>
        </>
      )}
    </Modal>
  );
}
