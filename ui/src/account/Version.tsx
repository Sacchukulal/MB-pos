/** The version this counter runs, the one waiting, and the way back off it. */

import { useCallback, useEffect, useState } from 'react';

import {
  Button,
  ConfirmDialog,
  Icon,
  Modal,
  Notice,
  Panel,
  Row,
  Spinner,
  plural,
  useToast,
} from '../kit';
import { call, inApp, isUiError, subscribe } from '../ipc/call';
import type { Pushed } from '../ipc/generated/Pushed';
import type { UpdateState } from '../ipc/generated/UpdateState';
import { Fact, Facts } from './Facts';

/** The one dialog every step of an update happens in. */
type Dialog =
  | { kind: 'checking' }
  | { kind: 'newest' }
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
  const [confirmingBack, setConfirmingBack] = useState(false);
  const toast = useToast();

  const complain = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  useEffect(() => {
    if (!inApp()) return;
    call('look_for_an_update').then(setView).catch(complain);
  }, [complain]);

  // Rust says how far the download has got; the dialog draws it.
  useEffect(() => {
    if (!inApp()) return undefined;
    let stop: (() => void) | undefined;
    subscribe((message: Pushed) => {
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
  }, []);

  if (!view) {
    return (
      <Panel title="Version">
        <Spinner label="Looking at this version" />
      </Panel>
    );
  }

  const check = () => {
    setDialog({ kind: 'checking' });
    call('look_for_an_update')
      .then((fresh) => {
        setView(fresh);
        setDialog(
          fresh.available
            ? {
                kind: 'found',
                version: fresh.available,
                notes: fresh.notes,
                downloaded: fresh.downloaded,
              }
            : { kind: 'newest' },
        );
      })
      .catch((cause: unknown) => {
        setDialog({
          kind: 'failed',
          says: isUiError(cause) ? cause.message : 'Magic Bill could not check for updates.',
        });
      });
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
            onClick={() =>
              setDialog({
                kind: 'found',
                version: view.available ?? '',
                notes: view.notes,
                downloaded: view.downloaded,
              })
            }
          >
            <Icon name="download" size="sm" />
            Install {view.available}
          </Button>
        ) : (
          <Button variant="secondary" onClick={check}>
            <Icon name="refresh" size="sm" />
            Check for updates
          </Button>
        )
      }
    >
      {view.available ? (
        <Notice tone="info" icon="download">
          Version {view.available} is ready to install.
          {view.notes ? ` ${view.notes}` : ''}
        </Notice>
      ) : null}
      {view.isDevBuild ? (
        <Notice tone="warn">This is a development build. It is not updated.</Notice>
      ) : null}

      <Facts>
        <Fact label="Running" code>
          {view.running}
        </Fact>
        <Fact label="Installed">
          {view.daysOnThisVersion === 0
            ? 'today'
            : `${plural(view.daysOnThisVersion, 'day')} ago`}
        </Fact>
        {view.previous ? (
          <Fact label="Before this" code>
            {view.previous}
          </Fact>
        ) : null}
      </Facts>

      {view.previous ? (
        <Row end>
          <Button variant="quiet" onClick={() => setConfirmingBack(true)}>
            Go back to {view.previous}
          </Button>
        </Row>
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

/** Every step of an update in one place: checking, what was found, the download, the hand-over. */
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

  if (dialog.kind === 'checking') {
    return (
      <Modal open title="Checking for updates" onClose={stay}>
        <div className="mb-account__wait">
          <Spinner label="Checking" />
          <span>Asking magicbill.in for the newest version.</span>
        </div>
      </Modal>
    );
  }

  if (dialog.kind === 'newest') {
    return (
      <Modal
        open
        title="Up to date"
        onClose={stay}
        actions={
          <Button variant="primary" onClick={onClose}>
            OK
          </Button>
        }
      >
        <p>This is the newest version of Magic Bill.</p>
      </Modal>
    );
  }

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
            <span className="mb-account__code">{downloading ? `${dialog.percent}%` : ''}</span>
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
