import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';

import { Button, EmptyState, Hint, Icon, Logo, Modal, plural, useToast, type IconName } from '../kit';
import { call, inApp, isUiError, subscribe } from '../ipc/call';
import type { AppStatus } from '../ipc/generated/AppStatus';
import type { LockState } from '../ipc/generated/LockState';
import type { SetupView } from '../ipc/generated/SetupView';
import type { PrintJobView } from '../ipc/generated/PrintJobView';
import type { NoticesView } from '../ipc/generated/NoticesView';
import type { PhonesView } from '../ipc/generated/PhonesView';
import type { DayStateView } from '../ipc/generated/DayStateView';
import { useTheme } from '../theme/ThemeProvider';
import { useTick } from '../clock';
import { Account } from '../account/Account';
import { UpdateOffer } from '../account/Update';
import { FirstRun } from '../setup/FirstRun';
import { AlertsPanel, loudest, type Alert } from './Alerts';
import { DayGate } from './DayGate';
import { More } from './More';
import { MayProvider } from './permissions';
import { Billing } from '../billing/Billing';
import { SettleDesk } from '../billing/SettleDesk';
import { Health } from '../health/Health';
import { Kitchen } from '../kitchen/Kitchen';
import { Gallery } from '../gallery/Gallery';
import { Lock } from '../auth/Lock';
import { Staff } from '../auth/Staff';
import { Audit } from '../auth/Audit';
import { Credit } from '../credit/Credit';
import { Expenses } from '../expenses/Expenses';
import { Stock } from '../stock/Stock';
import { Buying } from '../buying/Buying';
import { Floor } from '../floor/Floor';
import { Delivery } from '../delivery/Delivery';
import { Menu } from '../menu/Menu';
import { Phones } from '../phones/Phones';
import { Reports } from '../reports/Reports';
import { Settings } from '../settings/Settings';

import './shell.css';
import '../auth/auth.css';

const WORST: Record<Alert['tone'], number> = { danger: 3, warn: 2, accent: 1, info: 0 };

/** How often the gate asks again while somebody stays signed in across the day boundary. */
const GATE_EVERY_MS = 60 * 60 * 1000;

