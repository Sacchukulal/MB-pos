/** The account screen. */

import { useCallback, useEffect, useState } from 'react';

import { Badge, Button, Card, Input, Modal, SectionHeader, useToast, type BadgeTone,
  Page,
  PageHeader,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { LicenceView } from '../ipc/generated/LicenceView';

import './account.css';

/** The tone the counter chose, which is already a `BadgeTone` by name. */
const TONES: Record<string, BadgeTone> = {
  ok: 'ok',
  warn: 'warn',
  danger: 'danger',
};

/** magicbill.in, where a plan is actually bought. */
const RENEW_AT = 'https://magicbill.in/renew';

type Asking = 'activate' | 'transfer' | 'emergency' | null;

export function Account() {
  const [view, setView] = useState<LicenceView | null>(null);
  const [asking, setAsking] = useState<Asking>(null);
  const [key, setKey] = useState('');
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
        setCode('');
        if (done) toast.show("ok", done);
      })
      .catch(report)
      .finally(() => setBusy(false));
  };

  if (!view) return null;

  const tone = TONES[view.tone] ?? 'neutral';
  const cloudTone = TONES[view.cloudTone] ?? 'neutral';

  return (
    <Page className="mb-account">
      <PageHeader
        title="Account"
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

        {view.stillHeld !== '' && (
          <p className="mb-account__warn" role="status">
            {view.stillHeld}
          </p>
        )}
        {/* A warning, never a lock. */}
        {view.clockNote !== '' && (
          <p className="mb-account__warn" role="status">
            {view.clockNote}
          </p>
        )}

        {/* The trial is the website's: one sentence, no dialog, no contact box. */}
        {!view.isActivated && <p className="mb-account__note">{view.trialSentence}</p>}

        <div className="mb-account__actions mb-row mb-row--end">
          {/* "Check again" now actually checks again. */}
          <Button
            variant="quiet"
            disabled={busy}
            onClick={() =>
              run(() => call('refresh_licence'), 'Checked. This is the latest we have.')
            }
          >
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

      {/* What the plan includes. */}
      <Card>
        {/*
          A counter with no licence has no plan, and a heading that says "your plan" over a card
          that lists two phones and a till is a small lie an owner notices.
        */}
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
          {view.restaurantCode !== '' && (
            <div>
              {/* What staff type on a phone, beside the shop it names. */}
              <dt>Shop code for phones</dt>
              <dd className="mb-account__machine">{view.restaurantCode}</dd>
            </div>
          )}
        </dl>
        {view.included.length > 0 && (
          <p className="mb-account__included">{view.included.join(' · ')}</p>
        )}
      </Card>

      {/* The cloud copy, in Rust's words. */}
      <Card>
        <div className="mb-account__top mb-row">
          <SectionHeader title="Cloud copy" />
          <Badge tone={cloudTone}>
            {view.cloudTone === 'ok' ? 'Up to date' : view.cloudTone === 'danger' ? 'Stopped' : 'Behind'}
          </Badge>
        </div>
        <p className="mb-account__note" role="status">
          {view.cloudCopy}
        </p>
      </Card>

      {/*
        This computer. The id is here so support can ask for it, and the derivation is here
        because "we could not read this machine's id and made one up" is a thing an owner is
        entitled to know.
      */}
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

      {/* The dialogs. */}

      <Modal
        open={asking === 'activate' || asking === 'transfer'}
        title={asking === 'transfer' ? 'Move a licence to this computer' : 'Enter your licence key'}
        onClose={() => setAsking(null)}
      >
        <p className="mb-account__note">
          {asking === 'transfer'
            ? 'This will stop the licence working on the computer it is on now. The key is the one on your receipt, or at magicbill.in.'
            : 'The key is the one on your receipt, or at magicbill.in. Nobody else has it, so nobody else can use it.'}
        </p>
        {/*
          `autoComplete="off"`, found by driving it: WebView2 offered to remember the box and
          then drew its "Saved info" dropdown straight over the Activate button.
        */}
        <Input
          label="Licence key"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          autoComplete="off"
          autoFocus
        />
        <div className="mb-row mb-row--end">
          <Button variant="quiet" onClick={() => setAsking(null)}>
            Cancel
          </Button>
          <Button
            variant="primary"
            disabled={busy || key.trim() === ''}
            onClick={() =>
              run(
                () =>
                  asking === 'transfer'
                    ? call('transfer_here', { key })
                    : call('activate', { key }),
                asking === 'transfer' ? 'The licence is now on this computer.' : 'Activated.',
              )
            }
          >
            {asking === 'transfer' ? 'Move it here' : 'Activate'}
          </Button>
        </div>
      </Modal>

      <Modal
        open={asking === 'emergency'}
        title="Emergency unlock code"
        note="Call us and we will read out a code. It switches everything back on for three days, so you can keep working while we sort the licence out. Billing and printing are never affected."
        onClose={() => setAsking(null)}
      >
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
