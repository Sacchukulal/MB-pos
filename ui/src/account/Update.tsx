/** One update, from what was found to the hand-over. Updates and the counter share it. */

import { useCallback, useEffect, useRef, useState } from 'react';

import { Button, Icon, Modal, Spinner } from '../kit';
import { call, isUiError, subscribe } from '../ipc/call';
import type { Pushed } from '../ipc/generated/Pushed';
import { keep, remember } from '../remember';

const SNOOZE_MS = 24 * 60 * 60 * 1000;
const promptKey = (version: string) => `magicbill.update-prompt.${version}`;

/** A dismissed offer waits a day; an attempted installation waits for a manual retry. */
function mayOfferAutomatically(version: string): boolean {
  const saved = remember(promptKey(version), '');
  if (saved === 'manual') return false;
  const until = Number(saved);
  return !Number.isFinite(until) || until <= Date.now();
}

/** Where the update has got to. */
export type UpdateStep =
  | { kind: 'found'; version: string; notes: string; downloaded: boolean }
  | { kind: 'busy'; version: string; stage: string; percent: number; bytes: number; total: number }
  | { kind: 'closing'; says: string }
  | { kind: 'failed'; says: string };

/** What a screen holds to run an update: the step it is on, and the three ways to move it. */
export interface UpdateRun {
  step: UpdateStep | null;
  /** Put a found version on the screen. */
  offer: (version: string, notes: string, downloaded: boolean) => void;
  install: (version: string) => void;
  goBack: (version: string) => void;
  close: () => void;
  later: (version: string) => void;
  withdraw: () => void;
}

/** Bytes as a person reads them on a progress line. */
function megabytes(bytes: number): string {
  return `${(bytes / 1_000_000).toFixed(bytes < 10_000_000 ? 1 : 0)} MB`;
}

/** The steps and the calls that move between them — the whole update, in one place. */
export function useUpdateRun(): UpdateRun {
  const [step, setStep] = useState<UpdateStep | null>(null);
  const installing = useRef(false);
  const active = useRef<{ version: string; mayReadOutcome: boolean } | null>(null);

  // Rust says how far the download has got; the dialog draws it.
  useEffect(() => {
    let stop: (() => void) | undefined;
    let disposed = false;
    subscribe((message: Pushed) => {
      // The startup offer and Updates page can both be mounted. Only the one
      // that started this install should show its progress dialog.
      const attempt = active.current;
      if (!installing.current || !attempt) return;
      if (message.kind === 'version') {
        // Before progress, the new backend call may not yet have cleared an earlier
        // failure. Once it has started, a watchdog can report a failed handoff even
        // after the original install command returned successfully.
        if (!attempt.mayReadOutcome) return;
        call('look_for_an_update')
          .then((state) => {
            if (disposed || active.current !== attempt || !installing.current) return;
            if (state.lastFailure?.version !== attempt.version) return;
            installing.current = false;
            active.current = null;
            setStep({ kind: 'failed', says: state.lastFailure.message });
          })
          .catch(() => undefined);
        return;
      }
      if (message.kind !== 'update') return;
      attempt.version = message.version;
      attempt.mayReadOutcome = true;
      keep(promptKey(message.version), 'manual');
      setStep((current) => current?.kind === 'closing' ? current : {
        kind: 'busy',
        version: message.version,
        stage: message.stage,
        percent: message.percent,
        bytes: message.bytes,
        total: message.total,
      });
    })
      .then((unlisten) => {
        if (disposed) unlisten();
        else stop = unlisten;
      })
      .catch(() => undefined);
    return () => { disposed = true; stop?.(); };
  }, []);

  const offer = useCallback((version: string, notes: string, downloaded: boolean) => {
    setStep((current) => current?.kind === 'busy' || current?.kind === 'closing'
      ? current : { kind: 'found', version, notes, downloaded });
  }, []);

  const close = useCallback(() => {
    setStep((current) => current?.kind === 'busy' || current?.kind === 'closing' ? current : null);
  }, []);

  const later = useCallback((version: string) => {
    if (remember(promptKey(version), '') !== 'manual') {
      keep(promptKey(version), String(Date.now() + SNOOZE_MS));
    }
    close();
  }, [close]);

  const withdraw = useCallback(() => {
    setStep((current) => current?.kind === 'found' ? null : current);
  }, []);

  const install = useCallback((version: string) => {
    if (installing.current) return;
    installing.current = true;
    const attempt = { version, mayReadOutcome: false };
    active.current = attempt;
    // This survives a restart if the external installer cannot finish. The Updates page
    // remains available for an explicit retry; a new release gets its own prompt policy.
    keep(promptKey(version), 'manual');
    setStep({ kind: 'busy', version, stage: 'downloading', percent: 0, bytes: 0, total: 0 });
    call('install_update')
      .then((says) => {
        if (active.current !== attempt) return;
        attempt.mayReadOutcome = true;
        setStep({ kind: 'closing', says });
      })
      .catch((cause: unknown) => {
        if (active.current !== attempt) return;
        installing.current = false;
        active.current = null;
        setStep({
          kind: 'failed',
          says: isUiError(cause) ? cause.message : 'The update could not be installed.',
        });
      });
  }, []);

  const goBack = useCallback((version: string) => {
    if (installing.current) return;
    installing.current = true;
    const attempt = { version, mayReadOutcome: false };
    active.current = attempt;
    setStep({ kind: 'busy', version, stage: 'installing', percent: 100, bytes: 0, total: 0 });
    call('go_back_a_version')
      .then((says) => {
        if (active.current !== attempt) return;
        attempt.mayReadOutcome = true;
        setStep({ kind: 'closing', says });
      })
      .catch((cause: unknown) => {
        if (active.current !== attempt) return;
        installing.current = false;
        active.current = null;
        setStep({
          kind: 'failed',
          says: isUiError(cause) ? cause.message : 'The earlier version could not be started.',
        });
      });
  }, []);

  return { step, offer, install, goBack, close, later, withdraw };
}

