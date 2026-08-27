/** The things a cashier presses and types into. */

import {
  forwardRef,
  useId,
  type ButtonHTMLAttributes,
  type InputHTMLAttributes,
  type ReactNode,
  type SelectHTMLAttributes,
} from 'react';

import { cx } from './cx';
import { Icon } from './Icon';
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

/** The label / hint / error scaffolding, once. */
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
          {/* The icon is the form half of the signal. */}
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
  /** A mark drawn inside the box and outside the value — `₹`, `+91`. */
  prefix?: string;
}

export function Input({ label, hint, error, className, prefix, ...rest }: InputProps) {
  const field = (id: string, invalid: boolean) => (
    <input
          id={id}
          /*
           * Off by default across the whole product, and a caller may still say otherwise
           * because this sits before the spread.
           */
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
            {/*
              `aria-hidden`: the mark is a shape, and the label already says what the field is.
            */}
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

/** A number field. */
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

// The two fields that have a SHAPE.

/** What an amount may contain, and nothing else. */
export function onlyAmount(typed: string): string {
  // A dot with a space after it is punctuation, not a decimal point.
  let cleaned = typed.replace(/\.\s/g, ' ');
  // Then strip everything that is not a digit or a dot.
  cleaned = cleaned.replace(/[^0-9.]/g, '');
  // One dot only — the first one wins, so "12.5.7" becomes "12.57" rather than being thrown
  // away.
  const dot = cleaned.indexOf('.');
  if (dot !== -1) {
    cleaned = `${cleaned.slice(0, dot + 1)}${cleaned.slice(dot + 1).replace(/\./g, '')}`;
  }
  // Paise, and there are only two of them.
  const [whole = '', frac] = cleaned.split('.');
  if (frac === undefined) return whole;
  return `${whole}.${frac.slice(0, 2)}`;
}

export interface MoneyInputProps extends Omit<InputProps, 'onChange' | 'value'> {
  /** The plain string Rust parses — `"120.50"`. */
  value: string;
  /** Called with the cleaned value, so a screen never has to filter it again. */
  onChange: (value: string) => void;
}

/** An amount, with the rupee mark drawn beside it and never inside it. */
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

export const PHONE_DIGITS = 10;

/** A phone number, as this country writes one. */
export function onlyPhone(typed: string): string {
  const digits = typed.replace(/\D/g, '');
  // `+91` pasted in front, or the trunk `0` people still write.
  if (digits.length === 12 && digits.startsWith('91')) return digits.slice(2);
  if (digits.length === 11 && digits.startsWith('0')) return digits.slice(1);
  return digits.slice(0, PHONE_DIGITS);
}

export interface PhoneInputProps extends Omit<InputProps, 'onChange' | 'value'> {
  value: string;
  onChange: (value: string) => void;
}

/** The phone field. Ten digits, and it is the only way to collect one. */
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
  // No label means the box stands on its own — the caller names it with `aria-label`, because
  // something beside it already says what it is.
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
 * Search lives in the same place on every screen, so it is a component rather than an input
 * somebody styles again.
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

export interface StepperProps {
  /** What the group is called for a screen reader — "Quantity of Masala Dosa". */
  label: string;
  /** The thing being counted, for the two buttons — "Masala Dosa", "person". */
  what: string;
  onLess: () => void;
  onMore: () => void;
  lessDisabled?: boolean;
  moreDisabled?: boolean;
  /** The figure between the buttons: a `.mb-stepper__value` span, button or input. */
  children: ReactNode;
  className?: string;
}

/** − n +. The one stepper in the product. */
export function Stepper({
  label,
  what,
  onLess,
  onMore,
  lessDisabled,
  moreDisabled,
  children,
  className,
}: StepperProps) {
  return (
    <div className={cx('mb-stepper', className)} role="group" aria-label={label}>
      <button
        type="button"
        className="mb-stepper__step"
        onClick={onLess}
        disabled={lessDisabled}
        aria-label={`One less ${what}`}
      >
        <Icon name="minus" size="sm" />
      </button>
      {children}
      <button
        type="button"
        className="mb-stepper__step"
        onClick={onMore}
        disabled={moreDisabled}
        aria-label={`One more ${what}`}
      >
        <Icon name="plus" size="sm" />
      </button>
    </div>
  );
}

export interface KeypadProps {
  onPress: (key: string) => void;
  disabled?: boolean;
  /** Whether the decimal point is one of the keys. */
  dot?: boolean;
}

/** The touch keypad. */
export function Keypad({ onPress, disabled, dot = true }: KeypadProps) {
  const keys = ['7', '8', '9', '4', '5', '6', '1', '2', '3', dot ? '.' : '', '0', '⌫'];
  return (
    <div className="mb-keypad" role="group" aria-label="Number pad">
      {keys.map((key, index) =>
        key === '' ? (
          // The hole keeps 0 and ⌫ where fingers already expect them.
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
