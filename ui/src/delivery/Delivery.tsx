/**
 * **The board for orders that leave on a bike** — P29, scope 14.5.
 *
 * # Two questions, in this order
 *
 * 1. *Where is the food?* — the list, one row per delivery, with the state in
 *    words and the address the rider was given.
 * 2. *Where is the money?* — one line per rider, and the figure that matters:
 *    what they are carrying right now.
 *
 * The second one is the reason the screen exists. A shop can see where its food
 * is by looking out of the door; it cannot see that Kumar has nine hundred
 * rupees in his pocket, and at eleven o'clock that is the only question left.
 *
 * # Nothing here is arithmetic
 *
 * Every figure and every sentence — "Carrying ₹900.00", "Collect ₹640.00",
 * "Everything is back." — is written in Rust and printed here (R8). This file
 * decides where things sit and nothing else.
 */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  Checkbox,
  EmptyState,
  Icon,
  Input,
  Modal,
  Notice,
  Page,
  PageHeader,
  Panel,
  Row,
  Sections,
  Select,
  Stack,
  Table,
  Toolbar,
  useToast,
  type BadgeTone,
  type Column,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { DeliveryBoardView } from '../ipc/generated/DeliveryBoardView';
import type { DeliveryView } from '../ipc/generated/DeliveryView';
import type { PersonView } from '../ipc/generated/PersonView';
import type { RiderDayView } from '../ipc/generated/RiderDayView';

import './delivery.css';

/** The step a row offers next, and the words on the button. */
const NEXT: Record<string, { state: string; label: string } | undefined> = {
  pending: { state: 'assigned', label: 'Give to a rider' },
  assigned: { state: 'out', label: 'Send it out' },
  out: { state: 'delivered', label: 'It arrived' },
};

const TONES: Record<string, BadgeTone> = {
  pending: 'neutral',
  assigned: 'info',
  out: 'warn',
  delivered: 'ok',
  failed: 'danger',
};

