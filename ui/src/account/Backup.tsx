/** The backups: where they are, the last one, the list, and the way back to one. */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  ConfirmDialog,
  Notice,
  Panel,
  Row,
  Spinner,
  Table,
  useToast,
  type BadgeTone,
} from '../kit';
import { call, inApp, isUiError } from '../ipc/call';
import type { BackupRowView } from '../ipc/generated/BackupRowView';
import type { BackupView } from '../ipc/generated/BackupView';

const TONES: Record<string, BadgeTone> = {
  ok: 'ok',
  warn: 'warn',
  danger: 'danger',
};

/** A folder path with the buttons that change it. */
function FolderRow({
  label,
  path,
  children,
}: {
  label: string;
  path: string;
  children?: React.ReactNode;
}) {
  return (
    <div className="mb-account__folder">
      <span className="mb-account__label">{label}</span>
      <code className="mb-account__path">{path === '' ? 'Not set' : path}</code>
      {children ? <span className="mb-account__folderactions">{children}</span> : null}
    </div>
  );
}

export function Backup() {
  const [view, setView] = useState<BackupView | null>(null);
  const [working, setWorking] = useState(false);
  const [restoring, setRestoring] = useState<BackupRowView | null>(null);
  const toast = useToast();

  const report = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  useEffect(() => {
    if (!inApp()) return;
    call('backup_status').then(setView).catch(report);
  }, [report]);

  /** Run a command that answers with the whole view. */
  const run = (work: () => Promise<BackupView | null>, done?: string) => {
    setWorking(true);
    work()
      .then((next) => {
        if (next) setView(next);
        if (done) toast.show('ok', done);
      })
      .catch(report)
      .finally(() => setWorking(false));
  };

  /** The folder picker, then the command that takes what was picked. */
  const pick = (start: string, then: (picked: string) => Promise<BackupView | null>) =>
    run(() =>
      call('pick_a_folder', { start: start === '' ? null : start }).then((picked) =>
        picked ? then(picked) : null,
      ),
    );

  if (!view) {
    return (
      <Panel title="Backup">
        <Spinner label="Looking for backups" />
      </Panel>
    );
  }

  return (
    <Panel
      title="Backup"
      actions={
        <Button
          variant="primary"
          disabled={working}
          onClick={() => run(() => call('back_up_now'), 'Backed up and checked.')}
        >
          Back up now
        </Button>
      }
    >
      {view.restoreWaiting ? (
        <Notice
          tone="warn"
          action={
            <Button
              variant="secondary"
              disabled={working}
              onClick={() => run(() => call('cancel_restore'), 'The restore is cancelled.')}
            >
              Cancel restore
            </Button>
          }
        >
          A restore is waiting. Close Magic Bill and open it again to finish it.
        </Notice>
      ) : null}

      <div className="mb-account__last">
        <span className="mb-account__label">Last backup</span>
        <span>{view.last}</span>
        <Badge tone={TONES[view.tone] ?? 'neutral'}>
          {view.tone === 'ok' ? 'Good' : view.tone === 'warn' ? 'Not checked' : 'Needed'}
        </Badge>
      </div>

      <div className="mb-account__folders">
        <FolderRow label="Shop folder" path={view.shopFolder}>
          <Button
            size="sm"
            disabled={working}
            onClick={() =>
              pick(view.shopFolder, (picked) =>
                call('use_shop_folder', { folder: picked }).then(() => {
                  // Another shop is open now: every screen starts again from it.
                  window.location.reload();
                  return null;
                }),
              )
            }
          >
            Change
          </Button>
        </FolderRow>
        <FolderRow label="Backups" path={view.folder} />
        <FolderRow label="Second copy" path={view.secondFolder}>
          <Button
            size="sm"
            disabled={working}
            onClick={() =>
              pick(view.secondFolder, (picked) =>
                call('set_second_backup_folder', { folder: picked }),
              )
            }
          >
            Choose
          </Button>
          {view.secondFolder !== '' ? (
            <Button
              size="sm"
              variant="quiet"
              disabled={working}
              onClick={() => run(() => call('set_second_backup_folder', { folder: null }))}
            >
              Remove
            </Button>
          ) : null}
        </FolderRow>
      </div>

      <Table
        rows={view.backups}
        rowKey={(row) => row.path}
        empty="No backups yet."
        columns={[
          { key: 'taken', header: 'Taken', nowrap: true, render: (row) => row.takenAt },
          { key: 'size', header: 'Size', numeric: true, render: (row) => row.size },
          {
            key: 'checked',
            header: 'Checked',
            render: (row) => (
              <span
                className={
                  row.checkedOk
                    ? 'mb-account__good'
                    : row.checked === 'Failed'
                      ? 'mb-account__bad'
                      : 'mb-account__unchecked'
                }
              >
                {row.checked}
              </span>
            ),
          },
          {
            key: 'what',
            header: '',
            render: (row) => (
              <Row end wrap={false}>
                {!row.checkedOk ? (
                  <Button
                    size="sm"
                    disabled={working}
                    onClick={() =>
                      run(() =>
                        call('verify_backup', { path: row.path }).then((found) => {
                          toast.show(found.ok ? 'ok' : 'danger', found.message, found.detail);
                          return call('backup_status');
                        }),
                      )
                    }
                  >
                    Check
                  </Button>
                ) : null}
                <Button
                  size="sm"
                  variant="quiet"
                  disabled={working || !row.checkedOk}
                  onClick={() => setRestoring(row)}
                >
                  Restore
                </Button>
              </Row>
            ),
          },
        ]}
      />

      <ConfirmDialog
        open={restoring !== null}
        destructive
        title="Restore this backup?"
        body={
          restoring
            ? `Everything done since ${restoring.takenAt} is replaced. Magic Bill does it the next time it opens, and keeps a copy of what is there now.`
            : ''
        }
        confirmLabel="Restore on next start"
        cancelLabel="Cancel"
        onCancel={() => setRestoring(null)}
        onConfirm={() => {
          const chosen = restoring;
          setRestoring(null);
          if (!chosen) return;
          run(
            () => call('request_restore', { path: chosen.path }),
            'Close Magic Bill and open it again to finish the restore.',
          );
        }}
      />
    </Panel>
  );
}
