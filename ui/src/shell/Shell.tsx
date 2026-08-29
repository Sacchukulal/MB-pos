import { useCallback, useEffect, useState, type ReactNode } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';

import { Button, Icon, Logo, Modal, plural, useToast, type IconName } from '../kit';
import { call, inApp, isUiError, subscribe } from '../ipc/call';
import type { AppStatus } from '../ipc/generated/AppStatus';
import type { LockState } from '../ipc/generated/LockState';
import type { SetupView } from '../ipc/generated/SetupView';
import type { PrintJobView } from '../ipc/generated/PrintJobView';
import type { NoticesView } from '../ipc/generated/NoticesView';
import type { PhonesView } from '../ipc/generated/PhonesView';
import { useTheme } from '../theme/ThemeProvider';
import { Account } from '../account/Account';
import { FirstRun } from '../setup/FirstRun';
import { AlertsPanel, loudest, type Alert } from './Alerts';
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
import { Stock } from '../stock/Stock';
import { Buying } from '../buying/Buying';
import { Floor } from '../floor/Floor';
import { Delivery } from '../delivery/Delivery';
import { Devices } from '../devices/Devices';
import { Menu } from '../menu/Menu';
import { Reports } from '../reports/Reports';
import { Settings } from '../settings/Settings';

import './shell.css';
import '../auth/auth.css';

const WORST: Record<Alert['tone'], number> = { danger: 3, warn: 2, accent: 1, info: 0 };

export interface Screen {
  id: string;
  label: string;
  /** A name from the kit's icon set, not a character. */
  icon: IconName;
  /** True for a screen the counter uses every day. */
  daily?: boolean;
  /**
   * `go` opens another screen — `go('settings/network')` opens Settings on the Phones section;
   * `sub` is that part, for the screen that was asked for it.
   */
  render: (go: (screen: string) => void, sub?: string | null) => ReactNode;
  /** The permission this screen's commands check in Rust. */
  needs?: string;
  /** Any one of these opens it. */
  needsAny?: readonly string[];
}

