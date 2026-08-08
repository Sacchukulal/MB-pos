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
import { Billing } from '../billing/Billing';
import { Gallery } from '../gallery/Gallery';
import { Lock } from '../auth/Lock';
import { Staff } from '../auth/Staff';
import { Audit } from '../auth/Audit';
import { Bills } from '../corrections/Bills';
import { Menu } from '../menu/Menu';

import './shell.css';
import '../auth/auth.css';

export interface Screen {
  id: string;
  label: string;
  icon: string;
  render: () => ReactNode;
  /**
   * The permission this screen's commands check in Rust.
   *
   * **Hiding the rail item is a courtesy, not the control** — every command
   * behind it calls `guard::require`, and there is a Rust test that calls them
   * directly without permission. This only stops a cashier walking into a
   * screen that would refuse everything it tried to load.
   */
  needs?: string;
}

/**
 * Every screen in the product.
 *
 * P09 adds `{ id: 'billing', label: 'Billing', icon: '₹', render: () => <Billing /> }`
 * and is finished. Lazily rendered — nothing that is not on screen is built,
 * which is budget S1 and scope 16.14.
 */
const SCREENS: readonly Screen[] = [
  {
    id: "billing",
    label: "Billing",
    icon: "₹",
    render: () => <Billing />,
  },
  {
    id: 'bills',
    label: 'Bills',
    icon: '❐',
    render: () => <Bills />,
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
    id: 'gallery',
    label: 'Kit',
    icon: '◑',
    render: () => <Gallery />,
  },
];

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
  const allowed = SCREENS.filter(
    (item) => !item.needs || (lock?.permissions ?? []).includes(item.needs),
  );
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

        <main className="mb-main">{active?.render()}</main>
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
