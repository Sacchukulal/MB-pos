/**
 * **The icon set.** One set, one stroke weight, one optical size.
 *
 * # Why this file exists
 *
 * Until P27.5 this product had no icons. The left rail — the first thing
 * anybody saw, on every screen, all day — drew its navigation with bare Unicode
 * glyphs: `▦` for Floor, `☰` for Credit, `⌁` for Spends, `⬒` for Stock, `⇩` for
 * Buying. Which glyph a shop actually got depended on which font Windows chose
 * to substitute, so they arrived at different weights, different sizes and
 * different vertical positions. That single fact was most of what the owner
 * meant by *"it looks old-styled and unprofessional"*, and a shopkeeper read it
 * in the first two seconds.
 *
 * # The rules this file exists to keep
 *
 * 1. **One geometry.** Every icon is drawn on the same 24×24 grid, with the
 *    same 1.75 stroke, the same round cap and the same round join. Nothing here
 *    is filled. An icon that needs a different weight to look right is an icon
 *    that is drawn wrong.
 * 2. **`currentColor`, always.** An icon is text as far as colour is concerned,
 *    so it inherits — which is what makes it work in light, dark, contrast and
 *    any theme the owner adds later without this file knowing (D21).
 * 3. **Sized by the type scale, not by pixels.** `--icon` and friends are
 *    tokens, so the text-size setting scales icons with the words next to them
 *    rather than leaving them stranded.
 * 4. **Drawn here, not fetched.** No icon font, no CDN, no npm package. This
 *    app runs in a shop with the internet unplugged and that is a supported
 *    state, not a degraded one — an icon set that arrives over a network is an
 *    icon set that is sometimes a row of empty boxes. Under R6 that also means
 *    this costs no dependency at all: the geometry below is ours.
 *
 * # Adding one
 *
 * Add a line to `IconName` and a path to `PATHS`. `tsc` then fails on any typo
 * at the call site, which is the point of the union — a mistyped icon name used
 * to render nothing at all, silently, on whichever screen nobody opened.
 */

import type { ReactNode } from 'react';
import { cx } from './cx';

export type IconName =
  // The screens.
  | 'receipt'
  | 'grid'
  | 'wallet'
  | 'banknote'
  | 'boxes'
  | 'truck'
  | 'file'
  | 'chart'
  | 'book'
  | 'users'
  | 'clock'
  | 'settings'
  | 'badge'
  | 'flame'
  | 'pulse'
  // The window and the title bar.
  | 'printer'
  | 'lock'
  | 'sun'
  | 'moon'
  | 'contrast'
  | 'minimise'
  | 'maximise'
  | 'close'
  // Ordinary work.
  | 'search'
  | 'plus'
  | 'minus'
  | 'check'
  | 'x'
  | 'chevron-down'
  | 'chevron-up'
  | 'chevron-left'
  | 'chevron-right'
  | 'pencil'
  | 'trash'
  | 'warning'
  | 'bell'
  | 'info'
  | 'refresh'
  | 'arrow-right'
  | 'calendar'
  | 'filter'
  | 'more'
  | 'folder'
  | 'download'
  | 'upload'
  | 'user'
  | 'phone'
  | 'tag'
  | 'table'
  | 'parcel'
  | 'cash'
  | 'card'
  | 'qr'
  // P29 — the things a counter is plugged into, and the bike outside.
  | 'bike'
  | 'scan'
  | 'scale'
  | 'monitor'
  | 'plug';

/**
 * The geometry. 24×24, stroked, never filled.
 *
 * Kept as bare children rather than whole `<svg>` elements so that the wrapper
 * below owns the size, the stroke and the accessibility in ONE place — an icon
 * that carried its own `stroke-width` is an icon that would drift.
 */
