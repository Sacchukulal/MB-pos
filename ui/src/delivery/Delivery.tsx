/** The board for orders that leave on a bike. */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  Checkbox,
  EmptyState,
  Icon,
  Input,
  Modal,
  MoneyInput,
  Page,
  PageHeader,
  Panel,
  Row,
  Sections,
  Select,
  Stack,
  Table,
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
  pending: { state: 'assigned', label: 'Assign rider' },
  assigned: { state: 'out', label: 'Dispatch' },
  out: { state: 'delivered', label: 'Mark delivered' },
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
  /** Who is allowed to take an order out, which nothing could set. */
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
      header: 'Address',
      render: (r) => (
        <Stack gap="inline">
          <span className="mb-delivery__address">{r.address || 'No address typed'}</span>
          {r.phone ? <span className="mb-delivery__who">{r.phone}</span> : null}
        </Stack>
      ),
    },
    {
      key: 'state',
      header: 'Status',
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
              { value: '', label: 'Unassigned' },
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
        // A deliberate column, not a wrapped row: three buttons of very different widths
        // wrapping at the edge of a table cell came out ragged, and the eye reads the middle
        // one as belonging to the row below.
        return (
          <Stack gap="inline">
            {next ? (
              <Button size="sm" variant="primary" onClick={() => move(r, next.state)}>
                {next.label}
              </Button>
            ) : null}
            {r.state === 'out' ? (
              <Button size="sm" variant="quiet" onClick={() => setFailing(r)}>
                Did not arrive
              </Button>
            ) : null}
            <Button
              size="sm"
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
    { key: 'failed', header: 'Failed', numeric: true, render: (r) => r.failed },
    {
      key: 'collected',
      header: 'Cash collected',
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
      header: 'Cash with rider',
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
            size="sm"
            onClick={() => {
              setHandback(r);
              // Prefilled with what they are carrying, because that is what a rider hands over
              // nine times out of ten — and the tenth time the difference has to be TYPED, not
              // defaulted away.
              setAmount(r.carrying.text);
            }}
          >
            Record cash received
          </Button>
        ) : null,
    },
  ];

  return (
    <Page className="mb-delivery">
      {/* The day and the money on the road are the header's fact line, not a box each. */}
      <PageHeader
        title="Delivery"
        count={view.deliveries.length}
        subtitle={`${view.day} · ${view.deliveries.length === 0 ? 'No deliveries today' : view.says}`}
      />

      <Sections>
        <Panel title="Out today" flush>
          {view.deliveries.length === 0 ? (
            <EmptyState
              title="No deliveries today"
              hint="An order becomes a delivery when you set its type to Delivery on the billing screen."
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
          title="Riders"
          note="Cash collected, returned and still held by each rider."
          flush
          actions={
            <Button variant="secondary" onClick={() => setChoosingRiders(true)}>
              Delivery riders
            </Button>
          }
        >
          {view.riders.length === 0 ? (
            <EmptyState
              title="No active riders"
              /* The old words sent people to a screen that could not do it. */
              hint="Choose delivery riders using the button above."
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

      {/* A failure has to say why. */}
      <Modal
        open={failing !== null}
        title="Record failed delivery"
        onClose={() => setFailing(null)}
       actions={<Row end>
            <Button variant="quiet" onClick={() => setFailing(null)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                if (failing) move(failing, 'failed', undefined, why);
              }}
            >
              Record failure
            </Button>
          </Row>}>
        <Stack>
          <Input
            label="Failure reason"
            value={why}
            autoFocus
            onChange={(e) => setWhy(e.target.value)}
          />

        </Stack>
      </Modal>

      <Modal
        open={handback !== null}
        title={handback ? `Cash from ${handback.name}` : 'Cash from the rider'}
        onClose={() => setHandback(null)}
       actions={<Row end>
            <Button variant="quiet" onClick={() => setHandback(null)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                // The typed text goes to Rust exactly as typed.
                if (!handback || amount.trim() === '') return;
                call('record_handback', {
                  riderId: handback.id,
                  amount,
                  note: '',
                })
                  .then((fresh) => {
                    setView(fresh);
                    setHandback(null);
                    toast.show('ok', 'Cash received recorded.');
                  })
                  .catch(report);
              }}
            >
              Record cash received
            </Button>
          </Row>}>
        <Stack>
          <MoneyInput
            label="Cash received"
            value={amount}
            autoFocus
            onChange={setAmount}
          />

        </Stack>
      </Modal>
    </Page>
  );
}

/** Who is allowed to take an order out. */
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
    <Modal open title="Delivery riders" onClose={onClose} actions={<div className="mb-row mb-row--end">
        <Button variant="primary" onClick={onClose}>
          Close
        </Button>
      </div>}>
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

    </Modal>
  );
}
