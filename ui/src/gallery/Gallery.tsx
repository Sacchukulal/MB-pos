import { useEffect, useState } from 'react';

import {
  Badge,
  Button,
  Card,
  Checkbox,
  ConfirmDialog,
  DateRangePicker,
  EmptyState,
  Input,
  Keypad,
  Money,
  NumberInput,
  Radio,
  SaveBar,
  SearchField,
  SectionHeader,
  Select,
  Spinner,
  StatCard,
  Table,
  Tabs,
  useToast,
} from '../kit';
import { call, inApp } from '../ipc/call';
import type { PreviewDoc } from '../ipc/generated/PreviewDoc';
import { Receipt } from '../preview/Receipt';
import { TEXT_SIZES } from '../theme/themes';
import { useTheme } from '../theme/ThemeProvider';

export function Gallery() {
  const { theme, themes, setTheme, textSize, setTextSize } = useTheme();
  const toast = useToast();
  const [confirming, setConfirming] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [tab, setTab] = useState('controls');
  const [preview, setPreview] = useState<PreviewDoc | null>(null);
  const [typed, setTyped] = useState('');

  useEffect(() => {
    if (!inApp()) return;
    call('preview_test_page', { printerId: null })
      .then(setPreview)
      .catch(() => setPreview(null));
  }, []);

  return (
    <div className="mb-stack">
      <SectionHeader
        title="The kit"
        note="Every component, every state, in the current theme."
      />

      <Card>
        <SectionHeader
          title="Theme"
          note="Adding another one is a block in tokens.css and a line in themes.ts."
        />
        <div className="mb-row">
          {themes.map((t) => (
            <Button
              key={t.id}
              variant={t.id === theme.id ? 'primary' : 'secondary'}
              onClick={() => setTheme(t.id)}
            >
              {t.name}
            </Button>
          ))}
        </div>
        <Select
          label="Text size"
          value={textSize}
          onChange={(event) => setTextSize(event.target.value)}
          options={TEXT_SIZES.map((s) => ({ value: s.id, label: s.name }))}
          hint="Audit F9 — many owners are 50+ and the counter screen is across a desk."
        />
      </Card>

      <Tabs
        tabs={[
          { id: 'controls', label: 'Controls' },
          { id: 'states', label: 'States' },
          { id: 'data', label: 'Data' },
          { id: 'receipt', label: 'Receipt' },
        ]}
        active={tab}
        onChange={setTab}
      />

      {tab === 'controls' ? (
        <Card>
          <div className="mb-row">
            <Button variant="primary">Save</Button>
            <Button variant="secondary">Cancel</Button>
            <Button variant="quiet">Skip</Button>
            <Button variant="danger">Void bill</Button>
            <Button variant="primary" disabled>
              Disabled
            </Button>
          </div>
          <SearchField what="Search items" />
          <Input
            label="Customer name"
            value={typed}
            onChange={(event) => {
              setTyped(event.target.value);
              setDirty(true);
            }}
            hint="Optional."
          />
          <Input label="GSTIN" error="That is not a valid GSTIN." defaultValue="29ABC" />
          <NumberInput label="Quantity" defaultValue="1" />
          <Select
            label="Order type"
            options={[
              { value: 'dine', label: 'Dine in' },
              { value: 'parcel', label: 'Parcel' },
              { value: 'delivery', label: 'Delivery' },
            ]}
          />
          <div className="mb-row">
            <Checkbox label="Print kitchen ticket" defaultChecked />
            <Radio name="paper" label="58 mm" defaultChecked />
            <Radio name="paper" label="80 mm" />
          </div>
          <DateRangePicker from="2026-08-01" to="2026-08-04" onChange={() => undefined} />
          <Keypad onPress={(key) => toast.show('info', 'Key: ' + key)} />
        </Card>
      ) : null}

      {tab === 'states' ? (
        <Card>
          <SectionHeader
            title="States"
            note="Colour is never the only signal — grey-scale this and it still reads."
          />
          <div className="mb-row">
            <Badge>Draft</Badge>
            <Badge tone="ok">Paid</Badge>
            <Badge tone="warn">Waiting</Badge>
            <Badge tone="danger">Voided</Badge>
            <Badge tone="info">Parcel</Badge>
            <Badge tone="accent">Selected</Badge>
          </div>
          <div className="mb-row">
            <Button onClick={() => toast.show('ok', 'Bill settled.')}>Toast: ok</Button>
            <Button onClick={() => toast.show('warn', 'The kitchen printer is off.')}>
              Toast: warning
            </Button>
            <Button
              onClick={() =>
                toast.show(
                  'danger',
                  'That bill did not print.',
                  'Windows error 1801 — printer not found',
                )
              }
            >
              Toast: failure
            </Button>
            <Button onClick={() => setConfirming(true)}>Confirm dialog</Button>
          </div>
          <Spinner label="Backing up" />
          <EmptyState
            title="No open tables"
            body="Press a table number and Enter to start an order."
          />
        </Card>
      ) : null}

      {tab === 'data' ? (
        <Card>
          <div className="mb-row">
            <StatCard label="Today" value="₹12,480.00" />
            <StatCard label="Bills" value="47" />
            <StatCard label="Average" value="₹265.53" />
          </div>
          <Table
            columns={[
              { key: 'no', header: 'Bill', render: (r) => r.no },
              { key: 'table', header: 'Table', render: (r) => r.table },
              {
                key: 'total',
                header: 'Total',
                numeric: true,
                render: (r) => <Money value={r.total} />,
              },
              { key: 'state', header: '', render: (r) => r.state },
            ]}
            rows={SAMPLE_ROWS}
            rowKey={(r) => r.no}
          />
        </Card>
      ) : null}

      {tab === 'receipt' ? (
        <Card>
          <SectionHeader
            title="The bill, as it will print"
            note="The fourth sink — the same laid-out document the printer gets."
          />
          {preview ? (
            <Receipt doc={preview} />
          ) : (
            <EmptyState
              title="The preview needs the app"
              body="Run Magic Bill itself; a browser has no printer to lay out for."
            />
          )}
        </Card>
      ) : null}

      <ConfirmDialog
        open={confirming}
        title="Void this bill?"
        body="It will still appear in reports, marked as voided. This cannot be undone."
        confirmLabel="Void the bill"
        destructive
        onConfirm={() => {
          setConfirming(false);
          toast.show('ok', 'Bill voided.');
        }}
        onCancel={() => setConfirming(false)}
      />

      <SaveBar
        dirty={dirty}
        onSave={() => {
          setDirty(false);
          toast.show('ok', 'Saved.');
        }}
        onDiscard={() => setDirty(false)}
      />
    </div>
  );
}

/**
 * Sample rows. The amounts are shaped exactly as Rust sends them — integer paise plus the
 * string `Money::to_plain_string` produced — because a gallery that formatted its own numbers
 * would be demonstrating the wrong thing.
 */
const SAMPLE_ROWS = [
  {
    no: 'BIR/1207',
    table: '6',
    total: { paise: 64_600n, text: '646.00' },
    state: <Badge tone="ok">Paid</Badge>,
  },
  {
    no: 'BIR/1208',
    table: '2',
    total: { paise: 128_050n, text: '1,280.50' },
    state: <Badge tone="warn">Open</Badge>,
  },
  {
    no: 'BIR/1209',
    table: 'Parcel',
    total: { paise: 9_900n, text: '99.00' },
    state: <Badge tone="danger">Voided</Badge>,
  },
];
