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

import { forwardRef, type ReactNode } from 'react';

import { cx } from './cx';
import { Icon, type IconName } from './Icon';
import { InfoTip } from './InfoTip';

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
    <div className={cx('mb-page', scroll && 'mb-page--scroll', className)}>
      {children}
    </div>
  );
}

/**
 * An area that scrolls. **The only thing in the product that scrolls.**
 *
 * A screen keeps its own layout on this element — pass it as `className`, the
 * same way `Page` takes one — and gets the scrolling from here.
 *
 * The bar sits in the page margin rather than inside the content, so a table
 * in a scroller still lines up with the header above it. Every screen used to
 * write its own `overflow-y: auto`, which put the bar inside and left the
 * content 12px short of everything else on the page.
 *
 * `inset` is for a scroller that is not at the page's right edge — a side
 * panel, a rail, a dialog body — where there is no page margin to reach into.
 */
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
  note,
  count,
  actions,
}: {
  title: string;
  /**
   * **A live FACT about what is on screen, never an explanation of the
   * screen** — Stock's "4 items low, 1 out". If it is the same sentence every
   * time the screen opens, it is an explanation: put it in `note`.
   */
  subtitle?: string;
  /** What the screen is for, as a tip you can ask for. Never a paragraph. */
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
 * A panel down the side that folds away to a small button.
 *
 * The button that opens it and the chevron that closes it are in the same
 * place — the top of the column the panel opens into.
 *
 * `allowed={false}` hides it entirely. Rust's guard is the real control.
 */
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
  /** What is inside the panel. Only rendered while it is open. */
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
      ) : (
        <button
          type="button"
          className="mb-sidefold__strip"
          title={label}
          aria-label={label}
          aria-expanded={false}
          onClick={onOpen}
        >
          <Icon name="plus" size="sm" />
        </button>
      )}
      <div className="mb-sidefold__main">{children}</div>
    </div>
  );
}

/**
 * The way out and the Save, at the foot of something that scrolls.
 *
 * Pinned, so it is never below the fold — which is where Save sat in the item
 * panel and in any dialog that filled the window. Put it last inside a `Fields`
 * or a `Stack`; it takes the slack above it and sticks to the bottom edge.
 */
export function Foot({ children }: { children: ReactNode }) {
  return <div className="mb-foot">{children}</div>;
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
  /**
   * The explanation, as something you can **ask** for.
   *
   * Not drawn under the title — it becomes an `InfoTip` beside it. Same change
   * and same reason as `SectionHeader`: the owner, 2026-08-22, *"it makes the
   * app look cluttered and un professional… make it a kind of popup text, when
   * hovered."*
   */
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
  standing = false,
}: {
  tone?: 'info' | 'ok' | 'warn' | 'danger' | 'accent';
  icon?: IconName;
  children: ReactNode;
  action?: ReactNode;
  /**
   * **True for a banner that is always there** — the licence line, the no-PIN
   * warning, the held-bills line. It is a status strip rather than a card: one
   * line, tight above and below, centred on its icon.
   *
   * P30.5, and the owner's fresh install is why. A shop with no licence key sees
   * the licence line on every screen for as long as it takes them to buy one,
   * and a shop with no PIN sees that one too — so on a new install two cards
   * with card-sized padding sat between the top bar and the till, on a 768-pixel
   * screen, for ever. The words are the same; the box stops shouting.
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
