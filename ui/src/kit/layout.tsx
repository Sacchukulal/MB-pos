/**
 * **The layout primitives.** P27.5, and the reason the app stopped looking
 * "here and there".
 *
 * # What was wrong
 *
 * There was no `Page`, no `PageHeader`, no `Toolbar` and no `Panel`. So Stock's
 * header, Buying's header, Menu's header, Reports' header and Settings' header
 * were five separate pieces of CSS that were each meant to be the same thing.
 * They were not the same — different padding, different title size, different
 * gap to the content, different place for the actions — and they could never
 * have become the same by editing values, because the shape did not exist in
 * one place to edit.
 *
 * Nineteen sessions each did the reasonable thing in isolation. The result of
 * nineteen reasonable local decisions is a product that does not line up.
 *
 * # The rule these exist to enforce
 *
 * **A screen must not be able to invent its own layout.** After P27.5 a feature
 * CSS file describes only what is unique to that screen: the shape of a table
 * tile, the columns of the day close. The page margin, the header, the toolbar,
 * the gaps between sections and the surface a panel sits on all come from here,
 * and `scripts/check-layout.mjs` fails the build on a screen that sets its own
 * page padding or hand-rolls a header.
 *
 * If a screen cannot say what it needs with these, the answer is a new
 * primitive in this file — never a one-off in that screen.
 */

import type { ReactNode } from 'react';

import { Icon, type IconName } from './Icon';

/**
 * The page. **Owns the page margin and nothing else owns it.**
 *
 * `scroll` is the common case and the default: the header stays put and the
 * body scrolls under it, which is what stops a screen's own title scrolling
 * away from the table it names. Billing passes `scroll={false}` because it
 * manages its own three columns and must never scroll as a whole.
 */
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
    <div
      className={['mb-page', scroll ? 'mb-page--scroll' : '', className]
        .filter(Boolean)
        .join(' ')}
    >
      {children}
    </div>
  );
}

/**
 * The one page header in the product.
 *
 * `count` is set apart from the title rather than glued into it, because
 * "Menu (43)" is a title a translator cannot move and a number the eye has to
 * pick out of a word. Actions sit right, which is where a hand goes for them.
 */
export function PageHeader({
  title,
  subtitle,
  count,
  actions,
}: {
  title: string;
  subtitle?: string;
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
        </div>
        {subtitle ? <p className="mb-pagehead__sub">{subtitle}</p> : null}
      </div>
      {actions ? <div className="mb-pagehead__actions">{actions}</div> : null}
    </header>
  );
}

/**
 * Filters, a search box, a view switch — the controls that change what the
 * page below is showing, as opposed to the actions in the header that change
 * the shop.
 *
 * Always directly under the header, always the same height, always the same
 * gap. The floor screen used to put its section chips on the left as filled
 * accent squares and its view tabs on the right as bare text, on one row, for
 * the same kind of choice. Two control languages in one strip is the specific
 * thing this stops.
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

/**
 * A raised surface on the page. **The only thing in the product that is
 * raised**, per the elevation contract in `tokens.css`: the page is flat,
 * panels sit on it, dialogs pop over it, and there is no fourth level.
 *
 * `flush` drops the padding for a panel whose whole content is a table — a
 * table inside a padded panel reads as a table in a box in a box (§5, "avoid
 * cards inside cards").
 */
export function Panel({
  title,
  note,
  actions,
  children,
  flush = false,
  className,
}: {
  title?: string;
  note?: string;
  actions?: ReactNode;
  children: ReactNode;
  flush?: boolean;
  className?: string;
}) {
  return (
    <section
      className={['mb-panel', flush ? 'mb-panel--flush' : '', className]
        .filter(Boolean)
        .join(' ')}
    >
      {title || actions ? (
        <div className="mb-panel__head">
          <div className="mb-panel__what">
            {title ? <h2 className="mb-panel__title">{title}</h2> : null}
            {note ? <span className="mb-panel__note">{note}</span> : null}
          </div>
          {actions ? <div className="mb-panel__actions">{actions}</div> : null}
        </div>
      ) : null}
      <div className="mb-panel__body">{children}</div>
    </section>
  );
}

/**
 * A run of sections down a page, with the section gap between them. Use it
 * rather than stacking `Panel`s directly, so the gap is decided once.
 */
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
    <div className={['mb-fields', columns ? 'mb-fields--columns' : ''].filter(Boolean).join(' ')}>
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
      className={[
        'mb-row',
        `mb-row--gap-${gap}`,
        end ? 'mb-row--end' : '',
        wrap ? 'mb-row--wrap' : '',
      ]
        .filter(Boolean)
        .join(' ')}
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
    <div className={['mb-stack', `mb-stack--gap-${gap}`, className].filter(Boolean).join(' ')}>
      {children}
    </div>
  );
}

/**
 * A message about the state of the page, in a sentence, with the form signal
 * as well as the colour (§2 rule 2).
 *
 * One place turns a machine state into words (§6), and this is the shape those
 * words arrive in — so the licence line, the no-PIN banner and the till-queue
 * banner stop being three hand-written strips that happen to look similar.
 */
export function Notice({
  tone = 'info',
  icon,
  children,
  action,
}: {
  tone?: 'info' | 'ok' | 'warn' | 'danger' | 'accent';
  icon?: IconName;
  children: ReactNode;
  action?: ReactNode;
}) {
  const fallback: IconName =
    tone === 'danger' || tone === 'warn' ? 'warning' : tone === 'ok' ? 'check' : 'info';
  return (
    <div className={`mb-notice mb-notice--${tone}`} role="status">
      <Icon name={icon ?? fallback} size="sm" className="mb-notice__icon" />
      <div className="mb-notice__says">{children}</div>
      {action ? <div className="mb-notice__action">{action}</div> : null}
    </div>
  );
}