/** Every screen in the product. */
export const SHIPPED_SCREENS: readonly Screen[] = [
  {
    id: "billing",
    daily: true,
    label: "Billing",
    icon: 'receipt',
    render: () => <Billing />,
  },
  {
    // The floor answers a different question from the billing grid: not "which table am I
    // putting this dosa on" but "which table needs me".
    id: 'floor',
    daily: true,
    label: 'Floor',
    icon: 'grid',
    render: () => <Floor />,
  },
  {
    id: 'credit',
    daily: true,
    label: 'Credit',
    icon: 'wallet',
    render: () => <Credit />,
    needs: 'customers.manage',
  },
  {
    // "Spends", not "Expenses": the rail is read at a glance and the shorter word is the one a
    // shopkeeper uses.
    id: 'expenses',
    daily: true,
    label: 'Spends',
    icon: 'banknote',
    render: () => <Expenses />,
    needs: 'expenses.manage',
  },
  {
    // Next to Spends, because they are the same question from two sides: what left as money,
    // and what left as food.
    id: 'stock',
    label: 'Stock',
    icon: 'boxes',
    render: (go) => <Stock onGoTo={go} />,
    needs: 'inventory.view',
  },
  {
    // Next to Stock, because they are the same shelf from two ends: what came in, and what is
    // on it.
    id: 'buying',
    label: 'Buying',
    icon: 'truck',
    render: () => <Buying />,
    needs: 'purchases.manage',
  },
  {
    // Beside the floor, because it is the same question asked about the food that has left the
    // building: which orders are still out, and who is carrying the cash for them.
    id: 'delivery',
    label: 'Delivery',
    icon: 'bike',
    render: () => <Delivery />,
  },
  {
    id: 'bills',
    daily: true,
    label: 'Bills',
    icon: 'file',
    render: () => <Bills />,
    needs: 'reports.view',
  },
  {
    // Directly under Bills, because the two answer the same person's questions: "what did that
    // customer pay?" and "how did the month go?".
    id: 'reports',
    daily: true,
    label: 'Reports',
    icon: 'chart',
    // `go` so a licence refusal can hand somebody straight to the Account screen instead of
    // leaving them to find it.
    render: (go) => <Reports onGoTo={go} />,
    needs: 'reports.view',
  },
  {
    id: 'menu',
    label: 'Menu',
    icon: 'book',
    render: () => <Menu />,
    needs: 'menu.manage',
  },
  {
    id: 'staff',
    label: 'Staff',
    icon: 'users',
    render: () => <Staff />,
    needs: 'staff.manage',
  },
  {
    id: 'history',
    label: 'History',
    icon: 'clock',
    render: () => <Audit />,
    needs: 'audit.view',
  },
  {
    // Last but one, and below Menu: settings are what an owner opens once a month, so they must
    // not sit where a cashier's hand goes.
    id: 'settings',
    label: 'Settings',
    icon: 'settings',
    render: (_go, sub) => <Settings initial={sub} />,
    needsAny: ['settings.store', 'settings.tax', 'settings.printer', 'backup.run'],
  },
  {
    // Below Settings, because it is opened once a year — and above the Kit, because the Kit is
    // not a screen a shop has any use for.
    id: 'account',
    label: 'Account',
    icon: 'badge',
    render: () => <Account />,
    needs: 'reports.view',
  },
  {
    // On the counter it is a screen like any other, so a shop with one machine can run the
    // kitchen from it.
    id: 'kitchen',
    label: 'Kitchen',
    icon: 'flame',
    render: () => <Kitchen />,
    needs: 'bill.create',
  },
  {
    // Beside Health, because they are the same question about the two halves of one counter: is
    // the software all right, and is the hardware.
    id: 'devices',
    label: 'Devices',
    icon: 'plug',
    render: () => <Devices />,
    needs: 'settings.printer',
  },
  {
    // Beside Account, because the two answer "is my counter all right?" from the two directions
    // an owner asks it.
    id: 'health',
    label: 'Health',
    icon: 'pulse',
    render: (go) => <Health onGoTo={go} />,
    needs: 'reports.view',
  },
];

/** The component gallery is not a screen a shop has any use for. */
const SCREENS: readonly Screen[] = import.meta.env.DEV
  ? [
      ...SHIPPED_SCREENS,
      {
        id: 'gallery',
        label: 'Kit',
        icon: 'tag',
        render: () => <Gallery />,
      },
    ]
  : SHIPPED_SCREENS;

