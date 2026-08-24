/**
 * **Everything that used to be a banner** — P30.6.
 *
 * # Why this exists
 *
 * The owner installed the counter and found the top of every screen taken by
 * standing notices — the licence line, the no-PIN warning, the till line — and
 * under them a six-item set-up checklist with a "Do it" button on each row. On
 * a 720-pixel work area that is a third of the screen, permanently, on the one
 * page a cashier looks at all day. Their instruction on 2026-08-16:
 *
 * > *"instead of showing like this big line notification/error, just make a
 * > small bell button near sun moon button, so all push notifications and
 * > alerts are sent to there only, (for the same notification for future push
 * > notifications from super admin)"*
 *
 * So: one bell in the title bar, a count on it when something is waiting, and
 * a panel behind it. **Nothing writes its own strip above a screen any more.**
 *
 * # The rules this keeps from the banners it replaces
 *
 * 1. **The sentence is Rust's** (§6). Nothing here composes words about a
 *    machine state; every `says` arrives written.
 * 2. **Nothing is dismissible.** A dismissed warning is a fixed problem that
 *    was never fixed — the licence really has run out until somebody pays, and
 *    the shop really has no PIN until somebody sets one. The way to clear an
 *    alert is to deal with it, and each one carries the button that does.
 * 3. **It never blocks the till.** This is a panel somebody opens, never a
 *    dialog in front of a customer (D102, and PERFORMANCE S5).
 *
 * # The super-admin half
 *
 * The owner asked for this to be where messages from the backend land too.
 * That backend does not exist yet — it is Phase 10 — so there is nothing to
 * subscribe to. The shape is ready for it: an alert is `{ id, tone, says,
 * goTo }` and a pushed one is the same record with a different source. When
 * P31 builds the channel it adds a case, not a screen.
 */

import { Button, Icon, SectionHeader, type IconName } from '../kit';

/** One thing the shop should know about. */
export interface Alert {
  /** Stable, so a re-render does not make the same alert twice. */
  id: string;
  tone: 'info' | 'warn' | 'danger' | 'accent';
  icon: IconName;
  /** A short heading — what it is. */
  title: string;
  /** The whole sentence, written in Rust. */
  says: string;
  /** The screen that fixes it, if there is one. */
  goTo?: string;
  goLabel?: string;
}

/**
 * How loud the bell is: the worst tone of anything waiting.
 *
 * A count alone cannot say "your licence has ended" differently from "you have
 * not added your tables yet", and those two do not deserve the same colour.
 */
export function loudest(alerts: readonly Alert[]): Alert['tone'] | null {
  if (alerts.some((a) => a.tone === 'danger')) return 'danger';
  if (alerts.some((a) => a.tone === 'warn')) return 'warn';
  if (alerts.length > 0) return 'info';
  return null;
}

export function AlertsPanel({
  alerts,
  onGo,
  onClose,
}: {
  alerts: readonly Alert[];
  onGo: (screen: string) => void;
  onClose: () => void;
}) {
  return (
    <>
      {/* Pressing anywhere else closes it. A panel hanging off the title bar
          that needs its own close button found is a panel people leave open. */}
      <button
        type="button"
        className="mb-alerts__away"
        aria-label="Close the alerts"
        onClick={onClose}
      />
      <section className="mb-alerts" aria-label="Alerts">
        <SectionHeader
          title="Alerts"
          action={
            <button
              type="button"
              className="mb-topbar__button"
              onClick={onClose}
              aria-label="Close the alerts"
            >
              <Icon name="close" size="sm" />
            </button>
          }
        />

        {alerts.length === 0 ? (
          <p className="mb-alerts__quiet">
            Nothing needs you. Anything the counter wants to tell you turns up
            here.
          </p>
        ) : (
          <ul className="mb-alerts__list">
            {alerts.map((alert) => (
              <li key={alert.id} className={`mb-alerts__one mb-alerts__one--${alert.tone}`}>
                <Icon name={alert.icon} size="sm" className="mb-alerts__icon" />
                <div className="mb-alerts__body">
                  <span className="mb-alerts__what">{alert.title}</span>
                  <span className="mb-alerts__says">{alert.says}</span>
                </div>
                {alert.goTo ? (
                  <Button
                    small
                    variant="secondary"
                    onClick={() => {
                      onGo(alert.goTo as string);
                      onClose();
                    }}
                  >
                    {alert.goLabel ?? 'Do it'}
                  </Button>
                ) : null}
              </li>
            ))}
          </ul>
        )}
      </section>
    </>
  );
}
