/** Applying a theme, and that is all this does. The machine's browser storage is the one store. */

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
  LIGHT,
  NORMAL_TEXT,
  THEMES,
  themeById,
  toggleTarget,
  type Theme,
  type ThemeId,
} from './themes';
import { keep, remember } from '../remember';

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

const THEME_KEY = 'mb.theme';
const TEXT_SIZE_KEY = 'mb.textSize';

/** Put the look on the document. */
function apply(themeId: string, textSize: string): void {
  const root = document.documentElement;
  root.setAttribute('data-theme', (themeById(themeId as ThemeId) ?? LIGHT).id);
  const size = TEXT_SIZES.find((s) => s.id === textSize) ?? NORMAL_TEXT;
  // One variable. Every type size in tokens.css is calc()'d from it, so this reaches the cart,
  // the dialogs and the receipt preview at once.
  root.style.setProperty('--type-scale', String(size.scale));
}

/** Paint the remembered look before React mounts, so the window never flashes the wrong colours. */
export function applyRememberedLook(): void {
  apply(remember(THEME_KEY, DEFAULT_THEME), remember(TEXT_SIZE_KEY, DEFAULT_TEXT_SIZE));
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [themeId, setThemeId] = useState<ThemeId>(() =>
    remember(THEME_KEY, DEFAULT_THEME),
  );
  const [textSize, setTextSizeId] = useState<string>(() =>
    remember(TEXT_SIZE_KEY, DEFAULT_TEXT_SIZE),
  );

  const theme = themeById(themeId) ?? LIGHT;

  useEffect(() => apply(theme.id, textSize), [theme.id, textSize]);

  const setTheme = useCallback((id: ThemeId) => {
    setThemeId(id);
    keep(THEME_KEY, id);
  }, []);

  const setTextSize = useCallback((id: string) => {
    setTextSizeId(id);
    keep(TEXT_SIZE_KEY, id);
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
