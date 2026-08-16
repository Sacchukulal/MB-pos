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

import { Button, Icon, Modal, Notice, useToast, type IconName } from '../kit';
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

export interface Screen {
  id: string;
  label: string;
  /**
   * A name from the kit's icon set, not a character. **P27.5 changed this from
   * `string`** — it used to hold a Unicode glyph (`▦`, `☰`, `⌁`), so which
   * picture a shop actually saw depended on which font Windows substituted,
   * and they arrived at three different weights. A union type means a mistyped
   * name is now a compile error rather than a hole in the navigation.
   */
  icon: IconName;
  /**
   * **True for a screen the counter uses every day.** Those get a place in the
   * top bar; everything else lives behind "More".
   *
   * Thirteen destinations do not fit across 1366px with a readable label on
   * each, and §5 forbids the usual escape — *"icon-only is fast for a daily
   * user and hostile to a new one"*. So the split is by how often a shop opens
   * the screen rather than by what fits: billing, the floor, the day's bills,
   * credit, spends and reports are the counter's day; stock, buying, the menu,
   * staff, history, settings and the account are things somebody sits down to
   * do. Both halves keep their words.
   */
  daily?: boolean;
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
 * P09 adds `{ id: 'billing', label: 'Billing', icon: 'receipt', render: … }`
 * and is finished. Lazily rendered — nothing that is not on screen is built,
 * which is budget S1 and scope 16.14.
 */
export const SHIPPED_SCREENS: readonly Screen[] = [
  {
    id: "billing",
    daily: true,
    label: "Billing",
    icon: 'receipt',
    // The set-up list lives on this screen and needs to be able to send
    // somebody to Settings or Menu — see `Setup` and D102.
    render: (go) => <Billing onGoTo={go} />,
  },
  {
    // The floor answers a different question from the billing grid: not
    // "which table am I putting this dosa on" but "which table needs me".
    // Audit F5 is the second one going unanswered.
    id: 'floor',
    daily: true,
    label: 'Floor',
    icon: 'grid',
    render: () => <Floor />,
  },
  {
    // Not "Khata" — the owner renamed it on 2026-08-08. The screen answers
    // "who owes me money", which is why that is its default view rather than
    // an alphabetical list nobody opens.
    id: 'credit',
    daily: true,
    label: 'Credit',
    icon: 'wallet',
    render: () => <Credit />,
    needs: 'customers.manage',
  },
  {
    // "Spends", not "Expenses": the rail is read at a glance and the shorter
    // word is the one a shopkeeper uses.
    id: 'expenses',
    daily: true,
    label: 'Spends',
    icon: 'banknote',
    render: () => <Expenses />,
    needs: 'expenses.manage',
  },
  {
    // Next to Spends, because they are the same question from two sides: what
    // left as money, and what left as food. MARKET_GAP_ANALYSIS calls this
    // "the biggest single hole" in the product.
    id: 'stock',
    label: 'Stock',
    icon: 'boxes',
    render: () => <Stock />,
    needs: 'inventory.view',
  },
  {
    // Next to Stock, because they are the same shelf from two ends: what came
    // in, and what is on it. Buying holds deliveries, suppliers and orders;
    // the COUNT is a tab on Stock, because a count is a question about the
    // shelf and that is where the person already is.
    id: 'buying',
    label: 'Buying',
    icon: 'truck',
    render: () => <Buying />,
    needs: 'purchases.manage',
  },
  {
    // **P29.** Beside the floor, because it is the same question asked about
    // the food that has left the building: which orders are still out, and who
    // is carrying the cash for them.
    //
    // **Not `daily`, and that is a decision rather than an oversight.** The bar
    // is a fixed six because seven words do not fit across 1366px, and none of
    // the six is droppable for a shop that does no deliveries at all. So this
    // sits under More, where Stock and Buying already are — every screen a
    // particular shop lives in and a different shop never opens.
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
    // Directly under Bills, because the two answer the same person's
    // questions: "what did that customer pay?" and "how did the month go?"
    id: 'reports',
    daily: true,
    label: 'Reports',
    icon: 'chart',
    render: () => <Reports />,
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
    // Not "Audit". The owner must be able to answer "who voided that bill?"
    // without knowing our word for it (UI_GUIDELINES §6).
    label: 'History',
    icon: 'clock',
    render: () => <Audit />,
    needs: 'audit.view',
  },
  {
    // Last but one, and below Menu: settings are what an owner opens once a
    // month, so they must not sit where a cashier's hand goes.
    id: 'settings',
    label: 'Settings',
    icon: 'settings',
    render: () => <Settings />,
    needsAny: ['settings.store', 'settings.tax', 'settings.printer', 'backup.run'],
  },
  {
    // Below Settings, because it is opened once a year — and above the Kit,
    // because the Kit is not a screen a shop has any use for.
    id: 'account',
    label: 'Account',
    icon: 'badge',
    render: () => <Account />,
    needs: 'reports.view',
  },
  {
    // **P24.** On the counter it is a screen like any other, so a shop with one
    // machine can run the kitchen from it. On a wall tablet it is the whole
    // window — same page, same code.
    id: 'kitchen',
    label: 'Kitchen',
    icon: 'flame',
    render: () => <Kitchen />,
    needs: 'bill.create',
  },
  {
    // **P29.** Beside Health, because they are the same question about the two
    // halves of one counter: is the software all right, and is the hardware. A
    // dealer setting a shop up lives on this screen for an hour and never
    // opens it again.
    id: 'devices',
    label: 'Devices',
    icon: 'plug',
    render: () => <Devices />,
    needs: 'settings.printer',
  },
  {
    // Beside Account, because the two answer "is my counter all right?" from
    // the two directions an owner asks it.
    id: 'health',
    label: 'Health',
    icon: 'pulse',
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
        icon: 'tag',
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
  /**
   * **What this till is holding for the main one** (P27, D138) — the whole
   * sentence, written in Rust, empty when there is nothing to say.
   *
   * It lives in the shell for the same reason the print queue does (audit D4):
   * a shop must be able to SEE that its tills are apart, and a state that is
   * only visible on the screen nobody has open is a state that is hidden.
   */
  const [tillsSay, setTillsSay] = useState('');
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
      if (message.kind === 'tills') setTillsSay(message.says);
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
      />

      {/* The two standing banners. Both are `Notice` from the kit at P27.5
          rather than two hand-written strips that happened to look similar —
          §6: ONE place turns a machine state into words. Neither is
          dismissible, and for the same reason: a dismissed banner is a fixed
          bug that was never fixed. */}
      {lock?.nobodyHasAPin ? (
        <div className="mb-shell__banner">
          {/* Audit C1, on a shop that has not fixed it yet. */}
          <Notice tone="warn" icon="lock">
            <strong>Anybody can open this shop&rsquo;s reports and settings.</strong>{' '}
            Add a PIN in Staff so the counter locks itself.
          </Notice>
        </div>
      ) : null}

      {/* P27, D138. The accent tone and not the warning one, because nothing
          is wrong — the money is safe and it is going across — and a red bar
          over an ordinary Tuesday teaches a shop to ignore red bars. */}
      {tillsSay ? (
        <div className="mb-shell__banner">
          <Notice tone="accent" icon="refresh">
            {tillsSay}
          </Notice>
        </div>
      ) : null}

      <div className="mb-body">
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
            <div className="mb-shell__banner mb-shell__banner--inmain">
              <Notice
                tone={
                  status.licenceTone === 'danger'
                    ? 'danger'
                    : status.licenceTone === 'warn'
                      ? 'warn'
                      : 'info'
                }
                action={
                  <Button small variant="quiet" onClick={() => setScreen('account')}>
                    Open Account
                  </Button>
                }
              >
                {status.licence}
              </Notice>
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
/**
 * **The top bar** — P27.5, and it replaces the left rail entirely.
 *
 * The owner, 2026-08-15: *"the verticll left menu bars i didnt like, i wish to
 * see that in horizontal top side"*.
 *
 * # Why it is ONE strip and not two
 *
 * A navigation bar across the top normally costs a row of vertical space, and
 * at 1366x768 — the reference machine, D12 — vertical space is the scarce
 * dimension: it is what the cart, the table grid and every report are fighting
 * over. So the navigation does not get a row of its own. It shares the title
 * bar we already draw ourselves (audit F7): the wordmark at one end, the window
 * buttons at the other, and the screens along the middle. Against the old rail
 * this costs nothing vertically and gives the billing screen back 76px of
 * width.
 *
 * # Why six screens and then "More"
 *
 * Thirteen destinations do not fit across 1366px with a readable word under
 * each, and UI_GUIDELINES §5 rules out the usual escape: *"icon-only is fast
 * for a daily user and hostile to a new one. Solve it — do not just ship bare
 * icons."* Hiding the labels would have been shipping bare icons with extra
 * steps.
 *
 * So the split is by **how often a shop opens the screen**, and both halves
 * keep their words. Billing, Floor, Bills, Credit, Spends and Reports are the
 * counter's day and sit in the bar. Stock, Buying, Menu, Staff, History,
 * Settings and Account are things somebody sits down to do, and live one click
 * away behind More — which shows them as a proper labelled list, not a row of
 * mystery glyphs.
 *
 * The current screen is always visible with its label, even when it came from
 * More: a navigation that cannot show you where you are is worse than no
 * navigation.
 */
/**
 * **How the thirteen screens divide between the bar and the More sheet.**
 *
 * Pure, exported and tested (`tests/look.test.tsx`), because it is a rule
 * rather than a rendering detail — and because the first version of it was
 * wrong in a way only a running app showed.
 *
 * That first version added the current screen to the bar when it came from
 * More, so the bar always said where you were. Opening Stock proved it: eight
 * items plus the More button plus the tools is wider than 1366px, and "More"
 * ran straight over the signed-in name in the corner. **A navigation that
 * breaks the moment you use it is worse than one that is merely long.**
 *
 * So the bar is a FIXED six — the screens a counter opens every day — and the
 * More button itself carries the answer: inside Stock it reads "Stock", with
 * Stock's icon, marked as the current page. Nothing moves, nothing overflows,
 * and "where am I" still has an answer on screen.
 */
export function splitScreens(
  screens: readonly Screen[],
  current: string,
): { inBar: Screen[]; inMore: Screen[]; elsewhere: Screen | null } {
  const inBar = screens.filter((s) => s.daily);
  const inMore = screens.filter((s) => !s.daily);
  return { inBar, inMore, elsewhere: inMore.find((s) => s.id === current) ?? null };
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
}: {
  shopPath: string | null;
  screens: readonly Screen[];
  current: string;
  onGo: (screen: string) => void;
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
  const [moreOpen, setMoreOpen] = useState(false);

  const face: IconName =
    themeIcon === 'moon' ? 'moon' : themeIcon === 'contrast' ? 'contrast' : 'sun';

  const { inBar, inMore, elsewhere } = splitScreens(screens, current);

  // Close More on Escape and on going somewhere. A popover that outlives its
  // purpose is the thing that ends up covering the Complete Bill button.
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
      {/* The wordmark. **Not the database path** — that used to be printed
          across the title bar in full, which is developer output on a
          shopkeeper's screen. It is still reachable for a support call: it is
          the tooltip here, and it is on the Health screen in words. */}
      <div className="mb-topbar__brand" data-tauri-drag-region title={shopPath ?? undefined}>
        <span className="mb-topbar__mark" aria-hidden="true">
          <Icon name="receipt" size="sm" />
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
                {/* Clicking anywhere else closes it, including on the screen
                    behind — without this the only way out is the button, and
                    that is the popover people learn to dread. */}
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
        {/* Whose till this is, right now. Audit C3's other half: the name on
            the bill and the name on the screen are one fact. */}
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

        {/* The print queue. PERSISTENT — audit D4: a toast that has faded is
            not "the cashier can see it". */}
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
          className="mb-topbar__button"
          onClick={onToggleTheme}
          aria-label={`Theme: ${themeName}. Switch.`}
          title={`Theme: ${themeName}`}
        >
          <Icon name={face} size="md" />
        </button>

        {/* The window buttons, drawn on the same grid as every other icon —
            they used to be "–", "□" and "✕" typed as characters, from three
            different fonts, and they never lined up with each other. */}
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
 * Escape, on the document.
 *
 * A pair of one-line helpers rather than `document.addEventListener` inline,
 * because `window` is shadowed in `TopBar` by the Tauri window handle — and a
 * listener registered on the wrong object is a popover that never closes.
 */
function window_addEscape(handler: (event: KeyboardEvent) => void) {
  document.addEventListener('keydown', handler);
}

function window_removeEscape(handler: (event: KeyboardEvent) => void) {
  document.removeEventListener('keydown', handler);
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
