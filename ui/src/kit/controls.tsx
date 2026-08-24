/**
 * The things a cashier presses and types into.
 *
 * Three rules apply to every component in this file, and they come from
 * UI_GUIDELINES rather than from taste:
 *
 * * **mouse, keyboard AND touch, in one layout** (§1) — there is no touch mode
 *   to switch into, so every control is 44px tall and every popup closes by
 *   touch;
 * * **every interactive thing looks interactive and has a visible focus ring**
 *   (§5) — the keyboard-first rule is worthless if you cannot see where you
 *   are;
 * * **colour is never the only signal** (§2) — an invalid field has a border,
 *   a message and an icon, not just a red edge.
 */

import {
  forwardRef,
  useId,
  type ButtonHTMLAttributes,
  type InputHTMLAttributes,
  type ReactNode,
  type SelectHTMLAttributes,
} from 'react';

import { cx } from './cx';
import { InfoTip } from './InfoTip';

type Variant = 'primary' | 'secondary' | 'quiet' | 'danger';

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  small?: boolean;
  wide?: boolean;
  icon?: ReactNode;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  function Button(
    { variant = 'secondary', small, wide, icon, children, className, ...rest },
    ref,
  ) {
    const classes = cx(
      'mb-button',
      `mb-button--${variant}`,
      small && 'mb-button--small',
      wide && 'mb-button--wide',
      className,
    );
    return (
      <button ref={ref} type="button" className={classes} {...rest}>
        {icon}
        {children}
      </button>
    );
  },
);

interface FieldShellProps {
  label?: string;
  hint?: string;
  error?: string;
  children: (id: string, invalid: boolean) => ReactNode;
}

/**
 * The label / hint / error scaffolding, once.
 *
 * Audit F10 is about confirmation dialogs, but the same disease shows up in
 * fields: five screens each inventing where the error text goes. It goes here.
 *
 * # The hint is **asked for**, not given
 *
 * It used to be a grey sentence under every box. The owner, 2026-08-24: *"as i
 * said many times, i dont like you adding those sub lines below all those
 * feilds."* So it is an `InfoTip` beside the label — the same ruling
 * `SectionHeader`, `Panel` and `Modal` already follow for their notes, applied
 * to the last place a paragraph could still be printed at somebody.
 *
 * **One change rather than ninety.** Every screen that already passes a hint
 * gets the hover behaviour without being touched, and no screen has anywhere
 * left to put an explanation on the page.
 *
 * An error still prints. A hint is something you may want; a thing you typed
 * wrongly is something you must be told.
 */
function FieldShell({ label, hint, error, children }: FieldShellProps) {
  const id = useId();
  const invalid = Boolean(error);
  return (
    <div className="mb-field">
      {label || hint ? (
        <div className="mb-field__labelrow">
          {label ? (
            <label className="mb-field__label" htmlFor={id}>
              {label}
            </label>
          ) : null}
          {hint ? <InfoTip label={label ? `About ${label}` : undefined}>{hint}</InfoTip> : null}
        </div>
      ) : null}
      {children(id, invalid)}
      {error ? (
        <span className="mb-field__error" role="alert">
          {/* The icon is the form half of the signal (§2 rule 2). */}
          <span aria-hidden="true">⚠</span>
          {error}
        </span>
      ) : null}
    </div>
  );
}

export interface InputProps
  extends Omit<InputHTMLAttributes<HTMLInputElement>, 'id'> {
  label?: string;
  hint?: string;
  error?: string;
  /**
   * A mark drawn **inside the box and outside the value** — `₹`, `+91`.
   *
   * It is a sibling of the `<input>`, sitting in padding reserved for it,
   * because an `<input>` is a replaced element and `::before` does not render
   * on one. That is a browser fact rather than a preference, and it is the
   * whole reason this prop exists instead of a line of CSS.
   *
   * **It is never part of `value`.** See `MoneyInput` for why that matters:
   * what reaches Rust, and what `Money::parse` sees, is the digits alone.
   */
  prefix?: string;
}

