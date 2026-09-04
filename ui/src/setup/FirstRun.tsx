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
  MoneyInput,
  Notice,
  PhoneInput,
  Scroller,
  Select,
  NumberInput,
} from '../kit';
import { call, isUiError } from '../ipc/call';
import { PIN_DIGITS } from '../auth/keyboard';
import type { FirstRunView } from '../ipc/generated/FirstRunView';
import type { OwnerShopView } from '../ipc/generated/OwnerShopView';
import type { OwnerSignInView } from '../ipc/generated/OwnerSignInView';
import type { TaxSlabView } from '../ipc/generated/TaxSlabView';

import './firstrun.css';

/** The steps, in the order somebody actually does them. */
const STEPS = [
  { id: 'folder', label: 'Shop folder', must: true },
  { id: 'signin', label: 'Sign in', must: true },
  { id: 'details', label: 'Shop name', must: true },
  { id: 'pin', label: 'Your PIN', must: true },
  { id: 'menu', label: 'Your items', must: false },
  { id: 'tables', label: 'Your tables', must: false },
  { id: 'printer', label: 'Your printer', must: false },
] as const;

/** `code` is a screen without a dot, and that is deliberate. */
type StepId = (typeof STEPS)[number]['id'] | 'code';

/**
 * The step after the PIN: a shop that already has items is not asked for items, and one that
 * already has tables is not asked for tables — a seeded or restored shop goes straight to the
 * printer.
 */
function stepAfterPin(view: FirstRunView | null): StepId {
  if (view && view.hasItems && view.hasTables) return 'printer';
  if (view && view.hasItems) return 'tables';
  return 'menu';
}

