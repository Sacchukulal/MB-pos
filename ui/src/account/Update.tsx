/** One update, from what was found to the hand-over. Account offers it, and so does the counter. */

import { useCallback, useEffect, useState } from 'react';

import { Button, Icon, Modal, Spinner } from '../kit';
import { call, isUiError, subscribe } from '../ipc/call';
import type { Pushed } from '../ipc/generated/Pushed';

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
}

/** Bytes as a person reads them on a progress line. */
function megabytes(bytes: number): string {
  return `${(bytes / 1_000_000).toFixed(bytes < 10_000_000 ? 1 : 0)} MB`;
}

/** The steps and the calls that move between them — the whole update, in one place. */
export function useUpdateRun(): UpdateRun {
  const [step, setStep] = useState<UpdateStep | null>(null);

  // Rust says how far the download has got; the dialog draws it.
  useEffect(() => {
    let stop: (() => void) | undefined;
    subscribe((message: Pushed) => {
      if (message.kind !== 'update') return;
      setStep({
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

  const offer = useCallback((version: string, notes: string, downloaded: boolean) => {
    setStep({ kind: 'found', version, notes, downloaded });
  }, []);

  const close = useCallback(() => setStep(null), []);

  const install = useCallback((version: string) => {
    setStep({ kind: 'busy', version, stage: 'downloading', percent: 0, bytes: 0, total: 0 });
    call('install_update')
      .then((says) => setStep({ kind: 'closing', says }))
      .catch((cause: unknown) => {
        setStep({
          kind: 'failed',
          says: isUiError(cause) ? cause.message : 'The update could not be installed.',
        });
      });
  }, []);

  const goBack = useCallback((version: string) => {
    setStep({ kind: 'busy', version, stage: 'installing', percent: 100, bytes: 0, total: 0 });
    call('go_back_a_version')
      .then((says) => setStep({ kind: 'closing', says }))
      .catch((cause: unknown) => {
        setStep({
          kind: 'failed',
          says: isUiError(cause) ? cause.message : 'The earlier version could not be started.',
        });
      });
  }, []);

  return { step, offer, install, goBack, close };
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
    if (closable) run.close();
  };

  if (step.kind === 'found') {
    return (
      <Modal
        open
        title={`Version ${step.version} is ready`}
        onClose={stay}
        actions={
          <>
            <Button variant="quiet" onClick={run.close}>
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
          Magic Bill closes and opens again on the new version. Your data is not touched. Best
          done after the last bill of the day. Later leaves it on the Account page.
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
      title="The update did not go through"
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
 * The offer itself, over whatever screen is up: a version found is shown once each time the
 * counter is opened, and Later leaves it to the bell and the Account page.
 */
export function UpdateOffer({ version }: { version: string | null }) {
  const run = useUpdateRun();
  const [offered, setOffered] = useState<string | null>(null);
  const { offer } = run;

  useEffect(() => {
    if (version === null || version === offered) return;
    setOffered(version);
    // The notes and whether the installer is already down live in the same read the panel does.
    call('look_for_an_update')
      .then((state) => offer(version, state.notes, state.downloaded))
      .catch(() => offer(version, '', false));
  }, [version, offered, offer]);

  return <UpdateDialog run={run} />;
}
