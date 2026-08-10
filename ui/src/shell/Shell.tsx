/**
 * The shell every screen from P09 to P29 lives inside.
 *
 * Four regions, and they never move (UI_GUIDELINES §4): a title bar we draw
 * ourselves (audit F7), a left rail, the screen, and — the piece audit **D4**
 * demands — a **persistent** print-queue indicator.
 *
 * # Adding a screen touches one file
 *
 * `SCREENS` below. A route is `{ id, label, icon, render }`; there is no
 * router, no barrel to update and no rail to edit. P09 adds a line. If that
 * ever stops being true, the shell is wrong and the next twenty sessions each
 * pay for it.
 */

import { useCallback, useEffect, useState, type ReactNode } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';

import { Button, Modal, useToast } from '../kit';
import { call, inApp, isUiError, subscribe } from '../ipc/call';
import type { AppStatus } from '../ipc/generated/AppStatus';
import type { LockState } from '../ipc/generated/LockState';
import type { PrintJobView } from '../ipc/generated/PrintJobView';
import { useTheme } from '../theme/ThemeProvider';
import { Account } from '../account/Account';
import { Billing } from '../billing/Billing';
import { Health } from '../health/Health';
import { Kitchen } from '../kitchen/Kitchen';
import { Gallery } from '../gallery/Gallery';
import { Lock } from '../auth/Lock';
import { Staff } from '../auth/Staff';
import { Audit } from '../auth/Audit';
import { Bills } from '../corrections/Bills';
import { Credit } from '../credit/Credit';
import { Expenses } from '../expenses/Expenses';
import { Floor } from '../floor/Floor';
import { Menu } from '../menu/Menu';
import { Reports } from '../reports/Reports';
import { Settings } from '../settings/Settings';

import './shell.css';
import '../auth/auth.css';

export interface Screen {
  id: string;
  label: string;
  icon: string;
  /**
   * `go` opens another screen — P22's set-up list and health panel both send
   * somebody to the screen that does the job, rather than being a seventh
   * editor (D102).
   */
  render: (go: (screen: string) => void) => ReactNode;
  /**
   * The permission this screen's commands check in Rust.
   *
   * **Hiding the rail item is a courtesy, not the control** — every command
   * behind it calls `guard::require`, and there is a Rust test that calls them
   * directly without permission. This only stops a cashier walking into a
   * screen that would refuse everything it tried to load.
   */
  needs?: string;
  /**
   * **Any one of these opens it** — P17's settings screen, which is four
   * permissions in a trench coat (the shop's details, tax, printers, backup).
   * A shop that gives one person the printers and another the tax rates is
   * doing the normal thing, and neither of them should find the rail item
   * missing. `guard::Access::NeedsAny` is the Rust half, and the sections a
   * person may not change arrive marked read-only rather than absent.
   */
  needsAny?: readonly string[];
}

/**
 * Every screen in the product.
 *
 * P09 adds `{ id: 'billing', label: 'Billing', icon: '₹', render: () => <Billing /> }`
 * and is finished. Lazily rendered — nothing that is not on screen is built,
 * which is budget S1 and scope 16.14.
 */
