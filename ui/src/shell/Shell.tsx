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
import type { PrintJobView } from '../ipc/generated/PrintJobView';
import { useTheme } from '../theme/ThemeProvider';
import { Billing } from '../billing/Billing';
import { Gallery } from '../gallery/Gallery';

import './shell.css';

export interface Screen {
  id: string;
  label: string;
  icon: string;
  render: () => ReactNode;
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

  // **Rust pushes; React subscribes.** Budget M4, and PERFORMANCE.md §5 rule 6:
  // "a 250 ms poll loop is M4 gone before a single feature is written."
  useEffect(() => {
    if (!inApp()) return undefined;
    let stop: (() => void) | undefined;
    subscribe((message) => {
      if (message.kind === 'printQueue') setJobs(message.jobs);
    })
      .then((unlisten) => {
        stop = unlisten;
      })
      .catch(() => undefined);
    return () => stop?.();
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

  const active = SCREENS.find((s) => s.id === screen) ?? SCREENS[0];

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
      />

      <div className="mb-body">
        <nav className="mb-rail" aria-label="Screens">
          {SCREENS.map((item) => (
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
}: {
  shop: string | null;
  themeIcon: string;
  themeName: string;
  onToggleTheme: () => void;
  jobs: readonly PrintJobView[];
  needsAttention: boolean;
  onOpenQueue: () => void;
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