export function Input({ label, hint, error, className, prefix, ...rest }: InputProps) {
  const field = (id: string, invalid: boolean) => (
    <input
          id={id}
          /* **Off by default across the whole product**, and a caller may still
             say otherwise because this sits before the spread.

             This is a till on a shared counter, not a website. Windows' WebView
             offers a "Saved info" dropdown over any text field it recognises —
             found at first run, where it covered the tax selector while
             somebody was typing their menu in. Worse than covering the screen:
             it would offer one shop's customer names and phone numbers to
             whoever is standing at the counter next. Nothing here is a form
             anybody fills in twice, so there is nothing to save. */
          autoComplete="off"
          className={cx('mb-input', invalid && 'mb-input--invalid', className)}
      aria-invalid={invalid || undefined}
      {...rest}
    />
  );

  return (
    <FieldShell label={label} hint={hint} error={error}>
      {(id, invalid) =>
        prefix === undefined ? (
          field(id, invalid)
        ) : (
          <div className="mb-adorned">
            {/* `aria-hidden`: the mark is a shape, and the label already says
                what the field is. A screen reader announcing "rupees" before
                every amount is noise, and announcing "+91" before a phone is
                a country code the person did not type. */}
            <span className="mb-adorned__mark" aria-hidden="true">
              {prefix}
            </span>
            {field(id, invalid)}
          </div>
        )
      }
    </FieldShell>
  );
}

/**
 * A number field.
 *
 * `inputMode="decimal"` so a touch monitor shows a numeric keypad, and
 * `tabular-nums` so digits line up — §3, *"a column of rupees that doesn't line
 * up looks broken to a shopkeeper."*
 *
 * **It does no arithmetic.** Quantities and amounts are computed in Rust (R8);
 * this collects characters.
 *
 * For money use [`MoneyInput`], which is this plus the rupee mark and the rule
 * that what reaches Rust is only ever digits and one dot.
 */
export function NumberInput({ className, ...rest }: InputProps) {
  return (
    <Input
      inputMode="decimal"
      autoComplete="off"
      className={cx('mb-input--number', className)}
      {...rest}
    />
  );
}

// ---------------------------------------------------------------------------
// The two fields that have a SHAPE — 2026-08-22
// ---------------------------------------------------------------------------

/**
 * **What an amount may contain, and nothing else.**
 *
 * The owner, 2026-08-22: *"user needs to enter only numbers, not alphabet, it
 * would mess up calculations also, didn't you even consider that?"*
 *
 * Digits, at most one dot, at most two places after it. Everything else is
 * dropped as it is typed rather than refused on save — a field that lets you
 * type "12ab" and complains afterwards has already wasted the keystroke.
 *
 * Exported because [`MoneyInput`] is not the only door: the day-close counting
 * screen types denominations into its own grid, and it holds to the same rule
 * by calling this.
 */
export function onlyAmount(typed: string): string {
  // **A dot with a space after it is punctuation, not a decimal point.**
  //
  // Found by a test: pasting "Rs. 90" stripped the letters and left ".90",
  // which `Money::parse` reads as ninety PAISE. A silently wrong amount is the
  // exact failure the owner was worried about — *"it would mess up calculations
  // also"* — and it is worth one line to make impossible. A decimal point never
  // has a space after it; a full stop usually does.
  let cleaned = typed.replace(/\.\s/g, ' ');
  // Then strip everything that is not a digit or a dot. A pasted "₹1,200.50"
  // comes out as "1200.50", which is the point of doing this here rather than
  // refusing the paste.
  cleaned = cleaned.replace(/[^0-9.]/g, '');
  // One dot only — the first one wins, so "12.5.7" becomes "12.57" rather than
  // being thrown away. Somebody typing fast should not lose the number.
  const dot = cleaned.indexOf('.');
  if (dot !== -1) {
    cleaned = `${cleaned.slice(0, dot + 1)}${cleaned.slice(dot + 1).replace(/\./g, '')}`;
  }
  // Paise, and there are only two of them. `Money::parse` refuses a third with
  // `TooPrecise`; this stops it being typed at all.
  const [whole = '', frac] = cleaned.split('.');
  if (frac === undefined) return whole;
  return `${whole}.${frac.slice(0, 2)}`;
}