export interface Screen {
  id: string;
  label: string;
  /** A name from the kit's icon set, not a character. */
  icon: IconName;
  /** True for a screen the counter uses every day. */
  daily?: boolean;
  /**
   * `go` opens another screen — `go('settings/printers')` opens Settings on the Printers
   * section; `sub` is that part, for the screen that was asked for it.
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
    // `go` so an empty shop's counter can open the menu, the floor and the printers.
    render: (go) => <Billing onGoTo={go} />,
    needs: 'bill.create',
  },
  {
    // The floor answers a different question from the billing grid: not "which table am I
    // putting this dosa on" but "which table needs me".
    id: 'floor',
    daily: true,
    label: 'Floor',
    icon: 'grid',
    render: () => <Floor />,
    needs: 'bill.create',
  },
  {
    // Watched all day, and given to whoever may let a phone on — a manager as much as the
    // owner.
    id: 'phones',
    daily: true,
    label: 'Phones',
    icon: 'phone',
    render: () => <Phones />,
    needs: 'devices.pair',
  },
  {
    id: 'credit',
    label: 'Credit',
    icon: 'wallet',
    render: () => <Credit />,
    // Taking a repayment is done here too, so whoever may take one gets in.
    needsAny: ['customers.manage', 'credit.collect'],
  },
  {
    // "Spends", not "Expenses": the rail is read at a glance and the shorter word is the one a
    // shopkeeper uses.
    id: 'expenses',
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
    // A cook recording wastage and a helper counting shelves both work here — the same list
    // as Rust's `STOCK_PERMISSIONS`.
    needsAny: [
      'inventory.view',
      'inventory.manage',
      'stock.waste',
      'stock.count',
      'stock.adjust',
    ],
  },
  {
    // Next to Stock, because they are the same shelf from two ends: what came in, and what is
    // on it.
    id: 'buying',
    label: 'Buying',
    icon: 'truck',
    render: () => <Buying />,
    needsAny: ['purchases.manage', 'suppliers.manage'],
  },
  {
    // Beside the floor, because it is the same question asked about the food that has left the
    // building: which orders are still out, and who is carrying the cash for them.
    id: 'delivery',
    label: 'Delivery',
    icon: 'bike',
    render: () => <Delivery />,
    needsAny: ['bill.create', 'delivery.dispatch'],
  },
  {
    // Bills and Day open/close live inside Reports: "what did that customer pay?", "how did
    // the month go?" and "is today closed?" are the same person's questions. A cashier who
    // may close the day but not read reports opens it for Day open/close alone.
    id: 'reports',
    daily: true,
    label: 'Reports',
    icon: 'chart',
    // `go` so a licence refusal can hand somebody straight to the Account screen instead of
    // leaving them to find it; `sub` so an alert can land on Day open/close.
    render: (go, sub) => <Reports onGoTo={go} initial={sub} />,
    needsAny: ['reports.view', 'day.close'],
  },
  {
    // In the bar, after Reports: the owner opens it to check the plan, take a backup, or
    // update. Staff never see it.
    id: 'account',
    daily: true,
    label: 'Account',
    icon: 'badge',
    render: () => <Account />,
    needs: 'licence.manage',
  },
  {
    // In the bar, after Account: every setting is one press away, and never behind More.
    id: 'settings',
    daily: true,
    label: 'Settings',
    icon: 'settings',
    render: (_go, sub) => <Settings initial={sub} />,
    needsAny: ['settings.store', 'settings.tax', 'settings.printer', 'backup.run'],
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
    // Attendance, leave and pay live here too, so every one of those jobs opens it — the same
    // list as Rust's `STAFF_PERMISSIONS`. The tabs inside hide what the person may not do.
    needsAny: [
      'staff.manage',
      'attendance.mark',
      'attendance.correct',
      'leave.approve',
      'salary.view',
      'salary.manage',
    ],
  },
  {
    id: 'history',
    label: 'History',
    icon: 'clock',
    render: () => <Audit />,
    needs: 'audit.view',
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
        // A developer's screen, so it opens for whoever may manage the shop's staff — which
        // keeps it out of the way when a test signs in a role with nothing ticked.
        needs: 'staff.manage',
      },
    ]
  : SHIPPED_SCREENS;

export function Shell() {
  const [screen, setScreenOnly] = useState<string>('billing');
  /** The part of a screen that was asked for: `settings/printers` → `printers`. */
  const [sub, setSub] = useState<string | null>(null);
  /** The More screen last opened, which is where the More button goes back to. */
  const [lastMore, setLastMore] = useState<string | null>(null);
  const setScreen = useCallback((id: string) => {
    const slash = id.indexOf('/');
    setSub(slash < 0 ? null : id.slice(slash + 1));
    setScreenOnly(slash < 0 ? id : id.slice(0, slash));
  }, []);
  /** Whose screen this is. A different person signing in starts from the front. */
  const lastPerson = useRef<string | null>(null);
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
  /** The version waiting to be installed, if the last shelf read found one. */
  const [update, setUpdate] = useState<string | null>(null);
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

