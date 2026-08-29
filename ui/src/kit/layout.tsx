import { forwardRef, type ReactNode } from 'react';

import { cx } from './cx';
import { Icon, type IconName } from './Icon';
import { InfoTip } from './InfoTip';

/** The page. Owns the page margin and nothing else owns it. */
export function Page({
  children,
  scroll = true,
  className,
}: {
  children: ReactNode;
  scroll?: boolean;
  className?: string;
}) {
  return (
    <div className={cx('mb-page', scroll && 'mb-page--scroll', className)}>
      {children}
    </div>
  );
}

/** An area that scrolls. */
export const Scroller = forwardRef<
  HTMLDivElement,
  { children: ReactNode; inset?: boolean; className?: string }
>(function Scroller({ children, inset = false, className }, ref) {
  return (
    <div ref={ref} className={cx('mb-scroll', inset && 'mb-scroll--inset', className)}>
      {children}
    </div>
  );
});

/** The one page header in the product. */
export function PageHeader({
  title,
  subtitle,
  note,
  count,
  actions,
}: {
  title: string;
  /**
   * A live FACT about what is on screen, never an explanation of the screen — Stock's "4 items
   * low, 1 out".
   */
  subtitle?: string;
  /** What the screen is for, as a tip you can ask for. */
  note?: ReactNode;
  count?: number | string;
  actions?: ReactNode;
}) {
  return (
    <header className="mb-pagehead">
      <div className="mb-pagehead__what">
        <div className="mb-pagehead__titleline">
          <h1 className="mb-pagehead__title">{title}</h1>
          {count === undefined ? null : (
            <span className="mb-pagehead__count">{count}</span>
          )}
          {note ? <InfoTip label={`About ${title}`}>{note}</InfoTip> : null}
        </div>
        {subtitle ? <p className="mb-pagehead__sub">{subtitle}</p> : null}
      </div>
      {actions ? <div className="mb-pagehead__actions">{actions}</div> : null}
    </header>
  );
}

/**
 * Filters, a search box, a view switch — the controls that change what the page below is
 * showing, as opposed to the actions in the header that change the shop.
 */
export function Toolbar({
  children,
  end,
}: {
  children?: ReactNode;
  end?: ReactNode;
}) {
  return (
    <div className="mb-toolbar">
      <div className="mb-toolbar__start">{children}</div>
      {end ? <div className="mb-toolbar__end">{end}</div> : null}
    </div>
  );
}

/** A panel down the side that folds away to a small button. */
export function SideFold({
  open,
  label,
  onOpen,
  onFold,
  panel,
  children,
  allowed = true,
}: {
  open: boolean;
  /** What the panel is, and its heading. */
  label: string;
  onOpen: () => void;
  onFold: () => void;
  /** What is inside the panel. */
  panel: ReactNode;
  children: ReactNode;
  allowed?: boolean;
}) {
  const showing = allowed && open;
  return (
    <div
      className={cx(
        'mb-sidefold',
        showing && 'mb-sidefold--open',
        !allowed && 'mb-sidefold--none',
      )}
    >
      {!allowed ? null : showing ? (
        <aside className="mb-sidefold__panel" aria-label={label}>
          <div className="mb-sidefold__head">
            <h2 className="mb-sidefold__title">{label}</h2>
            <button
              type="button"
              className="mb-sidefold__fold"
              title={`Close ${label}`}
              aria-label={`Close ${label}`}
              aria-expanded
              onClick={onFold}
            >
              <Icon name="chevron-left" size="sm" />
            </button>
          </div>
          <Scroller inset className="mb-sidefold__body">
            {panel}
          </Scroller>
        </aside>
      ) : null}
      <div className="mb-sidefold__main">
        {/* The way in when folded: a labelled button at the top of the page. */}
        {allowed && !showing ? (
          <div className="mb-sidefold__open">
            <button
              type="button"
              className="mb-button mb-button--primary"
              aria-expanded={false}
              onClick={onOpen}
            >
              <Icon name="plus" size="sm" />
              {label}
            </button>
          </div>
        ) : null}
        {children}
      </div>
    </div>
  );
}

/** The way out and the Save, at the foot of something that scrolls. */
export function Foot({ children }: { children: ReactNode }) {
  return <div className="mb-foot">{children}</div>;
}

/** A raised surface on the page. */
export function Panel({
  title,
  note,
  actions,
  children,
  flush = false,
  className,
}: {
  title?: string;
  /** The explanation, as something you can ask for. */
  note?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  flush?: boolean;
  className?: string;
}) {
  return (
    <section
      className={cx('mb-panel', flush && 'mb-panel--flush', className)}
    >
      {title || actions ? (
        <div className="mb-panel__head">
          <div className="mb-panel__what">
            {title ? <h2 className="mb-panel__title">{title}</h2> : null}
            {note ? <InfoTip label={title ? `About ${title}` : undefined}>{note}</InfoTip> : null}
          </div>
          {actions ? <div className="mb-panel__actions">{actions}</div> : null}
        </div>
      ) : null}
      <div className="mb-panel__body">{children}</div>
    </section>
  );
}

/** A run of sections down a page, with the section gap between them. */
export function Sections({ children }: { children: ReactNode }) {
  return <div className="mb-sections">{children}</div>;
}

/** A run of fields, with the field gap between them. */
export function Fields({
  children,
  columns = false,
}: {
  children: ReactNode;
  /** Flow into as many columns as fit, rather than one long list. */
  columns?: boolean;
}) {
  return (
    <div className={cx('mb-fields', columns && 'mb-fields--columns')}>
      {children}
    </div>
  );
}

/** A run of controls across, with the field gap between them. */
export function Row({
  children,
  end = false,
  wrap = true,
  gap = 'field',
}: {
  children: ReactNode;
  /** Push to the right — where a hand goes for the way out of a dialog. */
  end?: boolean;
  wrap?: boolean;
  gap?: 'inline' | 'field' | 'group';
}) {
  return (
    <div
      className={cx(
        'mb-row',
        `mb-row--gap-${gap}`,
        end && 'mb-row--end',
        wrap && 'mb-row--wrap',
      )}
    >
      {children}
    </div>
  );
}

/** A run of things down, with a named gap between them. */
export function Stack({
  children,
  gap = 'field',
  className,
}: {
  children: ReactNode;
  gap?: 'inline' | 'field' | 'group' | 'section';
  className?: string;
}) {
  return (
    <div className={cx('mb-stack', `mb-stack--gap-${gap}`, className)}>
      {children}
    </div>
  );
}

/**
 * A message about the state of the page, in a sentence, with the form signal as well as the
 * colour.
 */
export function Notice({
  tone = 'info',
  icon,
  children,
  action,
  standing = false,
}: {
  tone?: 'info' | 'ok' | 'warn' | 'danger' | 'accent';
  icon?: IconName;
  children: ReactNode;
  action?: ReactNode;
  /**
   * True for a banner that is always there — the licence line, the no-PIN warning, the
   * held-bills line.
   */
  standing?: boolean;
}) {
  const fallback: IconName =
    tone === 'danger' || tone === 'warn' ? 'warning' : tone === 'ok' ? 'check' : 'info';
  return (
    <div
      className={cx('mb-notice', `mb-notice--${tone}`, standing && 'mb-notice--standing')}
      role="status"
    >
      <Icon name={icon ?? fallback} size="sm" className="mb-notice__icon" />
      <div className="mb-notice__says">{children}</div>
      {action ? <div className="mb-notice__action">{action}</div> : null}
    </div>
  );
}
