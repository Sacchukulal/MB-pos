/**
 * **The settings screen, and there is only one of it.**
 *
 * Audit Part 3 is four screens and about ninety settings, and v1 built them as
 * four hand-written forms over one 41-slot save. This is one form over a
 * catalogue: Rust sends `SettingsView`, every entry carries its own label, help
 * sentence, control kind, limits and choices, and this file draws whatever it
 * is given.
 *
 * **So adding a setting is a line in `catalog.rs` and nothing here.** That is
 * the same promise D21 makes about a theme, and it is the reason a session that
 * adds one setting cannot forget to add it to a screen.
 *
 * # What this file may and may not decide
 *
 * It may decide *where a box sits*. It may not decide what is valid, what the
 * default is, what a setting is called, or what it means — every one of those
 * is in the catalogue, and Rust refuses a bad value whatever this file thinks
 * (R8, and D45's argument applied to values rather than to permissions).
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import {
  Button,
  Card,
  Checkbox,
  ConfirmDialog,
  EmptyState,
  Input,
  NumberInput,
  SaveBar,
  SearchField,
  Select,
  Spinner,
  useToast,
} from '../kit';
import { call, inApp, isUiError } from '../ipc/call';
import type { GroupView } from '../ipc/generated/GroupView';
import type { SettingView } from '../ipc/generated/SettingView';
import type { SettingsView } from '../ipc/generated/SettingsView';

import './settings.css';

/** The edits a person has made and not yet saved, by key. */
type Edits = Record<string, string>;

