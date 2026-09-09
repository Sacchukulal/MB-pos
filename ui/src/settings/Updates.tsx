/** The version this counter runs, the one waiting, and the way back off it. */

import { useCallback, useEffect, useState } from 'react';

import { Button, Card, Icon, Modal, Notice, plural, SectionHeader, Spinner, useToast } from '../kit';
import { call, inApp, isUiError, subscribe } from '../ipc/call';
import type { Pushed } from '../ipc/generated/Pushed';
import type { UpdateState } from '../ipc/generated/UpdateState';

/** The one dialog every step of an update happens in. */
type Dialog =
  /** Asking the shelf. */
  | { kind: 'checking' }
  /** Nothing newer. */
  | { kind: 'newest' }
  /** Something newer, waiting for the word. */
  | { kind: 'found'; version: string; notes: string; downloaded: boolean }
  /** On its way in — what Rust pushes as it goes. */
  | { kind: 'busy'; version: string; stage: string; percent: number; bytes: number; total: number }
  /** Handed to the installer: the counter is closing. */
  | { kind: 'closing'; says: string }
  /** The reason it stopped, in Rust's words. */
  | { kind: 'failed'; says: string };

/** Bytes as a person reads them on a progress line. */
function megabytes(bytes: number): string {
  return `${(bytes / 1_000_000).toFixed(bytes < 10_000_000 ? 1 : 0)} MB`;
}

export function Updates() {
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
    // On open, not on a timer.
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

  if (!view) return <Spinner label="Looking at this version" />;

  /** Ask the shelf, and say what it answered. */
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
          says: isUiError(cause) ? cause.message : 'The update shelf could not be reached.',
        });
      });
  };

  /** Download, check, keep the way back, hand over to the installer. */
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
    <Card className="mb-updates">
      <SectionHeader
        title="This version"
        note="What this counter is running, what is waiting, and how to get off a new one if it goes wrong."
      />

      <dl className="mb-updates__facts">
        <dt>Running</dt>
        <dd className="mb-mono">
          {view.running}
          {view.isDevBuild ? ' (a development build)' : ''}
        </dd>
        <dt>Installed</dt>
        <dd>
          {view.daysOnThisVersion === 0
            ? 'today'
            : `${plural(view.daysOnThisVersion, 'day')} ago`}
        </dd>
        {view.previous ? (
          <>
            <dt>Before this</dt>
            <dd className="mb-mono">{view.previous}</dd>
          </>
        ) : null}
      </dl>

      {view.available ? (
        <Notice tone="info" icon="download">
          <strong>Version {view.available} is ready to install.</strong>
          {view.notes ? <p>{view.notes}</p> : null}
        </Notice>
      ) : (
        <p className="mb-muted">
          {view.isDevBuild
            ? 'This is a development build, so it is not checked against the released ones.'
            : 'This is the newest version we know about.'}
        </p>
      )}

      <div className="mb-row mb-row--end">
        <Button variant="danger" onClick={() => setConfirmingBack(true)}>
          Go back a version
        </Button>
        {view.available ? (
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
            Install {view.available}
          </Button>
        ) : (
          <Button variant="primary" onClick={check}>
            <Icon name="refresh" size="sm" />
            Check for updates
          </Button>
        )}
      </div>

      {confirmingBack ? (
        <Notice tone="warn" icon="warning">
          <strong>Go back to the version you had before?</strong>
          <p>
            Your shop&rsquo;s data is not touched — only the program is. Do this
            if a new version will not start, or has broken something you need
            tonight.
          </p>
          <div className="mb-row mb-row--end">
            <Button variant="quiet" onClick={() => setConfirmingBack(false)}>
              Stay on this one
            </Button>
            <Button variant="danger" onClick={goBack}>
              Go back
            </Button>
          </div>
        </Notice>
      ) : null}

      {dialog ? <UpdateDialog dialog={dialog} onInstall={install} onClose={() => setDialog(null)} /> : null}
    </Card>
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
        <div className="mb-updates__wait">
          <Spinner label="Asking for the newest version" />
          <span>Asking for the newest version…</span>
        </div>
      </Modal>
    );
  }

  if (dialog.kind === 'newest') {
    return (
      <Modal
        open
        title="You are up to date"
        onClose={stay}
        actions={
          <Button variant="primary" onClick={onClose}>
            OK
          </Button>
        }
      >
        <p>This counter is on the newest version of Magic Bill.</p>
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
          Magic Bill downloads it, checks it, then closes and opens again on the new version by
          itself. Your shop&rsquo;s data is not touched, and the version you have now is kept so
          you can go back. Best done after the last bill of the day.
        </p>
      </Modal>
    );
  }

  if (dialog.kind === 'busy') {
    const stage = STAGE_WORDS[dialog.stage] ?? 'Working';
    const downloading = dialog.stage === 'downloading';
    return (
      <Modal open title={`Updating to ${dialog.version}`} onClose={stay}>
        <div className="mb-updates__progress">
          <div className="mb-updates__stage">
            <span>{downloading ? `${stage}…` : `${stage}…`}</span>
            <span className="mb-mono">{downloading ? `${dialog.percent}%` : ''}</span>
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
                ? 'Magic Bill closes now and opens again on the new version by itself.'
                : 'One moment.'}
          </p>
        </div>
      </Modal>
    );
  }

  if (dialog.kind === 'closing') {
    return (
      <Modal open title="Installing" onClose={stay}>
        <div className="mb-updates__wait">
          <Spinner label="Closing for the installer" />
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
