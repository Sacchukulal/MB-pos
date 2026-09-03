/** The account screen: the licence, the phones, the cloud copy and this computer. */

import { useCallback, useEffect, useState, type ReactNode } from 'react';

import {
  Badge,
  Button,
  ConfirmDialog,
  Icon,
  Input,
  Modal,
  Notice,
  Page,
  PageHeader,
  Panel,
  Row,
  useToast,
  type BadgeTone,
  type IconName,
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

/** The mark on the verdict tile, by tone. */
const FACES: Record<string, IconName> = {
  ok: 'check',
  warn: 'warning',
  danger: 'warning',
};

/** What the cloud chip says, by tone. */
const CLOUD_CHIPS: Record<string, string> = {
  ok: 'Up to date',
  warn: 'Behind',
  danger: 'Stopped',
};

/** magicbill.in, where a plan is actually bought. */
const RENEW_AT = 'https://magicbill.in/renew';

type Asking = 'activate' | 'transfer' | 'emergency' | null;

/** One label over one value. */
function Fact({
  label,
  children,
  code = false,
}: {
  label: string;
  children: ReactNode;
  /** A string read out to support: monospaced and spaced. */
  code?: boolean;
}) {
  return (
    <div className="mb-account__fact">
      <dt>{label}</dt>
      <dd className={code ? 'mb-account__code' : undefined}>{children}</dd>
    </div>
  );
}

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
        if (done) toast.show('ok', done);
      })
      .catch(report)
      .finally(() => setBusy(false));
  };

  if (!view) return null;

  const tone = TONES[view.tone] ?? 'neutral';
  const cloudTone = TONES[view.cloudTone] ?? 'neutral';
  const may = view.mayManage;

  // The sentence under the verdict: the banner when there is one, else the renewal date, else
  // where a trial starts.
  const sentence =
    view.headline || view.renewalSentence || (view.isActivated ? '' : view.trialSentence);

  const subtitle = [view.shopName || 'This counter', view.isActivated ? view.planName : '']
    .filter(Boolean)
    .join(' · ');

  return (
    <Page className="mb-account">
      <PageHeader
        title="Account"
        subtitle={subtitle}
        actions={
          <>
            <Badge tone={tone}>{view.chip}</Badge>
            <Button
              variant="secondary"
              disabled={busy}
              onClick={() =>
                run(() => call('refresh_licence'), 'Checked. This is the latest we have.')
              }
            >
              <Icon name="refresh" size="sm" />
              Check again
            </Button>
          </>
        }
      />

      <Panel title="Licence">
        <div className="mb-account__verdict">
          <div className={`mb-account__face mb-account__face--${tone}`}>
            <Icon name={FACES[view.tone] ?? 'badge'} size="lg" />
          </div>
          <div className="mb-account__said">
            <div className="mb-account__standing">{view.chip}</div>
            {sentence !== '' && <p className="mb-account__sentence">{sentence}</p>}
          </div>
        </div>

        {view.stillHeld !== '' && <Notice tone="warn">{view.stillHeld}</Notice>}
        {view.clockNote !== '' && <Notice tone="warn">{view.clockNote}</Notice>}

        <dl className="mb-account__facts">
          <Fact label="Plan">{view.isActivated ? view.planName : '—'}</Fact>
          <Fact label="Phones">{view.phonesAllowed}</Fact>
          <Fact label="Tills">{view.tillsAllowed}</Fact>
          <Fact label="Renews on">{view.renewsOn || '—'}</Fact>
          <Fact label="Last checked">{view.checked}</Fact>
        </dl>

        {view.included.length > 0 && (
          <div className="mb-account__included">
            <span className="mb-account__label">Includes</span>
            {view.included.map((feature) => (
              <Badge key={feature}>{feature}</Badge>
            ))}
          </div>
        )}

        <div className="mb-account__actions">
          <div className="mb-account__aside">
            {view.isActivated && (
              <Button variant="danger" disabled={!may} onClick={() => setConfirming(true)}>
                Deactivate
              </Button>
            )}
            {!may && (
              <span className="mb-account__only">Only the owner can change the licence.</span>
            )}
          </div>
          <Row end wrap={false}>
            <Button variant="quiet" disabled={!may} onClick={() => setAsking('emergency')}>
              Emergency code
            </Button>
            {view.isActivated ? (
              <Button variant="secondary" onClick={() => window.open(RENEW_AT, '_blank')}>
                Renew
              </Button>
            ) : (
              <Button variant="primary" disabled={!may} onClick={() => setAsking('activate')}>
                Enter licence key
              </Button>
            )}
          </Row>
        </div>
      </Panel>

      <div className="mb-account__grid">
        <Panel title="Phones">
          {view.restaurantCode !== '' ? (
            <>
              <div className="mb-account__shopcode">{view.restaurantCode}</div>
              <p className="mb-account__note">
                Staff type this shop code in the Magic Bill phone app to join this counter.
              </p>
            </>
          ) : (
            <p className="mb-account__note">
              {view.isActivated
                ? 'The shop code for phones arrives with the next licence check.'
                : 'Phones can join once the licence key is entered.'}
            </p>
          )}
        </Panel>

        <Panel
          title="Cloud copy"
          actions={<Badge tone={cloudTone}>{CLOUD_CHIPS[view.cloudTone] ?? 'Behind'}</Badge>}
        >
          <p className="mb-account__note" role="status">
            {view.cloudCopy}
          </p>
        </Panel>

        <Panel title="This computer">
          <dl className="mb-account__facts">
            <Fact label="Computer" code>
              {view.machine}
            </Fact>
            <Fact label="Identified">{view.machineHow}</Fact>
          </dl>
          {view.machineIsFragile && (
            <Notice tone="warn">
              We could not read a permanent id from this computer, so we made one. If Magic Bill
              is reinstalled you may need to move the licence again.
            </Notice>
          )}
          <Row end>
            <Button variant="secondary" disabled={!may} onClick={() => setAsking('transfer')}>
              Move a licence here
            </Button>
          </Row>
        </Panel>
      </div>

      <Modal
        open={asking === 'activate' || asking === 'transfer'}
        title={asking === 'transfer' ? 'Move a licence to this computer' : 'Enter your licence key'}
        onClose={() => setAsking(null)}
        actions={
          <>
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
          </>
        }
      >
        <p className="mb-account__note">
          {asking === 'transfer'
            ? 'This will stop the licence working on the computer it is on now. The key is the one on your receipt, or at magicbill.in.'
            : 'The key is the one on your receipt, or at magicbill.in. Nobody else has it, so nobody else can use it.'}
        </p>
        {/* WebView2 offers to remember the box otherwise, and draws its list over the button. */}
        <Input
          label="Licence key"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          autoComplete="off"
          autoFocus
        />
      </Modal>

      <Modal
        open={asking === 'emergency'}
        title="Emergency unlock code"
        onClose={() => setAsking(null)}
        actions={
          <>
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
          </>
        }
      >
        <p className="mb-account__note">
          Call us and we will read out a code. It switches everything back on for three days, so
          you can keep working while we sort the licence out. Billing and printing are never
          affected.
        </p>
        <Input
          label="Code"
          value={code}
          onChange={(e) => setCode(e.target.value)}
          placeholder="K7M2Q-9XR4T-BW8HN-3PZ6D"
          autoComplete="off"
          autoFocus
        />
      </Modal>

      <ConfirmDialog
        open={confirming}
        title="Stop using this licence here?"
        body="We will release it so you can use it on another computer. This counter will keep billing and printing either way."
        confirmLabel="Deactivate"
        cancelLabel="Keep it"
        destructive
        onCancel={() => setConfirming(false)}
        onConfirm={() => {
          setConfirming(false);
          run(() => call('deactivate'));
        }}
      />
    </Page>
  );
}
