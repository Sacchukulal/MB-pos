import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react';
import { createPortal } from 'react-dom';
import { Button, FieldShell, Input } from './controls';
import { Icon } from './Icon';
import { useFocusScope } from './focus';
import './date-picker.css';

const months = Array.from({ length: 12 }, (_, month) =>
  new Intl.DateTimeFormat('en-IN', { month: 'long', timeZone: 'UTC' }).format(day(2024, month, 1)));
const weekdays = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
const fullDate = new Intl.DateTimeFormat('en-IN', { dateStyle: 'full', timeZone: 'UTC' });

// Calendar arithmetic uses UTC so daylight-saving changes cannot shift a chosen day.
function day(year: number, month: number, date: number): Date {
  const result = new Date(0);
  result.setUTCFullYear(year, month, date);
  return result;
}
function iso(date: Date): string {
  return `${String(date.getUTCFullYear()).padStart(4, '0')}-${String(date.getUTCMonth() + 1).padStart(2, '0')}-${String(date.getUTCDate()).padStart(2, '0')}`;
}
function parse(value: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return null;
  const result = day(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
  return Number(match[1]) >= 1 && iso(result) === value ? result : null;
}
function display(value: string): string {
  return parse(value) ? value.split('-').reverse().join('-') : value;
}
function typedDate(value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed) return '';
  const dmy = /^(\d{2})[-/](\d{2})[-/](\d{4})$/.exec(trimmed);
  const result = dmy ? `${dmy[3]}-${dmy[2]}-${dmy[1]}` : trimmed;
  return parse(result) ? result : null;
}
function today(): Date {
  const now = new Date();
  return day(now.getFullYear(), now.getMonth(), now.getDate());
}

export interface DatePickerProps {
  label?: string;
  'aria-label'?: string;
  hint?: string;
  error?: string;
  value: string;
  onChange: (value: string) => void;
  min?: string;
  max?: string;
  required?: boolean;
  disabled?: boolean;
  readOnly?: boolean;
  name?: string;
  size?: 'sm' | 'md';
}

/** One date field and calendar for the entire app. Values remain ISO dates across IPC. */
export function DatePicker({ label, 'aria-label': ariaLabel, hint, error, value, onChange,
  min = '0001-01-01', max = '9999-12-31', required, disabled, readOnly, name, size = 'md' }: DatePickerProps) {
  const [draft, setDraft] = useState(() => display(value));
  const [invalid, setInvalid] = useState('');
  const [open, setOpen] = useState(false);
  const emitted = useRef(value);
  const box = useRef<HTMLInputElement>(null);
  const anchor = useRef<HTMLDivElement>(null);
  const popupId = useId();
  const title = label ?? ariaLabel ?? 'Date';
  useEffect(() => {
    // Clearing the committed value while someone types must not erase their draft.
    if (value !== emitted.current) { setDraft(display(value)); setInvalid(''); }
    emitted.current = value;
  }, [value]);
  useEffect(() => { if (disabled || readOnly) setOpen(false); }, [disabled, readOnly]);
  const accept = (text: string, finished: boolean) => {
    const parsed = typedDate(text);
    const message = parsed === null ? 'Enter a valid date as DD-MM-YYYY.' :
      !parsed ? required ? 'Choose a date.' : '' :
      parsed < min ? `Choose ${display(min)} or later.` :
      parsed > max ? `Choose ${display(max)} or earlier.` : '';
    setInvalid(finished || parsed !== null ? message : '');
    const committed = parsed !== null && !message ? parsed : '';
    if (committed !== value) { emitted.current = committed; onChange(committed); }
    if (finished && parsed !== null && !message) setDraft(display(parsed));
    return !message;
  };
  const choose = (chosen: string) => {
    emitted.current = chosen; setDraft(display(chosen)); setInvalid(''); onChange(chosen); setOpen(false);
  };
  return <FieldShell label={label} hint={hint} error={invalid || error}>
    {(id, hasError) => <div className={`mb-date mb-date--${size}`} ref={anchor}>
      <input ref={box} id={id} className="mb-input" type="text" inputMode="numeric"
        autoComplete="off" placeholder="DD-MM-YYYY" aria-label={ariaLabel}
        aria-invalid={hasError || undefined} aria-describedby={hasError ? `${id}-error` : undefined}
        name={name} value={draft} required={required} disabled={disabled} readOnly={readOnly}
        onChange={(event) => { setDraft(event.target.value); accept(event.target.value, false); }}
        onBlur={() => accept(draft, true)}
        onKeyDown={(event) => {
          if ((event.altKey && event.key === 'ArrowDown') || event.key === 'F4') {
            if (!disabled && !readOnly) { event.preventDefault(); setOpen(true); }
          } else if (event.key === 'Enter' && !accept(draft, true)) {
            event.preventDefault(); event.stopPropagation();
          }
        }} />
      <Button variant="quiet" size="sm" iconOnly title={`Choose ${title.toLowerCase()}`}
        aria-label={`Choose ${title.toLowerCase()}`} aria-haspopup="dialog" aria-expanded={open}
        aria-controls={open ? popupId : undefined} disabled={disabled || readOnly}
        onClick={() => setOpen((shown) => !shown)}><Icon name="calendar" size="sm" /></Button>
      {open ? <Calendar id={popupId} title={title} anchor={anchor.current} value={value}
        min={min} max={max} required={required} onChoose={choose} onClose={() => setOpen(false)} /> : null}
    </div>}
  </FieldShell>;
}