const SHIPPED_SCREENS: readonly Screen[] = [
  {
    id: "billing",
    label: "Billing",
    icon: "₹",
    // The set-up list lives on this screen and needs to be able to send
    // somebody to Settings or Menu — see `Setup` and D102.
    render: (go) => <Billing onGoTo={go} />,
  },
  {
    // The floor answers a different question from the billing grid: not
    // "which table am I putting this dosa on" but "which table needs me".
    // Audit F5 is the second one going unanswered.
    id: 'floor',
    label: 'Floor',
    icon: '▦',
    render: () => <Floor />,
  },
  {
    // Not "Khata" — the owner renamed it on 2026-08-08. The screen answers
    // "who owes me money", which is why that is its default view rather than
    // an alphabetical list nobody opens.
    id: 'credit',
    label: 'Credit',
    icon: '☰',
    render: () => <Credit />,
    needs: 'customers.manage',
  },
  {
    // "Spends", not "Expenses": the rail is read at a glance and the shorter
    // word is the one a shopkeeper uses.
    id: 'expenses',
    label: 'Spends',
    icon: '⌁',
    render: () => <Expenses />,
    needs: 'expenses.manage',
  },
  {
    id: 'bills',
    label: 'Bills',
    icon: '❐',
    render: () => <Bills />,
    needs: 'reports.view',
  },
  {
    // Directly under Bills, because the two answer the same person's
    // questions: "what did that customer pay?" and "how did the month go?"
    id: 'reports',
    label: 'Reports',
    icon: '◫',
    render: () => <Reports />,
    needs: 'reports.view',
  },
  {
    id: 'menu',
    label: 'Menu',
    icon: '≣',
    render: () => <Menu />,
    needs: 'menu.manage',
  },
  {
    id: 'staff',
    label: 'Staff',
    icon: '☺',
    render: () => <Staff />,
    needs: 'staff.manage',
  },
  {
    id: 'history',
    // Not "Audit". The owner must be able to answer "who voided that bill?"
    // without knowing our word for it (UI_GUIDELINES §6).
    label: 'History',
    icon: '☷',
    render: () => <Audit />,
    needs: 'audit.view',
  },
  {
    // Last but one, and below Menu: settings are what an owner opens once a
    // month, so they must not sit where a cashier's hand goes.
    id: 'settings',
    label: 'Settings',
    icon: '⚙',
    render: () => <Settings />,
    needsAny: ['settings.store', 'settings.tax', 'settings.printer', 'backup.run'],
  },
  {
    // Below Settings, because it is opened once a year — and above the Kit,
    // because the Kit is not a screen a shop has any use for.
    id: 'account',
    label: 'Account',
    icon: '◇',
    render: () => <Account />,
    needs: 'reports.view',
  },
  {
    // **P24.** On the counter it is a screen like any other, so a shop with one
    // machine can run the kitchen from it. On a wall tablet it is the whole
    // window — same page, same code.
    id: 'kitchen',
    label: 'Kitchen',
    icon: '◉',
    render: () => <Kitchen />,
    needs: 'bill.create',
  },
  {
    // Beside Account, because the two answer "is my counter all right?" from
    // the two directions an owner asks it.
    id: 'health',
    label: 'Health',
    icon: '✚',
    render: (go) => <Health onGoTo={go} />,
    needs: 'reports.view',
  },
];

/**
 * **The component gallery is not a screen a shop has any use for.**
 *
 * It was in the rail from P08 because there was nothing else to look at. A
 * released counter shows twelve rail items on a 1366x768 screen and this is the
 * one a shopkeeper would open once, by accident, and never understand — so it
 * goes where it belongs, which is a development build. P22.
 */
const SCREENS: readonly Screen[] = import.meta.env.DEV
  ? [
      ...SHIPPED_SCREENS,
      {
        id: 'gallery',
        label: 'Kit',
        icon: '◑',
        render: () => <Gallery />,
      },
    ]
  : SHIPPED_SCREENS;

