/** The theme registry — and it is deliberately almost empty. */

export type ThemeId = string;

export interface Theme {
  readonly id: ThemeId;
  /** What the shop sees in the settings list. */
  readonly name: string;
  /** Which face the toggle shows while this theme is active. */
  readonly icon: 'sun' | 'moon';
  /** Whether this theme counts as the dark one for the sun/moon toggle. */
  readonly appearance: 'light' | 'dark';
}

/**
 * The one theme that is guaranteed to exist, so nothing has to handle the case where the
 * registry is empty.
 */
export const LIGHT: Theme = {
  id: 'light',
  name: 'Light',
  icon: 'sun',
  appearance: 'light',
};

/*
 * Two, and only two. The light theme is black on white at 15:1 and up, so it IS the
 * high-contrast theme; a third block was a second way to say the same thing (owner, 2026-09-03).
 */
export const THEMES: readonly Theme[] = [
  LIGHT,
  { id: 'dark', name: 'Dark', icon: 'moon', appearance: 'dark' },
];

export const DEFAULT_THEME: ThemeId = LIGHT.id;

/** Text size is a token too (`--type-scale`), so this is also just data. */
export interface TextSize {
  readonly id: string;
  readonly name: string;
  readonly scale: number;
}

/** "no font scaling for older owners". */
/** The default, as a value rather than an index. */
export const NORMAL_TEXT: TextSize = { id: 'normal', name: 'Normal', scale: 1 };

export const TEXT_SIZES: readonly TextSize[] = [
  { id: 'small', name: 'Small', scale: 0.9 },
  NORMAL_TEXT,
  { id: 'large', name: 'Large', scale: 1.15 },
  { id: 'xlarge', name: 'Extra large', scale: 1.3 },
];

export const DEFAULT_TEXT_SIZE = NORMAL_TEXT.id;

export function themeById(id: ThemeId): Theme | undefined {
  return THEMES.find((t) => t.id === id);
}

/** What the sun/moon button switches to. */
export function toggleTarget(current: ThemeId): ThemeId {
  const theme = themeById(current);
  if (!theme) return DEFAULT_THEME;
  const wanted = theme.appearance === 'light' ? 'dark' : 'light';
  return THEMES.find((t) => t.appearance === wanted)?.id ?? DEFAULT_THEME;
}