export function Settings() {
  const [view, setView] = useState<SettingsView | null>(null);
  const [group, setGroup] = useState<string>('store');
  const [edits, setEdits] = useState<Edits>({});
  const [saving, setSaving] = useState(false);
  const [query, setQuery] = useState('');
  const [matches, setMatches] = useState<readonly string[] | null>(null);
  /** Where the unsaved-changes guard was heading when it stopped us. */
  const [leavingTo, setLeavingTo] = useState<string | null>(null);
  const body = useRef<HTMLDivElement>(null);
  const toast = useToast();

  useEffect(() => {
    if (body.current) body.current.scrollTop = 0;
  }, [group, matches]);

  useEffect(() => {
    if (!inApp()) return;
    call('settings_all')
      .then(setView)
      .catch((cause) => {
        if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
      });
    // `toast` is stable for the life of the provider — deliberately, and P09
    // found out why: a context value that changed on every toast turned one
    // error into a few hundred in a couple of seconds.
  }, [toast]);

  const dirty = Object.keys(edits).length > 0;

  // **Search is Rust's** — the synonym list is part of the rule (T9). Debounced
  // by the box rather than by a timer: no screen in this product owns a clock.
  const onSearch = useCallback(
    (text: string) => {
      setQuery(text);
      if (text.trim() === '') {
        setMatches(null);
        return;
      }
      call('search_settings', { text })
        .then(setMatches)
        .catch(() => setMatches([]));
    },
    [],
  );

  const groups = view?.groups ?? [];
  const active = groups.find((g) => g.code === group) ?? groups[0];

  const go = useCallback(
    (code: string) => {
      // **The guard, and it is the whole of it.** v1 lost edits silently when
      // a person clicked another section; this asks, and Save actually saves
      // before moving (T8).
      if (dirty) {
        setLeavingTo(code);
        return;
      }
      setGroup(code);
    },
    [dirty],
  );

  const onSave = useCallback(
    async (then?: () => void) => {
      const list = Object.entries(edits).map(([key, value]) => ({ key, value }));
      if (list.length === 0) {
        then?.();
        return;
      }
      setSaving(true);
      try {
        const saved = await call('save_settings', { edits: list });
        setView(saved.settings);
        setEdits({});
        toast.show(
          'ok',
          saved.changed.length === 1
            ? `Saved. ${saved.changed[0]?.label} is now ${saved.changed[0]?.after}.`
            : `Saved ${saved.changed.length} settings.`,
        );
        then?.();
      } catch (cause) {
        if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
      } finally {
        setSaving(false);
      }
    },
    [edits, toast],
  );

  const onResetSection = useCallback(async () => {
    if (!active) return;
    try {
      const defaults = await call('settings_defaults_for', { group: active.code });
      // **Shown as unsaved edits, not written.** A reset a person cannot look
      // at and cancel is a reset that happens by accident.
      const wanted: Edits = { ...edits };
      let moved = 0;
      for (const setting of defaults) {
        const current = active.settings.find((s) => s.key === setting.key);
        const now = edits[setting.key] ?? current?.value ?? '';
        if (now !== setting.value) {
          wanted[setting.key] = setting.value;
          moved += 1;
        }
      }
      setEdits(wanted);
      toast.show(
        moved === 0 ? 'info' : 'warn',
        moved === 0
          ? 'Nothing in this section has been changed from standard.'
          : `${moved} setting${moved === 1 ? '' : 's'} put back to standard. Nothing is saved until you press Save.`,
      );
    } catch (cause) {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    }
  }, [active, edits, toast]);

  if (!view) {
    return (
      <div className="mb-settings">
        <Spinner label="Reading this shop's settings" />
      </div>
    );
  }

  // **No shop, no form** — found by running it. The configuration lives in
  // Rust and starts as the standard one, so on a machine whose database would
  // not open every setting drew perfectly and Save was the first thing to say
  // anything. That is audit A5's own situation, and a form somebody fills in
  // and cannot save is the worst thing to show them in it.
  if (!view.hasShop) {
    return (
      <div className="mb-settings mb-settings--empty">
        <EmptyState
          title="There is no shop to change the settings of"
          body={
            view.trouble ??
            'Create a shop or restore a backup, and these settings will be here.'
          }
        />
      </div>
    );
  }

  return (
    <div className="mb-settings">
      <nav className="mb-settings__rail" aria-label="Settings sections">
        <SearchField
          what="settings"
          value={query}
          placeholder="Search every setting"
          onChange={(event) => onSearch(event.currentTarget.value)}
        />
        {groups.map((section) => (
          <button
            key={section.code}
            type="button"
            className="mb-settings__section"
            aria-current={section.code === active?.code ? 'page' : undefined}
            onClick={() => go(section.code)}
          >
            <span>{section.label}</span>
            {section.canEdit ? null : (
              <span className="mb-settings__locked" title="You may look but not change this">
                read only
              </span>
            )}
          </button>
        ))}
      </nav>

      {/* **Back to the top when the section changes**, and this was a bug found
          by looking: the body kept the previous section's scroll position, so
          clicking "Your shop" after scrolling through the bill landed halfway
          down the shop's form with its heading off screen. */}
      <div className="mb-settings__body" ref={body}>
        {matches ? (
          <Found
            view={view}
            matches={matches}
            edits={edits}
            onChange={(key, value) => setEdits({ ...edits, [key]: value })}
            onClear={() => onSearch('')}
          />
        ) : active ? (
          <Section
            section={active}
            edits={edits}
            onChange={(key, value) => setEdits({ ...edits, [key]: value })}
            onReset={onResetSection}
          />
        ) : (
          <EmptyState
            title="Nothing here for you"
            body="You do not have permission to change any of this shop's settings."
          />
        )}
      </div>

      {/* **Its own row, spanning both columns**, and this was a bug found by
          looking: the screen is a two-column grid, so the save bar landed in
          the left cell under the section list, six words wide and six lines
          tall. Exactly P08's "every dialog was 64 px because a spacing token
          was used as a width" — a component that assumed it was a full-width
          row, put somewhere that is not one. */}
      <div className="mb-settings__save">
        <SaveBar
          dirty={dirty}
          saving={saving}
          note={`${Object.keys(edits).length} setting${
            Object.keys(edits).length === 1 ? '' : 's'
          } changed and not saved.`}
          onSave={() => void onSave()}
          onDiscard={() => setEdits({})}
        />
      </div>

      {/* T8 — Save / Discard / Cancel, and Save really saves first. */}
      <ConfirmDialog
        open={leavingTo !== null}
        title="You have unsaved changes"
        body="Save them before moving to another section?"
        confirmLabel="Save and move"
        cancelLabel="Stay here"
        onConfirm={() => {
          const to = leavingTo;
          setLeavingTo(null);
          void onSave(() => {
            if (to) setGroup(to);
          });
        }}
        onCancel={() => setLeavingTo(null)}
        otherLabel="Discard and move"
        onOther={() => {
          const to = leavingTo;
          setEdits({});
          setLeavingTo(null);
          if (to) setGroup(to);
        }}
      />
    </div>
  );
}

function Section({
  section,
  edits,
  onChange,
  onReset,
}: {
  section: GroupView;
  edits: Edits;
  onChange: (key: string, value: string) => void;
  onReset: () => void;
}) {
  return (
    <Card>
      <div className="mb-settings__head">
        <h2 className="mb-settings__title">{section.label}</h2>
        {section.canEdit ? (
          <Button small variant="quiet" onClick={onReset}>
            Put this section back to standard
          </Button>
        ) : null}
      </div>
      {/* **Sub-headings, and they were missing until somebody looked.** The
          bill is thirty-nine settings; one flat grid meant scrolling past
          twenty checkboxes to reach "Total size" with nothing to steer by.
          The heading text is Rust's, so this file decides nothing about it. */}
      {topicsOf(section).map(({ topic, settings }) => (
        <section key={topic} className="mb-settings__topic">
          {/* A setting with no heading of its own falls back to its section's
              name, which drew "YOUR SHOP" directly under the heading "Your
              shop" — a stutter, and found by looking at it. */}
          {topic === section.label ? null : (
            <h3 className="mb-settings__subtitle">{topic}</h3>
          )}
          <div className="mb-settings__fields">
            {settings.map((setting) => (
              <Field
                key={setting.key}
                setting={setting}
                value={edits[setting.key] ?? setting.value}
                changed={edits[setting.key] !== undefined}
                disabled={!section.canEdit}
                onChange={(value) => onChange(setting.key, value)}
              />
            ))}
          </div>
        </section>
      ))}
    </Card>
  );
}

