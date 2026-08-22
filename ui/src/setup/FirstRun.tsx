/**
 * **The first five minutes** — P30.5.
 *
 * # What this replaces
 *
 * A fresh install used to open straight onto the billing screen with no shop
 * behind it. Every screen's first call failed, each failure raised a toast, the
 * toasts stacked three deep, a six-item checklist ate the page, and there was
 * no way to create a shop at all. The owner installed the build on a second
 * computer and hit every bit of that inside ten seconds.
 *
 * # The rules this screen follows
 *
 * 1. **One thing on screen at a time.** Not six at once with a progress count.
 * 2. **The compulsory steps cannot be skipped, and there are only three** — a
 *    shop to put the data in, a name to print on the bill, and a PIN so that
 *    not everybody who walks behind the counter owns the till. Everything else
 *    a shop can decide after it has taken some money.
 * 3. **The skippable steps say so plainly.** "I will do this later" is a
 *    button, not a hidden escape.
 * 4. **Nothing nags.** No banner, no toast, no checklist. This screen IS the
 *    set-up, and when it is done it goes away for good.
 *
 * # Nothing here is arithmetic, and nothing here is a second copy
 *
 * Every step calls the same command the Settings screen calls. This file owns
 * the ORDER and the words, and not one rule (R8).
 */

import { useCallback, useEffect, useState } from 'react';

import { Button, Checkbox, freshId, Icon, Input, MoneyInput, Notice, PhoneInput, Select } from '../kit';
import { call, isUiError } from '../ipc/call';
import { PIN_DIGITS } from '../auth/keyboard';
import type { FirstRunView } from '../ipc/generated/FirstRunView';

import './firstrun.css';

/**
 * The steps, in the order somebody actually does them.
 *
 * `label` is what the row of dots says, and it is SHORT on purpose. The first
 * draft used the heading — "Where your shop lives", "Who is in charge" — and
 * four of those across one narrow panel came out as "Where yo…", "Who is in …",
 * which tells a person nothing at all. The heading is the sentence; the dot is
 * a place name.
 */
const STEPS = [
  { id: 'shop', label: 'Shop file', must: true },
  { id: 'details', label: 'Shop name', must: true },
  { id: 'pin', label: 'Your PIN', must: true },
  { id: 'menu', label: 'Your items', must: false },
  { id: 'done', label: 'Ready', must: false },
] as const;

