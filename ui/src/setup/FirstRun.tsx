/** The first five minutes. */

import { useCallback, useEffect, useState } from 'react';

import { Button, Checkbox, freshId, Icon, Input, MoneyInput, Notice, PhoneInput, Scroller, Select, NumberInput } from '../kit';
import { call, isUiError } from '../ipc/call';
import { PIN_DIGITS } from '../auth/keyboard';
import type { FirstRunView } from '../ipc/generated/FirstRunView';
import type { TaxClassView } from '../ipc/generated/TaxClassView';

import './firstrun.css';

/** The steps, in the order somebody actually does them. */
const STEPS = [
  { id: 'shop', label: 'Shop file', must: true },
  { id: 'details', label: 'Shop name', must: true },
  { id: 'pin', label: 'Your PIN', must: true },
  { id: 'menu', label: 'Your items', must: false },
  { id: 'tables', label: 'Your tables', must: false },
  { id: 'printer', label: 'Your printer', must: false },
] as const;

/** `code` is a screen without a dot, and that is deliberate. */
type StepId = (typeof STEPS)[number]['id'] | 'code';

export function FirstRun({ onDone }: { onDone: () => void }) {
  const [view, setView] = useState<FirstRunView | null>(null);
  const [step, setStep] = useState<StepId>('shop');
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState('');

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
   * The row we made for whoever is in charge, kept so a SECOND press of Next edits that person
   * rather than hiring another one.
   */
  const [personId, setPersonId] = useState('');

  // The first few items, so the till is not empty when it opens.
  const [itemName, setItemName] = useState('');
  const [itemPrice, setItemPrice] = useState('');
  const [itemClass, setItemClass] = useState('');
  const [added, setAdded] = useState<string[]>([]);
  // The shop's own classes, not a fourth copy of the slab list.
  const [classes, setClasses] = useState<readonly TaxClassView[]>([]);

  /** Where the shop's data will go. */
  const [folder, setFolder] = useState('');

  // The room: how many tables, numbered from one.
  const [tableCount, setTableCount] = useState('');
  // The printer: one of the ones Windows knows about.
  const [windowsPrinters, setWindowsPrinters] = useState<readonly string[]>([]);
  const [printerName, setPrinterName] = useState('');

  const complain = useCallback((cause: unknown) => {
    const said = isUiError(cause) ? cause.message : String(cause);
    // Rust writes its refusals as fragments — "a PIN is 6 to 8 digits" — because most of them
    // are read inside a longer sentence.
    setProblem(said.charAt(0).toUpperCase() + said.slice(1));
  }, []);

  useEffect(() => {
    call('first_run')
      .then((fresh) => {
        setView(fresh);
        // Somebody who already has a shop and a name lands where they left off rather than
        // being asked again.
        if (fresh.hasShop && fresh.hasDetails && fresh.hasPin) setStep('menu');
        else if (fresh.hasShop && fresh.hasDetails) setStep('pin');
        else if (fresh.hasShop) setStep('details');
      })
      .catch(complain);
  }, [complain]);

  // The tax classes are read when the items step opens.
  useEffect(() => {
    if (step !== 'menu') return;
    call('menu_tax_classes')
      .then((list) => {
        setClasses(list);
        setItemClass((was) => (was === '' && list[0] ? list[0].id : was));
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

  const makeShop = (folder: string) => {
    setBusy(true);
    setProblem('');
    call('create_shop', { folder })
      .then((fresh) => {
        setView(fresh);
        setStep('details');
      })
      .catch(complain)
      .finally(() => setBusy(false));
  };

  /**
   * Adopt a database that is already on this computer — a reinstall, or a drive letter that
   * changed.
   */
  const openExisting = (path: string) => {
    setBusy(true);
    setProblem('');
    call('use_existing_shop', { path })
      .then((fresh) => {
        setView(fresh);
        setStep(fresh.hasDetails ? 'pin' : 'details');
      })
      .catch(complain)
      .finally(() => setBusy(false));
  };

  /** Browse for a folder. */
  const browseForFolder = () => {
    setBusy(true);
    setProblem('');
    call('pick_a_folder', { start: view?.defaultFolder ?? null })
      .then((folder) => {
        if (folder) setFolder(folder);
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
    setProblem('');
    call('save_settings', {
      edits: [
        { key: 'store.name', value: name.trim() },
        { key: 'store.address', value: address.trim() },
        { key: 'store.phone', value: phone.trim() },
        { key: 'store.gstin', value: gstin.trim() },
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
    setProblem('');
    const id = personId === '' ? freshId('staff') : personId;
    setPersonId(id);
    call('save_staff_member', {
      staff: {
        id,
        name: person.trim(),
        code: null,
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
          .then(() => setStep(code ? 'code' : 'menu'));
      })
      .catch(complain)
      .finally(() => setBusy(false));
  };

  /** Tables 1 to N, four seats each, in no room — or none, and on to the printer. */
  const addTables = () => {
    const count = Number(tableCount);
    if (!(count > 0)) {
      setStep('printer');
      return;
    }
    setBusy(true);
    setProblem('');
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
    setProblem('');
    call('save_printer', {
      edit: {
        id: '',
        name: printerName,
        kind: 'spooler',
        address: printerName,
        paperMm: 80,
        isDefault: true,
        role: 'both',
        engine: 'raster',
        isBoldDark: false,
        canKickDrawer: false,
      },
    })
      .then(() => onDone())
      .catch(complain)
      .finally(() => setBusy(false));
  };

  const addItem = () => {
    if (itemName.trim() === '' || itemPrice.trim() === '') return;
    setBusy(true);
    setProblem('');
    call('save_menu_item', {
      edit: {
        id: freshId('itm'),
        name: itemName.trim(),
        categoryId: null,
        price: itemPrice.trim(),
        // A tax CLASS, not a rate: one place decides what 5% means, so changing the rate later
        // changes every item on it.
        taxClassId: itemClass === '' ? null : itemClass,
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

        {step === 'shop' ? (
          <section className="mb-firstrun__body">
            <h1 className="mb-firstrun__title">Welcome to Magic Bill</h1>
            {/* mb-layout-allow: a wizard step is one instruction — behind a tip it is a step nobody reads */}
            <p className="mb-firstrun__lede">
              Two minutes and your counter is ready. Nothing here goes to the
              internet — your shop&rsquo;s data stays on this computer.
            </p>

            {/* The folder, and a way to change it. */}
            <div className="mb-firstrun__where">
              <span className="mb-firstrun__label">Your data will be kept in</span>
              <code className="mb-firstrun__path">
                {folder === '' ? view.defaultFolder : folder}
              </code>
              <div className="mb-row">
                <Button variant="secondary" disabled={busy} onClick={browseForFolder}>
                  <Icon name="folder" size="sm" />
                  Choose a different folder
                </Button>
                {folder === '' ? null : (
                  <Button variant="quiet" disabled={busy} onClick={() => setFolder('')}>
                    Use the usual place
                  </Button>
                )}
              </div>
            </div>

            <div className="mb-firstrun__actions">
              <Button
                variant="primary"
                disabled={busy}
                onClick={() => makeShop(folder)}
              >
                Start a new shop
              </Button>
            </div>

            {view.found.length > 0 ? (
              <div className="mb-firstrun__found">
                <span className="mb-firstrun__label">
                  Or open a shop already on this computer
                </span>
                {view.found.map((found) => (
                  <Button
                    key={found}
                    variant="secondary"
                    disabled={busy}
                    onClick={() => openExisting(found.split(' — ')[0] ?? found)}
                  >
                    {found}
                  </Button>
                ))}
              </div>
            ) : null}

            {/* A database somewhere this counter did not look. */}
            <div className="mb-firstrun__found">
              <Button
                variant="quiet"
                disabled={busy}
                onClick={() => {
                  setBusy(true);
                  setProblem('');
                  call('pick_a_folder', { start: null })
                    .then((chosen) => {
                      if (chosen) openExisting(chosen);
                    })
                    .catch(complain)
                    .finally(() => setBusy(false));
                }}
              >
                My shop&rsquo;s data is somewhere else — find it
              </Button>
            </div>
          </section>
        ) : null}

        {step === 'details' ? (
          <section className="mb-firstrun__body">
            <h1 className="mb-firstrun__title">Your shop</h1>
            {/* mb-layout-allow: a wizard step is one instruction — behind a tip it is a step nobody reads */}
            <p className="mb-firstrun__lede">
              This goes at the top of every bill you print. A bill without it is
              not one a customer can claim.
            </p>

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
            <h1 className="mb-firstrun__title">Who is in charge</h1>
            {/* mb-layout-allow: a wizard step is one instruction — behind a tip it is a step nobody reads */}
            <p className="mb-firstrun__lede">
              Until somebody has a PIN, anybody who walks behind the counter can
              open your reports and change your prices. This is you — you can
              add your staff later.
            </p>

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
              <Button variant="primary" disabled={busy} onClick={savePin}>
                Next
              </Button>
            </div>
          </section>
        ) : null}

        {step === 'code' ? (
          <section className="mb-firstrun__body">
            <h1 className="mb-firstrun__title">Write this down</h1>
            {/* mb-layout-allow: a wizard step is one instruction — behind a tip it is a step nobody reads */}
            <p className="mb-firstrun__lede">
              If the PIN is ever forgotten, this code is the only way back into
              your shop. It is not shown again. It is printing on your printer
              now as well — but write it down, because a first run often has no
              printer set up yet.
            </p>

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
                onClick={() => setStep('menu')}
              >
                Next
              </Button>
            </div>
          </section>
        ) : null}

        {step === 'menu' ? (
          <section className="mb-firstrun__body">
            <h1 className="mb-firstrun__title">What you sell</h1>
            {/* mb-layout-allow: a wizard step is one instruction — behind a tip it is a step nobody reads */}
            <p className="mb-firstrun__lede">
              Add two or three now so you can print a real bill and see it. The
              rest can wait — the Menu screen imports a whole list.
            </p>

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
                label="Tax"
                value={itemClass}
                onChange={(e) => setItemClass(e.target.value)}
                options={
                  classes.length === 0
                    ? [{ value: '', label: 'No tax class' }]
                    : classes.map((c) => ({ value: c.id, label: `${c.name} — ${c.rate}` }))
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

            {/* One button, because there is only one outcome. */}
            <div className="mb-firstrun__actions">
              <Button variant="primary" onClick={() => setStep('tables')}>
                {added.length > 0 ? 'Next' : 'Skip this — next'}
              </Button>
            </div>
          </section>
        ) : null}

        {step === 'tables' ? (
          <section className="mb-firstrun__body">
            <h1 className="mb-firstrun__title">Your tables</h1>
            {/* mb-layout-allow: a wizard step is one instruction — behind a tip it is a step nobody reads */}
            <p className="mb-firstrun__lede">
              How many tables does the room have? They are numbered from 1; the
              Floor screen renames and arranges them later.
            </p>
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
              <Button variant="primary" disabled={busy} onClick={addTables}>
                {Number(tableCount) > 0 ? 'Add them — next' : 'Skip this — next'}
              </Button>
            </div>
          </section>
        ) : null}

        {step === 'printer' ? (
          <section className="mb-firstrun__body">
            <h1 className="mb-firstrun__title">Your printer</h1>
            {/* mb-layout-allow: a wizard step is one instruction — behind a tip it is a step nobody reads */}
            <p className="mb-firstrun__lede">
              Bills and kitchen tickets go to this one. Settings › Printers adds
              more, and a second printer for the kitchen.
            </p>
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