export function Shell() {
  const [screen, setScreen] = useState<string>('billing');
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [jobs, setJobs] = useState<readonly PrintJobView[]>([]);
  const [queueOpen, setQueueOpen] = useState(false);
  const [lock, setLock] = useState<LockState | null>(null);
  const { theme, toggle } = useTheme();
  const toast = useToast();

  // What the app is, once. Not polled — see below.
  useEffect(() => {
    if (!inApp()) return;
    call('app_status')
      .then(setStatus)
      .catch(() => {
        /* The shell opens regardless; the status is a nicety. */
      });
  }, []);

  const reloadLock = useCallback(() => {
    if (!inApp()) return;
    call('lock_state')
      .then(setLock)
      .catch(() => {
        /* A shop that will not answer opens LOCKED — `state::open_or_lock`
           takes the same view, and locked is the safe direction to be wrong
           in. `lock` stays null, which renders the lock screen. */
      });
  }, []);

  useEffect(reloadLock, [reloadLock]);

  // **Rust pushes; React subscribes.** Budget M4, and PERFORMANCE.md §5 rule 6:
  // "a 250 ms poll loop is M4 gone before a single feature is written."
  //
  // The idle lock arrives this way too: the timer that decides it lives in
  // Rust (P11), because a React timer would be a poll AND would be bypassed by
  // any screen that is not open.
  useEffect(() => {
    if (!inApp()) return undefined;
    let stop: (() => void) | undefined;
    subscribe((message) => {
      if (message.kind === 'printQueue') setJobs(message.jobs);
      if (message.kind === 'session') reloadLock();
    })
      .then((unlisten) => {
        stop = unlisten;
      })
      .catch(() => undefined);
    return () => stop?.();
  }, [reloadLock]);

  // **Ctrl+L locks the counter.** Registered in the billing screen's SHORTCUTS
  // table as well, because the help sheet is generated from that table (audit
  // F4) and a key documented nowhere is a key nobody learns.
  useEffect(() => {
    if (!inApp()) return undefined;
    const onKey = (event: KeyboardEvent) => {
      if (event.ctrlKey && event.key.toLowerCase() === 'l') {
        event.preventDefault();
        call('lock_now').then(setLock).catch(() => undefined);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const needsAttention = jobs.some((job) => job.needsAttention);

  const onRetry = useCallback(
    async (id: string) => {
      try {
        await call('retry_print_job', { id });
        toast.show('info', 'Trying that print again.');
      } catch (cause) {
        if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
      }
    },
    [toast],
  );

  const onDismiss = useCallback(
    async (id: string) => {
      try {
        await call('dismiss_print_job', { id });
      } catch (cause) {
        if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
      }
    },
    [toast],
  );

  // Everything this person may open. A screen with no `needs` is everybody's.
  const held = lock?.permissions ?? [];
  const allowed = SCREENS.filter((item) => {
    if (item.needs && !held.includes(item.needs)) return false;
    if (item.needsAny && !item.needsAny.some((need) => held.includes(need))) return false;
    return true;
  });
  const active = allowed.find((s) => s.id === screen) ?? allowed[0];

  // **Locked = there is nobody signed in.** Not a flag: the same fact Rust
  // holds, asked for rather than mirrored.
  const locked = inApp() && lock !== null && lock.signedInAs === null;

  if (inApp() && lock === null) {
    // Before the first answer. Deliberately nothing rather than the billing
    // screen: a flash of somebody else's till is worse than a blank moment.
    return <div className="mb-shell" />;
  }

  return (
    <div className="mb-shell">
      <TitleBar
        shop={status?.shopPath ?? null}
        themeIcon={theme.icon}
        themeName={theme.name}
        onToggleTheme={toggle}
        jobs={jobs}
        needsAttention={needsAttention}
        onOpenQueue={() => setQueueOpen(true)}
        who={lock?.signedInAs ?? null}
        role={lock?.role ?? null}
        onLock={() => {
          call('lock_now').then(setLock).catch(() => undefined);
        }}
      />

      {/* Audit C1, on a shop that has not fixed it yet. NOT dismissible: a
          dismissed banner is a fixed bug that was never fixed. */}
      {lock?.nobodyHasAPin ? (
        <div className="mb-nopin" role="status">
          <strong>Anybody can open this shop&rsquo;s reports and settings.</strong>
          <span>Add a PIN in Staff so the counter locks itself.</span>
        </div>
      ) : null}

      <div className="mb-body">
        <nav className="mb-rail" aria-label="Screens">
          {allowed.map((item) => (
            <button
              key={item.id}
              type="button"
              className="mb-rail__item"
              aria-current={item.id === screen ? 'page' : undefined}
              onClick={() => setScreen(item.id)}
            >
              {/* Icon AND label — §5: bare icons are hostile to a new cashier. */}
              <span className="mb-rail__icon" aria-hidden="true">
                {item.icon}
              </span>
              <span>{item.label}</span>
            </button>
          ))}
          <span className="mb-rail__spacer" />
        </nav>

        <main className="mb-main">
          {/*
            **The licence banner** (P21). A quiet line above the screen, never a
            modal — a dialog a cashier has to dismiss before every bill is how a
            licence system stops a restaurant trading without meaning to.

            It rides on `app_status`, which is `Access::Public`, because the
            person who needs to know the plan ran out is whoever is standing at
            the counter. Every sentence in it ends by saying what still works.

            **Not on the Account screen**, found by looking: that screen shows
            the same sentence in its own first card, so the banner made it
            appear twice, four centimetres apart.
          */}
          {status?.licence && screen !== 'account' ? (
            <div
              className={`mb-licence-note mb-licence-note--${status.licenceTone}`}
              role="status"
            >
              <span>{status.licence}</span>
              <button
                type="button"
                className="mb-licence-note__go"
                onClick={() => setScreen('account')}
              >
                Open Account
              </button>
            </div>
          ) : null}
          {active?.render(setScreen)}
        </main>
      </div>

      <PrintQueuePanel
        open={queueOpen}
        jobs={jobs}
        onClose={() => setQueueOpen(false)}
        onRetry={onRetry}
        onDismiss={onDismiss}
      />

      {/* Over everything, including the print queue panel and any toast — a
          toast floating above a locked screen is information leaking past the
          lock. The queue INDICATOR stays visible in the title bar, which is
          audit D4: paper coming out wrong is still the shop's problem while
          the screen is locked. */}
      {locked ? (
        <Lock
          people={lock?.people ?? []}
          canRecover={lock?.canRecover ?? false}
          onSignedIn={reloadLock}
        />
      ) : null}
    </div>
  );
}

function TitleBar({
  shop,
  themeIcon,
  themeName,
  onToggleTheme,
  jobs,
  needsAttention,
  onOpenQueue,
  who,
  role,
  onLock,
}: {
  shop: string | null;
  themeIcon: string;
  themeName: string;
  onToggleTheme: () => void;
  jobs: readonly PrintJobView[];
  needsAttention: boolean;
  onOpenQueue: () => void;
  who: string | null;
  role: string | null;
  onLock: () => void;
}) {
  const window = inApp() ? getCurrentWindow() : null;
  const face = themeIcon === 'moon' ? '☾' : themeIcon === 'contrast' ? '◐' : '☀';

  return (
    <header className="mb-titlebar" data-tauri-drag-region>
      <span className="mb-titlebar__name" data-tauri-drag-region>
        Magic Bill
      </span>
      {shop ? (
        <span className="mb-titlebar__name" data-tauri-drag-region>
          · {shop}
        </span>
      ) : null}
      <span className="mb-titlebar__spacer" data-tauri-drag-region />

      <div className="mb-titlebar__tools">
        {/* Whose till this is, right now. Audit C3's other half: the name on
            the bill and the name on the screen are one fact. */}
        {who ? (
          <span className="mb-who">
            <span className="mb-who__name">{who}</span>
            {role ? <span>{role}</span> : null}
          </span>
        ) : null}
        {who ? (
          <button
            type="button"
            className="mb-titlebar__button"
            onClick={onLock}
            aria-label="Lock the counter (Ctrl+L)"
            title="Lock the counter — Ctrl+L"
          >
            <span aria-hidden="true">⌧</span>
          </button>
        ) : null}

        {/* The print queue. PERSISTENT — audit D4: a toast that has faded is
            not "the cashier can see it". */}
        <button
          type="button"
          className={[
            'mb-queue',
            needsAttention ? 'mb-queue--attention' : '',
          ]
            .filter(Boolean)
            .join(' ')}
          onClick={onOpenQueue}
          aria-label={
            needsAttention
              ? 'A print did not come out — open the print queue'
              : 'Print queue'
          }
        >
          <span aria-hidden="true">🖨</span>
          <span>
            {needsAttention
              ? 'NOT PRINTED'
              : jobs.length > 0
                ? `${jobs.length} printing`
                : 'Printing'}
          </span>
        </button>

        {/* The sun/moon toggle the owner asked for by name. */}
        <button
          type="button"
          className="mb-titlebar__button"
          onClick={onToggleTheme}
          aria-label={`Theme: ${themeName}. Switch.`}
          title={`Theme: ${themeName}`}
        >
          <span aria-hidden="true">{face}</span>
        </button>

        <button
          type="button"
          className="mb-titlebar__button"
          onClick={() => window?.minimize()}
          aria-label="Minimise"
        >
          –
        </button>
        <button
          type="button"
          className="mb-titlebar__button"
          onClick={() => window?.toggleMaximize()}
          aria-label="Maximise"
        >
          □
        </button>
        <button
          type="button"
          className="mb-titlebar__button mb-titlebar__button--close"
          onClick={() => window?.close()}
          aria-label="Close"
        >
          ✕
        </button>
      </div>
    </header>
  );
}

function PrintQueuePanel({
  open,
  jobs,
  onClose,
  onRetry,
  onDismiss,
}: {
  open: boolean;
  jobs: readonly PrintJobView[];
  onClose: () => void;
  onRetry: (id: string) => void;
  onDismiss: (id: string) => void;
}) {
  return (
    <Modal open={open} title="Printing" onClose={onClose} wide>
      <div className="mb-queue__panel">
        {jobs.length === 0 ? (
          <span className="mb-muted">Everything has printed.</span>
        ) : (
          jobs.map((job) => (
            <div
              key={job.id}
              className={[
                'mb-queue__job',
                job.needsAttention ? 'mb-queue__job--attention' : '',
              ]
                .filter(Boolean)
                .join(' ')}
            >
              <div className="mb-stack">
                <span className="mb-queue__what">
                  {job.what}
                  {job.reason ? ` — ${job.reason}` : ''}
                </span>
                <span className="mb-queue__where">
                  {job.printer} · {job.state}
                </span>
                {job.lastError ? (
                  <span className="mb-field__hint">{job.lastError}</span>
                ) : null}
              </div>
              {job.needsAttention ? (
                <div className="mb-row">
                  <Button small onClick={() => onRetry(job.id)}>
                    Try again
                  </Button>
                  <Button small variant="quiet" onClick={() => onDismiss(job.id)}>
                    Give up
                  </Button>
                </div>
              ) : null}
            </div>
          ))
        )}
      </div>
    </Modal>
  );
}