/**
 * `code` is a screen without a dot, and that is deliberate.
 *
 * It shows the recovery code, and it belongs to the PIN step — it is the second
 * half of "you now have a way in". The first draft printed the code in a box
 * above the item form on the next step, which put the one line in the whole
 * flow that is never shown again at the top of a page somebody is busy typing
 * into, and made that page too tall for a 768-pixel screen. A page with one
 * thing on it is read.
 */
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
  /**
   * **The recovery code is shown once and never again.** So the way out of this
   * step is closed until somebody says they have it on paper. It is the only
   * tick-box in the whole flow, and it earns its place: without the code, a
   * forgotten PIN is a shop locked out of its own till.
   */
  const [wroteItDown, setWroteItDown] = useState(false);
  /**
   * The row we made for whoever is in charge, kept so a SECOND press of Next
   * edits that person rather than hiring another one.
   *
   * Found by looking: the first attempt typed a four-digit PIN, `save_staff_member`
   * succeeded, `set_staff_pin` refused, and the id was a fresh `Date.now()` on
   * every press — so a shop that mistyped its PIN once would have opened with
   * two owners in the staff list and no idea where the second came from.
   */
  const [personId, setPersonId] = useState('');

  // The first few items, so the till is not empty when it opens.
  const [itemName, setItemName] = useState('');
  const [itemPrice, setItemPrice] = useState('');
  const [itemClass, setItemClass] = useState('tax_food_5');
  const [added, setAdded] = useState<string[]>([]);

  /**
   * Where the shop's data will go. Empty means "the usual place", which is
   * what `create_shop` already understood and what nearly everybody uses — so
   * the common case is still one button and no decision.
   */
  const [folder, setFolder] = useState('');


  const complain = useCallback((cause: unknown) => {
    const said = isUiError(cause) ? cause.message : String(cause);
    // Rust writes its refusals as fragments — "a PIN is 6 to 8 digits" — because
    // most of them are read inside a longer sentence. Here one IS the sentence,
    // and a red box that opens in lower case reads like a crash rather than an
    // answer. The words stay Rust's; only the first letter is ours.
    setProblem(said.charAt(0).toUpperCase() + said.slice(1));
  }, []);

  useEffect(() => {
    call('first_run')
      .then((fresh) => {
        setView(fresh);
        // Somebody who already has a shop and a name lands where they left
        // off rather than being asked again.
        if (fresh.hasShop && fresh.hasDetails && fresh.hasPin) setStep('menu');
        else if (fresh.hasShop && fresh.hasDetails) setStep('pin');
        else if (fresh.hasShop) setStep('details');
      })
      .catch(complain);
  }, [complain]);

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
   * **Adopt a database that is already on this computer** — a reinstall, or a
   * drive letter that changed.
   *
   * `use_existing_shop` rather than `create_shop`: the two do the same thing
   * in Rust today, and they do not mean the same thing. One says "make me a
   * shop", the other says "that one is mine". A log line that says which is
   * worth a command name, and this one had never been called.
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

  /**
   * **Browse for a folder** — the owner's fifth item.
   *
   * The screen offered one path and no way to change it, so a shop that keeps
   * its data on D: had nowhere to say so. A webview cannot open a folder
   * picker at all, which is why this is a command: Rust opens the operating
   * system's own dialog, parented to this window, and hands back the path.
   *
   * Pressing Cancel returns nothing, and nothing is not an error.
   */
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
    // **The same rule Rust holds** — `mb_auth::pin::PIN_DIGITS`.
    //
    // The first draft said four here while Rust wanted six, which meant the
    // screen invited a PIN the program then refused. A form that asks for
    // something impossible is worse than one that asks for nothing.
    //
    // It is one constant on each side now rather than a pair. This screen was
    // already right — `length !== 4` — and the lock screen next door was not,
    // because it read the *minimum* of a range and let a PIN grow past it. Two
    // numbers is what made "the same rule" a thing you had to check by hand.
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
        // **Sign them in with the PIN they just chose.**
        //
        // Without this, finishing set-up drops somebody on the lock screen and
        // asks for the PIN they typed twenty seconds ago — which reads as the
        // program having lost it. Creating the owner account IS proving who you
        // are. If it fails the lock screen is still there and still works, so
        // this never stops the flow.
        return call('login', { staffId: id, pin })
          .catch(() => undefined)
          .then(() => setStep(code ? 'code' : 'menu'));
      })
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
        // **A tax CLASS, not a rate** (P13): one place decides what 5% means,
        // so changing the rate later changes every item on it. These four are
        // the ones migration 0001 seeds.
        taxClassId: itemClass,
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
        // **Back to the Item box.** Adding five things should be five names,
        // five prices and five Enters — and Enter is pressed in the PRICE box,
        // so without this the caret stays there and the sixth name is typed
        // into the price of the fifth. Found by adding two items and watching
        // the second one land in the wrong field.
        document
          .querySelector<HTMLInputElement>('input[name="firstrun-item"]')
          ?.focus();
      })
      .catch(complain)
      .finally(() => setBusy(false));
  };

  return (
    <div className="mb-firstrun">
      <div className="mb-firstrun__panel">
        {/* Where you are, and how much is left. Four dots, not a percentage:
            a percentage on a five-step form is a number nobody believes. */}
        <ol className="mb-firstrun__steps" aria-label="Setting up">
          {STEPS.slice(0, 4).map((s, n) => (
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
            <p className="mb-firstrun__lede">
              Two minutes and your counter is ready. Nothing here goes to the
              internet — your shop&rsquo;s data stays on this computer.
            </p>

            {/* **The folder, and a way to change it.**

                It used to be a path and nothing else, which read as an
                instruction rather than a choice — so a shop that keeps its
                data on a second drive had no way to say so and no sign that it
                was allowed to. The Browse button is Rust's, because a webview
                cannot open a folder picker at all. */}
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

            {/* **A database somewhere this counter did not look.** `find_shops`
                searches the usual places; an owner whose backup lives on a USB
                stick or a mapped drive is not in the usual places, and until
                now had no way in at all. */}
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
            {/* **"and it is printing" is true as of 2026-08-22.** The slip was
                promised by `mb_auth::recovery` and by the audit log and was
                never put on a printer — see `ipc::print_the_recovery_slip`. It
                is said here because this is the screen where somebody decides
                whether they need a pen. */}
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
                  // Enter goes to the price rather than doing nothing. Name,
                  // Enter, price, Enter — the rhythm somebody types a menu in.
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
                options={[
                  { value: 'tax_food_5', label: 'Restaurant food 5%' },
                  { value: 'tax_packaged_12', label: 'Packaged 12%' },
                  { value: 'tax_packaged_18', label: 'Packaged 18%' },
                  { value: 'tax_liquor', label: 'Liquor — outside GST' },
                ]}
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

            {/* **One button, because there is only one outcome.** The first
                draft had "I will do this later" beside "Skip and start
                billing", which are two ways of writing the same click — a
                person reads two buttons as two choices and stops to work out
                which. The label carries the difference instead. */}
            <div className="mb-firstrun__actions">
              <Button variant="primary" onClick={onDone}>
                {added.length > 0 ? 'Start billing' : 'Skip this — start billing'}
              </Button>
            </div>
          </section>
        ) : null}
      </div>
    </div>
  );
}