export function Delivery() {
  const [view, setView] = useState<DeliveryBoardView | null>(null);
  const [failing, setFailing] = useState<DeliveryView | null>(null);
  const [why, setWhy] = useState('');
  const [handback, setHandback] = useState<RiderDayView | null>(null);
  /** P31 — who is allowed to take an order out, which nothing could set. */
  const [choosingRiders, setChoosingRiders] = useState(false);
  const [amount, setAmount] = useState('');
  const toast = useToast();

  const report = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  const load = useCallback(() => {
    call('delivery_board', { day: null }).then(setView).catch(report);
  }, [report]);

  useEffect(load, [load]);

  const move = (row: DeliveryView, state: string, rider?: string, failure?: string) => {
    call('save_delivery', {
      edit: {
        orderId: row.orderId,
        address: '',
        customerId: '',
        riderId: rider ?? row.riderId,
        state,
        failure: failure ?? '',
      },
    })
      .then((fresh) => {
        setView(fresh);
        setFailing(null);
        setWhy('');
      })
      .catch(report);
  };

  if (!view) return <div className="mb-delivery" />;

  const may = view.mayDispatch;

  const columns: Column<DeliveryView>[] = [
    {
      key: 'bill',
      header: 'Bill',
      render: (r) => (
        <Stack gap="inline">
          <strong>{r.reference}</strong>
          <span className="mb-delivery__who">{r.customer || 'No name'}</span>
        </Stack>
      ),
    },
    {
      key: 'where',
      header: 'Where',
      render: (r) => (
        <Stack gap="inline">
          <span className="mb-delivery__address">{r.address || 'No address typed'}</span>
          {r.phone ? <span className="mb-delivery__who">{r.phone}</span> : null}
        </Stack>
      ),
    },
    {
      key: 'state',
      header: 'Where it is',
      render: (r) => (
        <Stack gap="inline">
          <Badge tone={TONES[r.state] ?? 'neutral'}>{r.stateSays}</Badge>
          {r.failure ? <span className="mb-delivery__why">{r.failure}</span> : null}
        </Stack>
      ),
    },
    {
      key: 'rider',
      header: 'Rider',
      render: (r) =>
        may ? (
          <Select
            label=""
            value={r.riderId}
            onChange={(e) => move(r, r.state, e.target.value)}
            options={[
              { value: '', label: 'Nobody yet' },
              ...view.allRiders.map((p) => ({ value: p.id, label: p.name })),
            ]}
          />
        ) : (
          <span>{r.riderName || '—'}</span>
        ),
    },
    {
      key: 'money',
      header: 'Money',
      numeric: true,
      render: (r) => (
        <Stack gap="inline">
          <span className="mb-mono">{r.total.text}</span>
          <span className={r.paid ? 'mb-delivery__paid' : 'mb-delivery__collect'}>
            {r.moneySays}
          </span>
        </Stack>
      ),
    },
    {
      key: 'do',
      header: '',
      render: (r) => {
        const next = NEXT[r.state];
        if (!may) return null;
        // A deliberate column, not a wrapped row: three buttons of very
        // different widths wrapping at the edge of a table cell came out
        // ragged, and the eye reads the middle one as belonging to the row
        // below.
        return (
          <Stack gap="inline">
            {next ? (
              <Button small variant="primary" onClick={() => move(r, next.state)}>
                {next.label}
              </Button>
            ) : null}
            {r.state === 'out' ? (
              <Button small variant="quiet" onClick={() => setFailing(r)}>
                Did not arrive
              </Button>
            ) : null}
            <Button
              small
              variant="quiet"
              onClick={() => {
                call('print_delivery_slip', { orderId: r.orderId })
                  .then(() => toast.show('ok', 'The slip is printing.'))
                  .catch(report);
              }}
            >
              <Icon name="printer" size="sm" />
              Slip
            </Button>
          </Stack>
        );
      },
    },
  ];

  const riderColumns: Column<RiderDayView>[] = [
    { key: 'name', header: 'Rider', render: (r) => <strong>{r.name}</strong> },
    { key: 'out', header: 'On the road', numeric: true, render: (r) => r.out },
    { key: 'done', header: 'Delivered', numeric: true, render: (r) => r.delivered },
    { key: 'failed', header: 'Did not arrive', numeric: true, render: (r) => r.failed },
    {
      key: 'collected',
      header: 'Cash taken',
      numeric: true,
      render: (r) => <span className="mb-mono">{r.collected.text}</span>,
    },
    {
      key: 'back',
      header: 'Handed back',
      numeric: true,
      render: (r) => <span className="mb-mono">{r.handedBack.text}</span>,
    },
    {
      key: 'carrying',
      header: 'Carrying',
      numeric: true,
      render: (r) => (
        <span className={r.carrying.paise > 0 ? 'mb-delivery__carrying' : 'mb-mono'}>
          {r.carrying.text}
        </span>
      ),
    },
    {
      key: 'do',
      header: '',
      render: (r) =>
        may && r.carrying.paise > 0 ? (
          <Button
            small
            onClick={() => {
              setHandback(r);
              // Prefilled with what they are carrying, because that is what a
              // rider hands over nine times out of ten — and the tenth time
              // the difference has to be TYPED, not defaulted away.
              setAmount(r.carrying.text);
            }}
          >
            Take the cash
          </Button>
        ) : null,
    },
  ];

  return (
    <Page className="mb-delivery">
      <PageHeader
        title="Delivery"
        subtitle="Where the food is, and where the money is."
        count={view.deliveries.length}
      />

      <Toolbar>
        <span className="mb-delivery__day">{view.day}</span>
      </Toolbar>

      {/* The one sentence the owner reads at eleven o'clock. */}
      <Notice tone={view.carrying.paise > 0 ? 'warn' : 'ok'} icon="bike">
        {view.says}
      </Notice>

      <Sections>
        <Panel title="Out today" flush>
          {view.deliveries.length === 0 ? (
            <EmptyState
              title="No deliveries today"
              body="An order becomes a delivery when you set its type to Delivery on the billing screen."
            />
          ) : (
            <Table
              rows={[...view.deliveries]}
              columns={columns}
              rowKey={(r) => r.orderId}
            />
          )}
        </Panel>

        <Panel
          title="The riders"
          note="What each of them collected, and what is still in their pocket."
          flush
          actions={
            <Button variant="secondary" onClick={() => setChoosingRiders(true)}>
              Who can ride
            </Button>
          }
        >
          {view.riders.length === 0 ? (
            <EmptyState
              title="Nobody is out"
              /* **The old words sent people to a screen that could not do it.**
                 They said "mark somebody as a rider on the Staff screen", and
                 `set_rider` had no button on the Staff screen or anywhere
                 else — it had no caller at all. The button is now beside this
                 sentence, which is where somebody reading it is looking. */
              body="Say who rides — the button above — and then give them an order."
            />
          ) : (
            <Table rows={[...view.riders]} columns={riderColumns} rowKey={(r) => r.id} />
          )}
        </Panel>
      </Sections>

      {choosingRiders ? (
        <WhoRides
          view={view}
          onClose={() => setChoosingRiders(false)}
          onChanged={setView}
          onFailed={report}
        />
      ) : null}

      {/* **A failure has to say why.** Not a default, not a shrug — the reason
          is what tells a shop whether the food comes back or is written off. */}
      <Modal
        open={failing !== null}
        title="Why did it not arrive?"
        onClose={() => setFailing(null)}
      >
        <Stack>
          <Input
            label="What happened"
            value={why}
            autoFocus
            onChange={(e) => setWhy(e.target.value)}
          />
          <Row end>
            <Button variant="quiet" onClick={() => setFailing(null)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                if (failing) move(failing, 'failed', undefined, why);
              }}
            >
              Record it
            </Button>
          </Row>
        </Stack>
      </Modal>

      <Modal
        open={handback !== null}
        title={handback ? `Cash from ${handback.name}` : 'Cash from the rider'}
        onClose={() => setHandback(null)}
      >
        <Stack>
          <Input
            label="How much they handed over"
            value={amount}
            autoFocus
            onChange={(e) => setAmount(e.target.value)}
          />
          <Row end>
            <Button variant="quiet" onClick={() => setHandback(null)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                // The typed text goes to Rust exactly as typed. R8 and D39:
                // there is one money parser in this product and it is not here.
                if (!handback || amount.trim() === '') return;
                call('record_handback', {
                  riderId: handback.id,
                  amount,
                  note: '',
                })
                  .then((fresh) => {
                    setView(fresh);
                    setHandback(null);
                    toast.show('ok', 'Taken.');
                  })
                  .catch(report);
              }}
            >
              Take it
            </Button>
          </Row>
        </Stack>
      </Modal>
    </Page>
  );
}

