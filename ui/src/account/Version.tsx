/** The version this counter runs, the one waiting, and the way back off it. */

import { useCallback, useEffect, useState } from 'react';

import {
  Button,
  ConfirmDialog,
  Icon,
  Modal,
  Notice,
  Panel,
  Spinner,
  plural,
  useToast,
} from '../kit';
import { call, isUiError, subscribe } from '../ipc/call';
import type { Pushed } from '../ipc/generated/Pushed';
import type { UpdateState } from '../ipc/generated/UpdateState';
import { Line, Lines } from './Lines';

/** The one dialog an update happens in, once one is found. */
type Dialog =
  | { kind: 'found'; version: string; notes: string; downloaded: boolean }
  | { kind: 'busy'; version: string; stage: string; percent: number; bytes: number; total: number }
  | { kind: 'closing'; says: string }
  | { kind: 'failed'; says: string };

/** Bytes as a person reads them on a progress line. */
function megabytes(bytes: number): string {
  return `${(bytes / 1_000_000).toFixed(bytes < 10_000_000 ? 1 : 0)} MB`;
}

export function Version() {
  const [view, setView] = useState<UpdateState | null>(null);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [checking, setChecking] = useState(false);
  const [confirmingBack, setConfirmingBack] = useState(false);
  const toast = useToast();

  const complain = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  // What is already known: no network, so the panel draws at once.
  useEffect(() => {
    call('look_for_an_update').then(setView).catch(complain);
  }, [complain]);

  // Rust says how far the download has got; the dialog draws it. A shelf read elsewhere
  // (the daily licence check) lands here too.
  useEffect(() => {
    let stop: (() => void) | undefined;
    subscribe((message: Pushed) => {
      if (message.kind === 'version') {
        call('look_for_an_update').then(setView).catch(complain);
      }
      if (message.kind !== 'update') return;
      setDialog({
        kind: 'busy',
        version: message.version,
        stage: message.stage,
        percent: message.percent,
        bytes: message.bytes,
        total: message.total,
      });
    })
      .then((unlisten) => {
        stop = unlisten;
      })
      .catch(() => undefined);
    return () => stop?.();
  }, [complain]);

  if (!view) {
    return (
      <Panel title="Version">
        <Spinner label="Looking at this version" />
      </Panel>
    );
  }

  const found = (fresh: UpdateState): Dialog | null =>
    fresh.available
      ? {
          kind: 'found',
          version: fresh.available,
          notes: fresh.notes,
          downloaded: fresh.downloaded,
        }
      : null;

  /** Ask the shelf. The button carries the spinner; the page stays alive. */
  const check = () => {
    setChecking(true);
    call('check_for_update')
      .then((fresh) => {
        setView(fresh);
        const dialogFound = found(fresh);
        if (dialogFound) setDialog(dialogFound);
        else toast.show('ok', `This is the newest version, ${fresh.running}.`);
      })
      .catch((cause: unknown) => {
        toast.show(
          'danger',
          isUiError(cause) ? cause.message : 'Magic Bill could not reach the update shelf.',
        );
      })
      .finally(() => setChecking(false));
  };

  const install = (version: string) => {
    setDialog({ kind: 'busy', version, stage: 'downloading', percent: 0, bytes: 0, total: 0 });
    call('install_update')
      .then((says) => setDialog({ kind: 'closing', says }))
      .catch((cause: unknown) => {
        setDialog({
          kind: 'failed',
          says: isUiError(cause) ? cause.message : 'The update could not be installed.',
        });
      });
  };

  const goBack = () => {
    setConfirmingBack(false);
    setDialog({
      kind: 'busy',
      version: view.previous ?? '',
      stage: 'installing',
      percent: 100,
      bytes: 0,
      total: 0,
    });
    call('go_back_a_version')
      .then((says) => setDialog({ kind: 'closing', says }))
      .catch((cause: unknown) => {
        setDialog({
          kind: 'failed',
          says: isUiError(cause) ? cause.message : 'The earlier version could not be started.',
        });
      });
  };

  return (
    <Panel
      title="Version"
      actions={
        view.available ? (
          <Button
            variant="primary"
            onClick={() => {
              const dialogFound = found(view);
              if (dialogFound) setDialog(dialogFound);
            }}
          >
            <Icon name="download" size="sm" />
            Install {view.available}
          </Button>
        ) : (
          <Button variant="secondary" disabled={checking} onClick={check}>
            {checking ? <Spinner label="Checking" /> : <Icon name="refresh" size="sm" />}
            Check for updates
          </Button>
        )
      }
    >
      {view.available ? (
        <Notice tone="accent" icon="download">
          Version {view.available} is ready to install.
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

      {view.previous ? (
        <Lines>
          <Line label="Before this">
            <span className="mb-code">{view.previous}</span>
            <Button size="sm" variant="quiet" onClick={() => setConfirmingBack(true)}>
              Go back
            </Button>
          </Line>
        </Lines>
      ) : null}

      <ConfirmDialog
        open={confirmingBack}
        destructive
        title={`Go back to ${view.previous ?? ''}?`}
        body="Only the program changes. Your shop's data stays as it is. Magic Bill closes and opens again on the earlier version."
        confirmLabel="Go back"
        cancelLabel="Stay on this one"
        onCancel={() => setConfirmingBack(false)}
        onConfirm={goBack}
      />

      {dialog ? (
        <UpdateDialog dialog={dialog} onInstall={install} onClose={() => setDialog(null)} />
      ) : null}
    </Panel>
  );
}

/** What each stage of an update is called on the screen. */
const STAGE_WORDS: Record<string, string> = {
  downloading: 'Downloading',
  checking: 'Checking the download',
  installing: 'Installing',
};

/** Every step of an update in one place: what was found, the download, the hand-over. */
function UpdateDialog({
  dialog,
  onInstall,
  onClose,
}: {
  dialog: Dialog;
  onInstall: (version: string) => void;
  onClose: () => void;
}) {
  // While the installer is on its way there is nothing to close: the counter closes itself.
  const closable = dialog.kind !== 'busy' && dialog.kind !== 'closing';
  const stay = () => {
    if (closable) onClose();
  };

  if (dialog.kind === 'found') {
    return (
      <Modal
        open
        title={`Version ${dialog.version} is ready`}
        onClose={stay}
        actions={
          <>
            <Button variant="quiet" onClick={onClose}>
              Later
            </Button>
            <Button variant="primary" onClick={() => onInstall(dialog.version)}>
              <Icon name="download" size="sm" />
              {dialog.downloaded ? 'Install now' : 'Download and install'}
            </Button>
          </>
        }
      >
        {dialog.notes ? <p>{dialog.notes}</p> : null}
        <p className="mb-muted">
          Magic Bill closes and opens again on the new version. Your data is not touched. Best
          done after the last bill of the day.
        </p>
      </Modal>
    );
  }

  if (dialog.kind === 'busy') {
    const stage = STAGE_WORDS[dialog.stage] ?? 'Working';
    const downloading = dialog.stage === 'downloading';
    return (
      <Modal open title={`Updating to ${dialog.version}`} onClose={stay}>
        <div className="mb-account__progress">
          <div className="mb-account__stage">
            <span>{stage}…</span>
            <span className="mb-code">{downloading ? `${dialog.percent}%` : ''}</span>
          </div>
          <div
            className="mb-progress"
            role="progressbar"
            aria-label={stage}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={downloading ? dialog.percent : undefined}
          >
            <div
              className={downloading ? 'mb-progress__bar' : 'mb-progress__bar mb-progress__bar--busy'}
              style={downloading ? { width: `${dialog.percent}%` } : undefined}
            />
          </div>
          <p className="mb-muted">
            {downloading && dialog.total > 0
              ? `${megabytes(dialog.bytes)} of ${megabytes(dialog.total)}`
              : dialog.stage === 'installing'
                ? 'Magic Bill closes now and opens again by itself.'
                : 'One moment.'}
          </p>
        </div>
      </Modal>
    );
  }

  if (dialog.kind === 'closing') {
    return (
      <Modal open title="Installing" onClose={stay}>
        <div className="mb-account__wait">
          <Spinner label="Closing" />
          <span>{dialog.says}</span>
        </div>
      </Modal>
    );
  }

  return (
    <Modal
      open
      title="The update did not go through"
      onClose={stay}
      actions={
        <Button variant="primary" onClick={onClose}>
          OK
        </Button>
      }
    >
      <p>{dialog.says}</p>
    </Modal>
  );
}
