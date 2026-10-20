/**
 * A section of settings drawn as a form. The Settings screen draws every section with it, and
 * a screen that keeps one section of its own (Day open/close) draws that section with it too.
 */

import { useId, type ReactNode } from 'react';

import {
  Card,
  Button,
  cx,
  Checkbox,
  Choice,
  InfoTip,
  Input,
  TextArea,
  MoneyInput,
  NumberInput,
  PhoneInput,
  Radio,
  SectionHeader,
  Select,
} from '../kit';
import type { GroupView } from '../ipc/generated/GroupView';
import type { SettingView } from '../ipc/generated/SettingView';
import { Logo } from './Logo';

import './settings.css';

/**
 * What a topic carries besides its settings, by the key of its first setting. The logo is a
 * FILE, so it cannot be in the catalogue; it sits with the two settings that place it.
 */
const TOPIC_EXTRAS: Record<string, () => ReactNode> = {
  'receipt.logo': () => <Logo />,
};

/** Choices of a handful, drawn as buttons rather than a list. */
const AS_BUTTONS = new Set(['receipt.design', 'receipt.font', 'kitchen.format', 'billing.parcel_charge_method']);

/** The edits a person has made and not yet saved, by key. */
export type Edits = Record<string, string>;

/** Put one value into the edits. */
export function withEdit(edits: Edits, key: string, value: string, saved: string | undefined): Edits {
  const next = { ...edits };
  if (value === saved) delete next[key];
  else next[key] = value;
  return next;
}

/** The edits as Rust takes them. */
export function editsList(edits: Edits): { key: string; value: string }[] {
  return Object.entries(edits).map(([key, value]) => ({ key, value }));
}

/** A section's settings: one card per topic, in the order the catalogue lists them. */
export function Section({
  section,
  edits,
  onChange,
  onOpenMenu,
}: {
  section: GroupView;
  edits: Edits;
  onChange: (key: string, value: string) => void;
  onOpenMenu?: () => void;
}) {
  const parcelMethod = edits['billing.parcel_charge_method']
    ?? section.settings.find((setting) => setting.key === 'billing.parcel_charge_method')?.value;
  const visible = section.settings.filter((setting) => {
    if (parcelMethod === undefined) return true;
    if (setting.key === 'billing.packing_charge') return parcelMethod === 'per_bill';
    if (setting.key === 'billing.parcel_charge_separate_screen'
      || setting.key === 'receipt.parcel_charge_separate') return parcelMethod === 'per_item';
    if (setting.key === 'billing.packing_charge_tax'
      || setting.key === 'billing.parcel_charge_delivery') return parcelMethod !== 'off';
    return true;
  });
  return (
    <>
      {topicsOf({ ...section, settings: visible }).map(({ topic, settings }) => (
        <Card key={topic}>
          {/* A setting with no heading of its own falls back to its section's name, which the
              panel already wears. */}
          {topic === section.label ? null : <SectionHeader title={topic} />}
          {TOPIC_EXTRAS[settings[0]!.key]?.()}
          {settings.some((setting) => setting.key === 'billing.parcel_charge_method') && (
            <div className="mb-settings__parcel-help">
              <p>
                {parcelMethod === 'per_item'
                  ? 'Each item’s parcel charge × quantity. Item charges are added together once.'
                  : parcelMethod === 'per_bill'
                    ? 'One fixed parcel charge for the whole bill. Item parcel charges are ignored.'
                    : 'No parcel charges are added. Saved item and bill amounts are kept for later.'}
              </p>
              <p>Applies to Parcel orders. Self service and Dine in do not add parcel charges.</p>
              {parcelMethod === 'per_item' && onOpenMenu && (
                <Button variant="quiet" onClick={onOpenMenu}>Edit item parcel charges in Menu</Button>
              )}
            </div>
          )}
          {/*
            A run of tick boxes packs tighter than a run of boxes to type in, and a run of
            shared lines stacks as a table.
          */}
          <div
            className={cx(
              'mb-settings__fields',
              section.code === 'billing' && 'mb-settings__fields--billing',
              allTicks(settings) && 'mb-settings__fields--ticks',
              allLines(settings) && 'mb-settings__fields--lines',
            )}
          >
            {linesOf(settings).map((line) =>
              line.row === '' ? (
                <Field
                  key={line.settings[0]!.key}
                  setting={line.settings[0]!}
                  value={edits[line.settings[0]!.key] ?? line.settings[0]!.value}
                  changed={edits[line.settings[0]!.key] !== undefined}
                  disabled={!section.canEdit}
                  onChange={(value) => onChange(line.settings[0]!.key, value)}
                />
              ) : (
                <Line
                  key={line.row}
                  row={line.row}
                  settings={line.settings}
                  edits={edits}
                  disabled={!section.canEdit}
                  onChange={onChange}
                />
              ),
            )}
          </div>
        </Card>
      ))}
    </>
  );
}