/**
 * **Who is allowed to take an order out** — `set_rider`, P29's command with no
 * caller until P31.
 *
 * The empty state on the riders panel used to say *"mark somebody as a rider on
 * the Staff screen"*, and there was no such control on the Staff screen or
 * anywhere else: the only riders a shop could ever have were the ones a
 * migration happened to flag. So the delivery board could be opened, and the
 * first thing it asked for could not be done.
 *
 * # A rider is a FLAG on a person, not a second kind of person
 *
 * They are already staff — they clock in, they have a PIN, they show up in the
 * audit trail. This says one more thing about them, which is why it is a
 * tick-box beside a name and not a form.
 */
function WhoRides({
  view,
  onClose,
  onChanged,
  onFailed,
}: {
  view: DeliveryBoardView;
  onClose: () => void;
  onChanged: (fresh: DeliveryBoardView) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [people, setPeople] = useState<readonly PersonView[]>([]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    call('list_staff').then(setPeople).catch(onFailed);
  }, [onFailed]);

  const isRider = (id: string) => view.riders.some((r) => r.id === id);

  return (
    <Modal open title="Who can ride" onClose={onClose}>
      <Stack>
        <p className="mb-muted">
          Anybody ticked here can be given a delivery, and the cash they collect
          is tracked against their name until they hand it over.
        </p>
        {people
          // Somebody who has left cannot be sent out with two thousand rupees.
          .filter((p) => p.status === 'active')
          .map((p) => (
            <Checkbox
              key={p.id}
              label={p.name}
              checked={isRider(p.id)}
              disabled={busy}
              onChange={(e) => {
                setBusy(true);
                call('set_rider', { staffId: p.id, isRider: e.target.checked })
                  .then(onChanged)
                  .catch(onFailed)
                  .finally(() => setBusy(false));
              }}
            />
          ))}
      </Stack>
      <div className="mb-row mb-row--end">
        <Button variant="primary" onClick={onClose}>
          Done
        </Button>
      </div>
    </Modal>
  );
}