/** Where a shop that is already open picks up. */
function stepFor(view: FirstRunView): StepId {
  if (!view.hasShop) return 'folder';
  if (view.hasDetails && view.hasPin) return stepAfterPin(view);
  if (view.hasDetails) return 'pin';
  return 'details';
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

  // The first few items, so the till is not empty when it opens.
  const [itemName, setItemName] = useState('');
  const [itemPrice, setItemPrice] = useState('');
  const [itemClass, setItemClass] = useState('');
  const [added, setAdded] = useState<string[]>([]);
  // The shop's own classes, not a fourth copy of the slab list.
  const [classes, setClasses] = useState<readonly TaxSlabView[]>([]);

  // The room: how many tables, numbered from one.
  const [tableCount, setTableCount] = useState('');
  // The printer: one of the ones Windows knows about.
  const [windowsPrinters, setWindowsPrinters] = useState<readonly string[]>([]);
  const [printerName, setPrinterName] = useState('');

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

  // The tax classes are read when the items step opens.
  useEffect(() => {
    if (step !== 'menu') return;
    call('tax_slabs')
      .then((list) => {
        const live = list.filter((c) => c.isActive);
        setClasses(live);
        setItemClass((was) => (was === '' && live[0] ? live[0].id : was));
      })
      .catch(() => undefined);
  }, [step]);

  // The printers Windows knows about are read when the printer step opens.
  useEffect(() => {
    if (step !== 'printer') return;
    Promise.resolve()
      .then(() => call('printer_setup'))
      .then((setup) => setWindowsPrinters(setup?.windows ?? []))
      .catch(() => setWindowsPrinters([]));
  }, [step]);

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

  /** Open one of the account's shops in the chosen folder. */
  const openShop = (restaurantId: string) => {
    setOpening(restaurantId);
    setBusy(true);
    clear();
    call('open_as_owner', { restaurantId, folder, moveHere })
      .then((opened) => {
        take(opened.firstRun);
        if (opened.cameDown) setCameDown(opened.cameDown);
        // The details step starts with what the account already knows about the shop.
        setName((was) => (was === '' ? opened.shop.name : was));
        setAddress((was) => (was === '' ? opened.shop.address : was));
        setGstin((was) => (was === '' ? opened.shop.gstin : was));
        // A shop that came back whole — a reinstall — needs nothing more from here.
        if (!opened.firstRun.needed) {
          onDone();
          return;
        }
        setStep(stepFor(opened.firstRun));
      })
      .catch(complain)
      .finally(() => setBusy(false));
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
        // Sign them in with the PIN they just chose.
        return call('login', { staffId: id, pin })
          .catch(() => undefined)
          .then(() => setStep(code ? 'code' : stepAfterPin(view)));
      })
      .catch(complain)
      .finally(() => setBusy(false));
  };

  /** Tables 1 to N, four seats each, in no room — or none, and on to the printer. */
  const addTables = () => {
    const count = Number(tableCount);
    if (!(count > 0)) {
      go('printer');
      return;
    }
    setBusy(true);
    clear();
    call('add_dining_tables', {
      sectionId: null,
      prefix: '',
      from: 1,
      to: Math.min(Math.floor(count), 200),
      seats: 4,
    })
      .then(() => setStep('printer'))
      .catch(complain)
      .finally(() => setBusy(false));
  };

  /** The chosen printer becomes the default for bills and tickets alike — or none does. */
  const usePrinter = () => {
    if (printerName === '') {
      onDone();
      return;
    }
    setBusy(true);
    clear();
    call('choose_bill_printer', { windowsName: printerName })
      .then(() => onDone())
      .catch(complain)
      .finally(() => setBusy(false));
  };

  const addItem = () => {
    if (itemName.trim() === '' || itemPrice.trim() === '') return;
    setBusy(true);
    clear();
    call('save_menu_item', {
      edit: {
        id: freshId('itm'),
        name: itemName.trim(),
        categoryId: null,
        price: itemPrice.trim(),
        // A tax CLASS, not a rate: one place decides what 5% means, so changing the rate later
        // changes every item on it.
        taxClassId: itemClass === '' ? null : itemClass,
        priceBasis: null,
        hsn: null,
        shortCode: null,
        cost: null,
        course: null,
        prepMinutes: null,
        isOpenPrice: false,
        isAvailable: true,
      },
    })
      .then(() => {
        setAdded((was) => [...was, `${itemName.trim()} — ${itemPrice.trim()}`]);
        setItemName('');
        setItemPrice('');
        // Back to the Item box.
        document
          .querySelector<HTMLInputElement>('input[name="firstrun-item"]')
          ?.focus();
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
              title={s.must ? undefined : 'You can skip this and do it later'}
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
                  opens is the one that account owns; the licence comes with it, so there is
                  no key to type. The same login works in the phone app. Staff never sign in
                  here — they get a PIN from you.
                </>
              }
            />

            <div className="mb-firstrun__where">
              <span className="mb-firstrun__label">Shop folder</span>
              <code className="mb-firstrun__path">{folder}</code>
            </div>

            {signedIn === null ? (
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

            <div className="mb-firstrun__actions">
              {back('folder')}
              {signedIn === null ? (
                <Button
                  variant="primary"
                  disabled={busy || email.trim() === '' || password === ''}
                  onClick={signIn}
                >
                  Sign in
                </Button>
              ) : problemCode === 'licence.bound_elsewhere' ? (
                <Button
                  variant="primary"
                  disabled={busy || !moveHere || opening === ''}
                  onClick={() => openShop(opening)}
                >
                  Open it here
                </Button>
              ) : null}
            </div>
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
              <Button
                variant="primary"
                disabled={!wroteItDown}
                onClick={() => go(stepAfterPin(view))}
              >
                Next
              </Button>
            </div>
          </section>
        ) : null}

        {step === 'menu' ? (
          <section className="mb-firstrun__body">
            <Heading
              title="What you sell"
              tip={
                <>
                  Add two or three now so you can print a real bill and see it. The rest can
                  wait — the Menu screen imports a whole list.
                </>
              }
            />

            <div className="mb-firstrun__row">
              <Input
                label="Item"
                autoFocus
                name="firstrun-item"
                value={itemName}
                placeholder="Masala Dosa"
                onChange={(e) => setItemName(e.target.value)}
                onKeyDown={(e) => {
                  // Enter goes to the price rather than doing nothing.
                  if (e.key !== 'Enter') return;
                  e.preventDefault();
                  document
                    .querySelector<HTMLInputElement>('input[name="firstrun-price"]')
                    ?.focus();
                }}
              />
              <MoneyInput
                label="Price"
                name="firstrun-price"
                value={itemPrice}
                placeholder="80"
                onChange={setItemPrice}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') addItem();
                }}
              />
              <Select
                label="Tax slab"
                value={itemClass}
                onChange={(e) => setItemClass(e.target.value)}
                options={
                  classes.length === 0
                    ? [{ value: '', label: 'No tax slab' }]
                    : classes.map((c) => ({ value: c.id, label: c.name }))
                }
              />
              <Button disabled={busy} onClick={addItem}>
                Add
              </Button>
            </div>

            {added.length > 0 ? (
              <ul className="mb-firstrun__added">
                {added.map((line) => (
                  <li key={line}>
                    <Icon name="check" size="sm" /> {line}
                  </li>
                ))}
              </ul>
            ) : null}

            {/* One button forward, because there is only one outcome. */}
            <div className="mb-firstrun__actions">
              {back('details')}
              <Button variant="primary" onClick={() => go(view.hasTables ? 'printer' : 'tables')}>
                {added.length > 0 ? 'Next' : 'Skip this — next'}
              </Button>
            </div>
          </section>
        ) : null}

        {step === 'tables' ? (
          <section className="mb-firstrun__body">
            <Heading
              title="Your tables"
              tip={
                <>
                  How many tables does the room have? They are numbered from 1; the Floor
                  screen renames and arranges them later.
                </>
              }
            />
            <div className="mb-firstrun__fields">
              <NumberInput
                label="Tables"
                autoFocus
                value={tableCount}
                placeholder="12"
                onChange={(e) => setTableCount(e.target.value.replace(/[^0-9]/g, ''))}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') addTables();
                }}
              />
            </div>
            <div className="mb-firstrun__actions">
              {back('menu')}
              <Button variant="primary" disabled={busy} onClick={addTables}>
                {Number(tableCount) > 0 ? 'Add them — next' : 'Skip this — next'}
              </Button>
            </div>
          </section>
        ) : null}

        {step === 'printer' ? (
          <section className="mb-firstrun__body">
            <Heading
              title="Your printer"
              tip={
                <>
                  Bills and kitchen tickets go to this one. Settings › Printers adds more, and a
                  second printer for the kitchen.
                </>
              }
            />
            <div className="mb-firstrun__fields">
              <Select
                label="Printer"
                value={printerName}
                onChange={(e) => setPrinterName(e.target.value)}
                options={[
                  { value: '', label: 'No printer yet' },
                  ...windowsPrinters.map((name) => ({ value: name, label: name })),
                ]}
              />
            </div>
            <div className="mb-firstrun__actions">
              {back(view.hasTables ? 'menu' : 'tables')}
              <Button variant="primary" disabled={busy} onClick={usePrinter}>
                {printerName === '' ? 'Skip this — start billing' : 'Use it — start billing'}
              </Button>
            </div>
          </section>
        ) : null}
      </div>
    </Scroller>
  );
}
