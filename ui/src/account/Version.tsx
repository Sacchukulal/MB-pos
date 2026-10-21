/** The version this counter runs, the one waiting, and the way back off it. */

import { useCallback, useEffect, useState } from 'react';

import {
  Button,
  ConfirmDialog,
  Icon,
  Notice,
  Panel,
  Spinner,
  plural,
  useToast,
} from '../kit';
import { call, isUiError, subscribe } from '../ipc/call';
import type { Pushed } from '../ipc/generated/Pushed';
import type { UpdateState } from '../ipc/generated/UpdateState';
import { useMay } from '../shell/permissions';
import { Line, Lines } from './Lines';
import { UpdateDialog, useUpdateRun } from './Update';

export function Version() {
  const may = useMay();
  const mayGoBack = may('settings.store');
  const [view, setView] = useState<UpdateState | null>(null);
  const run = useUpdateRun();
  const { withdraw } = run;
  const [checking, setChecking] = useState(false);
  const [confirmingBack, setConfirmingBack] = useState(false);
  const toast = useToast();

  const complain = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  const refresh = useCallback(() => {
    call('look_for_an_update')
      .then((fresh) => {
        setView(fresh);
        if (!fresh.available) withdraw();
      })
      .catch(complain);
  }, [complain, withdraw]);

  // What is already known: no network, so the panel draws at once.
  useEffect(() => {
    refresh();
  }, [refresh]);

  const failed = run.step?.kind === 'failed';
  useEffect(() => {
    if (failed) refresh();
  }, [failed, refresh]);

  // A shelf read elsewhere — the update watcher — lands here too.
  useEffect(() => {
    let stop: (() => void) | undefined;
    let disposed = false;
    subscribe((message: Pushed) => {
      if (message.kind !== 'version') return;
      refresh();
    })
      .then((unlisten) => {
        if (disposed) unlisten();
        else stop = unlisten;
      })
      .catch(() => undefined);
    return () => { disposed = true; stop?.(); };
  }, [refresh]);

  if (!view) {
    return (
      <Panel title="Version">
        <Spinner label="Loading version" />
      </Panel>
    );
  }

  /** Ask the shelf. The button carries the spinner; the page stays alive. */
  const check = () => {
    setChecking(true);
    call('check_for_update')
      .then((fresh) => {
        setView(fresh);
        if (fresh.available) run.offer(fresh.available, fresh.notes, fresh.downloaded);
        else {
          withdraw();
          toast.show('ok', `This is the newest version, ${fresh.running}.`);
        }
      })
      .catch((cause: unknown) => {
        toast.show(
          'danger',
          isUiError(cause) ? cause.message : 'Magic Bill could not reach the update shelf.',
        );
      })
      .finally(() => setChecking(false));
  };

  /** The version waiting on the shelf, if one is. */
  const waiting = view.available;

  const openOffer = () => {
    setChecking(true);
    call('look_for_an_update')
      .then((fresh) => {
        setView(fresh);
        if (fresh.available) run.offer(fresh.available, fresh.notes, fresh.downloaded);
        else {
          run.withdraw();
          toast.show('info', 'This update is no longer available. Check for updates for the latest version.');
        }
      })
      .catch(complain)
      .finally(() => setChecking(false));
  };

  return (
    <Panel
      title="Version"
      actions={
        <>
          <Button variant="secondary" disabled={checking} onClick={check}>
            {checking ? <Spinner label="Checking" /> : <Icon name="refresh" size="sm" />}
            Check for updates
          </Button>
          {waiting ? (
            <Button
              variant="primary"
              disabled={checking}
              onClick={openOffer}
            >
              <Icon name="download" size="sm" />
              Install {waiting}
            </Button>
          ) : null}
        </>
      }
    >
      {view.lastFailure ? (
        <Notice tone="warn">
          Update to {view.lastFailure.version} did not finish. {view.lastFailure.message}
          {' '}This counter is running version {view.running}. Check for updates before trying again.
        </Notice>
      ) : null}
      {waiting ? (
        <Notice tone="accent" icon="download">
          Version {waiting} is ready to install.
          {view.notes ? ` ${view.notes}` : ''}
        </Notice>
      ) : null}
      {view.isDevBuild ? (
        <Notice tone="warn">This is a development build. It is not updated.</Notice>
      ) : null}

      <div className="mb-account__version">
        <span className="mb-account__number">{view.running}</span>
        <span className="mb-muted">
          {view.daysOnThisVersion === 0
            ? 'Installed today'
            : `Installed ${plural(view.daysOnThisVersion, 'day')} ago`}
        </span>
      </div>

      {view.previous && mayGoBack ? (
        <Lines>
          <Line label="Previous version">
            <span className="mb-code">{view.previous}</span>
            <Button size="sm" variant="quiet" onClick={() => setConfirmingBack(true)}>
              Go back
            </Button>
          </Line>
        </Lines>
      ) : null}

      <ConfirmDialog
        open={confirmingBack && mayGoBack}
        destructive
        title={`Go back to ${view.previous ?? ''}?`}
        body="Only the program changes. Your shop's data stays as it is. Magic Bill closes and opens again on the earlier version."
        confirmLabel="Go back"
        cancelLabel="Stay on this one"
        onCancel={() => setConfirmingBack(false)}
        onConfirm={() => {
          setConfirmingBack(false);
          run.goBack(view.previous ?? '');
        }}
      />

      <UpdateDialog run={run} />
    </Panel>
  );
}
