import { Button, Caption, Icon, SectionHeader, type IconName } from '../kit';
import type { NoticeView } from '../ipc/generated/NoticeView';

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
  notices = [],
  onGo,
  onClose,
}: {
  alerts: readonly Alert[];
  /** From Magic Bill, newest first. Shown under the counter's own alerts. */
  notices?: readonly NoticeView[];
  onGo: (screen: string) => void;
  onClose: () => void;
}) {
  const nothing = alerts.length === 0 && notices.length === 0;

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

        {nothing ? (
          <p className="mb-alerts__quiet">
            Nothing needs you. Anything the counter wants to tell you turns up here.
          </p>
        ) : null}

        {alerts.length > 0 ? (
          <ul className="mb-alerts__list">
            {alerts.map((alert) => (
              <li key={alert.id} className={`mb-alerts__one mb-alerts__one--${alert.tone}`}>
                <Icon name={alert.icon} size="sm" className="mb-alerts__icon" />
                <div className="mb-alerts__body">
                  <span className="mb-alerts__what">{alert.title}</span>
                  <span className="mb-alerts__says">{alert.says}</span>
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
        ) : null}

        {/* Notices from Magic Bill. Read once the panel opens; the number on the bell counts the unread. */}
        {notices.length > 0 ? (
          <>
            <div className="mb-alerts__group">
              <Caption>From Magic Bill</Caption>
            </div>
            <ul className="mb-alerts__list" aria-label="Notices from Magic Bill">
              {notices.map((notice) => (
                <li
                  key={notice.id}
                  className={`mb-alerts__one mb-alerts__one--${notice.isSeen ? 'info' : 'accent'}`}
                >
                  <Icon name="info" size="sm" className="mb-alerts__icon" />
                  <div className="mb-alerts__body">
                    <span className="mb-alerts__what">{notice.title}</span>
                    {notice.body ? <span className="mb-alerts__says">{notice.body}</span> : null}
                    <span className="mb-alerts__when">{notice.when}</span>
                  </div>
                </li>
              ))}
            </ul>
          </>
        ) : null}
      </section>
    </>
  );
}
