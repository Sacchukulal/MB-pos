/**
 * **The account screen** — scope 2.10, and P21's face.
 *
 * # What v1 put here, and what was wrong with it
 *
 * *"After activation: your name, business, mobile, email, plan name, status
 * chip, **next billing date**, days left, 'Renew Plan' link, 'Sync Data' and
 * 'Deactivate' buttons."*
 *
 * Two things. The first is that **"your plan renews on 12 September" beats a
 * date field** — a screen that makes an owner do the arithmetic has not
 * answered the question they opened it to ask. The second is that Deactivate
 * lied: it cleared local storage and left the server holding the binding, so
 * the owner then could not activate on a new PC (BACKEND-C5). Here, Deactivate
 * says what it actually did, in both cases.
 *
 * # Nothing on this screen decides anything
 *
 * R8. Every sentence — the banner, the refusal, "checked 4 minutes ago", the
 * still-held warning, the clock note — is composed in `src-tauri/src/words.rs`,
 * which is *the one place a machine state becomes words* (crown jewel 14). This
 * file chooses which of them to draw and where. There is no date arithmetic, no
 * plural, and no "if expired then" in here.
 *
 * Every command returns the whole `LicenceView`, so pressing a button is
 * `setView(await call(...))` and there is no local state to fall out of step.
 */

import { useCallback, useEffect, useState } from 'react';

