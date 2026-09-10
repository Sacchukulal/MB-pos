/** The settings screen, and there is only one of it. */

import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';

import {
  Button,
  Card,
  cx,
  ConfirmDialog,
  EmptyState,
  InfoTip,
  Panel,
  plural,
  Rail,
  RailItem,
  SaveBar,
  Scroller,
  SectionHeader,
  SearchField,
  Spinner,
  useToast,
} from '../kit';
import { call, inApp, isUiError } from '../ipc/call';
import type { PreviewView } from '../ipc/generated/PreviewView';
import type { SettingsView } from '../ipc/generated/SettingsView';
import { Receipt } from '../preview/Receipt';
import { Appearance } from './Appearance';
import { Devices } from './Devices';
import { Field, Section, editsList, withEdit, type Edits } from './Form';
import { Numbering } from './Numbering';
import { Printers } from './Printers';
import { Tax } from './Tax';
import { Tills } from './Tills';

/** Sections that carry a screen as well as (or instead of) a form. */
const OWN_SCREEN: Record<string, () => ReactNode> = {
  printers: () => <Printers />,
  tax: () => <Tax />,
  numbering: () => <Numbering />,
  appearance: () => <Appearance />,
  tills: () => <Tills />,
  // The list of what is plugged in, under the settings that drive each device.
  devices: () => <Devices />,
};

/** Sections whose own screen draws their settings, so the generic form stays away. */
const DRAWS_OWN_SETTINGS = new Set(['tax']);

/** Sections that are not settings. */
const EXTRA_SECTIONS = [
  // A till is a row, not a setting.
  { code: 'tills', label: 'Tills', canEdit: true, settings: [] },
];

/** Which sections show the paper beside them. */
const SHOWS_PAPER = new Set(['receipt', 'kitchen']);