/** What each stage of an update is called on the screen. */
const STAGE_WORDS: Record<string, string> = {
  downloading: 'Downloading',
  checking: 'Checking the download',
  installing: 'Installing',
};

/** Every step of an update in one dialog: what was found, the download, the hand-over. */
export function UpdateDialog({ run }: { run: UpdateRun }) {
  const { step } = run;
  if (!step) return null;

  // While the installer is on its way there is nothing to close: the counter closes itself.
  const closable = step.kind !== 'busy' && step.kind !== 'closing';
  const stay = () => {
    if (step.kind === 'found') run.later(step.version);
    else if (closable) run.close();
  };

  if (step.kind === 'found') {
    return (
      <Modal
        open
        title={`Version ${step.version} is ready`}
        onClose={stay}
        actions={
          <>
            <Button variant="quiet" onClick={stay}>
              Later
            </Button>
            <Button variant="primary" onClick={() => run.install(step.version)}>
              <Icon name="download" size="sm" />
              {step.downloaded ? 'Install now' : 'Download and install'}
            </Button>
          </>
        }
      >
        {step.notes ? <p>{step.notes}</p> : null}
        <p className="mb-muted">
          Magic Bill closes while the update installs, then opens again. If installation fails,
          Updates will explain what happened. Your data is not touched. Best done before the
          first bill or after the last bill of the day. Finish or save your
          current bill first. Later pauses this reminder for a day. You can still install from Updates.
        </p>
      </Modal>
    );
  }

  if (step.kind === 'busy') {
    const stage = STAGE_WORDS[step.stage] ?? 'Working';
    const downloading = step.stage === 'downloading';
    return (
      <Modal open title={`Updating to ${step.version}`} onClose={stay}>
        <div className="mb-account__progress">
          <div className="mb-account__stage">
            <span>{stage}…</span>
            <span className="mb-code">{downloading ? `${step.percent}%` : ''}</span>
          </div>
          <div
            className="mb-progress"
            role="progressbar"
            aria-label={stage}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={downloading ? step.percent : undefined}
          >
            <div
              className={downloading ? 'mb-progress__bar' : 'mb-progress__bar mb-progress__bar--busy'}
              style={downloading ? { width: `${step.percent}%` } : undefined}
            />
          </div>
          <p className="mb-muted">
            {downloading && step.total > 0
              ? `${megabytes(step.bytes)} of ${megabytes(step.total)}`
              : step.stage === 'installing'
                ? 'Magic Bill closes now and opens again by itself.'
                : 'One moment.'}
          </p>
        </div>
      </Modal>
    );
  }

  if (step.kind === 'closing') {
    return (
      <Modal open title="Installing" onClose={stay}>
        <div className="mb-account__wait">
          <Spinner label="Closing" />
          <span>{step.says}</span>
        </div>
      </Modal>
    );
  }

  return (
    <Modal
      open
      title="Update failed"
      onClose={stay}
      actions={
        <Button variant="primary" onClick={run.close}>
          OK
        </Button>
      }
    >
      <p>{step.says}</p>
    </Modal>
  );
}

/**
 * Automatic offers use the current backend snapshot and respect this computer's reminder
 * preference across locking and restarts. Failed attempts are retried from Updates.
 */
export function UpdateOffer({ version }: { version: string | null }) {
  const run = useUpdateRun();
  const offered = useRef(new Set<string>());
  const { offer, withdraw } = run;

  useEffect(() => {
    if (version === null) {
      withdraw();
      return;
    }
    let disposed = false;
    // The event may be stale by the time this read finishes. Only its fresh snapshot can
    // offer a version; a failed read must never manufacture an installation prompt.
    call('look_for_an_update')
      .then((state) => {
        if (disposed) return;
        const available = state.available;
        if (!available || state.lastFailure?.version === available || !mayOfferAutomatically(available)) {
          withdraw();
          return;
        }
        if (offered.current.has(available)) return;
        offered.current.add(available);
        offer(available, state.notes, state.downloaded);
      })
      .catch(() => undefined);
    return () => { disposed = true; };
  }, [version, offer, withdraw]);

  return <UpdateDialog run={run} />;
}
