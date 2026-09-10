/** The owner's page: the licence across the top, then the backups beside the version. */

import { useCallback, useEffect, useState } from 'react';

import {
  Badge,
  Button,
  ConfirmDialog,
  Fact,
  Icon,
  Input,
  Modal,
  Notice,
  Page,
  PageHeader,
  Panel,
  Row,
  Spinner,
  useToast,
  type BadgeTone,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import type { LicenceView } from '../ipc/generated/LicenceView';
import { Backup } from './Backup';
import { ChangeLicence } from './ChangeLicence';
import { Line, Lines } from './Lines';
import { Version } from './Version';

import './account.css';

/** Rust's tone words are the badge's own. */
const TONES: Record<string, BadgeTone> = {
  ok: 'ok',
  warn: 'warn',
  danger: 'danger',
};

/** The cloud copy's state in one word, beside Rust's sentence. */
const CLOUD_WORDS: Record<string, string> = {
  ok: 'Up to date',
  warn: 'Waiting',
  danger: 'Behind',
};

/** How long the copy button shows its tick. */
const COPIED_FOR_MS = 1500;

type Dialog = 'change' | 'sign-out' | 'code' | null;

export function Account() {
  const [view, setView] = useState<LicenceView | null>(null);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [code, setCode] = useState('');
  const [busy, setBusy] = useState(false);
  const [checking, setChecking] = useState(false);
  const [copied, setCopied] = useState(false);
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

  useEffect(() => {
    if (!copied) return undefined;
    const timer = window.setTimeout(() => setCopied(false), COPIED_FOR_MS);
    return () => window.clearTimeout(timer);
  }, [copied]);

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

  /** Ask magicbill.in now. The button carries its own spinner; the page stays alive. */
  const checkNow = () => {
    setChecking(true);
    call('refresh_licence')
      .then((fresh) => {
        setView(fresh);
        toast.show('ok', 'Checked with magicbill.in.');
      })
      .catch(report)
      .finally(() => setChecking(false));
  };

  const copyKey = () => {
    if (!view) return;
    navigator.clipboard
      .writeText(view.key)
      .then(() => setCopied(true))
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
              <Button variant="secondary" disabled={busy || checking} onClick={checkNow}>
                {checking ? <Spinner label="Checking" /> : <Icon name="refresh" size="sm" />}
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
            <dl className="mb-account__grid">
              <Fact label="Owner">{view.ownerName || '—'}</Fact>
              <Fact label="Mobile">{view.ownerPhone || '—'}</Fact>
              <Fact label="Plan">{view.planName}</Fact>
              <Fact label={view.dateLabel || 'Valid'}>{view.date || '—'}</Fact>
              {view.key !== '' ? (
                <Fact label="Licence key" code>
                  <span className="mb-account__key">
                    {view.key}
                    <Button
                      size="sm"
                      variant="quiet"
                      iconOnly
                      title={copied ? 'Copied' : 'Copy the key'}
                      aria-label="Copy the key"
                      onClick={copyKey}
                    >
                      <Icon name={copied ? 'check' : 'copy'} size="sm" />
                    </Button>
                  </span>
                </Fact>
              ) : null}
              <Fact label="Shop code for phones" code>
                {view.restaurantCode || '—'}
              </Fact>
              <Fact label="Phones">{view.phonesAllowed}</Fact>
              <Fact label="Tills">{view.tillsAllowed}</Fact>
            </dl>

            <Lines>
              {view.included.length > 0 ? (
                <Line label="Includes">
                  {view.included.map((feature) => (
                    <Badge key={feature}>{feature}</Badge>
                  ))}
                </Line>
              ) : null}
              <Line label="Cloud copy">
                <Badge tone={cloudTone}>{CLOUD_WORDS[view.cloudTone] ?? 'Cloud copy'}</Badge>
                <span>{view.cloudCopy}</span>
              </Line>
              <Line label="Last checked">{view.checked}</Line>
            </Lines>

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

      <div className="mb-account__two">
        <Backup />
        <Version />
      </div>

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
