/**
 * Applying a theme, and that is all this does.
 *
 * It sets two attributes on `<html>` and stores the choice. It does not know
 * what a colour is, and no component below it does either — which is what
 * makes the owner's ruling true rather than merely intended: swapping the look
 * touches `tokens.css`, and this file does not change.
 *
 * **Why an attribute and not a class or a context value.** CSS custom
 * properties cascade from `<html>`, so one attribute restyles the entire tree
 * — including anything rendered into a portal, which a class on a React root
 * would miss. It is also why the theme swap costs nothing at runtime: no
 * component re-renders when the theme changes, because no component reads the
 * theme.
 */

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';

import {
  DEFAULT_TEXT_SIZE,
  DEFAULT_THEME,
  TEXT_SIZES,
  THEMES,
  themeById,
  toggleTarget,
  type Theme,
  type ThemeId,
} from './themes';

interface ThemeState {
  readonly theme: Theme;
  readonly themes: readonly Theme[];
  readonly textSize: string;
  setTheme: (id: ThemeId) => void;
  /** The sun/moon button. */
  toggle: () => void;
  setTextSize: (id: string) => void;
}

const ThemeContext = createContext<ThemeState | null>(null);

/**
 * Where the choice is kept until P17 owns a settings screen.
 *
 * Deliberately NOT the database: the theme has to be applied before anything
 * is opened, and audit A5 is a whole finding about a v1 that kept load-bearing
 * state where it could vanish. This is a look preference and losing it costs a
 * click; the database path is not, and lives in the app config file (P08 item
 * 2, step 2).
 */
const THEME_KEY = 'mb.theme';
const TEXT_SIZE_KEY = 'mb.textSize';

function read(key: string, fallback: string): string {
  try {
    return window.localStorage.getItem(key) ?? fallback;
  } catch {
    // A webview with storage disabled still has to open and still has to be
    // readable. It just forgets the choice.
    return fallback;
  }
}

function write(key: string, value: string): void {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // See above. Never throw out of a look preference.
  }
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [themeId, setThemeId] = useState<ThemeId>(() =>
    read(THEME_KEY, DEFAULT_THEME),
  );
  const [textSize, setTextSizeId] = useState<string>(() =>
    read(TEXT_SIZE_KEY, DEFAULT_TEXT_SIZE),
  );

  const theme = themeById(themeId) ?? themeById(DEFAULT_THEME) ?? THEMES[0];

  useEffect(() => {
    const root = document.documentElement;
    root.setAttribute('data-theme', theme.id);
    const size = TEXT_SIZES.find((s) => s.id === textSize) ?? TEXT_SIZES[1];
    // One variable. Every type size in tokens.css is calc()'d from it, so
    // this reaches the cart, the dialogs and the receipt preview at once.
    root.style.setProperty('--type-scale', String(size.scale));
  }, [theme.id, textSize]);

  const setTheme = useCallback((id: ThemeId) => {
    setThemeId(id);
    write(THEME_KEY, id);
  }, []);

  const setTextSize = useCallback((id: string) => {
    setTextSizeId(id);
    write(TEXT_SIZE_KEY, id);
  }, []);

  const toggle = useCallback(() => {
    setTheme(toggleTarget(theme.id));
  }, [setTheme, theme.id]);

  const value = useMemo<ThemeState>(
    () => ({
      theme,
      themes: THEMES,
      textSize,
      setTheme,
      toggle,
      setTextSize,
    }),
    [theme, textSize, setTheme, toggle, setTextSize],
  );

  return (
    <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
  );
}

export function useTheme(): ThemeState {
  const found = useContext(ThemeContext);
  if (!found) {
    throw new Error('useTheme was called outside ThemeProvider');
  }
  return found;
}