import { Badge, Button, Card, Input, Modal, SectionHeader, useToast, type BadgeTone,
  Page,
  PageHeader,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { LicenceView } from '../ipc/generated/LicenceView';

import './account.css';

/**
 * The tone the counter chose, which is already a `BadgeTone` by name.
 *
 * Deliberately a lookup and not a cast: `LicenceView.tone` is a `String` on the
 * Rust side, so nothing but this line would notice if `tone_for` ever grew a
 * fourth answer. Colour is never the only carrier (§2) — the chip and the
 * sentence both say it too, so an unknown tone falls back to neutral and loses
 * nothing.
 */
const TONES: Record<string, BadgeTone> = {
  ok: 'ok',
  warn: 'warn',
  danger: 'danger',
};

/** magicbill.in, where a plan is actually bought. Phase 10 owns payment. */
const RENEW_AT = 'https://magicbill.in/renew';

type Asking = 'activate' | 'transfer' | 'trial' | 'emergency' | null;

export function Account() {
  const [view, setView] = useState<LicenceView | null>(null);
  const [asking, setAsking] = useState<Asking>(null);
  const [key, setKey] = useState('');
  const [proof, setProof] = useState('');
  const [contact, setContact] = useState('');
  const [code, setCode] = useState('');
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const toast = useToast();

  const report = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  const load = useCallback(() => {
    call('account').then(setView).catch(report);
  }, [report]);

  useEffect(load, [load]);

  /** Run a licensing command, take the whole view back, and close the dialog. */
  const run = (work: () => Promise<LicenceView>, done?: string) => {
    setBusy(true);
    work()
      .then((fresh) => {
        setView(fresh);
        setAsking(null);
        setKey('');
        setProof('');
        setCode('');
        setContact('');
        if (done) toast.show("ok", done);
      })
      .catch(report)
      .finally(() => setBusy(false));
  };

  if (!view) return null;

  const tone = TONES[view.tone] ?? 'neutral';

  return (
    <Page className="mb-account">
      <PageHeader
        title="Account"
        subtitle="Your plan, this computer, and how to move your licence."
      />

      {/* The state of things, in one card. */}
      <Card>
        <div className="mb-account__top mb-row">
          <div className="mb-account__who">
            <div className="mb-account__shop">{view.shopName || 'This counter'}</div>
            <div className="mb-account__plan">{view.planName}</div>
          </div>
          <Badge tone={tone}>{view.chip}</Badge>
        </div>

        {view.headline !== '' && <p className="mb-account__headline">{view.headline}</p>}
        {view.renewalSentence !== '' && (
          <p className="mb-account__renews">{view.renewalSentence}</p>
        )}

        {/* BACKEND-C5. The one sentence v1 never said. */}
        {view.stillHeld !== '' && (
          <p className="mb-account__warn" role="status">
            {view.stillHeld}
          </p>
        )}
        {/* D90 — a warning, never a lock. */}
        {view.clockNote !== '' && (
          <p className="mb-account__warn" role="status">
            {view.clockNote}
          </p>
        )}

        <div className="mb-account__actions mb-row mb-row--end">
          <Button variant="quiet" onClick={load}>
            Check again
          </Button>
          {view.isActivated ? (
            <>
              <Button
                variant="quiet"
                onClick={() => {
                  window.open(RENEW_AT, '_blank');
                }}
              >
                Renew
              </Button>
              <Button
                variant="quiet"
                disabled={!view.mayManage}
                onClick={() => setAsking('emergency')}
              >
                Emergency code
              </Button>
              <Button
                variant="danger"
                disabled={!view.mayManage}
                onClick={() => setConfirming(true)}
              >
                Deactivate
              </Button>
            </>
          ) : (
            <>
              <Button
                variant="quiet"
                disabled={!view.mayManage}
                onClick={() => setAsking('emergency')}
              >
                Emergency code
              </Button>
              <Button
                variant="quiet"
                disabled={!view.mayManage}
                onClick={() => setAsking('trial')}
              >
                Start a free trial
              </Button>
              <Button
                variant="primary"
                disabled={!view.mayManage}
                onClick={() => setAsking('activate')}
              >
                Enter licence key
              </Button>
            </>
          )}
        </div>
      </Card>

      {/* What the plan includes. Reading a limit here is how an owner finds out
          why a sixteenth phone would not join. */}
      <Card>
        {/* A counter with no licence has no plan, and a heading that says "your
            plan" over a card that lists two phones and a till is a small lie an
            owner notices. Found by looking at it. */}
        <SectionHeader
          title={view.isActivated ? 'What your plan includes' : 'What you can use now'}
        />
        <dl className="mb-account__facts">
          <div>
            <dt>Phones</dt>
            <dd>{view.phonesAllowed}</dd>
          </div>
          <div>
            <dt>Tills</dt>
            <dd>{view.tillsAllowed}</dd>
          </div>
          {view.renewsOn !== '' && (
            <div>
              <dt>Renews</dt>
              <dd>{view.renewsOn}</dd>
            </div>
          )}
          <div>
            <dt>Last checked</dt>
            <dd>{view.checked}</dd>
          </div>
        </dl>
        {view.included.length > 0 && (
          <p className="mb-account__included">{view.included.join(' · ')}</p>
        )}
      </Card>

      {/* This computer. The id is here so support can ask for it, and the
          derivation is here because "we could not read this machine's id and
          made one up" is a thing an owner is entitled to know. */}
      <Card>
        <SectionHeader title="This computer" />
        <dl className="mb-account__facts">
          <div>
            <dt>Computer</dt>
            <dd className="mb-account__machine">{view.machine}</dd>
          </div>
          <div>
            <dt>Identified</dt>
            <dd>{view.machineHow}</dd>
          </div>
        </dl>
        {view.machineIsFragile && (
          <p className="mb-account__warn">
            We could not read a permanent id from this computer, so we made one.
            If Magic Bill is reinstalled you may need to move the licence again.
          </p>
        )}
        <div className="mb-account__actions mb-row mb-row--end">
          <Button variant="quiet" disabled={!view.mayManage} onClick={() => setAsking('transfer')}>
            Move a licence here
          </Button>
        </div>
      </Card>

      {/* --- the dialogs ---------------------------------------------------- */}

      <Modal
        open={asking === 'activate' || asking === 'transfer'}
        title={asking === 'transfer' ? 'Move a licence to this computer' : 'Enter your licence key'}
        onClose={() => setAsking(null)}
      >
        <p className="mb-account__note">
          {asking === 'transfer'
            ? 'This will stop the licence working on the computer it is on now. We will send a code to the mobile number registered with the licence.'
            : 'We will send a code to the mobile number registered with your licence, so that nobody else can use your key.'}
        </p>
        {/*
          `autoComplete="off"` on both, found by driving it: WebView2 offered to
          remember the verification code and then drew its "Saved info" dropdown
          straight over the Activate button. A one-time code is the last thing a
          browser should keep, and a suggestion list covering the only button in
          a dialog is the kind of thing no test can see.
        */}
        <Input
          label="Licence key"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          autoComplete="off"
          autoFocus
        />
        <Input
          label="Code we sent you"
          value={proof}
          onChange={(e) => setProof(e.target.value)}
          autoComplete="off"
        />
        <div className="mb-row mb-row--end">
          <Button variant="quiet" onClick={() => setAsking(null)}>
            Cancel
          </Button>
          <Button
            variant="primary"
            disabled={busy || key.trim() === '' || proof.trim() === ''}
            onClick={() =>
              run(
                () =>
                  asking === 'transfer'
                    ? call('transfer_here', { key, proof })
                    : call('activate', { key, proof }),
                asking === 'transfer' ? 'The licence is now on this computer.' : 'Activated.',
              )
            }
          >
            {asking === 'transfer' ? 'Move it here' : 'Activate'}
          </Button>
        </div>
      </Modal>

      <Modal open={asking === 'trial'} title="Start a free trial" onClose={() => setAsking(null)}>
        <p className="mb-account__note">
          Tell us where to reach you and everything switches on. You can choose a
          plan later without setting anything up again.
        </p>
        <Input label="Mobile or email" value={contact} onChange={(e) => setContact(e.target.value)} autoFocus />
        <div className="mb-row mb-row--end">
          <Button variant="quiet" onClick={() => setAsking(null)}>
            Cancel
          </Button>
          <Button
            variant="primary"
            disabled={busy || contact.trim() === ''}
            onClick={() => run(() => call('start_trial', { contact }), 'Your trial has started.')}
          >
            Start
          </Button>
        </div>
      </Modal>

      <Modal
        open={asking === 'emergency'}
        title="Emergency unlock code"
        onClose={() => setAsking(null)}
      >
        <p className="mb-account__note">
          Call us and we will read out a code. It switches everything back on for
          three days, so you can keep working while we sort the licence out.
          Billing and printing are never affected.
        </p>
        <Input
          label="Code"
          value={code}
          onChange={(e) => setCode(e.target.value)}
          placeholder="K7M2Q-9XR4T-BW8HN-3PZ6D"
          autoComplete="off"
          autoFocus
        />
        <div className="mb-row mb-row--end">
          <Button variant="quiet" onClick={() => setAsking(null)}>
            Cancel
          </Button>
          <Button
            variant="primary"
            disabled={busy || code.trim() === ''}
            onClick={() =>
              run(() => call('use_emergency_code', { code }), 'Everything is switched back on.')
            }
          >
            Unlock
          </Button>
        </div>
      </Modal>

      <Modal
        open={confirming}
        title="Stop using this licence here?"
        onClose={() => setConfirming(false)}
      >
        <p className="mb-account__note">
          We will release it so you can use it on another computer. This counter
          will keep billing and printing either way.
        </p>
        <div className="mb-row mb-row--end">
          <Button variant="quiet" onClick={() => setConfirming(false)}>
            Keep it
          </Button>
          <Button
            variant="danger"
            disabled={busy}
            onClick={() => {
              setConfirming(false);
              run(() => call('deactivate'));
            }}
          >
            Deactivate
          </Button>
        </div>
      </Modal>
    </Page>
  );
}
