import { Button, Icon, SectionHeader, type IconName } from '../kit';

/**
 * One thing the shop should know about — the counter's own (a set-up step, the licence, a
 * closed day) or a notice from Magic Bill. One shape, one list, one rule for the bell: the
 * number is what has not been looked at; the list is everything current.
 */
export interface Alert {
  /** Stable, so a re-render does not make the same alert twice. */
  id: string;
  tone: 'info' | 'warn' | 'danger' | 'accent';
  icon: IconName;
  /** A short heading — what it is. */
  title: string;
  /** The whole sentence, written in Rust. */
  says: string;
  /** Looked at already: it stays in the list, and stops counting on the bell. */
  seen: boolean;
  /** The screen that fixes it, if there is one. */
  goTo?: string;
  goLabel?: string;
  /** "8 Aug, 4:32 pm" — a notice turned up at a time; a condition simply holds. */
  when?: string;
  /** Who it is from, when it is not the counter itself: "Magic Bill". */
  from?: string;
}

/**
 * What Rust remembers an alert by once it has been looked at. The sentence is part of it, so
 * an alert that changes what it says — a licence with fewer days left — is new again.
 */
export function alertKey(alert: Pick<Alert, 'id' | 'says'>): string {
  return `${alert.id}:${alert.says}`;
}

/** How loud the bell is: the worst tone of anything current, read or not. */
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
  /** Unseen first, so what rang the bell is at the top. */
  alerts: readonly Alert[];
  onGo: (screen: string) => void;
  onClose: () => void;
}) {
  return (
    <>
      {/* Pressing anywhere else closes it. */}
      <button
        type="button"
        className="mb-alerts__away"
        aria-label="Close the alerts"
        onClick={onClose}
      />
      <section className="mb-alerts" aria-label="Alerts">
        {/* The head stays put while the list under it scrolls. */}
        <div className="mb-alerts__head">
          <SectionHeader
            title="Alerts"
            action={
              <Button
                variant="quiet"
                size="sm"
                iconOnly
                icon={<Icon name="close" size="sm" />}
                title="Close the alerts"
                aria-label="Close the alerts"
                onClick={onClose}
              />
            }
          />
        </div>

        {alerts.length === 0 ? (
          <p className="mb-alerts__quiet">
            Nothing needs you. Anything the counter wants to tell you turns up here.
          </p>
        ) : (
          <ul className="mb-alerts__list">
            {alerts.map((alert) => (
              <li key={alert.id} className={`mb-alerts__one mb-alerts__one--${alert.tone}`}>
                <Icon name={alert.icon} size="sm" className="mb-alerts__icon" />
                <div className="mb-alerts__body">
                  {/* Where it came from, over the heading: the panel is the counter's own voice unless it says otherwise. */}
                  {alert.from ? <span className="mb-alerts__from">From {alert.from}</span> : null}
                  <span className="mb-alerts__what">{alert.title}</span>
                  {alert.says ? <span className="mb-alerts__says">{alert.says}</span> : null}
                  {alert.when ? <span className="mb-alerts__when">{alert.when}</span> : null}
                  {/* Under the words, not beside them: a panel this narrow has one column. */}
                  {alert.goTo ? (
                    <Button
                      className="mb-alerts__do"
                      size="sm"
                      variant="secondary"
                      onClick={() => {
                        onGo(alert.goTo as string);
                        onClose();
                      }}
                    >
                      {alert.goLabel ?? 'Do it'}
                    </Button>
                  ) : null}
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>
    </>
  );
}