export function Shell() {
  const [screen, setScreenOnly] = useState<string>('billing');
  /** The part of a screen that was asked for: `settings/network` → `network`. */
  const [sub, setSub] = useState<string | null>(null);
  const setScreen = useCallback((id: string) => {
    const slash = id.indexOf('/');
    setSub(slash < 0 ? null : id.slice(slash + 1));
    setScreenOnly(slash < 0 ? id : id.slice(0, slash));
  }, []);
  /** The phones: live now, and asking to join. */
  const [phones, setPhones] = useState<PhonesView>({ connected: 0, waiting: 0 });
  const [status, setStatus] = useState<AppStatus | null>(null);
  /** Is this counter set up? */
  const [setUp, setSetUp] = useState<boolean | null>(null);
  const [jobs, setJobs] = useState<readonly PrintJobView[]>([]);
  const [queueOpen, setQueueOpen] = useState(false);
  const [lock, setLock] = useState<LockState | null>(null);
  /**
   * What this till is holding for the main one — the whole sentence, written in Rust, empty
   * when there is nothing to say.
   */
  const [tillsSay, setTillsSay] = useState('');
  /** What the set-up list still wants. */
  const [setup, setSetup] = useState<SetupView | null>(null);
  const [alertsOpen, setAlertsOpen] = useState(false);
  /** Notices from Magic Bill, and how many are unread — the bell's other half. */
  const [notices, setNotices] = useState<NoticesView>({ unseen: 0, notices: [] });
  const { theme, toggle } = useTheme();
  const toast = useToast();

  // What the app is, once.
  useEffect(() => {
    if (!inApp()) return;
    call('app_status')
      .then(setStatus)
      .catch(() => {
        /* The shell opens regardless; the status is a nicety. */
      });
  }, []);

  /** The set-up steps, re-read whenever the counter is signed into or the screen changes. */
  useEffect(() => {
    if (!inApp() || lock === null || lock.signedInAs === null) return;
    call('setup_list')
      .then(setSetup)
      .catch(() => {
        /* A counter that cannot say what is left still bills. */
      });
  }, [lock, screen]);

  /** Is this counter set up? */
  useEffect(() => {
    if (!inApp()) return;
    call('first_run')
      .then((first) => setSetUp(!first.needed))
      .catch(() => setSetUp(true));
  }, []);

  const reloadLock = useCallback(() => {
    if (!inApp()) return;
    call('lock_state')
      .then(setLock)
      .catch(() => {
        /*
         * A shop that will not answer opens LOCKED — `state::open_or_lock` takes the same view,
         * and locked is the safe direction to be wrong in.
         */
      });
  }, []);

  useEffect(reloadLock, [reloadLock]);

  /** What Magic Bill has to say, once signed in and again whenever Rust says it changed. */
  const reloadNotices = useCallback(() => {
    if (!inApp()) return;
    call('notices')
      // Checked, not trusted — the same rule the print queue follows.
      .then((fresh) => {
        if (fresh && Array.isArray(fresh.notices)) setNotices(fresh);
      })
      // Locked, or no shop: the bell simply has nothing from the cloud yet.
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    if (lock === null || lock.signedInAs === null) return;
    reloadNotices();
    // The phones' number, once; Rust pushes every change after that.
    if (inApp()) call('phones_now').then(setPhones).catch(() => undefined);
  }, [lock, reloadNotices]);

  // Rust pushes; React subscribes.
  useEffect(() => {
    if (!inApp()) return undefined;
    let stop: (() => void) | undefined;
    subscribe((message) => {
      if (message.kind === 'printQueue') setJobs(message.jobs);
      if (message.kind === 'session') reloadLock();
      if (message.kind === 'tills') setTillsSay(message.says);
      if (message.kind === 'notices') reloadNotices();
      if (message.kind === 'phones') setPhones({ connected: message.connected, waiting: message.waiting });
      if (message.kind === 'licence') {
        setStatus((was) => (was ? { ...was, licence: message.says, licenceTone: message.tone } : was));
      }
    })
      .then((unlisten) => {
        stop = unlisten;
      })
      .catch(() => undefined);
    return () => stop?.();
  }, [reloadLock, reloadNotices]);

  /** Opening the bell: fetch what is new, then mark everything read. */
  const openAlerts = useCallback(() => {
    setAlertsOpen(true);
    if (!inApp()) return;
    call('pull_from_cloud')
      .catch(() => undefined)
      .then(() => call('notices_seen'))
      .then((fresh) => {
        if (fresh && Array.isArray(fresh.notices)) setNotices(fresh);
      })
      .catch(() => undefined);
  }, []);

  /** The queue as it is right now, once, when the shell mounts. */
  useEffect(() => {
    if (!inApp()) return;
    call('list_print_jobs')
      // Checked, not trusted. The queue is the one thing on this screen that is allowed not to
      // answer — a shop mid-restore has no queue at all — and an empty list is the honest
      // reading of that.
      .then((fresh) => setJobs(Array.isArray(fresh) ? fresh : []))
      // Silent: a queue that will not read must not put an error over a counter somebody is
      // billing on.
      .catch(() => undefined);
  }, []);

  // Ctrl+L locks the counter.
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

  /** One press for every job that did not print. */
  const onRetryAll = useCallback(async () => {
    try {
      const put = await call('retry_parked_print_jobs');
      toast.show('info', `Trying ${plural(put, 'print')} again.`);
    } catch (cause) {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    }
  }, [toast]);

  const onDismissAll = useCallback(async () => {
    try {
      await call('dismiss_all_print_jobs');
    } catch (cause) {
      if (isUiError(cause)) toast.show('danger', cause.message, cause.detail ?? undefined);
    }
  }, [toast]);

  // Everything this person may open.
  const held = lock?.permissions ?? [];
  const allowed = SCREENS.filter((item) => {
    if (item.id === 'kitchen' && !status?.kitchenScreen) return false;
    if (item.needs && !held.includes(item.needs)) return false;
    if (item.needsAny && !item.needsAny.some((need) => held.includes(need))) return false;
    return true;
  });
  const active = allowed.find((s) => s.id === screen) ?? allowed[0];

  // Locked = there is nobody signed in.
  const locked = inApp() && lock !== null && lock.signedInAs === null;

  /** Everything the shop should know, in one list. */
  const alerts: Alert[] = [];
  if (status?.licence) {
    alerts.push({
      id: 'licence',
      tone:
        status.licenceTone === 'danger'
          ? 'danger'
          : status.licenceTone === 'warn'
            ? 'warn'
            : 'info',
      icon: 'badge',
      title: 'Your licence',
      says: status.licence,
      goTo: 'account',
      goLabel: 'Open Account',
    });
  }
  if (lock?.nobodyHasAPin) {
    alerts.push({
      id: 'no-pin',
      tone: 'warn',
      icon: 'lock',
      title: 'Anybody can open your reports and settings',
      says: 'Add a PIN in Staff so the counter locks itself.',
      goTo: 'staff',
      goLabel: 'Open Staff',
    });
  }
  if (tillsSay) {
    alerts.push({
      id: 'tills',
      tone: 'accent',
      icon: 'refresh',
      title: 'Your other till',
      says: tillsSay,
    });
  }
  for (const step of setup?.steps ?? []) {
    if (step.done) continue;
    alerts.push({
      id: `setup-${step.id}`,
      tone: 'info',
      icon: 'info',
      title: step.title,
      says: step.why,
      goTo: step.goTo,
    });
  }
  alerts.sort((a, b) => WORST[b.tone] - WORST[a.tone]);

  if (inApp() && lock === null) {
    // Before the first answer.
    return <div className="mb-shell" />;
  }

  // Nothing renders until we know whether there is a shop.
  if (inApp() && setUp === null) {
    return <div className="mb-shell" />;
  }

  // A shop that is not set up does not show the counter at all.
  if (inApp() && setUp === false) {
    return (
      <div className="mb-shell">
        <BareBar />
        <FirstRun
          onDone={() => {
            setSetUp(true);
            // Everything the shell holds was read against a shop that did not exist a minute
            // ago.
            call('app_status').then(setStatus).catch(() => undefined);
            reloadLock();
          }}
        />
      </div>
    );
  }

  return (
    <div className="mb-shell">
      <TopBar
        shopPath={status?.shopPath ?? null}
        screens={allowed}
        current={screen}
        onGo={setScreen}
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
        alertCount={alerts.length + notices.unseen}
        alertTone={loudest(alerts) ?? (notices.unseen > 0 ? 'accent' : null)}
        onOpenAlerts={openAlerts}
        phones={phones}
        onOpenPhones={() => setScreen('settings/network')}
      />

      <div className="mb-body">
        <main className="mb-main">
          {/* Nothing is rendered behind the lock. */}
          {locked ? null : active?.render(setScreen, active.id === screen ? sub : null)}
        </main>
      </div>

      {alertsOpen ? (
        <AlertsPanel
          alerts={alerts}
          notices={notices.notices}
          onGo={setScreen}
          onClose={() => setAlertsOpen(false)}
        />
      ) : null}

      <PrintQueuePanel
        open={queueOpen}
        jobs={jobs}
        onClose={() => setQueueOpen(false)}
        onRetry={onRetry}
        onDismiss={onDismiss}
        onRetryAll={onRetryAll}
        onDismissAll={onDismissAll}
      />

      {/*
        Over everything, including the print queue panel and any toast — a toast floating above
        a locked screen is information leaking past the lock.
      */}
      {locked ? (
        <Lock
          people={lock?.people ?? []}
          // Two lists, because they are two questions.
          recoverable={lock?.recoverable ?? []}
          canRecover={lock?.canRecover ?? false}
          onSignedIn={reloadLock}
        />
      ) : null}
    </div>
  );
}
/** The top bar. */
/** How the thirteen screens divide between the bar and the More sheet. */
export function splitScreens(
  screens: readonly Screen[],
  current: string,
): { inBar: Screen[]; inMore: Screen[]; elsewhere: Screen | null } {
  const inBar = screens.filter((s) => s.daily);
  const inMore = screens.filter((s) => !s.daily);
  return { inBar, inMore, elsewhere: inMore.find((s) => s.id === current) ?? null };
}

