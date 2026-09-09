/** The first five minutes. */

import { useCallback, useEffect, useState, type ReactNode } from 'react';

import {
  Button,
  Checkbox,
  freshId,
  Icon,
  InfoTip,
  Input,
  Logo,
  Notice,
  PhoneInput,
  Scroller,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import { PIN_DIGITS } from '../auth/keyboard';
import type { FirstRunView } from '../ipc/generated/FirstRunView';
import type { OwnerOpenedView } from '../ipc/generated/OwnerOpenedView';
import type { OwnerShopView } from '../ipc/generated/OwnerShopView';
import type { OwnerSignInView } from '../ipc/generated/OwnerSignInView';

import './firstrun.css';

/**
 * The steps, in the order somebody actually does them. Four, and every one is needed: the
 * folder, the account, the name on the bill, the PIN. The menu, the tables and the printer are
 * the counter's own screens, and the counter opens onto them.
 */
const STEPS = [
  { id: 'folder', label: 'Shop folder' },
  { id: 'signin', label: 'Sign in' },
  { id: 'details', label: 'Shop name' },
  { id: 'pin', label: 'Your PIN' },
] as const;

/** `code` is a screen without a dot, and that is deliberate. */
type StepId = (typeof STEPS)[number]['id'] | 'code';

/** Where a shop that is already open picks up. */
function stepFor(view: FirstRunView): StepId {
  if (!view.hasShop) return 'folder';
  if (!view.hasDetails) return 'details';
  return 'pin';
}

/** A step's heading, with its explanation behind the tip rather than under the title. */
function Heading({ title, tip }: { title: string; tip: ReactNode }) {
  return (
    <div className="mb-firstrun__heading">
      <h1 className="mb-firstrun__title">{title}</h1>
      <InfoTip label={`About ${title}`}>{tip}</InfoTip>
    </div>
  );
}

export function FirstRun({ onDone }: { onDone: () => void }) {
  const [view, setView] = useState<FirstRunView | null>(null);
  const [step, setStep] = useState<StepId>('folder');
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState('');
  /** Rust's code for the problem, for the one refusal a checkbox answers. */
  const [problemCode, setProblemCode] = useState('');

  /** Where the shop's data will go. Nobody chooses it but the owner. */
  const [folder, setFolder] = useState('');

  // The owner's account.
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [signedIn, setSignedIn] = useState<OwnerSignInView | null>(null);
  /** The shop being opened, kept for a second press after "move the licence here". */
  const [opening, setOpening] = useState('');
  const [moveHere, setMoveHere] = useState(false);
  /** What came down, in Rust's words. */
  const [cameDown, setCameDown] = useState('');
  /** The other way in: the licence key from the magicbill.in dashboard. */
  const [keyText, setKeyText] = useState('');
  /** Where Sign up sent them, once the browser has been asked to open it. */
  const [signupSaid, setSignupSaid] = useState('');

  // The shop's details.
  const [name, setName] = useState('');
  const [address, setAddress] = useState('');
  const [phone, setPhone] = useState('');
  const [gstin, setGstin] = useState('');

  // Whoever is in charge.
  const [person, setPerson] = useState('');
  const [pin, setPin] = useState('');
  const [pinAgain, setPinAgain] = useState('');
  const [recovery, setRecovery] = useState('');
  /** The recovery code is shown once and never again. */
  const [wroteItDown, setWroteItDown] = useState(false);
  /**
   * The owner's row: the one Rust made from the account, or the one a second press of Next
   * edits rather than hiring another one.
   */
  const [personId, setPersonId] = useState('');

  const complain = useCallback((cause: unknown) => {
    const said = isUiError(cause) ? cause.message : String(cause);
    setProblemCode(isUiError(cause) ? cause.code : '');
    // Rust writes its refusals as fragments — "a PIN is 6 to 8 digits" — because most of them
    // are read inside a longer sentence.
    setProblem(said.charAt(0).toUpperCase() + said.slice(1));
  }, []);

  const clear = () => {
    setProblem('');
    setProblemCode('');
  };

  /** A view from Rust: what it says about the owner's row is what the PIN step edits. */
  const take = useCallback((fresh: FirstRunView) => {
    setView(fresh);
    if (fresh.owner) {
      setPersonId(fresh.owner.id);
      setPerson((was) => (was === '' ? fresh.owner?.name ?? '' : was));
    }
  }, []);

  useEffect(() => {
    call('first_run')
      .then((fresh) => {
        take(fresh);
        // Somebody who already has a shop lands where they left off rather than being asked
        // again.
        setStep(stepFor(fresh));
      })
      .catch(complain);
  }, [complain, take]);

  if (!view) return <div className="mb-firstrun" />;

  // `code` has no dot of its own — it is the back half of the PIN step.
  const index = STEPS.findIndex((s) => s.id === (step === 'code' ? 'pin' : step));

  const go = (to: StepId) => {
    clear();
    setStep(to);
  };

  /** Browse for the folder. */
  const browseForFolder = () => {
    setBusy(true);
    clear();
    call('pick_a_folder', { start: folder === '' ? null : folder })
      .then((chosen) => {
        if (chosen) setFolder(chosen);
      })
      .catch(complain)
      .finally(() => setBusy(false));
  };

  /** A shop Rust opened, whichever way in: the details step starts with what the cloud knows. */
  const opened = (opening: Promise<OwnerOpenedView>) => {
    setBusy(true);
    clear();
    opening
      .then((shop) => {
        take(shop.firstRun);
        if (shop.cameDown) setCameDown(shop.cameDown);
        setName((was) => (was === '' ? shop.shop.name : was));
        setAddress((was) => (was === '' ? shop.shop.address : was));
        setPhone((was) => (was === '' ? shop.shop.phone : was));
        setGstin((was) => (was === '' ? shop.shop.gstin : was));
        // A shop that came back whole — a reinstall — needs nothing more from here.
        if (!shop.firstRun.needed) {
          onDone();
          return;
        }
        setStep(stepFor(shop.firstRun));
      })
      .catch(complain)
      .finally(() => setBusy(false));
  };

  /** Open one of the account's shops in the chosen folder. */
  const openShop = (restaurantId: string) => {
    setOpening(restaurantId);
    opened(call('open_as_owner', { restaurantId, folder, moveHere }));
  };

  /** Open the shop the pasted licence key names. */
  const openWithKey = () => {
    setOpening('');
    opened(call('open_with_key', { key: keyText.trim(), folder, moveHere }));
  };

  /** The way back in after "move the licence here": whichever door was tried. */
  const openAgain = () => {
    if (opening !== '') openShop(opening);
    else openWithKey();
  };

  /**
   * No account yet: the website makes one. The browser opens on magicbill.in's sign-up, the
   * owner comes back here with the email and password, or the key from the dashboard.
   */
  const openSignup = () => {
    clear();
    call('open_magicbill', { page: 'signup' })
      .then((url) =>
        setSignupSaid(
          `${url} is opening in your browser. Make your account there, then sign in here ` +
            'with it or paste the licence key from your dashboard.',
        ),
      )
      .catch(complain);
  };

  /** The owner's account: which shops it owns is the answer. */
  const signIn = () => {
    if (email.trim() === '' || password === '') {
      setProblem('Type the email and the password of your Magic Bill account.');
      return;
    }
    setBusy(true);
    clear();
    call('sign_in_owner', { email: email.trim(), password })
      .then((who) => {
        setSignedIn(who);
        setPassword('');
        // One shop is the usual answer, and it opens without another press.
        const only = who.shops.length === 1 ? who.shops[0] : undefined;
        if (only) openShop(only.id);
      })
      .catch(complain)
      .finally(() => setBusy(false));
  };

  const saveDetails = () => {
    if (name.trim() === '') {
      setProblem('Your shop needs a name — it goes on every bill you print.');
      return;
    }
    setBusy(true);
    clear();
    call('save_settings', {
      edits: [
        { key: 'store.name', value: name.trim() },
        { key: 'store.address', value: address.trim() },
        { key: 'store.phone', value: phone.trim() },
        { key: 'store.gstin', value: gstin.trim() },
        // A GST number starts with the state code, and a shop with no number is billed as
        // unregistered — the two settings the Tax page would otherwise ask for.
        {
          key: 'store.registration',
          value: gstin.trim() === '' ? 'unregistered' : 'regular',
        },
        { key: 'store.state_code', value: gstin.trim().slice(0, 2) },
      ],
    })
      .then(() => setStep('pin'))
      .catch(complain)
      .finally(() => setBusy(false));
  };

  const savePin = () => {
    if (person.trim() === '') {
      setProblem('Type your name, so the bills say who took the money.');
      return;
    }
    // The same rule Rust holds — `mb_auth::pin::PIN_DIGITS`.
    if (pin.length !== PIN_DIGITS) {
      setProblem(`A PIN is ${PIN_DIGITS} digits.`);
      return;
    }
    if (pin !== pinAgain) {
      setProblem('The two PINs are not the same. Type it again.');
      return;
    }
    setBusy(true);
    clear();
    const id = personId === '' ? freshId('staff') : personId;
    setPersonId(id);
    call('save_staff_member', {
      staff: {
        id,
        name: person.trim(),
        roleId: 'role_owner',
        status: 'active',
      },
    })
      .then(() => call('set_staff_pin', { staffId: id, pin }))
      .then((code) => {
        if (code) setRecovery(code);
        // Sign them in with the PIN they just chose. The recovery code is the last screen; a
        // shop that already had one goes straight to the counter.
        return call('login', { staffId: id, pin })
          .catch(() => undefined)
          .then(() => {
            if (code) setStep('code');
            else onDone();
          });
      })
      .catch(complain)
      .finally(() => setBusy(false));
  };

  /** The way back, on the left of every step that has one. */
  const back = (to: StepId) => (
    <Button variant="quiet" className="mb-firstrun__back" disabled={busy} onClick={() => go(to)}>
      <Icon name="chevron-left" size="sm" />
      Back
    </Button>
  );

  const shopsToPick = signedIn && signedIn.shops.length > 1 ? signedIn.shops : [];

  return (
    <Scroller inset className="mb-firstrun">
      <div className="mb-firstrun__panel">
        {/* Where you are, and how much is left. */}
        <ol className="mb-firstrun__steps" aria-label="Setting up">
          {STEPS.map((s, n) => (
            <li
              key={s.id}
              className={[
                'mb-firstrun__step',
                n < index ? 'mb-firstrun__step--done' : '',
                n === index ? 'mb-firstrun__step--now' : '',
              ]
                .filter(Boolean)
                .join(' ')}
            >
              <span className="mb-firstrun__dot">
                {n < index ? <Icon name="check" size="sm" /> : n + 1}
              </span>
              <span className="mb-firstrun__steplabel">{s.label}</span>
            </li>
          ))}
        </ol>

        {problem ? (
          <Notice tone="danger">{problem}</Notice>
        ) : null}

        {step === 'folder' ? (
          <section className="mb-firstrun__body">
            <Logo size="lg" />
            <Heading
              title="Welcome to Magic Bill"
              tip={
                <>
                  Two minutes and your counter is ready. Everything about your shop — bills,
                  menu, staff, settings and licence — lives in one folder on this computer,
                  and you choose which. Keep it on a drive you back up. A reinstall opens the
                  same folder and finds the shop as it was.
                </>
              }
            />

            <div className="mb-firstrun__where">
              <span className="mb-firstrun__label">Where your shop&rsquo;s data will be kept</span>
              <code className="mb-firstrun__path">
                {folder === '' ? 'No folder chosen yet' : folder}
              </code>
              <div className="mb-row">
                <Button variant="secondary" disabled={busy} onClick={browseForFolder}>
                  <Icon name="folder" size="sm" />
                  {folder === '' ? 'Choose the folder' : 'Choose a different folder'}
                </Button>
              </div>
            </div>

            <div className="mb-firstrun__actions">
              <Button
                variant="primary"
                disabled={busy || folder === ''}
                onClick={() => go('signin')}
              >
                Next
              </Button>
            </div>
          </section>
        ) : null}

        {step === 'signin' ? (
          <section className="mb-firstrun__body">
            <Heading
              title="Sign in"
              tip={
                <>
                  Use the email and password of your Magic Bill account — the one you started
                  your trial or bought your plan with at magicbill.in. The shop this counter
                  opens is the one that account owns, and the licence comes with it. The
                  licence key from your dashboard opens the same shop without the password.
                  No account yet? Sign up opens magicbill.in in your browser; come back here
                  once it is made. Staff never sign in here — they get a PIN from you.
                </>
              }
            />

            <div className="mb-firstrun__where">
              <span className="mb-firstrun__label">Shop folder</span>
              <code className="mb-firstrun__path">{folder}</code>
            </div>

            {signedIn === null ? (
              <>
                <div className="mb-firstrun__fields">
                  <Input
                    label="Email"
                    type="email"
                    autoComplete="username"
                    autoFocus
                    value={email}
                    placeholder="you@example.com"
                    onChange={(e) => setEmail(e.target.value)}
                  />
                  <Input
                    label="Password"
                    type="password"
                    autoComplete="current-password"
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') signIn();
                    }}
                  />
                </div>

                <div className="mb-firstrun__actions">
                  {back('folder')}
                  <Button variant="secondary" disabled={busy} onClick={openSignup}>
                    Sign up
                  </Button>
                  <Button
                    variant="primary"
                    disabled={busy || email.trim() === '' || password === ''}
                    onClick={signIn}
                  >
                    Sign in
                  </Button>
                </div>

                {signupSaid !== '' ? <Notice tone="info">{signupSaid}</Notice> : null}

                {/* The other door: the key from the dashboard, for whoever has it to hand. */}
                <p className="mb-firstrun__or" role="separator">
                  or paste the licence key from your magicbill.in dashboard
                </p>
                <div className="mb-firstrun__row">
                  <Input
                    label="Licence key"
                    value={keyText}
                    placeholder="MB-XXXX-XXXX-XXXX"
                    onChange={(e) => setKeyText(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' && keyText.trim() !== '') openWithKey();
                    }}
                  />
                  <Button
                    variant="secondary"
                    disabled={busy || keyText.trim() === ''}
                    onClick={openWithKey}
                  >
                    Continue
                  </Button>
                </div>
              </>
            ) : (
              <div className="mb-firstrun__where">
                <span className="mb-firstrun__label">Signed in as</span>
                <code className="mb-firstrun__path">
                  {signedIn.name} · {signedIn.email}
                </code>
              </div>
            )}

            {shopsToPick.length > 0 ? (
              <div className="mb-firstrun__shops">
                <span className="mb-firstrun__label">Which shop is this counter for?</span>
                {shopsToPick.map((shop: OwnerShopView) => (
                  <Button
                    key={shop.id}
                    variant={opening === shop.id ? 'primary' : 'secondary'}
                    disabled={busy}
                    onClick={() => openShop(shop.id)}
                  >
                    {shop.name}
                    {shop.address ? ` — ${shop.address}` : ''}
                  </Button>
                ))}
              </div>
            ) : null}

            {problemCode === 'licence.bound_elsewhere' ? (
              <Checkbox
                label="The old computer is gone — move the licence here"
                checked={moveHere}
                onChange={(e) => setMoveHere(e.target.checked)}
              />
            ) : null}

            {signedIn !== null || problemCode === 'licence.bound_elsewhere' ? (
              <div className="mb-firstrun__actions">
                {signedIn === null ? null : back('folder')}
                {problemCode === 'licence.bound_elsewhere' ? (
                  <Button variant="primary" disabled={busy || !moveHere} onClick={openAgain}>
                    Open it here
                  </Button>
                ) : null}
              </div>
            ) : null}
          </section>
        ) : null}

        {cameDown !== '' ? <Notice tone="info">{cameDown}</Notice> : null}

        {step === 'details' ? (
          <section className="mb-firstrun__body">
            <Heading
              title="Your shop"
              tip={
                <>
                  This goes at the top of every bill you print. A bill without it is not one a
                  customer can claim. What your account already knows is filled in — correct
                  anything that is wrong.
                </>
              }
            />

            <div className="mb-firstrun__fields">
              <Input
                label="Shop name"
                autoFocus
                value={name}
                placeholder="Anand Bhavan"
                onChange={(e) => setName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') saveDetails();
                }}
              />
              <Input
                label="Address"
                value={address}
                placeholder="14 Kamaraj Street, Chennai"
                onChange={(e) => setAddress(e.target.value)}
              />
              <PhoneInput
                label="Phone"
                value={phone}
                placeholder="9840011223"
                onChange={setPhone}
              />
              <Input
                label="GSTIN (leave blank if you do not have one)"
                value={gstin}
                onChange={(e) => setGstin(e.target.value)}
              />
            </div>

            <div className="mb-firstrun__actions">
              <Button variant="primary" disabled={busy} onClick={saveDetails}>
                Next
              </Button>
            </div>
          </section>
        ) : null}

        {step === 'pin' ? (
          <section className="mb-firstrun__body">
            <Heading
              title="Your PIN"
              tip={
                <>
                  Until somebody has a PIN, anybody who walks behind the counter can open your
                  reports and change your prices. This is you, the owner — four digits you
                  type at the counter every day. Your staff get their own PINs later, from the
                  Staff screen.
                </>
              }
            />

            <div className="mb-firstrun__fields">
              <Input
                label="Your name"
                autoFocus
                value={person}
                placeholder="Meena"
                onChange={(e) => setPerson(e.target.value)}
              />
              <Input
                label={`A PIN, ${PIN_DIGITS} digits`}
                maxLength={PIN_DIGITS}
                value={pin}
                type="password"
                inputMode="numeric"
                onChange={(e) => setPin(e.target.value.replace(/[^0-9]/g, ''))}
              />
              <Input
                label="The same PIN again"
                maxLength={PIN_DIGITS}
                value={pinAgain}
                type="password"
                inputMode="numeric"
                onChange={(e) => setPinAgain(e.target.value.replace(/[^0-9]/g, ''))}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') savePin();
                }}
              />
            </div>

            <div className="mb-firstrun__actions">
              {back('details')}
              <Button variant="primary" disabled={busy} onClick={savePin}>
                Next
              </Button>
            </div>
          </section>
        ) : null}

        {step === 'code' ? (
          <section className="mb-firstrun__body">
            <Heading
              title="Write this down"
              tip={
                <>
                  If the PIN is ever forgotten, this code is the only way back into your shop.
                  It is not shown again. It is printing on your printer now as well — but write
                  it down, because a first run often has no printer set up yet.
                </>
              }
            />

            <p className="mb-firstrun__code">{recovery}</p>

            <Notice tone="warn" icon="lock">
              Keep it somewhere that is not this computer — a diary, or the back
              of the licence certificate. Nobody at Magic Bill can look it up
              for you, because it was never sent anywhere.
            </Notice>

            <Checkbox
              label="I have written it down"
              checked={wroteItDown}
              onChange={(e) => setWroteItDown(e.target.checked)}
            />

            <div className="mb-firstrun__actions">
              <Button variant="primary" disabled={!wroteItDown} onClick={onDone}>
                Start billing
              </Button>
            </div>
          </section>
        ) : null}
      </div>
    </Scroller>
  );
}
