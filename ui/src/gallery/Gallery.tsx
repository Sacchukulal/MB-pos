import { useEffect, useState } from 'react';

import {
  Badge,
  Button,
  Card,
  Caption,
  Checkbox,
  ConfirmDialog,
  DateRangePicker,
  EmptyState,
  Fields,
  Icon,
  Input,
  Keypad,
  Modal,
  Money,
  MoneyInput,
  Notice,
  NumberInput,
  Page,
  PageHeader,
  Panel,
  PhoneInput,
  Radio,
  Row,
  RowMenu,
  SaveBar,
  Scroller,
  SearchField,
  SectionHeader,
  Sections,
  Select,
  Spinner,
  Stack,
  StatCard,
  Stats,
  Stepper,
  Table,
  Tabs,
  useToast,
} from '../kit';
import { call, inApp } from '../ipc/call';
import type { PreviewDoc } from '../ipc/generated/PreviewDoc';
import { Receipt } from '../preview/Receipt';
import { TEXT_SIZES } from '../theme/themes';
import { useTheme } from '../theme/ThemeProvider';

/** The whole system on one screen, in the current theme. Dev only. */
export function Gallery() {
  const { theme, themes, setTheme, textSize, setTextSize } = useTheme();
  const toast = useToast();
  const [confirming, setConfirming] = useState(false);
  const [dialog, setDialog] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [tab, setTab] = useState('controls');
  const [preview, setPreview] = useState<PreviewDoc | null>(null);
  const [typed, setTyped] = useState('');
  const [money, setMoney] = useState('120.50');
  const [phone, setPhone] = useState('9840011223');
  const [qty, setQty] = useState(2);

  useEffect(() => {
    if (!inApp()) return;
    call('preview_test_page', { printerId: null })
      .then(setPreview)
      .catch(() => setPreview(null));
  }, []);

  return (
    <Page>
      <PageHeader
        title="The kit"
        count={themes.length}
        subtitle="Every kind, every size, in the current theme"
        note="Every component, every state, in the current theme. Adding a theme is one block in tokens.css."
        actions={
          <Card>
            {themes.map((t) => (
              <Button
                key={t.id}
                variant={t.id === theme.id ? 'primary' : 'secondary'}
                onClick={() => setTheme(t.id)}
              >
                {t.name}
              </Button>
            ))}
            <Select
              aria-label="Text size"
              className="mb-input--sm"
              value={textSize}
              onChange={(event) => setTextSize(event.target.value)}
              options={TEXT_SIZES.map((s) => ({ value: s.id, label: s.name }))}
            />
          </Card>
        }
      />

      <Tabs
        tabs={[
          { id: 'controls', label: 'Controls' },
          { id: 'surfaces', label: 'Surfaces' },
          { id: 'states', label: 'States' },
          { id: 'data', label: 'Data' },
          { id: 'receipt', label: 'Receipt' },
        ]}
        active={tab}
        onChange={setTab}
      />

      <Scroller>
        <Sections>
          {tab === 'controls' ? (
            <>
              <Stack gap="group">
                <SectionHeader title="Buttons" note="Four kinds, three sizes. A screen uses one primary." />
                <Row>
                  <Button variant="primary" size="lg">Complete bill</Button>
                  <Button size="lg">Kitchen ticket</Button>
                  <Button variant="primary">Save</Button>
                  <Button>Cancel</Button>
                  <Button variant="quiet">Skip</Button>
                  <Button variant="danger">Void bill</Button>
                  <Button variant="primary" disabled>Disabled</Button>
                </Row>
                <Row>
                  <Button size="sm">Edit</Button>
                  <Button size="sm" variant="quiet">Reprint</Button>
                  <Button size="sm" variant="danger">Void</Button>
                  <Button size="sm" iconOnly title="Refresh" icon={<Icon name="refresh" size="sm" />} />
                  <RowMenu label="More for this row">
                    <Button size="sm" variant="quiet">Invoice PDF</Button>
                    <Button size="sm" variant="danger">Void</Button>
                  </RowMenu>
                  <Button icon={<Icon name="download" size="sm" />}>Save as a file</Button>
                  <Button icon={<Icon name="plus" size="sm" />} variant="primary">Add an item</Button>
                </Row>
                <Row>
                  <Stepper
                    label="Quantity"
                    what="dosa"
                    onLess={() => setQty((n) => Math.max(0, n - 1))}
                    onMore={() => setQty((n) => n + 1)}
                  >
                    <span className="mb-stepper__value">{qty}</span>
                  </Stepper>
                  <span className="mb-kbd">Esc</span>
                  <span className="mb-kbd">Enter</span>
                </Row>
              </Stack>

              <Stack gap="group">
                <SectionHeader title="Segment and tabs" note="A segment is exactly one of these. Tabs are a lens. They never look alike." />
                <Row>
                  <div className="mb-segment">
                    <button type="button" className="mb-segment__option" aria-pressed="true">Dine in</button>
                    <button type="button" className="mb-segment__option">Parcel</button>
                    <button type="button" className="mb-segment__option">Delivery</button>
                  </div>
                  <div className="mb-segment mb-segment--lg">
                    <button type="button" className="mb-segment__option" aria-pressed="true">Cash</button>
                    <button type="button" className="mb-segment__option">Card</button>
                    <button type="button" className="mb-segment__option">UPI</button>
                  </div>
                </Row>
              </Stack>

              <Stack gap="group">
                <SectionHeader title="Fields" note="Widths come from the size scale: xs, sm, md, lg, fill." />
                <Row>
                  <SearchField what="Find an item" />
                  <div className="mb-field mb-field--xs">
                    <NumberInput label="Qty" defaultValue="1" />
                  </div>
                  <div className="mb-field mb-field--sm">
                    <MoneyInput label="Amount" value={money} onChange={setMoney} />
                  </div>
                  <div className="mb-field mb-field--md">
                    <PhoneInput label="Phone" value={phone} onChange={setPhone} />
                  </div>
                </Row>
                <Fields columns>
                  <Input
                    label="Customer name"
                    value={typed}
                    onChange={(event) => {
                      setTyped(event.target.value);
                      setDirty(true);
                    }}
                    hint="Optional."
                  />
                  <Input label="GSTIN" error="That is not a valid GSTIN." defaultValue="29ABC" className="mb-code" />
                  <Select
                    label="Order type"
                    options={[
                      { value: 'dine', label: 'Dine in' },
                      { value: 'parcel', label: 'Parcel' },
                      { value: 'delivery', label: 'Delivery' },
                    ]}
                  />
                  <Input label="Disabled" disabled defaultValue="Not yours to change" />
                </Fields>
                <Caption>Taking money by UPI</Caption>
                <Row>
                  <Checkbox label="Print kitchen ticket" defaultChecked hint="Off for a shop with no kitchen printer." />
                  <Radio name="paper" label="58 mm" defaultChecked />
                  <Radio name="paper" label="80 mm" />
                </Row>
                <DateRangePicker from="2026-08-01" to="2026-08-04" onChange={() => undefined} />
                <div className="mb-field mb-field--sm">
                  <Keypad onPress={(key) => toast.show('info', 'Key: ' + key)} />
                </div>
              </Stack>
            </>
          ) : null}

          {tab === 'surfaces' ? (
            <>
              <Stack gap="group">
                <SectionHeader title="Panel" note="The one raised surface: a fill and a shadow, no border. Never inside another." />
                <Panel title="A panel" note="With a head." actions={<Button size="sm">Action</Button>}>
                  Body text at 14 px on the surface.
                </Panel>
                <Panel flush title="A flush panel">
                  <Table
                    columns={[
                      { key: 'no', header: 'Bill', render: (r) => r.no },
                      { key: 'total', header: 'Total', numeric: true, render: (r) => <Money value={r.total} /> },
                    ]}
                    rows={SAMPLE_ROWS}
                    rowKey={(r) => r.no}
                  />
                </Panel>
              </Stack>
              <Stack gap="group">
                <SectionHeader title="Notices" note="One line, an icon, an action if there is one." />
                <Notice tone="info" action={<Button size="sm">Do it</Button>}>Tell us about your shop.</Notice>
                <Notice tone="ok">Everything is back.</Notice>
                <Notice tone="warn">Yesterday was never closed.</Notice>
                <Notice tone="danger">Nothing is printing.</Notice>
                <Notice tone="warn" standing>This computer has no licence yet.</Notice>
              </Stack>
              <Stack gap="group">
                <SectionHeader title="Dialog" note="The title and the actions stay put; the body scrolls." />
                <Row>
                  <Button onClick={() => setDialog(true)}>Open a dialog</Button>
                  <Button onClick={() => setConfirming(true)}>Confirm dialog</Button>
                </Row>
              </Stack>
            </>
          ) : null}

          {tab === 'states' ? (
            <>
              <Stack gap="group">
                <SectionHeader title="Chips" note="A soft fill and a word. No border." />
                <Row>
                  <Badge>Draft</Badge>
                  <Badge tone="ok">Paid</Badge>
                  <Badge tone="warn">Waiting</Badge>
                  <Badge tone="danger">Voided</Badge>
                  <Badge tone="info">Parcel</Badge>
                  <Badge tone="accent">Selected</Badge>
                </Row>
              </Stack>
              <Stack gap="group">
                <SectionHeader title="Toasts" />
                <Row>
                  <Button onClick={() => toast.show('ok', 'Bill settled.')}>Toast: ok</Button>
                  <Button onClick={() => toast.show('warn', 'The kitchen printer is off.')}>Toast: warning</Button>
                  <Button
                    onClick={() =>
                      toast.show('danger', 'That bill did not print.', 'Windows error 1801 — printer not found')
                    }
                  >
                    Toast: failure
                  </Button>
                </Row>
                <Spinner label="Backing up" />
              </Stack>
              <Stack gap="group">
                <SectionHeader title="Empty" note="One line, and what to do about it." />
                <EmptyState
                  title="No open tables"
                  hint="Press a table number and Enter to start an order."
                  action={<Button variant="primary">New order</Button>}
                />
                <EmptyState small title="Nothing on this bill yet" says="Type an item or a table number." />
              </Stack>
            </>
          ) : null}

          {tab === 'data' ? (
            <>
              <Stack gap="group">
                <SectionHeader title="Stats" note="Equal widths, one row, four at most." />
                <Stats>
                  <StatCard label="Takings" value="12,480.00" note="47 bills" />
                  <StatCard label="Average bill" value="265.53" />
                  <StatCard label="In the drawer" value="−240.00" note="Before counting." />
                  <StatCard label="Voided" value="0.00" />
                </Stats>
              </Stack>
              <Stack gap="group">
                <SectionHeader title="Table" note="Rows of 40 px; figures tabular; two actions at most, the rest in ⋯." />
                <Table
                  columns={[
                    { key: 'no', header: 'Bill', render: (r) => r.no },
                    { key: 'table', header: 'Table', render: (r) => r.table },
                    { key: 'total', header: 'Total', numeric: true, render: (r) => <Money value={r.total} /> },
                    { key: 'state', header: 'State', render: (r) => r.state },
                    {
                      key: 'act',
                      header: '',
                      render: () => (
                        <Row gap="inline" wrap={false}>
                          <Button size="sm" variant="quiet">Reprint</Button>
                          <RowMenu>
                            <Button size="sm" variant="quiet">Invoice PDF</Button>
                            <Button size="sm" variant="danger">Void</Button>
                          </RowMenu>
                        </Row>
                      ),
                    },
                  ]}
                  rows={SAMPLE_ROWS}
                  rowKey={(r) => r.no}
                  footer={['Total', '', <Money key="t" value={{ paise: 202_550n, text: '2,025.50' }} />, '', '']}
                />
              </Stack>
            </>
          ) : null}

          {tab === 'receipt' ? (
            <Stack gap="group">
              <SectionHeader title="The bill, as it will print" note="The same laid-out document the printer gets." />
              {preview ? (
                <Receipt doc={preview} />
              ) : (
                <EmptyState title="The preview needs the app" hint="Run Magic Bill itself; a browser has no printer to lay out for." />
              )}
            </Stack>
          ) : null}
        </Sections>
      </Scroller>

      <Modal
        open={dialog}
        title="A dialog"
        note="What this dialog is for."
        onClose={() => setDialog(false)}
        actions={
          <>
            <Button onClick={() => setDialog(false)}>Cancel</Button>
            <Button variant="primary" onClick={() => setDialog(false)}>Save</Button>
          </>
        }
      >
        <Fields>
          <Input label="Name" />
          <Select label="Role" options={[{ value: 'c', label: 'Cashier' }]} />
        </Fields>
      </Modal>

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
    </Page>
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