/** The window buttons, and nothing else. */
function BareBar() {
  const window = inApp() ? getCurrentWindow() : null;
  return (
    <header className="mb-topbar" data-tauri-drag-region>
      <div className="mb-topbar__brand" data-tauri-drag-region>
        <span className="mb-topbar__mark" aria-hidden="true">
          <Logo size="sm" />
        </span>
        <span className="mb-topbar__name">Magic Bill</span>
      </div>
      <div className="mb-topbar__spacer" data-tauri-drag-region />
      <div className="mb-topbar__tools">
        <span className="mb-topbar__windows">
          <button
            type="button"
            className="mb-topbar__button"
            onClick={() => window?.minimize()}
            aria-label="Minimise"
          >
            <Icon name="minimise" size="sm" />
          </button>
          <button
            type="button"
            className="mb-topbar__button"
            onClick={() => window?.toggleMaximize()}
            aria-label="Maximise"
          >
            <Icon name="maximise" size="sm" />
          </button>
          <button
            type="button"
            className="mb-topbar__button mb-topbar__button--close"
            onClick={() => window?.close()}
            aria-label="Close"
          >
            <Icon name="close" size="sm" />
          </button>
        </span>
      </div>
    </header>
  );
}

function TopBar({
  shopPath,
  screens,
  current,
  onGo,
  themeIcon,
  themeName,
  onToggleTheme,
  jobs,
  needsAttention,
  onOpenQueue,
  who,
  role,
  onLock,
  alertCount,
  alertTone,
  onOpenAlerts,
  phones,
  onOpenPhones,
}: {
  shopPath: string | null;
  screens: readonly Screen[];
  current: string;
  onGo: (screen: string) => void;
  /** How many phones are live, and how many are asking to join. */
  phones: PhonesView;
  onOpenPhones: () => void;
  themeIcon: string;
  themeName: string;
  onToggleTheme: () => void;
  jobs: readonly PrintJobView[];
  needsAttention: boolean;
  onOpenQueue: () => void;
  who: string | null;
  role: string | null;
  onLock: () => void;
  /** How many alerts are waiting. */
  alertCount: number;
  /** The worst of them, so the badge is not one colour for everything. */
  alertTone: Alert['tone'] | null;
  onOpenAlerts: () => void;
}) {
  const window = inApp() ? getCurrentWindow() : null;
  const [moreOpen, setMoreOpen] = useState(false);

  const face: IconName =
    themeIcon === 'moon' ? 'moon' : themeIcon === 'contrast' ? 'contrast' : 'sun';

  const { inBar, inMore, elsewhere } = splitScreens(screens, current);

  // Close More on Escape and on going somewhere.
  useEffect(() => {
    if (!moreOpen) return undefined;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setMoreOpen(false);
    };
    window_addEscape(onKey);
    return () => window_removeEscape(onKey);
  }, [moreOpen]);

  const go = (id: string) => {
    setMoreOpen(false);
    onGo(id);
  };

  return (
    <header className="mb-topbar" data-tauri-drag-region>
      <div className="mb-topbar__brand" data-tauri-drag-region title={shopPath ?? undefined}>
        <span className="mb-topbar__mark" aria-hidden="true">
          <Logo size="sm" />
        </span>
        <span className="mb-topbar__name">Magic Bill</span>
      </div>

      <nav className="mb-nav" aria-label="Screens">
        {inBar.map((item) => (
          <button
            key={item.id}
            type="button"
            className="mb-nav__item"
            aria-current={item.id === current ? 'page' : undefined}
            onClick={() => go(item.id)}
          >
            {/* Icon AND label — §5: bare icons are hostile to a new cashier. */}
            <Icon name={item.icon} size="md" />
            <span className="mb-nav__label">{item.label}</span>
          </button>
        ))}

        {inMore.length > 0 ? (
          <div className="mb-nav__more">
            <button
              type="button"
              className="mb-nav__item mb-nav__item--more"
              aria-expanded={moreOpen}
              aria-haspopup="menu"
              aria-current={elsewhere ? 'page' : undefined}
              onClick={() => setMoreOpen((was) => !was)}
            >
              <Icon name={elsewhere?.icon ?? 'more'} size="md" />
              <span className="mb-nav__label">{elsewhere?.label ?? 'More'}</span>
              <Icon name={moreOpen ? 'chevron-up' : 'chevron-down'} size="sm" />
            </button>

            {moreOpen ? (
              <>
                {/*
                  Clicking anywhere else closes it, including on the screen behind — without
                  this the only way out is the button, and that is the popover people learn to
                  dread.
                */}
                <button
                  type="button"
                  className="mb-nav__scrim"
                  aria-label="Close"
                  onClick={() => setMoreOpen(false)}
                />
                <div className="mb-nav__sheet" role="menu">
                  {inMore.map((item) => (
                    <button
                      key={item.id}
                      type="button"
                      className="mb-nav__sheetitem"
                      role="menuitem"
                      aria-current={item.id === current ? 'page' : undefined}
                      onClick={() => go(item.id)}
                    >
                      <Icon name={item.icon} size="md" />
                      <span>{item.label}</span>
                    </button>
                  ))}
                </div>
              </>
            ) : null}
          </div>
        ) : null}
      </nav>

      <div className="mb-topbar__tools">
        {/* Whose till this is, right now. */}
        {who ? (
          <button
            type="button"
            className="mb-who"
            onClick={onLock}
            aria-label={`Signed in as ${who}. Lock the counter (Ctrl+L)`}
            title="Lock the counter — Ctrl+L"
          >
            <Icon name="user" size="sm" />
            <span className="mb-who__name">{who}</span>
            {role ? <span className="mb-who__role">{role}</span> : null}
            <Icon name="lock" size="sm" className="mb-who__lock" />
          </button>
        ) : null}

        {/* The print queue: only while something is printing, or did not print. */}
        {jobs.length === 0 ? null : (
        <button
          type="button"
          className={['mb-queue', needsAttention ? 'mb-queue--attention' : '']
            .filter(Boolean)
            .join(' ')}
          onClick={onOpenQueue}
          aria-label={
            needsAttention
              ? 'A print did not come out — open the print queue'
              : 'Print queue'
          }
        >
          <Icon name={needsAttention ? 'warning' : 'printer'} size="sm" />
          {/*
            The word goes below 1120px so the navigation keeps its own — the `aria-label` above
            carries the whole sentence either way, and a printer icon beside a count is not a
            thing anybody has to learn.
          */}
          <span className="mb-queue__word">
            {needsAttention
              ? 'NOT PRINTED'
              : jobs.length > 0
                ? `${jobs.length} printing`
                : 'Printing'}
          </span>
        </button>
        )}

        {/* The phones: live now, or asking to join. One click lands on Settings › Phones. */}
        {phones.connected > 0 || phones.waiting > 0 ? (
          <button
            type="button"
            className={[
              'mb-phones',
              phones.waiting > 0 ? 'mb-phones--asking' : phones.connected > 0 ? 'mb-phones--live' : '',
            ]
              .filter(Boolean)
              .join(' ')}
            onClick={onOpenPhones}
            aria-label={
              phones.waiting > 0
                ? `${phones.waiting} phone${phones.waiting === 1 ? '' : 's'} asking to join — open Phones`
                : `${phones.connected} phone${phones.connected === 1 ? '' : 's'} live — open Phones`
            }
            title={phones.waiting > 0 ? 'A phone is asking to join' : 'Phones live now'}
          >
            <Icon name="phone" size="sm" />
            <span className="mb-phones__count">
              {phones.waiting > 0 ? `${phones.waiting} asking` : phones.connected}
            </span>
          </button>
        ) : null}

        <button
          type="button"
          className={['mb-bell', alertTone ? `mb-bell--${alertTone}` : ''].filter(Boolean).join(' ')}
          onClick={onOpenAlerts}
          aria-label={
            alertCount === 0
              ? 'Alerts — nothing needs you'
              : `Alerts — ${alertCount} waiting`
          }
          title={alertCount === 0 ? 'Alerts' : `${alertCount} waiting`}
        >
          <Icon name="bell" size="sm" />
          {alertCount > 0 ? <span className="mb-bell__count">{alertCount}</span> : null}
        </button>

        <button
          type="button"
          className="mb-topbar__button"
          onClick={onToggleTheme}
          aria-label={`Theme: ${themeName}. Switch.`}
          title={`Theme: ${themeName}`}
        >
          <Icon name={face} size="md" />
        </button>

        <span className="mb-topbar__windows">
          <button
            type="button"
            className="mb-topbar__button"
            onClick={() => window?.minimize()}
            aria-label="Minimise"
          >
            <Icon name="minimise" size="sm" />
          </button>
          <button
            type="button"
            className="mb-topbar__button"
            onClick={() => window?.toggleMaximize()}
            aria-label="Maximise"
          >
            <Icon name="maximise" size="sm" />
          </button>
          <button
            type="button"
            className="mb-topbar__button mb-topbar__button--close"
            onClick={() => window?.close()}
            aria-label="Close"
          >
            <Icon name="close" size="sm" />
          </button>
        </span>
      </div>
    </header>
  );
}

