/**
 * **What is plugged in, and what it is actually sending** — P29, scope 7.6–7.9.
 *
 * # The screen exists for the dealer, not the shopkeeper
 *
 * Somebody sets a shop up once: a scanner, maybe a scale, maybe a second
 * screen. They need one place that says what is connected, a Test button per
 * device, and — the part that matters — **the raw data the device is sending**.
 * A dealer who can see the bytes can configure a brand nobody here has ever
 * seen, without a phone call.
 *
 * # Nothing here is a fault
 *
 * A shop with no scale is finished, not broken. So "Not set up" is a plain
 * grey line and never a warning: an interface that nags about hardware nobody
 * owns is an interface people stop reading.
 */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  Icon,
  Notice,
  Page,
  PageHeader,
  Panel,
  Row,
  Stack,
  Table,
  useToast,
  type IconName,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { DeviceTest } from '../ipc/generated/DeviceTest';
import type { DevicesView } from '../ipc/generated/DevicesView';

import './devices.css';

const ICONS: Record<string, IconName> = {
  printer: 'printer',
  scanner: 'scan',
  scale: 'scale',
  display: 'monitor',
  label: 'tag',
  payment: 'card',
};

export function Devices() {
  const [view, setView] = useState<DevicesView | null>(null);
  const [result, setResult] = useState<Record<string, DeviceTest>>({});
  const toast = useToast();

  const report = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  const load = useCallback(() => {
    call('device_manager').then(setView).catch(report);
  }, [report]);

  useEffect(load, [load]);

  if (!view) return <div className="mb-devices" />;

  const test = (kind: string) => {
    if (kind === 'scale') {
      call('read_scale_once')
        .then((answer) => setResult((was) => ({ ...was, scale: answer })))
        .catch(report);
      return;
    }
    if (kind === 'display') {
      call('show_customer_display', { on: true })
        .then((fresh) => {
          setView(fresh);
          toast.show('ok', 'The customer window is open on your second screen.');
        })
        .catch(report);
      return;
    }
    if (kind === 'printer') {
      // The printer test lives on the Settings screen, where the printers
      // themselves are — sending somebody there beats a second half-working
      // copy of it here (D102).
      toast.show('info', 'Open Settings, then Printers, to test a printer.');
      return;
    }
    if (kind === 'label') {
      call('print_label', { line: 'Test label', token: 'TEST' })
        .then(() => toast.show('ok', 'A test label is printing.'))
        .catch(report);
    }
  };

  return (
    <Page className="mb-devices">
      <PageHeader
        title="Devices"
        count={view.devices.filter((d) => d.setUp).length}
        actions={
          <Button variant="secondary" onClick={load}>
            <Icon name="refresh" size="sm" />
            Check again
          </Button>
        }
      />

      <Notice tone="info" icon="plug">
        {view.says}
      </Notice>

      {/* **One list, not six cards.** Six panels for six one-line facts is a
          screen a dealer scrolls; the whole point is to see everything that is
          plugged in at once. */}
      <Panel title="What is plugged in" flush>
        <Table
          rows={[...view.devices]}
          rowKey={(d) => d.kind}
          columns={[
            {
              key: 'what',
              header: 'Device',
              render: (d) => (
                <Row gap="inline">
                  <Icon name={ICONS[d.kind] ?? 'plug'} size="sm" />
                  <Stack gap="inline">
                    <strong>{d.name}</strong>
                    <span className="mb-devices__says">{d.what}</span>
                  </Stack>
                </Row>
              ),
            },
            {
              key: 'state',
              header: 'How it is',
              render: (d) => (
                <Stack gap="inline">
                  <Badge tone={d.setUp ? 'ok' : 'neutral'}>
                    {d.setUp ? 'Set up' : 'Not set up'}
                  </Badge>
                  <span className="mb-devices__says">{d.says}</span>
                </Stack>
              ),
            },
            {
              key: 'answer',
              header: 'What it said',
              render: (d) => {
                const answer = result[d.kind];
                if (!answer) return <span className="mb-devices__quiet">—</span>;
                return (
                  <Stack gap="inline">
                    <span className={answer.answered ? 'mb-devices__ok' : 'mb-devices__quiet'}>
                      {answer.says}
                    </span>
                    {/* **The raw bytes.** The whole reason a dealer can set up
                        a scale nobody here has ever seen. */}
                    {answer.raw ? <pre className="mb-devices__raw">{answer.raw}</pre> : null}
                  </Stack>
                );
              },
            },
            {
              key: 'do',
              header: '',
              render: (d) =>
                d.testable ? (
                  <Button small onClick={() => test(d.kind)}>
                    Test it
                  </Button>
                ) : null,
            },
          ]}
        />
      </Panel>
    </Page>
  );
}