/** A section's settings in the runs they share a heading with. */
function topicsOf(section: GroupView): { topic: string; settings: SettingView[] }[] {
  const runs: { topic: string; settings: SettingView[] }[] = [];
  for (const setting of section.settings) {
    const last = runs[runs.length - 1];
    if (last && last.topic === setting.topic) last.settings.push(setting);
    else runs.push({ topic: setting.topic, settings: [setting] });
  }
  return runs;
}

/** A topic's settings, in the lines they share. */
function linesOf(settings: SettingView[]): { row: string; settings: SettingView[] }[] {
  const lines: { row: string; settings: SettingView[] }[] = [];
  for (const setting of settings) {
    const last = lines[lines.length - 1];
    if (setting.row !== '' && last && last.row === setting.row) last.settings.push(setting);
    else lines.push({ row: setting.row, settings: [setting] });
  }
  // The address gets its own full-width row after the shop's shorter details.
  return lines.sort((a, b) => Number(a.settings[0]?.key === 'store.address') - Number(b.settings[0]?.key === 'store.address'));
}

/** Whether a run is nothing but tick boxes — see the note where it is used. */
function allTicks(settings: SettingView[]): boolean {
  return settings.every((setting) => setting.control === 'tick');
}

/** Whether a run is nothing but shared lines — the text sizes. */
function allLines(settings: SettingView[]): boolean {
  return settings.every((setting) => setting.row !== '');
}

/** Settings that are one decision, on one line. */
function Line({
  row,
  settings,
  edits,
  disabled,
  onChange,
}: {
  row: string;
  settings: SettingView[];
  edits: Edits;
  disabled: boolean;
  onChange: (key: string, value: string) => void;
}) {
  const changed = settings.some((setting) => edits[setting.key] !== undefined);
  // The controls lose their own labels here, and the tip goes with a label — so the line keeps
  // it.
  const hint = settings.find((setting) => setting.help !== '')?.help;
  return (
    <div className={cx('mb-settings__line', changed && 'mb-settings__field--changed')}>
      <span className="mb-settings__linename">
        {row}
        {hint ? <InfoTip label={`About ${row}`}>{hint}</InfoTip> : null}
      </span>
      <div className="mb-settings__linecontrols">
        {settings.map((setting) => (
          <Field
            key={setting.key}
            setting={setting}
            value={edits[setting.key] ?? setting.value}
            changed={false}
            disabled={disabled}
            inLine
            onChange={(value) => onChange(setting.key, value)}
          />
        ))}
      </div>
    </div>
  );
}

