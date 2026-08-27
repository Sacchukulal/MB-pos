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

/** How loud the bell is: the worst tone of anything waiting. */
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
      {/* Pressing anywhere else closes it. */}
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
