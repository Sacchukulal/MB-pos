/** The owner's page: the licence, the backups and the version, each on its own panel. */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  ConfirmDialog,
  Fact,
  Facts,
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
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { LicenceView } from '../ipc/generated/LicenceView';
import { Backup } from './Backup';
import { ChangeLicence } from './ChangeLicence';
import { Version } from './Version';

import './account.css';

/** Rust's tone words are the badge's own. */
const TONES: Record<string, BadgeTone> = {
  ok: 'ok',
  warn: 'warn',
  danger: 'danger',
};

type Dialog = 'change' | 'sign-out' | 'code' | null;

export function Account() {
  const [view, setView] = useState<LicenceView | null>(null);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [code, setCode] = useState('');
  const [busy, setBusy] = useState(false);
  const toast = useToast();

  const report = useCallback(
    (cause: unknown) => {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    },
    [toast],
  );

  useEffect(() => {
    call('account').then(setView).catch(report);
  }, [report]);

  /** Run a licence command, take the whole view back, and close whatever was open. */
  const run = (work: () => Promise<LicenceView>, done?: string) => {
    setBusy(true);
    work()
      .then((fresh) => {
        setView(fresh);
        setDialog(null);
        setCode('');
        if (done) toast.show('ok', done);
      })
      .catch(report)
      .finally(() => setBusy(false));
  };

  const copyKey = () => {
    if (!view) return;
    navigator.clipboard
      .writeText(view.key)
      .then(() => toast.show('ok', 'Copied.'))
      .catch(() => toast.show('danger', 'The key could not be copied.'));
  };

  if (!view) return null;

  const tone = TONES[view.tone] ?? 'neutral';
  const cloudTone = TONES[view.cloudTone] ?? 'neutral';
  const subtitle = [view.shopName, view.hasLicence ? view.planName : '']
    .filter(Boolean)
    .join(' · ');

  return (
    <Page className="mb-account">
      <PageHeader
        title="Account"
        subtitle={subtitle}
        actions={
          view.hasLicence ? (
            <>
              <Button
                variant="secondary"
                disabled={busy}
                onClick={() => run(() => call('refresh_licence'), 'Checked with magicbill.in.')}
              >
                <Icon name="refresh" size="sm" />
                Check now
              </Button>
              <Button
                variant="primary"
                onClick={() => call('open_magicbill', { page: 'renew' }).catch(report)}
              >
                Manage plan
              </Button>
            </>
          ) : (
            <Button
              variant="primary"
              onClick={() => call('open_magicbill', { page: 'renew' }).catch(report)}
            >
              Buy a plan
            </Button>
          )
        }
      />

      <Panel title="Licence" actions={<Badge tone={tone}>{view.chip}</Badge>}>
        {view.headline !== '' && (
          <Notice
            tone={tone === 'neutral' ? 'info' : tone}
            action={
              view.boundElsewhere ? (
                <Button
                  variant="secondary"
                  disabled={busy}
                  onClick={() =>
                    run(() => call('bring_licence_here'), 'The licence is on this computer again.')
                  }
                >
                  Use it on this computer
                </Button>
              ) : !view.hasLicence ? (
                <Button variant="primary" onClick={() => setDialog('change')}>
                  Sign in
                </Button>
              ) : view.standing !== 'fine' ? (
                <Button variant="quiet" onClick={() => setDialog('code')}>
                  Code from support
                </Button>
              ) : undefined
            }
          >
            {view.headline}
          </Notice>
        )}
        {view.stillHeld !== '' && <Notice tone="warn">{view.stillHeld}</Notice>}
        {view.clockNote !== '' && <Notice tone="warn">{view.clockNote}</Notice>}

        {view.hasLicence && (
          <>
            <Facts>
              <Fact label="Owner">{view.ownerName || '—'}</Fact>
              <Fact label="Mobile">{view.ownerPhone || '—'}</Fact>
              <Fact label="Shop">{view.shopName || '—'}</Fact>
              <Fact label="Plan">{view.planName}</Fact>
              {view.dateLabel !== '' && <Fact label={view.dateLabel}>{view.date || '—'}</Fact>}
              <Fact label="Last checked">{view.checked}</Fact>
              {view.key !== '' && (
                <Fact label="Licence key" code>
                  <span className="mb-account__key">
                    {view.key}
                    <Button size="sm" variant="quiet" onClick={copyKey} aria-label="Copy the key">
                      <Icon name="copy" size="sm" />
                      Copy
                    </Button>
                  </span>
                </Fact>
              )}
              <Fact label="Shop code for phones" code>
                {view.restaurantCode || '—'}
              </Fact>
              <Fact label="Phones">{view.phonesAllowed}</Fact>
              <Fact label="Tills">{view.tillsAllowed}</Fact>
            </Facts>

            {view.included.length > 0 && (
              <div className="mb-account__included">
                <span className="mb-account__label">Includes</span>
                {view.included.map((feature) => (
                  <Badge key={feature}>{feature}</Badge>
                ))}
              </div>
            )}

            <div className="mb-account__cloud" role="status">
              <Badge tone={cloudTone}>Cloud copy</Badge>
              <span>{view.cloudCopy}</span>
            </div>

            <Row end>
              <Button variant="quiet" disabled={busy} onClick={() => setDialog('sign-out')}>
                Sign out of this computer
              </Button>
              <Button variant="secondary" disabled={busy} onClick={() => setDialog('change')}>
                Change licence
              </Button>
            </Row>
          </>
        )}
      </Panel>

      <Backup />
      <Version />

      <ChangeLicence
        open={dialog === 'change'}
        hasLicence={view.hasLicence}
        onClose={() => setDialog(null)}
        onChanged={(fresh) => {
          setView(fresh);
          setDialog(null);
          toast.show(
            'ok',
            `This computer now runs on ${fresh.shopName || 'the new'} licence. You are signed in as ${fresh.ownerName || 'the owner'}.`,
          );
        }}
      />

      <ConfirmDialog
        open={dialog === 'sign-out'}
        title="Sign out of this computer?"
        body="The licence is released so another computer can use it. Bills, menu and staff stay here, and billing carries on."
        confirmLabel="Sign out"
        cancelLabel="Keep it"
        destructive
        onCancel={() => setDialog(null)}
        onConfirm={() => run(() => call('sign_out_licence'), 'Signed out. The licence is free.')}
      />

      <Modal
        open={dialog === 'code'}
        title="Code from support"
        onClose={() => setDialog(null)}
        actions={
          <>
            <Button variant="quiet" onClick={() => setDialog(null)}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={busy || code.trim() === ''}
              onClick={() =>
                run(() => call('use_emergency_code', { code }), 'Everything is switched on again.')
              }
            >
              Unlock
            </Button>
          </>
        }
      >
        <p className="mb-muted">
          Support reads out a code on the phone. It switches everything on for three days.
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
    </Page>
  );
}