function Calendar({ id, title, anchor, value, min, max, required, onChoose, onClose }: {
  id: string; title: string; anchor: HTMLElement | null; value: string; min: string; max: string;
  required?: boolean; onChoose: (value: string) => void; onClose: () => void;
}) {
  const constrain = (date: Date) => parse(iso(date) < min ? min : iso(date) > max ? max : iso(date)) ?? today();
  const [cursor, setCursor] = useState(() => constrain(parse(value) ?? today()));
  const [pickingMonth, setPickingMonth] = useState(false);
  const [yearText, setYearText] = useState(String(cursor.getUTCFullYear()));
  const panel = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState({ left: 0, top: 0 });
  const year = cursor.getUTCFullYear();
  const month = cursor.getUTCMonth();
  const caption = `${months[month]} ${year}`;
  const enabled = (date: Date) => date.getUTCFullYear() >= 1 && date.getUTCFullYear() <= 9999 && iso(date) >= min && iso(date) <= max;

  useLayoutEffect(() => {
    const position = () => {
      if (!panel.current || !anchor) return;
      const bounds = anchor.getBoundingClientRect();
      const popup = panel.current.getBoundingClientRect();
      const edge = parseFloat(getComputedStyle(panel.current).paddingTop) || 0;
      const below = bounds.bottom + edge;
      const above = bounds.top - popup.height - edge;
      setPlace({
        left: Math.max(edge, Math.min(bounds.left, window.innerWidth - popup.width - edge)),
        top: Math.max(edge, Math.min(below + popup.height <= window.innerHeight - edge ? below : above,
          window.innerHeight - popup.height - edge)),
      });
    };
    position();
    window.addEventListener('resize', position);
    window.addEventListener('scroll', position, true);
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(position);
    if (panel.current) observer?.observe(panel.current);
    return () => { window.removeEventListener('resize', position); window.removeEventListener('scroll', position, true); observer?.disconnect(); };
  }, [anchor]);
  useFocusScope(panel, true, () => panel.current?.querySelector<HTMLElement>('[data-calendar-cursor="true"]'));
  const move = (date: Date) => { const next = constrain(date); setCursor(next); setYearText(String(next.getUTCFullYear())); };
  const moveMonth = (offset: number) => {
    const first = day(year, month + offset, 1);
    const last = day(first.getUTCFullYear(), first.getUTCMonth() + 1, 0).getUTCDate();
    move(day(first.getUTCFullYear(), first.getUTCMonth(), Math.min(cursor.getUTCDate(), last)));
  };
  const focusDate = useRef(false);
  useLayoutEffect(() => {
    if (focusDate.current) { panel.current?.querySelector<HTMLElement>('[data-calendar-cursor="true"]')?.focus(); focusDate.current = false; }
  }, [cursor, pickingMonth]);
  const keys = (event: KeyboardEvent<HTMLButtonElement>) => {
    const shifts: Record<string, number> = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -7, ArrowDown: 7,
      Home: -cursor.getUTCDay(), End: 6 - cursor.getUTCDay() };
    if (event.key in shifts) {
      event.preventDefault(); focusDate.current = true;
      move(day(year, month, cursor.getUTCDate() + shifts[event.key]!));
    } else if (event.key === 'PageUp' || event.key === 'PageDown') {
      event.preventDefault(); focusDate.current = true;
      moveMonth((event.key === 'PageUp' ? -1 : 1) * (event.shiftKey ? 12 : 1));
    }
  };
  const start = day(year, month, 1).getUTCDay();
  return createPortal(<>
    <button type="button" className="mb-sheetscrim mb-select__scrim" tabIndex={-1}
      aria-label={`Close ${title.toLowerCase()} calendar`} onMouseDown={(event) => event.preventDefault()} onClick={onClose} />
    <div ref={panel} id={id} className="mb-sheet mb-calendar" role="dialog" aria-modal="true"
      aria-label={`${title} calendar`} tabIndex={-1}
      style={{ left: place.left, top: place.top }} /* mb-tokens-allow: popup coordinates measured from the field and viewport */
      onKeyDown={(event) => {
        if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); onClose(); }
        if (event.key === 'Enter') event.stopPropagation();
      }}>
      <div className="mb-calendar__head">
        <Button variant="quiet" size="sm" iconOnly title={pickingMonth ? 'Previous year' : 'Previous month'}
          disabled={pickingMonth ? year <= Number(min.slice(0, 4)) : iso(day(year, month, 0)) < min}
          onClick={() => moveMonth(pickingMonth ? -12 : -1)}><Icon name="chevron-left" size="sm" /></Button>
        <Button variant="quiet" size="sm" aria-label="Choose month and year" aria-expanded={pickingMonth}
          onClick={() => setPickingMonth((shown) => !shown)}>{caption}<Icon name="chevron-down" size="sm" /></Button>
        <Button variant="quiet" size="sm" iconOnly title={pickingMonth ? 'Next year' : 'Next month'}
          disabled={pickingMonth ? year >= Number(max.slice(0, 4)) : iso(day(year, month + 1, 1)) > max}
          onClick={() => moveMonth(pickingMonth ? 12 : 1)}><Icon name="chevron-right" size="sm" /></Button>
      </div>
      <span className="mb-visually-hidden" aria-live="polite">{caption}</span>
      {pickingMonth ? <>
        <Input label="Year" inputMode="numeric" value={yearText} maxLength={4}
          onChange={(event) => {
            const text = event.target.value.replace(/\D/g, ''); setYearText(text);
            if (text.length === 4 && Number(text) >= 1) setCursor(constrain(day(Number(text), month, 1)));
          }} onBlur={() => setYearText(String(year))} />
        <div className="mb-calendar__months">{months.map((name, index) => <Button key={name}
          variant="quiet" size="sm" aria-pressed={index === month}
          disabled={iso(day(year, index + 1, 0)) < min || iso(day(year, index, 1)) > max}
          onClick={() => { focusDate.current = true; move(day(year, index, 1)); setPickingMonth(false); }}>{name}</Button>)}</div>
      </> : <div role="grid" aria-label={caption} className="mb-calendar__grid">
        <div role="row" className="mb-calendar__week">{weekdays.map((name) => <span key={name} role="columnheader">{name}</span>)}</div>
        {Array.from({ length: 6 }, (_, week) => <div role="row" className="mb-calendar__week" key={week}>
          {Array.from({ length: 7 }, (_, weekday) => {
            const date = day(year, month, week * 7 + weekday - start + 1);
            const dateValue = iso(date);
            return <button key={weekday} type="button" role="gridcell" className="mb-calendar__day"
              aria-label={fullDate.format(date)} aria-selected={dateValue === value}
              aria-current={dateValue === iso(today()) ? 'date' : undefined}
              data-outside={date.getUTCMonth() !== month || undefined}
              data-calendar-cursor={dateValue === iso(cursor)} tabIndex={dateValue === iso(cursor) ? 0 : -1}
              disabled={!enabled(date)}
              onKeyDown={keys} onClick={() => onChoose(dateValue)}>{date.getUTCDate()}</button>;
          })}
        </div>)}
      </div>}
      <div className="mb-calendar__foot">
        {!required ? <Button variant="quiet" size="sm" onClick={() => onChoose('')}>Clear</Button> : null}
        <Button variant="quiet" size="sm" disabled={!enabled(today())} onClick={() => onChoose(iso(today()))}>Today</Button>
        <Button variant="quiet" size="sm" onClick={onClose}>Close</Button>
      </div>
    </div>
  </>, document.body);
}
