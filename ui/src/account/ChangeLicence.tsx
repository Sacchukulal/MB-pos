/**
 * Putting a licence on this computer: the two doors the first run has, then the owner's PIN.
 * Rust takes the licence, renames the owner's row after whoever holds it, sets the PIN and
 * signs them in.
 */

import { useState } from 'react';

import { Button, Checkbox, Input, Modal, Notice } from '../kit';
import { call, isUiError } from '../ipc/call';
import type { LicenceDoor } from '../ipc/generated/LicenceDoor';
import type { LicenceView } from '../ipc/generated/LicenceView';
import type { OwnerShopView } from '../ipc/generated/OwnerShopView';
import { PIN_DIGITS } from '../auth/keyboard';

type Step =
  /** Email and password, or the key. */
  | { kind: 'door' }
  /** The account owns more than one shop. */
  | { kind: 'shop'; shops: OwnerShopView[] }
  /** The PIN, twice. */
  | { kind: 'pin'; door: LicenceDoor; shopName: string };

export function ChangeLicence({
  open,
  hasLicence,
  onClose,
  onChanged,
}: {
  open: boolean;
  /** True when a licence is being replaced rather than put on for the first time. */
  hasLicence: boolean;
  onClose: () => void;
  onChanged: (view: LicenceView) => void;
}) {
  const [step, setStep] = useState<Step>({ kind: 'door' });
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [key, setKey] = useState('');
  const [pin, setPin] = useState('');
  const [again, setAgain] = useState('');
  const [moveHere, setMoveHere] = useState(false);
  const [problem, setProblem] = useState<{ code: string; says: string } | null>(null);
  const [busy, setBusy] = useState(false);

  const close = () => {
    setStep({ kind: 'door' });
    setEmail('');
    setPassword('');
    setKey('');
    setPin('');
    setAgain('');
    setMoveHere(false);
    setProblem(null);
    onClose();
  };

  const failed = (cause: unknown) => {
    setProblem(
      isUiError(cause)
        ? { code: cause.code, says: cause.message }
        : { code: '', says: 'That could not be done. Try again.' },
    );
  };

  /** The key box filled means the key door; otherwise the account. */
  const next = () => {
    setProblem(null);
    if (key.trim() !== '') {
      setStep({ kind: 'pin', door: { by: 'key', key: key.trim() }, shopName: '' });
      return;
    }
    setBusy(true);
    call('licence_shops', { email: email.trim(), password })
      .then((signedIn) => {
        const [only] = signedIn.shops;
        if (signedIn.shops.length === 1 && only) {
          setStep({
            kind: 'pin',
            door: { by: 'shop', restaurantId: only.id },
            shopName: only.name,
          });
        } else {
          setStep({ kind: 'shop', shops: signedIn.shops });
        }
      })
      .catch(failed)
      .finally(() => setBusy(false));
  };

  const change = () => {
    if (step.kind !== 'pin') return;
    setProblem(null);
    setBusy(true);
    call('change_licence', { door: step.door, moveHere, newPin: pin })
      .then((view) => {
        onChanged(view);
        close();
      })
      .catch(failed)
      .finally(() => setBusy(false));
  };

  const pinOk = pin.length === PIN_DIGITS && again === pin;
  const title = hasLicence ? 'Change licence' : 'Put a licence on this computer';

  return (
    <Modal
      open={open}
      title={title}
      onClose={close}
      actions={
        step.kind === 'door' ? (
          <>
            <Button variant="quiet" onClick={close}>
              Cancel
            </Button>
            <Button
              variant="primary"
              disabled={busy || (key.trim() === '' && (email.trim() === '' || password === ''))}
              onClick={next}
            >
              Next
            </Button>
          </>
        ) : step.kind === 'shop' ? (
          <Button variant="quiet" onClick={() => setStep({ kind: 'door' })}>
            Back
          </Button>
        ) : (
          <>
            <Button variant="quiet" disabled={busy} onClick={() => setStep({ kind: 'door' })}>
              Back
            </Button>
            <Button
              variant="primary"
              disabled={busy || !pinOk || (problem?.code === 'licence.bound_elsewhere' && !moveHere)}
              onClick={change}
            >
              Use this licence
            </Button>
          </>
        )
      }
    >
      {step.kind === 'door' && (
        <div className="mb-account__door">
          <Input
            label="Email"
            type="email"
            autoComplete="username"
            autoFocus
            value={email}
            onChange={(e) => setEmail(e.target.value)}
          />
          <Input
            label="Password"
            type="password"
            autoComplete="current-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') next();
            }}
          />
          <p className="mb-account__or" role="separator">
            or paste the licence key from your magicbill.in dashboard
          </p>
          <Input
            label="Licence key"
            value={key}
            placeholder="MB-XXXX-XXXX-XXXX"
            autoComplete="off"
            onChange={(e) => setKey(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') next();
            }}
          />
        </div>
      )}

      {step.kind === 'shop' && (
        <div className="mb-account__door">
          <p className="mb-muted">Which shop is this counter for?</p>
          {step.shops.map((shop) => (
            <Button
              key={shop.id}
              variant="secondary"
              disabled={busy}
              onClick={() =>
                setStep({
                  kind: 'pin',
                  door: { by: 'shop', restaurantId: shop.id },
                  shopName: shop.name,
                })
              }
            >
              {shop.name}
              {shop.address ? ` — ${shop.address}` : ''}
            </Button>
          ))}
        </div>
      )}

      {step.kind === 'pin' && (
        <div className="mb-account__door">
          {hasLicence && (
            <Notice tone="warn">
              {step.shopName !== '' ? `${step.shopName} takes over this counter. ` : ''}
              The old licence is released. Bills, menu, tables and staff stay on this computer.
              Phones join again with the new shop code.
            </Notice>
          )}
          <Input
            label="Your PIN"
            hint={`${PIN_DIGITS} digits. This is how the owner signs in from now on.`}
            type="password"
            inputMode="numeric"
            maxLength={PIN_DIGITS}
            autoComplete="new-password"
            autoFocus
            value={pin}
            onChange={(e) => setPin(e.target.value.replace(/\D/g, ''))}
          />
          <Input
            label="The same PIN again"
            type="password"
            inputMode="numeric"
            maxLength={PIN_DIGITS}
            autoComplete="new-password"
            value={again}
            error={again !== '' && again !== pin ? 'The two PINs are not the same.' : undefined}
            onChange={(e) => setAgain(e.target.value.replace(/\D/g, ''))}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && pinOk) change();
            }}
          />
          {problem?.code === 'licence.bound_elsewhere' && (
            <Checkbox
              label="The old computer is gone. Move the licence here."
              checked={moveHere}
              onChange={(e) => setMoveHere(e.target.checked)}
            />
          )}
        </div>
      )}

      {problem && <Notice tone="danger">{problem.says}</Notice>}
    </Modal>
  );
}
