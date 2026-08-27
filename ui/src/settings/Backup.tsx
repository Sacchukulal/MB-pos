import { useCallback, useEffect, useState } from 'react';

import { Button, Card, ConfirmDialog, EmptyState, SectionHeader, Spinner, Table, useToast } from '../kit';
import { call, inApp, isUiError } from '../ipc/call';
import type { BackupRowView } from '../ipc/generated/BackupRowView';
import type { BackupView } from '../ipc/generated/BackupView';

export function Backup() {
  const [view, setView] = useState<BackupView | null>(null);
  const [working, setWorking] = useState(false);
  const [restoring, setRestoring] = useState<BackupRowView | null>(null);
  const [places, setPlaces] = useState<readonly string[] | null>(null);
  const toast = useToast();

  const load = useCallback(() => {
    if (!inApp()) return;
    call('backup_status')
      .then(setView)
      .catch((cause) => {
        if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
      });
  }, [toast]);

  useEffect(load, [load]);

  const run = useCallback(
    async <T,>(work: Promise<T>, then?: (value: T) => void) => {
      setWorking(true);
      try {
        then?.(await work);
      } catch (cause) {
        if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
      } finally {
        setWorking(false);
      }
    },
    [toast],
  );

  if (!view) return <Spinner label="Looking for this shop's backups" />;

  return (
    <div className="mb-backup">
      {/*
        The headline. Colour AND words, because colour is never the only signal — and this is
        the one line that has to be read.
      */}
      <p className={`mb-backup__headline mb-backup__headline--${view.tone}`}>{view.headline}</p>

      {view.restoreWaiting ? (
        <Card>
          <SectionHeader
            title="A restore is waiting for the next start"
            note="Magic Bill will put this backup in place when it next opens. Nothing has changed yet."
          />
          <p className="mb-field__hint">{view.restoreWaiting}</p>
          <Button
            onClick={() =>
              void run(call('cancel_restore'), (next) => {
                setView(next);
                toast.show('info', 'The restore has been called off. Nothing was changed.');
              })
            }
          >
            Do not restore after all
          </Button>
        </Card>
      ) : null}

      <div className="mb-row">
        <Button
          variant="primary"
          disabled={working}
          onClick={() =>
            void run(call('back_up_now'), (next) => {
              setView(next);
              toast.show('ok', 'Backed up. Check it now, so you know it is good.');
            })
          }
        >
          Back up now
        </Button>
        <Button
          disabled={working}
          onClick={() =>
            void run(call('find_shops'), (found) => {
              setPlaces(found);
              if (found.length === 0) {
                toast.show('info', 'No other Magic Bill data files were found on this machine.');
              }
            })
          }
        >
          Find my shop
        </Button>
      </div>

      <Card>
        <SectionHeader title="Where things are" />
        <dl className="mb-backup__where">
          <dt>This shop&rsquo;s data</dt>
          <dd>{view.database}</dd>
          <dt>Backups go to</dt>
          <dd>{view.folder}</dd>
          <dt>Second copy</dt>
          <dd>
            {view.secondFolder === ''
              ? 'Nowhere — one copy on one disk is not a backup. Set a second folder on a pen drive or a network share in Settings.'
              : view.secondFolder}
          </dd>
        </dl>
        {places ? (
          <ul className="mb-backup__places">
            {places.map((place) => (
              <li key={place}>{place}</li>
            ))}
          </ul>
        ) : null}
      </Card>

      {view.backups.length === 0 ? (
        <EmptyState
          title="There are no backups yet"
          body="Press Back up now. It takes a snapshot while the counter keeps billing."
        />
      ) : (
        <Card>
          <SectionHeader title="The backups you have" note="Newest first." />
          <Table
            rows={view.backups}
            rowKey={(row) => row.path}
            columns={[
              { key: 'taken', header: 'Taken', render: (row) => row.takenAt },
              { key: 'size', header: 'Size', numeric: true, render: (row) => row.size },
              {
                key: 'checked',
                header: 'Checked',
                render: (row) =>
                  row.verified === null ? (
                    <span className="mb-backup__unchecked">Never checked</span>
                  ) : (
                    <span
                      className={
                        row.verifiedOk ? 'mb-backup__good' : 'mb-backup__bad'
                      }
                    >
                      {row.verified}
                    </span>
                  ),
              },
              {
                key: 'what',
                header: '',
                render: (row) => (
                  <div className="mb-row mb-row--end">
                    <Button
                      small
                      disabled={working}
                      onClick={() =>
                        void run(call('verify_backup', { path: row.path }), (report) => {
                          toast.show(
                            report.ok ? 'ok' : 'danger',
                            report.message,
                            report.detail,
                          );
                          load();
                        })
                      }
                    >
                      Check it
                    </Button>
                    <Button small variant="quiet" onClick={() => setRestoring(row)}>
                      Restore
                    </Button>
                  </div>
                ),
              },
            ]}
          />
        </Card>
      )}

      {/* This asks, and then asks the app to restart. */}
      <ConfirmDialog
        open={restoring !== null}
        destructive
        title="Put this backup in place?"
        body={
          restoring
            ? `Everything this shop has done since ${restoring.takenAt} will be replaced. Magic Bill will do it the next time it starts, and it keeps a copy of what is there now.`
            : ''
        }
        confirmLabel="Restore on the next start"
        cancelLabel="Leave it alone"
        onCancel={() => setRestoring(null)}
        onConfirm={() => {
          const chosen = restoring;
          setRestoring(null);
          if (!chosen) return;
          void run(call('request_restore', { path: chosen.path }), (next) => {
            setView(next);
            toast.show(
              'warn',
              'Close Magic Bill and open it again to finish the restore.',
            );
          });
        }}
      />
    </div>
  );
}