/**
 * The section's settings, in runs of the same heading.
 *
 * **In catalogue order, and never sorted.** The order is a decision somebody
 * made in `catalog.rs` — a size next to its own bold tick, the QR mode next to
 * its width — and re-ordering here would quietly overrule it.
 */
function topicsOf(section: GroupView): { topic: string; settings: SettingView[] }[] {
  const runs: { topic: string; settings: SettingView[] }[] = [];
  for (const setting of section.settings) {
    const last = runs[runs.length - 1];
    if (last && last.topic === setting.topic) last.settings.push(setting);
    else runs.push({ topic: setting.topic, settings: [setting] });
  }
  return runs;
}

/** What a search found, across every section, with the section named. */
function Found({
  view,
  matches,
  edits,
  onChange,
  onClear,
}: {
  view: SettingsView;
  matches: readonly string[];
  edits: Edits;
  onChange: (key: string, value: string) => void;
  onClear: () => void;
}) {
  const found = useMemo(
    () =>
      view.groups.flatMap((group) =>
        group.settings
          .filter((setting) => matches.includes(setting.key))
          .map((setting) => ({ group, setting })),
      ),
    [view, matches],
  );

  if (found.length === 0) {
    return (
      <EmptyState
        title="Nothing matches that"
        body="Try a plainer word: QR, thank you, round off, backup."
        action={<Button onClick={onClear}>Show the sections again</Button>}
      />
    );
  }

  return (
    <Card>
      <div className="mb-settings__head">
        <h2 className="mb-settings__title">
          {found.length} setting{found.length === 1 ? '' : 's'} found
        </h2>
        <Button small variant="quiet" onClick={onClear}>
          Clear
        </Button>
      </div>
      <div className="mb-settings__fields">
        {found.map(({ group, setting }) => (
          <div key={setting.key} className="mb-settings__hit">
            <span className="mb-settings__where">{group.label}</span>
            <Field
              setting={setting}
              value={edits[setting.key] ?? setting.value}
              changed={edits[setting.key] !== undefined}
              disabled={!group.canEdit}
              onChange={(value) => onChange(setting.key, value)}
            />
          </div>
        ))}
      </div>
    </Card>
  );
}

/**
 * One setting, drawn as whatever it says it is.
 *
 * Five controls cover all ninety. There is deliberately no per-setting special
 * case here: the moment one appears, the thing it knows belongs in the
 * catalogue instead.
 */
function Field({
  setting,
  value,
  changed,
  disabled,
  onChange,
}: {
  setting: SettingView;
  value: string;
  changed: boolean;
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  const hint = setting.help === '' ? undefined : setting.help;
  const body = (() => {
    switch (setting.control) {
      case 'tick':
        return (
          <Checkbox
            label={setting.label}
            checked={value === '1'}
            disabled={disabled}
            onChange={(event) => onChange(event.currentTarget.checked ? '1' : '0')}
          />
        );
      case 'choice':
        return (
          <Select
            label={setting.label}
            hint={hint}
            value={value}
            disabled={disabled}
            options={setting.choices}
            onChange={(event) => onChange(event.currentTarget.value)}
          />
        );
      case 'number':
        return (
          <NumberInput
            label={setting.label}
            hint={
              setting.unit === ''
                ? hint
                : `${hint ? `${hint} ` : ''}${setting.min}–${setting.max} ${setting.unit}.`
            }
            value={value}
            disabled={disabled}
            onChange={(event) => onChange(event.currentTarget.value)}
          />
        );
      case 'amount':
        return (
          <NumberInput
            label={setting.label}
            hint={hint}
            value={value}
            disabled={disabled}
            onChange={(event) => onChange(event.currentTarget.value)}
          />
        );
      default:
        return (
          <Input
            label={setting.label}
            hint={hint}
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
      className={['mb-settings__field', changed ? 'mb-settings__field--changed' : '']
        .filter(Boolean)
        .join(' ')}
    >
      {body}
      {/* A tick has its label inside the control, so its help needs a home. */}
      {setting.control === 'tick' && hint ? (
        <span className="mb-field__hint">{hint}</span>
      ) : null}
      {changed ? (
        <span className="mb-settings__mark" aria-label="changed and not saved">
          not saved
        </span>
      ) : null}
    </div>
  );
}