/** One setting, drawn as whatever it says it is. */
export function Field({
  setting,
  value,
  changed,
  disabled,
  inLine = false,
  onChange,
}: {
  setting: SettingView;
  value: string;
  changed: boolean;
  disabled: boolean;
  /** Drawn inside a shared line, which already carries the name. */
  inLine?: boolean;
  onChange: (value: string) => void;
}) {
  const choiceName = useId();
  const hint = setting.help === '' ? undefined : setting.help;
  // On a shared line the heading beside it says "Total"; the control says "Size".
  const shown = inLine ? setting.short : setting.label;
  const body = (() => {
    // One persisted flag, two exclusive choices: old shops keep their saved mode.
    if (setting.key === 'billing.kitchen_ticket_off') return (
      <div className="mb-field">
        <div className="mb-field__labelrow">
          <span className="mb-field__label">{setting.label}</span>
          {hint ? <InfoTip label={`About ${setting.label}`}>{hint}</InfoTip> : null}
        </div>
        <div className="mb-settings__orderchoices" role="radiogroup" aria-label={setting.label}>
          <Radio name={choiceName} label="Print KOT" checked={value === '0'} disabled={disabled} onChange={() => onChange('0')} />
          <Radio name={choiceName} label="Save order only" checked={value === '1'} disabled={disabled} onChange={() => onChange('1')} />
        </div>
      </div>
    );
    if (setting.key === 'store.address') return <TextArea label={setting.label} hint={hint}
      value={value} disabled={disabled} maxLength={200} onChange={(event) => onChange(event.currentTarget.value)} />;
    switch (setting.control) {
      case 'tick':
        return (
          <Checkbox
            label={shown}
            aria-label={setting.label}
            hint={hint}
            checked={value === '1'}
            disabled={disabled}
            onChange={(event) => onChange(event.currentTarget.checked ? '1' : '0')}
          />
        );
      case 'choice':
        if (AS_BUTTONS.has(setting.key)) {
          return (
            <Choice
              label={setting.label}
              hint={inLine ? undefined : hint}
              value={value}
              options={setting.choices}
              disabled={disabled}
              onPick={onChange}
            />
          );
        }
        return (
          <Select
            label={inLine ? undefined : setting.label}
            aria-label={setting.label}
            hint={inLine ? undefined : hint}
            value={value}
            disabled={disabled}
            options={setting.choices}
            onChange={(event) => onChange(event.currentTarget.value)}
          />
        );
      case 'number':
        return (
          <NumberInput
            size={setting.key === 'billing.bill_copies' ? 'xs' : undefined}
            label={inLine ? undefined : setting.label}
            aria-label={setting.label}
            hint={
              // On a shared line the name beside the box carries the tip, so a second one
              // floating over the box is an orphan.
              inLine
                ? undefined
                : setting.unit === ''
                  ? hint
                  : `${hint ? `${hint} ` : ''}${setting.min}–${setting.max} ${setting.unit}.`
            }
            value={value}
            disabled={disabled}
            /*
             * A count, so digits and nothing else — `Kind::Int` has no room for a dot and less
             * for a letter.
             */
            onChange={(event) => onChange(event.currentTarget.value.replace(/[^0-9-]/g, ''))}
          />
        );
      case 'amount':
        return (
          <MoneyInput
            label={inLine ? undefined : setting.label}
            aria-label={setting.label}
            hint={inLine ? undefined : hint}
            value={value}
            disabled={disabled}
            onChange={onChange}
          />
        );
      /* A clock time — "05:00" — and Rust holds the minutes. */
      case 'time':
        return (
          <Input
            type="time"
            label={inLine ? undefined : setting.label}
            aria-label={setting.label}
            hint={inLine ? undefined : hint}
            value={value}
            disabled={disabled}
            onChange={(event) => onChange(event.currentTarget.value)}
          />
        );
      /* The shop's own number, and it is a phone like every other. */
      case 'phone':
        return (
          <PhoneInput
            label={setting.label}
            hint={hint}
            value={value}
            disabled={disabled}
            onChange={onChange}
          />
        );
      default:
        return (
          <Input
            label={inLine ? undefined : setting.label}
            aria-label={setting.label}
            hint={inLine ? undefined : hint}
            value={value}
            disabled={disabled}
            maxLength={setting.maxLen > 0 ? setting.maxLen : undefined}
            onChange={(event) => onChange(event.currentTarget.value)}
          />
        );
    }
  })();

  return (
    <div
      data-setting={setting.key}
      className={cx(
        'mb-settings__field',
        setting.control === 'tick' && 'mb-settings__field--tick',
        changed && 'mb-settings__field--changed',
        // A row of buttons wants the whole line, or it wraps into two.
        AS_BUTTONS.has(setting.key) && 'mb-settings__field--wide',
      )}
    >
      {body}
      {setting.control === 'number' && setting.unit !== '' ? (
        <span className="mb-settings__unit">
          {setting.unit}{setting.key === 'billing.service_charge_bp' ? ' · 500 = 5%' : ''}
        </span>
      ) : null}
    </div>
  );
}