export interface MoneyInputProps extends Omit<InputProps, 'onChange' | 'value'> {
  /** The plain string Rust parses — `"120.50"`. Never carries the ₹. */
  value: string;
  /** Called with the cleaned value, so a screen never has to filter it again. */
  onChange: (value: string) => void;
}

/**
 * **An amount, with the rupee mark drawn beside it and never inside it.**
 *
 * The owner asked for three things on 2026-08-22 and this is all three:
 *
 * 1. *"Only numbers"* — see [`onlyAmount`]. Letters cannot be typed.
 * 2. *"in menu item adding, there is 0.00, but in other places empty. make it
 *    look same, no need for 0.00, just keep it empty."* Empty is empty. The
 *    `0.00` came from the menu screen seeding its field with a formatted zero;
 *    an amount nobody has typed is not zero rupees, it is nothing yet.
 * 3. *"add a rupee symbol so that user knows it is amount feild… make sure when
 *    you add rupee symbol, it does not interfere with calculations (do not save
 *    rupee symbol also with amount to db)."*
 *
 * # How the ₹ cannot reach the database
 *
 * It is **not in the value**. It is a `<span>` beside the box, and the box's
 * `value` is the plain string it always was — so what travels to Rust, and
 * what `Money::parse` sees, is exactly `"120.50"`.
 *
 * That is structural rather than careful: there is no code path that could put
 * the symbol into the value, because the symbol is never a character in it.
 * (`Money::parse` happens to strip `₹` anyway, which is a second net under
 * this one and not the reason it is safe.)
 */
export function MoneyInput({
  value,
  onChange,
  className,
  ...rest
}: MoneyInputProps) {
  return (
    <Input
      inputMode="decimal"
      autoComplete="off"
      value={value}
      onChange={(event) => onChange(onlyAmount(event.target.value))}
      className={cx('mb-input--money', className)}
      prefix="₹"
      {...rest}
    />
  );
}

/** Ten digits. India, and the owner's instruction of 2026-08-22. */
export const PHONE_DIGITS = 10;

/**
 * **A phone number, as this country writes one.**
 *
 * The owner: *"i can enter alphabets, more than 10 numbers, fix it, this app is
 * india only so only 10 digits needed."*
 *
 * Digits only, ten of them. A pasted `+91 98765 43210` keeps its last ten
 * rather than being refused — the country code and the spacing are how the
 * number arrives from a phone's contact list, and throwing the paste away is
 * the version that makes somebody retype it.
 */
export function onlyPhone(typed: string): string {
  const digits = typed.replace(/\D/g, '');
  // `+91` pasted in front, or the trunk `0` people still write. Only when the
  // rest is exactly ten, so a genuine ten-digit number starting 91 survives.
  if (digits.length === 12 && digits.startsWith('91')) return digits.slice(2);
  if (digits.length === 11 && digits.startsWith('0')) return digits.slice(1);
  return digits.slice(0, PHONE_DIGITS);
}

export interface PhoneInputProps extends Omit<InputProps, 'onChange' | 'value'> {
  value: string;
  onChange: (value: string) => void;
}

/**
 * **The phone field. Ten digits, and it is the only way to collect one.**
 *
 * `+91` is drawn beside the box rather than typed into it, for the same reason
 * the rupee mark is: what is stored is the ten digits, so two shops' customer
 * lists can be compared, `mb_core::credit::phone_key` can match them, and a
 * bill 32 columns wide has room to print one.
 */
export function PhoneInput({
  value,
  onChange,
  className,
  ...rest
}: PhoneInputProps) {
  return (
    <Input
      type="tel"
      inputMode="numeric"
      autoComplete="off"
      maxLength={PHONE_DIGITS}
      value={value}
      onChange={(event) => onChange(onlyPhone(event.target.value))}
      className={cx('mb-input--phone', className)}
      prefix="+91"
      {...rest}
    />
  );
}

