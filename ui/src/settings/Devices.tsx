/** Settings › Devices: what is plugged in, under the settings that drive it. */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  Card,
  Icon,
  Row,
  SectionHeader,
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

  if (!view) return null;

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
      // The printer test lives under Printers, where the printers themselves are.
      toast.show('info', 'Test the printer under Printers.');
      return;
    }
    if (kind === 'label') {
      call('print_label', { line: 'Test label', token: 'TEST' })
        .then(() => toast.show('ok', 'A test label is printing.'))
        .catch(report);
    }
  };

  return (
    <Card className="mb-devices">
      <SectionHeader
        title="What is plugged in"
        note={view.says}
        action={
          <Button size="sm" variant="secondary" onClick={load}>
            <Icon name="refresh" size="sm" />
            Check again
          </Button>
        }
      />
      {/* One list, not six cards. */}
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
                <Badge tone={d.setUp ? 'ok' : 'neutral'}>{d.setUp ? 'Set up' : 'Not set up'}</Badge>
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
                  {/* The raw bytes: how a dealer sets up a scale nobody here has seen. */}
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
                <Button size="sm" onClick={() => test(d.kind)}>
                  Test it
                </Button>
              ) : null,
          },
        ]}
      />
    </Card>
  );
}
