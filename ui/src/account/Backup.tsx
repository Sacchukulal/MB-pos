/** The backups: where they go, when they are taken, which folders get a copy, and the way back. */

import { useCallback, useEffect, useState } from 'react';

import {
  Button,
  ConfirmDialog,
  Icon,
  Input,
  Notice,
  Panel,
  Row,
  Select,
  Spinner,
  Table,
  useToast,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { BackupRowView } from '../ipc/generated/BackupRowView';
import type { BackupView } from '../ipc/generated/BackupView';
import type { CopyView } from '../ipc/generated/CopyView';
import { FolderPath, Line, Lines } from './Lines';

/** Rust's tone words are the notice's own. */
const TONES: Record<string, 'ok' | 'warn' | 'danger'> = {
  ok: 'ok',
  warn: 'warn',
  danger: 'danger',
};

/** The last backup's state in one word. */
const STATE_WORDS: Record<string, string> = {
  ok: 'Good',
  warn: 'Not checked',
  danger: 'Needed',
};

/** The schedule choices, in Rust's words. */
const SCHEDULES = [
  { value: 'off', label: 'Only when I press Back up now' },
  { value: 'hourly', label: 'Every hour' },
  { value: 'daily', label: 'Every day at' },
  { value: 'day_close', label: 'When the day is closed' },
];

/** How many backups the list shows before it asks to be opened out. */
const FEW = 6;

/** A copy folder's state, in a few words. */
function copyWhen(copy: CopyView): string {
  if (!copy.reachable) return 'Not reachable';
  return copy.last === '' ? 'Nothing copied yet' : `Copied ${copy.last}`;
}

/** "Pen drive (SANDISK)" or "Google Drive". */
function copyName(copy: CopyView): string {
  return copy.name === '' ? copy.kind : `${copy.kind} (${copy.name})`;
}

export function Backup() {
  const [view, setView] = useState<BackupView | null>(null);
  const [working, setWorking] = useState(false);
  const [restoring, setRestoring] = useState<BackupRowView | null>(null);
  const [all, setAll] = useState(false);
  /** The daily clock time as typed; saved when the box is left. */
  const [dailyAt, setDailyAt] = useState<string | null>(null);
  const toast = useToast();

  const report = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  useEffect(() => {
    call('backup_status').then(setView).catch(report);
  }, [report]);

  /** Run a command that answers with the whole view. */
  const run = (work: () => Promise<BackupView | null>, done?: string | ((v: BackupView) => string)) => {
    setWorking(true);
    work()
      .then((next) => {
        if (next) {
          setView(next);
          if (done) toast.show('ok', typeof done === 'function' ? done(next) : done);
        }
      })
      .catch(report)
      .finally(() => setWorking(false));
  };

  /** The folder picker, then the command that takes what was picked. */
  const pick = (
    start: string,
    then: (picked: string) => Promise<BackupView | null>,
    done?: string | ((v: BackupView) => string),
  ) =>
    run(
      () =>
        call('pick_a_folder', { start: start === '' ? null : start }).then((picked) =>
          picked ? then(picked) : null,
        ),
      done,
    );

  if (!view) {
    return (
      <Panel title="Backup" className="mb-account__backup">
        <Spinner label="Looking for backups" />
      </Panel>
    );
  }

  const saveSchedule = (schedule: string, at: string) =>
    run(() => call('set_backup_schedule', { schedule, dailyAt: at }));

  const shown = all ? view.backups : view.backups.slice(0, FEW);
  const tone = TONES[view.tone] ?? 'info';

  return (
    <>
      <Panel
        title="Backup"
        className="mb-account__backup"
        actions={
          <>
            <Button
              variant="secondary"
              size="sm"
              disabled={working}
              onClick={() =>
                pick(
                  '',
                  (picked) => call('save_backup_to', { folder: picked }),
                  'Saved. A fresh backup is in that folder.',
                )
              }
            >
              <Icon name="folder" size="sm" />
              Save a copy…
            </Button>
            <Button
              variant="primary"
              size="sm"
              disabled={working}
              onClick={() => run(() => call('back_up_now'), (v) => `Backed up. ${v.last}.`)}
            >
              Back up now
            </Button>
          </>
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

        <Notice tone={tone}>
          <span className="mb-account__state">
            <strong>{STATE_WORDS[view.tone] ?? view.tone}</strong>
            <span>{view.backups.length === 0 ? view.last : `Last backup ${view.last}`}</span>
            <span className="mb-muted">{view.scheduleSays}</span>
          </span>
        </Notice>

        <Lines>
          <Line
            label="Shop folder"
            action={
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
            }
          >
            <FolderPath path={view.shopFolder} />
          </Line>

          <Line
            label="Backups go to"
            action={
              <>
                {!view.folderIsDefault ? (
                  <Button
                    size="sm"
                    variant="quiet"
                    disabled={working}
                    onClick={() => run(() => call('set_backup_folder', { folder: null }))}
                  >
                    Back to the shop folder
                  </Button>
                ) : null}
                <Button
                  size="sm"
                  disabled={working}
                  onClick={() =>
                    pick(
                      view.folder,
                      (picked) => call('set_backup_folder', { folder: picked }),
                      'Backups go there from now on.',
                    )
                  }
                >
                  Change
                </Button>
              </>
            }
          >
            <FolderPath path={view.folder} />
          </Line>

          <Line label="Schedule">
            <Select
              aria-label="Schedule"
              options={SCHEDULES}
              value={view.schedule}
              disabled={working}
              onChange={(event) => saveSchedule(event.target.value, view.dailyAt)}
            />
            {view.schedule === 'daily' ? (
              <Input
                type="time"
                aria-label="Every day at"
                value={dailyAt ?? view.dailyAt}
                disabled={working}
                onChange={(event) => setDailyAt(event.currentTarget.value)}
                onBlur={() => {
                  if (dailyAt !== null && dailyAt !== '' && dailyAt !== view.dailyAt) {
                    saveSchedule(view.schedule, dailyAt);
                  }
                  setDailyAt(null);
                }}
              />
            ) : null}
          </Line>

          <Line label="Copies to">
            <ul className="mb-account__copies" aria-label="Folders that get a copy">
              {view.copies.length === 0 ? (
                <li className="mb-muted">None yet</li>
              ) : (
                view.copies.map((copy) => (
                  <li key={copy.path} className="mb-account__copy">
                    <span className="mb-account__copykind">
                      <Icon name="folder" size="sm" />
                      {copyName(copy)}
                    </span>
                    <FolderPath path={copy.path} />
                    <span
                      className={copy.reachable ? 'mb-account__copywhen' : 'mb-account__bad'}
                    >
                      {copyWhen(copy)}
                    </span>
                    <Button
                      size="sm"
                      variant="quiet"
                      disabled={working}
                      onClick={() =>
                        run(() => call('remove_backup_copy', { folder: copy.path }))
                      }
                    >
                      Remove
                    </Button>
                  </li>
                ))
              )}
              <li>
                <Row gap="inline">
                  {view.suggested.map((place) => (
                    <Button
                      key={place.path}
                      size="sm"
                      disabled={working}
                      title={place.path}
                      onClick={() =>
                        run(
                          () => call('add_backup_copy', { folder: place.path }),
                          `Every backup is copied to ${copyName(place)} from now on.`,
                        )
                      }
                    >
                      <Icon name="plus" size="sm" />
                      {copyName(place)}
                    </Button>
                  ))}
                  <Button
                    size="sm"
                    variant="quiet"
                    disabled={working}
                    onClick={() =>
                      pick(
                        '',
                        (picked) => call('add_backup_copy', { folder: picked }),
                        'Every backup is copied there from now on.',
                      )
                    }
                  >
                    <Icon name="plus" size="sm" />
                    Another folder…
                  </Button>
                </Row>
              </li>
            </ul>
          </Line>
        </Lines>
      </Panel>

      <Panel
        title="Backups taken"
        className="mb-account__history"
        flush
        actions={
          view.backups.length > FEW ? (
            <Button size="sm" variant="quiet" onClick={() => setAll((was) => !was)}>
              {all ? 'Show fewer' : `Show all ${view.backups.length}`}
            </Button>
          ) : undefined
        }
      >
        <Table
          rows={shown}
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
      </Panel>

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
    </>
  );
}
