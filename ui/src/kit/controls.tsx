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
    const classes = [
      'mb-button',
      `mb-button--${variant}`,
      small ? 'mb-button--small' : '',
      wide ? 'mb-button--wide' : '',
      className ?? '',
    ]
      .filter(Boolean)
      .join(' ');
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
 */
function FieldShell({ label, hint, error, children }: FieldShellProps) {
  const id = useId();
  const invalid = Boolean(error);
  return (
    <div className="mb-field">
      {label ? (
        <label className="mb-field__label" htmlFor={id}>
          {label}
        </label>
      ) : null}
      {children(id, invalid)}
      {hint && !error ? <span className="mb-field__hint">{hint}</span> : null}
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
}

export function Input({ label, hint, error, className, ...rest }: InputProps) {
  return (
    <FieldShell label={label} hint={hint} error={error}>
      {(id, invalid) => (
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
          className={[
            'mb-input',
            invalid ? 'mb-input--invalid' : '',
            className ?? '',
          ]
            .filter(Boolean)
            .join(' ')}
          aria-invalid={invalid || undefined}
          {...rest}
        />
      )}
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
 */
export function NumberInput({ className, ...rest }: InputProps) {
  return (
    <Input
      inputMode="decimal"
      autoComplete="off"
      className={['mb-input--number', className ?? ''].filter(Boolean).join(' ')}
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
          className={['mb-input', className ?? ''].filter(Boolean).join(' ')}
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
  label: string;
}

export function Checkbox({ label, ...rest }: CheckboxProps) {
  return (
    <label className="mb-checkbox">
      <input type="checkbox" {...rest} />
      <span>{label}</span>
    </label>
  );
}

export function Radio({ label, ...rest }: CheckboxProps) {
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