const PATHS: Record<IconName, ReactNode> = {
  // ---- the screens -------------------------------------------------------
  receipt: (
    <>
      <path d="M5 3.5h14v17l-2.5-1.5L14 20.5 11.5 19 9 20.5 6.5 19 5 20.5z" />
      <path d="M9 8h6M9 12h6" />
    </>
  ),
  grid: (
    <>
      <rect x="3.5" y="3.5" width="7" height="7" rx="1.5" />
      <rect x="13.5" y="3.5" width="7" height="7" rx="1.5" />
      <rect x="3.5" y="13.5" width="7" height="7" rx="1.5" />
      <rect x="13.5" y="13.5" width="7" height="7" rx="1.5" />
    </>
  ),
  wallet: (
    <>
      <path d="M3.5 7.5A2 2 0 0 1 5.5 5.5H18a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H5.5a2 2 0 0 1-2-2z" />
      <path d="M3.5 8.5h13.5a1.5 1.5 0 0 1 0 0" />
      <circle cx="16" cy="13" r="1.25" />
    </>
  ),
  banknote: (
    <>
      <rect x="2.5" y="6" width="19" height="12" rx="2" />
      <circle cx="12" cy="12" r="2.5" />
      <path d="M6 12h.01M18 12h.01" />
    </>
  ),
  boxes: (
    <>
      <path d="M12 3.5 20 7v10l-8 3.5L4 17V7z" />
      <path d="M4 7l8 3.5L20 7M12 10.5V20.5" />
    </>
  ),
  truck: (
    <>
      <path d="M2.5 6.5h11v9h-11z" />
      <path d="M13.5 10h4l3 3.5v2h-7z" />
      <circle cx="7" cy="18" r="1.75" />
      <circle cx="17" cy="18" r="1.75" />
      <path d="M8.75 18h6.5" />
    </>
  ),
  file: (
    <>
      <path d="M13.5 3.5H7a1.5 1.5 0 0 0-1.5 1.5v14A1.5 1.5 0 0 0 7 20.5h10a1.5 1.5 0 0 0 1.5-1.5V8.5z" />
      <path d="M13.5 3.5v5h5M8.5 12.5h7M8.5 16h5" />
    </>
  ),
  chart: (
    <>
      <path d="M4 20.5V4" />
      <path d="M4 20.5h16.5" />
      <path d="M8 17V11M12.5 17V7M17 17v-4" />
    </>
  ),
  book: (
    <>
      <path d="M3.5 5.5A2 2 0 0 1 5.5 3.5H11v16H5.5a2 2 0 0 0-2 2z" />
      <path d="M20.5 5.5a2 2 0 0 0-2-2H13v16h5.5a2 2 0 0 1 2 2z" />
    </>
  ),
  users: (
    <>
      <circle cx="9.5" cy="8" r="3.25" />
      <path d="M3.5 20c0-3.2 2.7-5.5 6-5.5s6 2.3 6 5.5" />
      <path d="M16 5.2a3.25 3.25 0 0 1 0 5.6M17.5 14.9c1.9.8 3 2.6 3 5.1" />
    </>
  ),
  clock: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 7v5.2l3.2 2" />
    </>
  ),
  // **Sliders, not a cog.** The first drawing here was the usual eight-spoke
  // gear, and at 20px on a dark bar it was indistinguishable from the sun in
  // the theme toggle four inches to its right — two different icons that read
  // as the same picture is worse than one ugly icon. Sliders say "the things
  // you set" and cannot be mistaken for anything else in this product.
  settings: (
    <>
      <path d="M4 7h9M17 7h3M4 17h3M11 17h9" />
      <circle cx="15" cy="7" r="2.25" />
      <circle cx="9" cy="17" r="2.25" />
    </>
  ),
  badge: (
    <>
      <circle cx="12" cy="9" r="3.5" />
      <path d="M5.5 20.5c0-3.4 2.9-6 6.5-6s6.5 2.6 6.5 6" />
      <rect x="3.5" y="3.5" width="17" height="17" rx="3" />
    </>
  ),
  // The kitchen. A flame with its own inner flame — the single-outline version
  // read as a water droplet at 20px, which is the wrong kitchen entirely.
  flame: (
    <>
      <path d="M12 2.8c3.4 3.5 6 6 6 9.7a6 6 0 0 1-12 0c0-2 .9-3.7 2.3-5.2.3 1.1 1 1.9 2 2.3-.1-2.6.6-4.8 1.7-6.8z" />
      <path d="M12 20.5a2.6 2.6 0 0 1-2.6-2.6c0-1.5 1.2-2.4 2.6-4 1.4 1.6 2.6 2.5 2.6 4a2.6 2.6 0 0 1-2.6 2.6z" />
    </>
  ),
  pulse: (
    <>
      <path d="M2.5 12h4l2.5-6 4 12 2.5-6h6" />
    </>
  ),

  // ---- the window and the title bar --------------------------------------
  printer: (
    <>
      <path d="M7 8.5V3.5h10v5" />
      <path d="M5 8.5h14a2 2 0 0 1 2 2v5h-4v5H7v-5H3v-5a2 2 0 0 1 2-2z" />
      <path d="M7 15.5h10" />
    </>
  ),
  lock: (
    <>
      <rect x="4.5" y="10.5" width="15" height="10" rx="2" />
      <path d="M8 10.5V7.5a4 4 0 0 1 8 0v3" />
    </>
  ),
  sun: (
    <>
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2.5v2M12 19.5v2M21.5 12h-2M4.5 12h-2M18.4 5.6 17 7M7 17l-1.4 1.4M18.4 18.4 17 17M7 7 5.6 5.6" />
    </>
  ),
  moon: (
    <>
      <path d="M20 14.5A8.5 8.5 0 0 1 9.5 4 8.5 8.5 0 1 0 20 14.5z" />
    </>
  ),
  contrast: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 3.5a8.5 8.5 0 0 1 0 17z" />
    </>
  ),
  // The window buttons. Drawn rather than typed, so they sit on the same grid
  // as everything else — the old ones were "–", "□" and "✕" from three
  // different fonts and never lined up with each other.
  minimise: <path d="M5.5 12h13" />,
  maximise: <rect x="5.5" y="5.5" width="13" height="13" rx="1.5" />,
  close: <path d="M6 6l12 12M18 6 6 18" />,

  // ---- ordinary work -----------------------------------------------------
  search: (
    <>
      <circle cx="10.5" cy="10.5" r="6.5" />
      <path d="M15.5 15.5 20.5 20.5" />
    </>
  ),
  plus: <path d="M12 5.5v13M5.5 12h13" />,
  minus: <path d="M5.5 12h13" />,
  check: <path d="M5 12.5 9.5 17 19 7.5" />,
  x: <path d="M6.5 6.5l11 11M17.5 6.5l-11 11" />,
  'chevron-down': <path d="M6 9.5 12 15.5 18 9.5" />,
  'chevron-up': <path d="M6 14.5 12 8.5 18 14.5" />,
  'chevron-left': <path d="M14.5 6 8.5 12 14.5 18" />,
  'chevron-right': <path d="M9.5 6 15.5 12 9.5 18" />,
  pencil: (
    <>
      <path d="M4 20h4L19.5 8.5a2.1 2.1 0 0 0-3-3L5 17z" />
      <path d="M14.5 6.5l3 3" />
    </>
  ),
  trash: (
    <>
      <path d="M4 6.5h16M9.5 6.5V4.5h5v2" />
      <path d="M6 6.5l1 13a1.5 1.5 0 0 0 1.5 1.4h7A1.5 1.5 0 0 0 17 19.5l1-13" />
      <path d="M10 10.5v6.5M14 10.5v6.5" />
    </>
  ),
  warning: (
    <>
      <path d="M12 3.8 21.5 20H2.5z" />
      <path d="M12 9.5v5M12 17.5h.01" />
    </>
  ),
  /** The alerts button (P30.6). Everything that used to be a banner lives
      behind this, so the counter is the counter. */
  bell: (
    <>
      <path d="M18 8.5a6 6 0 1 0-12 0c0 5-2 6.5-2 6.5h16s-2-1.5-2-6.5" />
      <path d="M13.7 19a2 2 0 0 1-3.4 0" />
    </>
  ),
  info: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 11v5.5M12 7.8h.01" />
    </>
  ),
  refresh: (
    <>
      <path d="M20 12a8 8 0 1 1-2.6-5.9" />
      <path d="M20.5 4v4.5H16" />
    </>
  ),
  'arrow-right': <path d="M4.5 12h14M13 6.5l5.5 5.5-5.5 5.5" />,
  calendar: (
    <>
      <rect x="3.5" y="5.5" width="17" height="15" rx="2" />
      <path d="M3.5 10h17M8 3.5v4M16 3.5v4" />
    </>
  ),
  filter: <path d="M3.5 5.5h17l-6.5 7.5v6l-4 2v-8z" />,
  more: (
    <>
      <circle cx="5.5" cy="12" r="1.4" />
      <circle cx="12" cy="12" r="1.4" />
      <circle cx="18.5" cy="12" r="1.4" />
    </>
  ),
  // P31. Browse — for choosing a logo, and for choosing where a shop lives.
  folder: (
    <path d="M3.5 6.5a1 1 0 0 1 1-1h4l2 2.5h8a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1h-14a1 1 0 0 1-1-1z" />
  ),
  download: <path d="M12 3.5v12M7 10.5l5 5 5-5M4.5 20.5h15" />,
  upload: <path d="M12 20.5v-12M7 13.5l5-5 5 5M4.5 3.5h15" />,
  user: (
    <>
      <circle cx="12" cy="8" r="3.75" />
      <path d="M4.5 20.5c0-4 3.4-7 7.5-7s7.5 3 7.5 7" />
    </>
  ),
  phone: (
    <>
      <path d="M6.5 3.5h4l1.5 4.5-2.2 1.6a12 12 0 0 0 4.6 4.6L16 12l4.5 1.5v4a2 2 0 0 1-2.2 2C10.6 18.8 5.2 13.4 4.5 5.7a2 2 0 0 1 2-2.2z" />
    </>
  ),
  tag: (
    <>
      <path d="M3.5 11V4.5H10L20 14.5a2 2 0 0 1 0 2.8l-3.7 3.7a2 2 0 0 1-2.8 0z" />
      <circle cx="7.5" cy="8.5" r="1.4" />
    </>
  ),
  table: (
    <>
      <rect x="3.5" y="5.5" width="17" height="13" rx="2" />
      <path d="M3.5 10.5h17M9.5 10.5v8" />
    </>
  ),
  parcel: (
    <>
      <path d="M3.5 7.5 12 3.5l8.5 4v9L12 20.5l-8.5-4z" />
      <path d="M3.5 7.5 12 11.5l8.5-4M12 11.5v9M7.75 5.5l8.5 4" />
    </>
  ),
  cash: (
    <>
      <rect x="2.5" y="6.5" width="19" height="11" rx="2" />
      <circle cx="12" cy="12" r="2.5" />
    </>
  ),
  card: (
    <>
      <rect x="2.5" y="5.5" width="19" height="13" rx="2" />
      <path d="M2.5 10h19M6 14.5h4" />
    </>
  ),
  qr: (
    <>
      <rect x="3.5" y="3.5" width="7" height="7" rx="1" />
      <rect x="13.5" y="3.5" width="7" height="7" rx="1" />
      <rect x="3.5" y="13.5" width="7" height="7" rx="1" />
      <path d="M13.5 13.5h3v3h-3zM20.5 13.5v3M17.5 20.5h3M13.5 20.5h.01" />
    </>
  ),
  // ---- P29 ----------------------------------------------------------------
  bike: (
    <>
      <circle cx="5.5" cy="16.5" r="3" />
      <circle cx="18.5" cy="16.5" r="3" />
      <path d="M5.5 16.5h7l3-7h-3" />
      <path d="M15.5 9.5h2.5l1.5 7" />
      <path d="M9.5 6.5h3" />
    </>
  ),
  scan: (
    <>
      <path d="M3.5 8V5.5a2 2 0 0 1 2-2H8M16 3.5h2.5a2 2 0 0 1 2 2V8" />
      <path d="M20.5 16v2.5a2 2 0 0 1-2 2H16M8 20.5H5.5a2 2 0 0 1-2-2V16" />
      <path d="M3.5 12h17" />
    </>
  ),
  scale: (
    <>
      <path d="M4 20.5h16" />
      <path d="M5.5 20.5V13h13v7.5" />
      <path d="M8.5 13V9.5a3.5 3.5 0 0 1 7 0V13" />
      <path d="M9 16.5h6" />
    </>
  ),
  monitor: (
    <>
      <rect x="2.5" y="4" width="19" height="12.5" rx="2" />
      <path d="M9 20.5h6M12 16.5v4" />
    </>
  ),
  plug: (
    <>
      <path d="M9 3.5v5M15 3.5v5" />
      <path d="M6.5 8.5h11v3a5.5 5.5 0 0 1-11 0z" />
      <path d="M12 17v3.5" />
    </>
  ),
};

export interface IconProps {
  name: IconName;
  /**
   * `sm` for inside a dense row, `lg` for a page header or an empty state.
   * The default sits on the body type size, which is where an icon next to a
   * word belongs.
   */
  size?: 'sm' | 'md' | 'lg';
  /**
   * A label makes the icon *the* meaning of its control, for a screen reader.
   * Leave it off — the default — when there is a visible word next to it, or
   * the reader says everything twice (§7).
   */
  label?: string;
  className?: string;
}

export function Icon({ name, size = 'md', label, className }: IconProps) {
  return (
    <svg
      className={cx('mb-icon', `mb-icon--${size}`, className)}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.75"
      strokeLinecap="round"
      strokeLinejoin="round"
      role={label ? 'img' : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      focusable="false"
    >
      {PATHS[name]}
    </svg>
  );
}