/** `initial` opens a section straight away — Billing's printer button lands on Printers. */
export function Settings({ initial }: { initial?: string | null } = {}) {
  const [view, setView] = useState<SettingsView | null>(null);
  const [group, setGroup] = useState<string>(initial ?? 'store');
  useEffect(() => {
    if (initial) setGroup(initial);
  }, [initial]);
  const [edits, setEdits] = useState<Edits>({});
  const [saving, setSaving] = useState(false);
  const [query, setQuery] = useState('');
  const [matches, setMatches] = useState<readonly string[] | null>(null);
  /** Where the unsaved-changes guard was heading when it stopped us. */
  const [leavingTo, setLeavingTo] = useState<string | null>(null);
  const [paper, setPaper] = useState<PreviewView | null>(null);
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
    // `toast` is stable for the life of the provider.
  }, [toast]);

  const dirty = Object.keys(edits).length > 0;

  /** What is saved for a key right now — what an edit is measured against. */
  const savedValue = useCallback(
    (key: string) => view?.groups.flatMap((g) => g.settings).find((s) => s.key === key)?.value,
    [view],
  );

  const onChange = useCallback(
    (key: string, value: string) => {
      setEdits((was) => withEdit(was, key, value, savedValue(key)));
    },
    [savedValue],
  );

  // Search is Rust's — the synonym list is part of the rule.
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

  const groups = [...(view?.groups ?? []), ...EXTRA_SECTIONS];
  const active = groups.find((g) => g.code === group) ?? groups[0];
  const showsPaper = active !== undefined && SHOWS_PAPER.has(active.code) && matches === null;

  // The live preview: every edit, saved or not, is on the paper as soon as Rust has drawn it.
  useEffect(() => {
    if (!inApp() || !showsPaper || !active) return;
    const list = editsList(edits);
    let stale = false;
    call('preview_settings', { group: active.code, edits: list })
      .then((next) => {
        if (!stale) setPaper(next);
      })
      .catch(() => {
        // A preview that will not draw must not take the screen down with it.
        if (!stale) setPaper(null);
      });
    return () => {
      stale = true;
    };
  }, [active, edits, showsPaper]);

  const go = useCallback(
    (code: string) => {
      // The guard, and it is the whole of it.
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
      const list = editsList(edits);
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
      // Shown as unsaved edits, not written.
      let wanted: Edits = { ...edits };
      let moved = 0;
      for (const setting of defaults) {
        const current = active.settings.find((s) => s.key === setting.key);
        const now = edits[setting.key] ?? current?.value ?? '';
        if (now !== setting.value) moved += 1;
        // Standard may be what is already saved — then this drops the edit rather than adding
        // one that saves nothing.
        wanted = withEdit(wanted, setting.key, setting.value, current?.value);
      }
      setEdits(wanted);
      toast.show(
        moved === 0 ? 'info' : 'warn',
        moved === 0
          ? 'Nothing in this section has been changed from standard.'
          : `${moved} setting${moved === 1 ? '' : 's'} reset. Nothing is saved until you press Save.`,
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

  // No shop, no form — found by running it.
  if (!view.hasShop) {
    return (
      <div className="mb-settings mb-settings--empty">
        <EmptyState
          title="There is no shop to change the settings of"
          says={
            view.trouble ??
            'Create a shop or restore a backup, and these settings will be here.'
          }
        />
      </div>
    );
  }

  const hasForm =
    active !== undefined && active.settings.length > 0 && !DRAWS_OWN_SETTINGS.has(active.code);

  return (
    <div className="mb-settings">
      <div className="mb-settings__rail">
        <SearchField
          what="settings"
          value={query}
          placeholder="Search every setting"
          onChange={(event) => onSearch(event.currentTarget.value)}
        />
        {/* Only the sections scroll. */}
        <Scroller inset className="mb-settings__sections">
          <Rail label="Settings sections">
            {groups.map((section) => (
              <RailItem
                key={section.code}
                current={section.code === active?.code}
                onClick={() => go(section.code)}
                end={section.canEdit ? null : 'read only'}
              >
                {section.label}
              </RailItem>
            ))}
          </Rail>
        </Scroller>
      </div>

      {/* Two columns, two scrollbars. */}
      <div className={cx('mb-settings__panes', showsPaper && 'mb-settings__panes--paper')}>
        {/* Back to the top when the section changes: the body would keep the last scroll. */}
        <Scroller className="mb-settings__body" ref={body}>
          {matches ? (
            <Found
              view={view}
              matches={matches}
              edits={edits}
              onChange={onChange}
              onClear={() => onSearch('')}
            />
          ) : active ? (
            /*
             * ONE bordered panel for the whole section, named once at its head. Every card
             * inside it is a group parted from the next by a hairline, which is what the kit
             * does with a card in a panel — so the section reads as one form, not as a stack
             * of boxes.
             */
            <Panel
              className="mb-settings__form"
              title={active.label}
              actions={
                hasForm && active.canEdit ? (
                  <Button size="sm" variant="quiet" onClick={onResetSection}>
                    Reset this section
                  </Button>
                ) : null
              }
            >
              <div className="mb-sections">
                {hasForm ? <Section section={active} edits={edits} onChange={onChange} /> : null}
                {OWN_SCREEN[active.code]?.()}
              </div>
            </Panel>
          ) : (
            <EmptyState
              title="Nothing here for you"
              hint="You do not have permission to change any of this shop's settings."
            />
          )}
        </Scroller>

        {showsPaper ? <Paper preview={paper} kitchen={active?.code === 'kitchen'} /> : null}
      </div>

      {/* Its own row, spanning both columns: the screen is a two-column grid. */}
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

      {/* Save / Discard / Cancel, and Save really saves first. */}
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

/**
 * The paper, beside the settings that change it. Its width is the column's, and the roll is
 * drawn as wide as that column allows; the paper size and the typeface are facts here, set on
 * Printers and on The bill.
 */
function Paper({ preview, kitchen }: { preview: PreviewView | null; kitchen: boolean }) {
  return (
    <aside className="mb-settings__paper" aria-label="Preview of what prints">
      <div className="mb-settings__paperhead">
        <span className="mb-settings__papertitle">
          {kitchen ? 'The kitchen ticket' : 'The bill'}
        </span>
        {preview ? (
          <span className="mb-settings__papernote">
            {preview.paper} · {preview.face}
            <InfoTip label="About the paper and the typeface">
              The paper is set on Printers. The typeface is set on The bill.
            </InfoTip>
          </span>
        ) : null}
      </div>

      {/* The roll's own scrollbar. */}
      <Scroller inset className="mb-settings__paperroll">
        {preview ? (
          <>
            {/* The printer's own raster, dot for dot. */}
            <Receipt doc={preview.doc} />
            {/*
              Half-typed is a normal state, and saying which box is not usable yet beats
              blanking the paper or shouting on every keystroke.
            */}
            {preview.notUsableYet.length > 0 ? (
              <p className="mb-settings__papernote">
                Not used yet: {preview.notUsableYet.join(', ')}.
              </p>
            ) : null}
          </>
        ) : (
          <Spinner label="Drawing the sample" />
        )}
      </Scroller>
    </aside>
  );
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
        hint="Try a plainer word: QR, thank you, round off, backup."
        action={<Button onClick={onClear}>Show the sections again</Button>}
      />
    );
  }

  return (
    <Card>
      <SectionHeader
        title={`${plural(found.length, 'setting')} found`}
        sticky
        action={
          <Button size="sm" variant="quiet" onClick={onClear}>
            Clear
          </Button>
        }
      />
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