/** Escape, on the document. */
function window_addEscape(handler: (event: KeyboardEvent) => void) {
  document.addEventListener('keydown', handler);
}

function window_removeEscape(handler: (event: KeyboardEvent) => void) {
  document.removeEventListener('keydown', handler);
}

export function PrintQueuePanel({
  open,
  jobs,
  onClose,
  onRetry,
  onDismiss,
  onRetryAll,
  onDismissAll,
}: {
  open: boolean;
  jobs: readonly PrintJobView[];
  onClose: () => void;
  onRetry: (id: string) => void;
  onDismiss: (id: string) => void;
  /** The same two things, for every parked job at once. */
  onRetryAll: () => void;
  onDismissAll: () => void;
}) {
  const parked = jobs.filter((job) => job.needsAttention).length;
  return (
    <Modal
      open={open}
      title="Printing"
      onClose={onClose}
      wide
      actions={
        jobs.length > 1 ? (
          <>
            <Button variant="quiet" onClick={onDismissAll}>
              Give up on all {jobs.length}
            </Button>
            {parked > 1 ? <Button onClick={onRetryAll}>Try all {parked} again</Button> : null}
          </>
        ) : undefined
      }
    >
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
              {/* Any job can be given up on; only a parked one can be tried again. */}
              <div className="mb-row">
                {job.needsAttention ? (
                  <Button small onClick={() => onRetry(job.id)}>
                    Try again
                  </Button>
                ) : null}
                <Button small variant="quiet" onClick={() => onDismiss(job.id)}>
                  Give up
                </Button>
              </div>
            </div>
          ))
        )}
      </div>
    </Modal>
  );
}