export interface SelectProps
  extends Omit<SelectHTMLAttributes<HTMLSelectElement>, 'id'> {
  label?: string;
  hint?: string;
  error?: string;
  options: readonly { value: string; label: string }[];
}

export function Select({
  label,
  hint,
  error,
  options,
  className,
  ...rest
}: SelectProps) {
  return (
    <FieldShell label={label} hint={hint} error={error}>
      {(id, invalid) => (
        <select
          id={id}
          className={cx('mb-input', className)}
          aria-invalid={invalid || undefined}
          {...rest}
        >
          {options.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      )}
    </FieldShell>
  );
}

export interface CheckboxProps
  extends Omit<InputHTMLAttributes<HTMLInputElement>, 'type'> {
  /** Leave it out where something beside the box already names it. */
  label?: string;
  /** Asked for, like every other hint — a tip beside the box, never under it. */
  hint?: string;
}

export function Checkbox({ label, hint, ...rest }: CheckboxProps) {
  // No label means the box stands on its own — the caller names it with
  // `aria-label`, because something beside it already says what it is.
  const box = (
    <label className="mb-checkbox">
      <input type="checkbox" {...rest} />
      {label ? <span>{label}</span> : null}
    </label>
  );
  // The tip sits OUTSIDE the `<label>`: a button inside one toggles the box.
  return hint ? (
    <div className="mb-field__labelrow">
      {box}
      <InfoTip label={`About ${label}`}>{hint}</InfoTip>
    </div>
  ) : (
    box
  );
}

export function Radio({ label, hint: _hint, ...rest }: CheckboxProps) {
  return (
    <label className="mb-radio">
      <input type="radio" {...rest} />
      <span>{label}</span>
    </label>
  );
}

export interface SearchFieldProps extends InputProps {
  /** What is being searched, for the screen reader. */
  what?: string;
}

/**
 * Search lives in the same place on every screen (UI_GUIDELINES §1), so it is a
 * component rather than an input somebody styles again.
 */
export const SearchField = forwardRef<HTMLInputElement, SearchFieldProps>(
  function SearchField({ what = 'Search', ...rest }, ref) {
    return (
      <div className="mb-search">
        <span className="mb-search__icon" aria-hidden="true">
          ⌕
        </span>
        <input
          ref={ref}
          type="search"
          className="mb-input"
          placeholder={what}
          aria-label={what}
          autoComplete="off"
          {...rest}
        />
      </div>
    );
  },
);

export interface KeypadProps {
  onPress: (key: string) => void;
  disabled?: boolean;
  /**
   * Whether the decimal point is one of the keys. Default yes — a money pad
   * needs it.
   *
   * **A PIN pad does not**, and the one on the lock screen was drawing it
   * anyway. A key that is guaranteed to do nothing is not neutral: it is the
   * bottom-left corner of a three-by-four grid, exactly where a thumb lands,
   * and it teaches somebody who presses it that the pad is ignoring them. The
   * reducer already threw the dot away; this stops offering it.
   */
  dot?: boolean;
}

/**
 * The touch keypad — audit F1: *"many Indian counters now use touch monitors
 * and tablets."*
 *
 * It sends the same keys the keyboard sends, so P10's keyboard engine handles
 * both and there is one state machine rather than two.
 */
export function Keypad({ onPress, disabled, dot = true }: KeypadProps) {
  const keys = ['7', '8', '9', '4', '5', '6', '1', '2', '3', dot ? '.' : '', '0', '⌫'];
  return (
    <div className="mb-keypad" role="group" aria-label="Number pad">
      {keys.map((key, index) =>
        key === '' ? (
          // The hole keeps 0 and ⌫ where fingers already expect them. A grid
          // that reflows when the dot goes is a grid that moves Delete.
          <span key={`gap-${index}`} className="mb-keypad__gap" aria-hidden="true" />
        ) : (
          <Button
            key={key}
            className="mb-keypad__key"
            disabled={disabled}
            onClick={() => onPress(key === '⌫' ? 'Backspace' : key)}
            aria-label={key === '⌫' ? 'Delete' : key}
          >
            {key}
          </Button>
        ),
      )}
    </div>
  );
}
