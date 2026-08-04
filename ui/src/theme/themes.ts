/**
 * The theme registry — and it is deliberately almost empty.
 *
 * A theme is **data**: a block of values in `tokens.css` and one line here.
 * This file holds the id, the name a shop sees, and which icon the sun/moon
 * toggle shows. It holds no colour, because a colour here would be a colour
 * outside `tokens.css`, which is the one thing the whole system forbids.
 *
 * The owner's ruling, 2026-08-04:
 *
 * > *"Design a central theme system so that in future it can be changed easily
 * > with my suggestion without touching any functionality of the app."*
 *
 * Adding a theme is therefore: copy a block in `tokens.css`, add a line below.
 * `kit/__tests__/theme.test.tsx` adds a throwaway theme that way and asserts
 * it renders — so if this ever stops being true, the build says so.
 */

export type ThemeId = string;

export interface Theme {
  readonly id: ThemeId;
  /** What the shop sees in the settings list. Translated at P23. */
  readonly name: string;
  /** Which face the toggle shows while this theme is active. */
  readonly icon: 'sun' | 'moon' | 'contrast';
  /**
   * Whether this theme counts as the dark one for the sun/moon toggle.
   * The toggle flips between the first light and the first dark theme;
   * anything else is chosen from settings (P17).
   */
  readonly appearance: 'light' | 'dark';
}

export const THEMES: readonly Theme[] = [
  { id: 'light', name: 'Light', icon: 'sun', appearance: 'light' },
  { id: 'dark', name: 'Dark', icon: 'moon', appearance: 'dark' },
  {
    id: 'contrast',
    name: 'High contrast',
    icon: 'contrast',
    appearance: 'light',
  },
];

export const DEFAULT_THEME: ThemeId = 'light';

/** Text size is a token too (`--type-scale`), so this is also just data. */
export interface TextSize {
  readonly id: string;
  readonly name: string;
  readonly scale: number;
}

/**
 * Audit F9: *"no font scaling for older owners"*. UI_GUIDELINES §3: *"many
 * owners are 50+ and the counter screen is across a desk."*
 */
export const TEXT_SIZES: readonly TextSize[] = [
  { id: 'small', name: 'Small', scale: 0.9 },
  { id: 'normal', name: 'Normal', scale: 1 },
  { id: 'large', name: 'Large', scale: 1.15 },
  { id: 'xlarge', name: 'Extra large', scale: 1.3 },
];

export const DEFAULT_TEXT_SIZE = 'normal';

export function themeById(id: ThemeId): Theme | undefined {
  return THEMES.find((t) => t.id === id);
}

/**
 * What the sun/moon button switches to.
 *
 * From a light theme it goes to the first dark one and back again; from
 * anything else (high contrast, or whatever the owner adds later) it returns
 * to the default rather than guessing.
 */
export function toggleTarget(current: ThemeId): ThemeId {
  const theme = themeById(current);
  if (!theme) return DEFAULT_THEME;
  const wanted = theme.appearance === 'light' ? 'dark' : 'light';
  return THEMES.find((t) => t.appearance === wanted)?.id ?? DEFAULT_THEME;
}
