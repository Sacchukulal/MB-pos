/** The things a cashier presses and types into. */

import {
  forwardRef,
  useEffect,
  useId,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type InputHTMLAttributes,
  type KeyboardEvent,
  type ReactNode,
  type SelectHTMLAttributes,
  type Ref,
} from 'react';
import { createPortal } from 'react-dom';

import { cx } from './cx';
import { Icon } from './Icon';
import { InfoTip } from './InfoTip';

type Variant = 'primary' | 'secondary' | 'quiet' | 'danger';

/** lg — a hand on a touch screen; md — a page's own buttons; sm — inside a row. */
type Size = 'sm' | 'md' | 'lg';

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  wide?: boolean;
  icon?: ReactNode;
  /** An icon and nothing else: square, and `title` is compulsory. */
  iconOnly?: boolean;
  /**
   * A pressable row in a list — a person on the lock screen, a section in a rail. May hold two
   * lines; its height comes from its words.
   */
  list?: boolean;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  function Button(
    { variant = 'secondary', size = 'md', wide, icon, iconOnly, list, children, className, ...rest },
    ref,
  ) {
    const classes = cx(
      'mb-button',
      `mb-button--${variant}`,
      size !== 'md' && `mb-button--${size}`,
      wide && 'mb-button--wide',
      iconOnly && 'mb-button--icon',
      list && 'mb-button--list',
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
  /** The box itself, for a caller that has to focus it — the Cash button on the till. */
  ref?: Ref<HTMLInputElement>;
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

/** Where the sheet lies: fixed to the window, under the box or above it. */
interface SheetPlace {
  left: number;
  width: number;
  top: number | null;
  bottom: number | null;
  maxHeight: number;
}

/** Room the sheet keeps from the window's edge, in pixels. */
const SHEET_EDGE = 8;

/**
 * A dropdown. The `<select>` stays — it holds the value, carries the label, takes the keys
 * and is what a form or a test reads — but the list it opens is the kit's own sheet, the same
 * one a menu opens, rather than the one the operating system draws.
 */
export function Select({
  label,
  hint,
  error,
  options,
  className,
  onKeyDown,
  ...rest
}: SelectProps) {
  const box = useRef<HTMLSelectElement>(null);
  const sheet = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<SheetPlace | null>(null);
  const [highlighted, setHighlighted] = useState(0);
  const typed = useRef({ text: '', at: 0 });
  const open = place !== null;

  const chosenIndex = () => {
    const current = box.current?.value;
    const at = options.findIndex((o) => o.value === current);
    return at < 0 ? 0 : at;
  };

  const show = () => {
    const el = box.current;
    if (!el || el.disabled || options.length === 0) return;
    const rect = el.getBoundingClientRect();
    const below = window.innerHeight - rect.bottom - SHEET_EDGE;
    const above = rect.top - SHEET_EDGE;
    // The whole list, one row per option, at the box's own height.
    const needed = el.offsetHeight * options.length + SHEET_EDGE * 2;
    // The sheet drops unless it would be cut short below and has more room above.
    const up = below < needed && above > below;
    setHighlighted(chosenIndex());
    setPlace({
      left: rect.left,
      width: rect.width,
      top: up ? null : rect.bottom,
      bottom: up ? window.innerHeight - rect.top : null,
      maxHeight: up ? above : below,
    });
  };

  const hide = () => setPlace(null);

  const choose = (index: number) => {
    const el = box.current;
    const option = options[index];
    hide();
    if (!el || !option) return;
    if (el.value !== option.value) {
      el.value = option.value;
      // Through the element, so the caller's `onChange` fires the way a keypress would.
      el.dispatchEvent(new Event('change', { bubbles: true }));
    }
    el.focus();
  };

  // A page that scrolls or a window that resizes moves the box; the sheet goes rather than
  // drift. The sheet's own scrolling is not that.
  useEffect(() => {
    if (!open) return undefined;
    const off = (event?: Event) => {
      if (event?.target instanceof Node && sheet.current?.contains(event.target)) return;
      hide();
    };
    window.addEventListener('scroll', off, true);
    window.addEventListener('resize', off);
    return () => {
      window.removeEventListener('scroll', off, true);
      window.removeEventListener('resize', off);
    };
  }, [open]);

  // The highlighted row is never out of sight.
  useEffect(() => {
    if (!open) return;
    const row = sheet.current?.children[highlighted];
    if (row instanceof HTMLElement && typeof row.scrollIntoView === 'function') {
      row.scrollIntoView({ block: 'nearest' });
    }
  }, [open, highlighted]);

  const move = (to: number) => {
    setHighlighted(Math.max(0, Math.min(options.length - 1, to)));
  };

  /** Letters jump to the first option that starts with what was typed. */
  const jump = (key: string) => {
    const now = Date.now();
    const prefix = (now - typed.current.at < 600 ? typed.current.text : '') + key.toLowerCase();
    typed.current = { text: prefix, at: now };
    const at = options.findIndex((o) => o.label.toLowerCase().startsWith(prefix));
    if (at >= 0) setHighlighted(at);
  };

  const keys = (event: KeyboardEvent<HTMLSelectElement>) => {
    onKeyDown?.(event);
    if (event.defaultPrevented) return;
    if (!open) {
      if (event.key === ' ' || event.key === 'Enter' || (event.altKey && event.key === 'ArrowDown')) {
        event.preventDefault();
        show();
      }
      return;
    }
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault();
        move(highlighted + 1);
        break;
      case 'ArrowUp':
        event.preventDefault();
        move(highlighted - 1);
        break;
      case 'Home':
        event.preventDefault();
        move(0);
        break;
      case 'End':
        event.preventDefault();
        move(options.length - 1);
        break;
      case 'Enter':
      case ' ':
        event.preventDefault();
        choose(highlighted);
        break;
      case 'Escape':
        event.preventDefault();
        event.stopPropagation();
        hide();
        break;
      case 'Tab':
        hide();
        break;
      default:
        if (event.key.length === 1 && !event.ctrlKey && !event.altKey && !event.metaKey) {
          event.preventDefault();
          jump(event.key);
        }
    }
  };

  return (
    <FieldShell label={label} hint={hint} error={error}>
      {(id, invalid) => (
        <>
          <select
            id={id}
            ref={box}
            className={cx('mb-input', 'mb-select', className)}
            aria-invalid={invalid || undefined}
            aria-expanded={open}
            data-open={open || undefined}
            // The press opens the kit's sheet instead of the operating system's list.
            onMouseDown={(event) => {
              if (event.button !== 0) return;
              event.preventDefault();
              box.current?.focus();
              if (open) hide();
              else show();
            }}
            onKeyDown={keys}
            {...rest}
          >
            {options.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
          {place
            ? createPortal(
                <>
                  <button
                    type="button"
                    className="mb-sheetscrim mb-select__scrim"
                    aria-label="Close"
                    tabIndex={-1}
                    onMouseDown={(event) => event.preventDefault()}
                    onClick={hide}
                  />
                  <div
                    ref={sheet}
                    className="mb-sheet mb-select__sheet"
                    role="listbox"
                    aria-labelledby={id}
                    style={{ /* mb-tokens-allow: where the box is on the window, measured when it opened */
                      left: `${place.left}px`,
                      minWidth: `${place.width}px`,
                      top: place.top === null ? undefined : `${place.top}px`,
                      bottom: place.bottom === null ? undefined : `${place.bottom}px`,
                      maxHeight: `${place.maxHeight}px`,
                    }}
                  >
                    {options.map((option, index) => (
                      <button
                        key={option.value}
                        type="button"
                        role="option"
                        tabIndex={-1}
                        aria-selected={option.value === box.current?.value}
                        className={cx(
                          'mb-sheet__item',
                          index === highlighted && 'mb-sheet__item--on',
                        )}
                        // The box keeps the focus; a press here must not take it.
                        onMouseDown={(event) => event.preventDefault()}
                        onMouseEnter={() => setHighlighted(index)}
                        onClick={() => choose(index)}
                      >
                        {option.label}
                      </button>
                    ))}
                  </div>
                </>,
                document.body,
              )
            : null}
        </>
      )}
    </FieldShell>
  );
}

export interface CheckboxProps
  extends Omit<InputHTMLAttributes<HTMLInputElement>, 'type'> {
  /** The box itself — for a group tick that has to be set indeterminate. */
  ref?: Ref<HTMLInputElement>;
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
