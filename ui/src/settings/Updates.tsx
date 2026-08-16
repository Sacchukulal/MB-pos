/**
 * **The version this counter runs, and the way back off it** — P31.
 *
 * # What was missing
 *
 * P22 built all of this in Rust: `look_for_an_update`, `dismiss_update`,
 * `go_back_a_version`, the D98 start counter that notices a release which will
 * not start, and the kept installer. Nothing ever called any of it.
 *
 * `main.rs` even logs, on a machine that has failed to start twice on the same
 * version:
 *
 * > *"version 2.4.5 has failed to start repeatedly — the previous version
 * > should be restored (**Settings > Go back**)"*
 *
 * and Settings had no Go back. This is that page — audit **E9**, **I1** and
 * **ANDROID-G2/G4**, which are all one sentence: *a shop must be able to get
 * off a bad version tonight, without us.*
 *
 * # Why "Go back" asks twice and does not do it itself
 *
 * `go_back_a_version` returns **words**, not an action: it finds the installer
 * that was kept, and says where it is. Launching a process that replaces the
 * running one is deliberately not Rust's job here — see `updates.rs`. So the
 * shop gets a sentence with a path in it, which is what a person on the phone
 * to support can act on and what a test can assert.
 */

import { useCallback, useEffect, useState } from 'react';

import { Button, Card, Notice, SectionHeader, Spinner, useToast } from '../kit';
import { call, inApp, isUiError } from '../ipc/call';
import type { UpdateState } from '../ipc/generated/UpdateState';

export function Updates() {
  const [view, setView] = useState<UpdateState | null>(null);
  const [busy, setBusy] = useState(false);
  /** The sentence `go_back_a_version` answered with, once it has. */
  const [goingBack, setGoingBack] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const toast = useToast();

  const complain = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  useEffect(() => {
    if (!inApp()) return;
    // **On open, not on a timer.** A counter does not poll for updates while
    // somebody is billing on it (M4); this asks once, when an owner is on the
    // settings screen and has time for the answer.
    call('look_for_an_update').then(setView).catch(complain);
  }, [complain]);

  if (!view) return <Spinner label="Looking at this version" />;

  return (
    <Card className="mb-updates">
      <SectionHeader
        title="This version"
        note="What this counter is running, and how to get off it if a new one goes wrong."
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
            : `${view.daysOnThisVersion} day(s) ago`}
        </dd>
      </dl>

      {view.available ? (
        <Notice tone="info">
          <strong>Version {view.available} is available.</strong>
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
        <Button
          variant="quiet"
          disabled={busy}
          onClick={() => {
            setBusy(true);
            call('look_for_an_update')
              .then((fresh) => {
                setView(fresh);
                toast.show(
                  'ok',
                  fresh.available
                    ? `Version ${fresh.available} is available.`
                    : 'You are on the newest version.',
                );
              })
              .catch(complain)
              .finally(() => setBusy(false));
          }}
        >
          Check for an update
        </Button>

        {/* **A dismissal lasts until tomorrow and no longer** — I1, and Rust
            is what enforces that. The button says the shorter thing it means
            rather than "Dismiss for ever", which it is not. */}
        {view.available && view.dismissedOn === null ? (
          <Button
            variant="quiet"
            disabled={busy}
            onClick={() => {
              setBusy(true);
              call('dismiss_update')
                .then((fresh) => {
                  setView(fresh);
                  toast.show('ok', 'Not today. We will mention it again tomorrow.');
                })
                .catch(complain)
                .finally(() => setBusy(false));
            }}
          >
            Not today
          </Button>
        ) : null}

        <Button variant="danger" disabled={busy} onClick={() => setConfirming(true)}>
          Go back a version
        </Button>
      </div>

      {confirming ? (
        <Notice tone="warn" icon="warning">
          <strong>Go back to the version you had before?</strong>
          <p>
            Your shop&rsquo;s data is not touched — only the program is. Do this
            if a new version will not start, or has broken something you need
            tonight.
          </p>
          <div className="mb-row mb-row--end">
            <Button variant="quiet" onClick={() => setConfirming(false)}>
              Stay on this one
            </Button>
            <Button
              variant="danger"
              disabled={busy}
              onClick={() => {
                setBusy(true);
                call('go_back_a_version')
                  .then((says) => {
                    setConfirming(false);
                    setGoingBack(says);
                  })
                  .catch(complain)
                  .finally(() => setBusy(false));
              }}
            >
              Go back
            </Button>
          </div>
        </Notice>
      ) : null}

      {/* The sentence Rust wrote, with the installer's path in it. Shown and
          left on screen rather than flashed as a toast: it is an instruction
          somebody has to follow, and a toast that has faded is not one. */}
      {goingBack ? (
        <Notice tone="info">
          <p>{goingBack}</p>
          <div className="mb-row mb-row--end">
            <Button variant="quiet" onClick={() => setGoingBack(null)}>
              Close
            </Button>
          </div>
        </Notice>
      ) : null}
    </Card>
  );
}