  /**
   * The gate: a day left open is asked about whenever somebody signs in, and again every hour
   * for the one who stays signed in past 5 am. `gateWaived` is the way past it while an open
   * order from yesterday is settled; it lasts until the next ask.
   */
  const [dayState, setDayState] = useState<DayStateView | null>(null);
  const [gateWaived, setGateWaived] = useState(false);
  /**
   * `asking` is a fresh ask — signing in, or the hour turning — and only those put the gate
   * back up. Reading the day again because somebody walked to another screen must not, or the
   * way past it would last exactly as long as standing still.
   */
  const reloadDay = useCallback((asking: boolean) => {
    if (!inApp()) return;
    call('day_state', { holidays: null })
      // Checked, not trusted, like every other answer the shell draws.
      .then((fresh) => {
        if (fresh && Array.isArray(fresh.pending)) {
          setDayState(fresh);
          if (asking) setGateWaived(false);
        }
      })
      .catch(() => undefined);
  }, []);
  const askedAt = useRef(0);
  useEffect(() => {
    if (lock === null || lock.signedInAs === null) {
      setDayState(null);
      return;
    }
    askedAt.current = Date.now();
    reloadDay(true);
  }, [lock, reloadDay]);
  // The one shared clock, not a timer of its own: once an hour is enough to catch 5 am.
  const tick = useTick();
  useEffect(() => {
    if (lock === null || lock.signedInAs === null) return;
    if (Date.now() - askedAt.current < GATE_EVERY_MS) return;
    askedAt.current = Date.now();
    reloadDay(true);
  }, [tick, lock, reloadDay]);
  // And again on arriving at another screen. Somebody who has just closed today walks straight
  // back to Billing, and the counter must already know it will not take money.
  useEffect(() => {
    if (lock === null || lock.signedInAs === null) return;
    reloadDay(false);
  }, [screen, lock, reloadDay]);

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
      if (message.kind === 'version') setUpdate(message.available);
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
    // The kitchen screen is always listed; the page itself says when it is switched off (owner, 2026-09-03).
    if (item.needs && !held.includes(item.needs)) return false;
    if (item.needsAny && !item.needsAny.some((need) => held.includes(need))) return false;
    return true;
  });
  const active = allowed.find((s) => s.id === screen) ?? allowed[0];
  const { inMore } = splitScreens(allowed, screen);
  const behindMore = active !== undefined && inMore.some((s) => s.id === active.id);
  useEffect(() => {
    if (behindMore && active) setLastMore(active.id);
  }, [behindMore, active]);

  // Somebody else signing in gets the front screen, not the last person's page — which they
  // may not be allowed to open, and which More would otherwise keep trying to go back to.
  const signedInAs = lock?.signedInAs ?? null;
  useEffect(() => {
    if (signedInAs === null) return;
    if (lastPerson.current !== null && lastPerson.current !== signedInAs) {
      setScreenOnly(allowed[0]?.id ?? 'billing');
      setSub(null);
      setLastMore(null);
    }
    lastPerson.current = signedInAs;
    // `allowed` is derived from the same lock state, so the person is the only real trigger.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signedInAs]);

  // Locked = there is nobody signed in.
  const locked = inApp() && lock !== null && lock.signedInAs === null;

  /** Everything the shop should know, in one list. */
  const alerts: Alert[] = [];
  // The shelf is read after each licence check; the bell says so, and Account installs it.
  const waiting = update ?? status?.update ?? null;
  if (waiting) {
    alerts.push({
      id: 'update',
      tone: 'accent',
      icon: 'download',
      title: 'Update ready',
      says: `Version ${waiting} is ready to install.`,
      goTo: 'account',
      goLabel: 'Open Account',
    });
  }
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
  // The counter has stopped taking money and the billing screen cannot say so itself: without
  // this the first anybody knows is a refused bill with a customer standing there.
  if (dayState && dayState.todayState !== 'open') {
    alerts.push({
      id: 'day-closed',
      tone: 'warn',
      icon: 'calendar',
      title: dayState.todayState === 'holiday' ? 'Today is a holiday' : 'Today is closed',
      says: `${dayState.todayClosedSays} Nothing more can be billed into it until somebody opens it again.`,
      goTo: dayState.mayAct ? 'reports/days' : undefined,
      goLabel: dayState.mayAct ? 'Day open/close' : undefined,
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
        // Behind the lock there is no navigation: the bar shows the brand and the tools only.
        screens={locked ? [] : allowed}
        // The screen actually drawn, which is the first allowed one when `screen` is not.
        current={active?.id ?? screen}
        onGo={setScreen}
        lastMore={lastMore}
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
        onOpenPhones={() => setScreen('phones')}
      />

      <div className="mb-body">
        <main className="mb-main">
          {/* Nothing is rendered behind the lock. */}
          {locked ? null : !active ? (
            // A role with no screen at all. The counter still locks, so the next person can
            // sign in; the owner fixes the role.
            <EmptyState
              title="Nothing to open here"
              hint="This role has no screens on the counter yet. The owner can change what it may do under Staff, on the Roles tab."
            />
          ) : (
            <MayProvider held={held}>
              {behindMore ? (
                <More screens={inMore} current={active.id} onGo={setScreen}>
                  {active.render(setScreen, active.id === screen ? sub : null)}
                </More>
              ) : (
                active.render(setScreen, active.id === screen ? sub : null)
              )}
            </MayProvider>
          )}
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

      {/* Billing waits: a day left open is closed, or called a holiday, before anything else. */}
      {/* What the phones asked the counter to settle — over whichever screen is up. */}
      {!locked ? <SettleDesk /> : null}
      {/*
        A new version, offered once each time the counter is opened, to whoever can install it.
        Later leaves it to the bell and the Account page.
      */}
      {!locked && held.includes('settings.store') ? <UpdateOffer version={waiting} /> : null}
      {!locked && dayState && dayState.pending.length > 0 && !gateWaived ? (
        <DayGate
          state={dayState}
          onChange={setDayState}
          onEscape={() => setGateWaived(true)}
          onSignOut={() => {
            call('lock_now').then(setLock).catch(() => undefined);
          }}
        />
      ) : null}

      {/*
        Over everything, including the print queue panel and any toast — a toast floating above
        a locked screen is information leaking past the lock.
      */}
      {locked ? (
        <Lock
          people={lock?.people ?? []}
          owner={lock?.owner ?? null}
          lastSignedIn={lock?.lastSignedIn ?? null}
          onSignedIn={reloadLock}
        />
      ) : null}
    </div>
  );
}
/** The top bar. */
/**
 * Where the More button goes: the More page already up, else the one last opened — only if THIS
 * person may open it — else the first one they may. The last person's page is not this
 * person's, and pressing More must never land on the bar's first screen.
 */
export function moreTarget(
  inMore: readonly Screen[],
  elsewhere: Screen | null,
  lastMore: string | null,
  current: string,
): string {
  return (
    elsewhere?.id ?? inMore.find((s) => s.id === lastMore)?.id ?? inMore[0]?.id ?? current
  );
}

/** How the thirteen screens divide between the bar and the More sheet. */
export function splitScreens(
  screens: readonly Screen[],
  current: string,
): { inBar: Screen[]; inMore: Screen[]; elsewhere: Screen | null } {
  const inBar = screens.filter((s) => s.daily);
  const inMore = screens.filter((s) => !s.daily);
  return {
    inBar,
    inMore,
    elsewhere: inMore.find((s) => s.id === current) ?? null,
  };
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

/**
 * How long the bell rings, in milliseconds — asked of the theme, never decided here. A
 * counter with motion turned off has the beat set to zero there, so this comes back zero and
 * the bell never rings at all.
 */
function ringsFor(): number {
  const theme = getComputedStyle(document.documentElement);
  const beat = Number.parseFloat(theme.getPropertyValue('--motion-beat'));
  const beats = Number.parseFloat(theme.getPropertyValue('--beats'));
  return Number.isFinite(beat) && Number.isFinite(beats) ? beat * beats : 0;
}

function TopBar({
  shopPath,
  screens,
  current,
  onGo,
  lastMore,
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
  /** The More screen last opened, so the More button goes back to it. */
  lastMore: string | null;
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

  const face: IconName = themeIcon === 'moon' ? 'moon' : 'sun';

  /**
   * The bell rings when something NEW turns up — a count that went UP, not one that has been
   * sitting there all morning. It stops on its own, and opening the alerts stops it too.
   */
  const [ringing, setRinging] = useState(false);
  const counted = useRef(alertCount);
  useEffect(() => {
    const was = counted.current;
    counted.current = alertCount;
    if (alertCount <= was) return;
    setRinging(true);
    const stop = setTimeout(() => setRinging(false), ringsFor());
    return () => clearTimeout(stop);
  }, [alertCount]);

  const { inBar, inMore, elsewhere } = splitScreens(screens, current);

  const go = (id: string) => onGo(id);

  return (
    <header className="mb-topbar" data-tauri-drag-region>
      <div className="mb-topbar__brand" data-tauri-drag-region title={shopPath ?? undefined}>
        <span className="mb-topbar__mark" aria-hidden="true">
          <Logo size="sm" />
        </span>
        <span className="mb-topbar__name">Magic Bill</span>
      </div>
      <span className="mb-topbar__divider" aria-hidden="true" />

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
          <button
            type="button"
            className="mb-nav__item mb-nav__item--more"
            aria-current={elsewhere ? 'page' : undefined}
            onClick={() => go(moreTarget(inMore, elsewhere, lastMore, current))}
          >
            {/* Always "More": the page names itself, so the bar never reflows. */}
            <Icon name="more" size="md" />
            <span className="mb-nav__label">More</span>
          </button>
        ) : null}
      </nav>

      <div className="mb-topbar__tools">
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

        {/* The phones: live now, or asking to join. One click opens Phones. */}
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
          className={[
            'mb-bell',
            alertTone ? `mb-bell--${alertTone}` : 'mb-bell--quiet',
            ringing ? 'mb-bell--ringing' : '',
          ]
            .filter(Boolean)
            .join(' ')}
          onClick={() => {
            setRinging(false);
            onOpenAlerts();
          }}
          aria-label={
            alertCount === 0
              ? 'Alerts — nothing needs you'
              : `Alerts — ${alertCount} waiting`
          }
          title={alertCount === 0 ? 'Alerts' : `${alertCount} waiting`}
        >
          {/* The ring: a halo that beats a few times behind the bell, never over it. */}
          <span className="mb-bell__ring" aria-hidden="true" />
          <Icon name="bell" size="lg" className="mb-bell__glyph" />
          {alertCount > 0 ? <span className="mb-bell__count">{alertCount}</span> : null}
        </button>

        <button
          type="button"
          className="mb-topbar__button"
          onClick={onToggleTheme}
          aria-label={`Theme: ${themeName}. Switch.`}
          title={`Theme: ${themeName}`}
        >
          <Icon name={face} size="lg" />
        </button>

        {/* Whose till this is, right now: the face, the name, the role. */}
        {who ? (
          <>
            <span className="mb-topbar__divider" aria-hidden="true" />
            <button
              type="button"
              className="mb-who"
              onClick={onLock}
              aria-label={`Signed in as ${who}${role ? `, ${role}` : ''}. Lock the counter (Ctrl+L)`}
              title={`${role ? `${role} — ` : ''}Lock the counter — Ctrl+L`}
            >
              <span className="mb-face" aria-hidden="true">
                {who.trim().charAt(0).toUpperCase()}
              </span>
              <span className="mb-who__name">{who}</span>
              <Icon name="lock" size="sm" className="mb-who__lock" />
            </button>
          </>
        ) : null}
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
                  <Hint>{job.lastError}</Hint>
                ) : null}
              </div>
              {/* Any job can be given up on; only a parked one can be tried again. */}
              <div className="mb-row">
                {job.needsAttention ? (
                  <Button size="sm" onClick={() => onRetry(job.id)}>
                    Try again
                  </Button>
                ) : null}
                <Button size="sm" variant="quiet" onClick={() => onDismiss(job.id)}>
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
