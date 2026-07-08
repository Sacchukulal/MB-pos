/**
 * Theme Context & Provider
 * ========================
 * Central React context that manages theme state across the app.
 *
 * Usage:
 *   import { useTheme } from '../theme/ThemeContext';
 *   const { theme, setTheme, themes } = useTheme();
 *
 * The provider reads/writes the theme preference to localStorage
 * under the key "magicbill_theme" and applies it as a data-theme
 * attribute on <html>.
 *
 * CUSTOM PALETTE (contrast-safe by design)
 * ----------------------------------------
 * The "custom" theme is built from only THREE inputs:
 *   - mode   : "dark" | "light"  (overall lightness direction)
 *   - base   : a single tint colour that seeds ALL backgrounds,
 *              surfaces, borders and text as one coherent ramp
 *   - accent : the highlight colour (buttons, active states)
 *
 * Every other token is DERIVED so backgrounds and text always live in
 * the same tonal family — guaranteeing readable contrast no matter what
 * the user picks. The values are written as inline CSS variables on
 * <html>. Custom settings persist under "magicbill_custom_colors".
 */

import { createContext, useContext, useState, useEffect, ReactNode } from "react";

export type ThemeName = "dark" | "light" | "blue" | "custom";
export type ThemeMode = "dark" | "light";

export interface ThemeOption {
  id: ThemeName;
  label: string;
  description: string;
  preview: {
    bg: string;
    panelBg: string;
    headerBg: string;
    sidebar: string;
    accent: string;
    text: string;
  };
}

/** The user-editable inputs that drive the custom palette. */
export interface CustomColors {
  mode: ThemeMode;
  base: string;   // Base tint — seeds backgrounds, surfaces, borders, text
  accent: string; // Accent — buttons, highlights, active states
}

/** Sensible starting point — a refined slate + indigo accent. */
export const DEFAULT_CUSTOM_COLORS: CustomColors = {
  mode: "dark",
  base: "#1b2233",
  accent: "#6366f1",
};

export const THEMES: ThemeOption[] = [
  {
    id: "dark",
    label: "Dark",
    description: "Easy on the eyes, perfect for night use",
    preview: { bg: "#0a0a0f", panelBg: "#111827", headerBg: "#1a1f2e", sidebar: "#0f1115", accent: "#ffffff", text: "#f0f0f5" },
  },
  {
    id: "light",
    label: "Light",
    description: "Clean and bright, ideal for well-lit spaces",
    preview: { bg: "#eef0f4", panelBg: "#ffffff", headerBg: "#e4e7ed", sidebar: "#f5f7fa", accent: "#000000", text: "#0f172a" },
  },
  {
    id: "blue",
    label: "Ocean Blue",
    description: "Cool, calm navy with a vivid blue accent",
    preview: { bg: "#0c1222", panelBg: "#111a2e", headerBg: "#162040", sidebar: "#0a1020", accent: "#3b82f6", text: "#e2e8f0" },
  },
  {
    id: "custom",
    label: "Custom",
    description: "Your base tint + accent, auto-balanced for readability",
    preview: { bg: "#11151f", panelBg: "#1b2233", headerBg: "#252e44", sidebar: "#0d111a", accent: "#6366f1", text: "#eef1f6" },
  },
];

const STORAGE_KEY = "magicbill_theme";
const CUSTOM_KEY = "magicbill_custom_colors";

interface ThemeContextValue {
  theme: ThemeName;
  setTheme: (t: ThemeName) => void;
  themes: ThemeOption[];
  customColors: CustomColors;
  setCustomColor: <K extends keyof CustomColors>(key: K, value: CustomColors[K]) => void;
  resetCustomColors: () => void;
}

const ThemeContext = createContext<ThemeContextValue | undefined>(undefined);

/* ============================================================
 * Colour maths (hex ↔ hsl, contrast, alpha)
 * ============================================================ */

function hexToRgb(hex: string): [number, number, number] {
  let h = hex.replace("#", "").trim();
  if (h.length === 3) h = h.split("").map((c) => c + c).join("");
  const n = parseInt(h, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function clampByte(v: number) {
  return Math.round(Math.max(0, Math.min(255, v)));
}

function rgbToHex(r: number, g: number, b: number): string {
  const c = (v: number) => clampByte(v).toString(16).padStart(2, "0");
  return `#${c(r)}${c(g)}${c(b)}`;
}

/** hex -> [h(0..360), s(0..1), l(0..1)] */
function hexToHsl(hex: string): [number, number, number] {
  const [r0, g0, b0] = hexToRgb(hex).map((v) => v / 255) as [number, number, number];
  const max = Math.max(r0, g0, b0);
  const min = Math.min(r0, g0, b0);
  const d = max - min;
  let h = 0;
  if (d !== 0) {
    if (max === r0) h = ((g0 - b0) / d) % 6;
    else if (max === g0) h = (b0 - r0) / d + 2;
    else h = (r0 - g0) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  const l = (max + min) / 2;
  const s = d === 0 ? 0 : d / (1 - Math.abs(2 * l - 1));
  return [h, s, l];
}

/** [h(0..360), s(0..1), l(0..1)] -> hex */
function hslToHex(h: number, s: number, l: number): string {
  s = Math.max(0, Math.min(1, s));
  l = Math.max(0, Math.min(1, l));
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const hp = ((h % 360) + 360) % 360 / 60;
  const x = c * (1 - Math.abs((hp % 2) - 1));
  let r = 0, g = 0, b = 0;
  if (hp >= 0 && hp < 1) [r, g, b] = [c, x, 0];
  else if (hp < 2) [r, g, b] = [x, c, 0];
  else if (hp < 3) [r, g, b] = [0, c, x];
  else if (hp < 4) [r, g, b] = [0, x, c];
  else if (hp < 5) [r, g, b] = [x, 0, c];
  else [r, g, b] = [c, 0, x];
  const m = l - c / 2;
  return rgbToHex((r + m) * 255, (g + m) * 255, (b + m) * 255);
}

function rgba(hex: string, a: number): string {
  const [r, g, b] = hexToRgb(hex);
  return `rgba(${r}, ${g}, ${b}, ${a})`;
}

/** WCAG relative luminance (0..1). */
function luminance(hex: string): number {
  const [r, g, b] = hexToRgb(hex).map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** Pick a readable foreground (near-black / near-white) for a given bg. */
function contrastFg(hex: string): string {
  return luminance(hex) > 0.5 ? "#0c0c10" : "#ffffff";
}

/** Shift a colour's lightness by `delta` (in L units, e.g. -0.08). */
function shiftLightness(hex: string, delta: number): string {
  const [h, s, l] = hexToHsl(hex);
  return hslToHex(h, s, l + delta);
}

/** All inline CSS variables we manage for the custom theme (for cleanup). */
const CUSTOM_VARS = [
  "--bg-primary", "--bg-secondary", "--bg-tertiary", "--bg-hover", "--bg-elevated", "--bg-inset",
  "--text-primary", "--text-secondary", "--text-tertiary", "--text-inverse",
  "--border-color", "--border-hover", "--border-subtle",
  "--accent", "--accent-hover", "--accent-fg", "--accent-subtle", "--accent-muted",
  "--sidebar-bg", "--sidebar-text", "--sidebar-active-text", "--sidebar-active-bg", "--sidebar-hover-bg", "--sidebar-border",
  "--success", "--success-subtle", "--success-fg",
  "--warning", "--warning-subtle", "--warning-fg",
  "--danger", "--danger-subtle", "--danger-fg",
  "--info", "--info-subtle", "--info-fg",
];

/** Mode-appropriate semantic colours (contrast-checked against the mode's bg). */
const SEMANTICS = {
  dark: {
    "--success": "#34d399", "--success-subtle": "rgba(52, 211, 153, 0.12)", "--success-fg": "#022c22",
    "--warning": "#fbbf24", "--warning-subtle": "rgba(251, 191, 36, 0.12)", "--warning-fg": "#422006",
    "--danger":  "#f87171", "--danger-subtle":  "rgba(248, 113, 113, 0.12)", "--danger-fg":  "#450a0a",
    "--info":    "#60a5fa", "--info-subtle":    "rgba(96, 165, 250, 0.12)",  "--info-fg":    "#172554",
  },
  light: {
    "--success": "#047857", "--success-subtle": "rgba(4, 120, 87, 0.12)",   "--success-fg": "#ffffff",
    "--warning": "#b45309", "--warning-subtle": "rgba(180, 83, 9, 0.12)",   "--warning-fg": "#ffffff",
    "--danger":  "#b91c1c", "--danger-subtle":  "rgba(185, 28, 28, 0.12)",  "--danger-fg":  "#ffffff",
    "--info":    "#1d4ed8", "--info-subtle":    "rgba(29, 78, 216, 0.12)",  "--info-fg":    "#ffffff",
  },
} as const;

/**
 * Build a full, coherent token set from { mode, base, accent }.
 * Backgrounds/surfaces/borders/text are all generated from the base
 * hue at controlled lightness steps, so contrast is always safe.
 */
function deriveCustomVars(c: CustomColors): Record<string, string> {
  const dark = c.mode !== "light";
  const [bh, bsRaw] = hexToHsl(c.base);
  // Keep tint tasteful: backgrounds get a moderate amount of the base hue.
  const sBg = Math.min(bsRaw, dark ? 0.42 : 0.30);
  const sText = sBg * 0.45;

  // Neutral surface at lightness `l` carrying the base hue.
  const n = (l: number) => hslToHex(bh, sBg, l);
  // Text tone (less saturated so it stays neutral/legible).
  const t = (l: number) => hslToHex(bh, sText, l);

  const accent = c.accent;
  const accentFg = contrastFg(accent);

  const overlay = dark ? "#ffffff" : "#000000";

  const bgVars = dark
    ? {
        "--bg-primary":   n(0.065),
        "--bg-secondary": n(0.105),
        "--bg-tertiary":  n(0.155),
        "--bg-hover":     n(0.195),
        "--bg-elevated":  n(0.135),
        "--bg-inset":     rgba("#000000", 0.28),

        "--text-primary":   t(0.97),
        "--text-secondary": t(0.80),
        "--text-tertiary":  t(0.66),
        "--text-inverse":   n(0.065),

        "--border-color":  n(0.27),
        "--border-hover":  n(0.36),
        "--border-subtle": rgba(overlay, 0.14),

        "--sidebar-bg":         n(0.045),
        "--sidebar-text":       t(0.76),
        "--sidebar-hover-bg":   rgba(overlay, 0.07),
        "--sidebar-border":     rgba(overlay, 0.10),
      }
    : {
        "--bg-primary":   n(0.95),
        "--bg-secondary": hslToHex(bh, sBg * 0.5, 1.0),
        "--bg-tertiary":  n(0.90),
        "--bg-hover":     n(0.85),
        "--bg-elevated":  hslToHex(bh, sBg * 0.4, 1.0),
        "--bg-inset":     n(0.93),

        "--text-primary":   t(0.11),
        "--text-secondary": t(0.26),
        "--text-tertiary":  t(0.37),
        "--text-inverse":   hslToHex(bh, sBg * 0.3, 0.99),

        "--border-color":  n(0.74),
        "--border-hover":  n(0.60),
        "--border-subtle": rgba(overlay, 0.12),

        "--sidebar-bg":         n(0.97),
        "--sidebar-text":       t(0.30),
        "--sidebar-hover-bg":   rgba(overlay, 0.05),
        "--sidebar-border":     rgba(overlay, 0.10),
      };

  return {
    ...bgVars,
    ...(dark ? SEMANTICS.dark : SEMANTICS.light),
    "--accent":         accent,
    "--accent-hover":   shiftLightness(accent, dark ? 0.07 : -0.07),
    "--accent-fg":      accentFg,
    "--accent-subtle":  rgba(accent, dark ? 0.16 : 0.12),
    "--accent-muted":   rgba(accent, 0.32),
    "--sidebar-active-text": accent,
    "--sidebar-active-bg":   rgba(accent, dark ? 0.18 : 0.14),
  };
}

/** 4-swatch preview for the Custom card, reflecting current inputs. */
export function getCustomPreview(c: CustomColors) {
  const v = deriveCustomVars(c);
  return { bg: v["--bg-primary"], panelBg: v["--bg-secondary"], headerBg: v["--bg-tertiary"], accent: v["--accent"] };
}

/* ============================================================
 * Persistence + application
 * ============================================================ */

function getInitialTheme(): ThemeName {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored && THEMES.some((t) => t.id === stored)) {
      return stored as ThemeName;
    }
  } catch {
    // localStorage unavailable
  }
  return "dark"; // default
}

function getInitialCustomColors(): CustomColors {
  try {
    const stored = localStorage.getItem(CUSTOM_KEY);
    if (stored) {
      const parsed = JSON.parse(stored);
      return {
        mode: parsed.mode === "light" ? "light" : "dark",
        base: typeof parsed.base === "string" ? parsed.base : DEFAULT_CUSTOM_COLORS.base,
        accent: typeof parsed.accent === "string" ? parsed.accent : DEFAULT_CUSTOM_COLORS.accent,
      };
    }
  } catch {
    // localStorage unavailable / bad JSON
  }
  return DEFAULT_CUSTOM_COLORS;
}

function applyTheme(theme: ThemeName, customColors: CustomColors) {
  const root = document.documentElement;
  root.setAttribute("data-theme", theme);

  if (theme === "custom") {
    const vars = deriveCustomVars(customColors);
    for (const [k, v] of Object.entries(vars)) {
      root.style.setProperty(k, v);
    }
    root.style.colorScheme = customColors.mode === "light" ? "light" : "dark";
  } else {
    // Clear any inline custom overrides so the stylesheet theme takes over.
    for (const v of CUSTOM_VARS) root.style.removeProperty(v);
    root.style.removeProperty("color-scheme");
  }
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<ThemeName>(getInitialTheme);
  const [customColors, setCustomColors] = useState<CustomColors>(getInitialCustomColors);

  // Apply on mount
  useEffect(() => {
    applyTheme(theme, customColors);
  }, []);

  // Re-apply live whenever custom inputs change while the custom theme is active.
  useEffect(() => {
    if (theme === "custom") {
      applyTheme("custom", customColors);
    }
  }, [customColors, theme]);

  const setTheme = (newTheme: ThemeName) => {
    setThemeState(newTheme);
    applyTheme(newTheme, customColors);
    try {
      localStorage.setItem(STORAGE_KEY, newTheme);
    } catch {
      // localStorage unavailable
    }
  };

  const persistCustom = (next: CustomColors) => {
    try {
      localStorage.setItem(CUSTOM_KEY, JSON.stringify(next));
    } catch {
      // localStorage unavailable
    }
  };

  const setCustomColor = <K extends keyof CustomColors>(key: K, value: CustomColors[K]) => {
    setCustomColors((prev) => {
      const next = { ...prev, [key]: value };
      persistCustom(next);
      return next;
    });
  };

  const resetCustomColors = () => {
    setCustomColors(DEFAULT_CUSTOM_COLORS);
    persistCustom(DEFAULT_CUSTOM_COLORS);
  };

  return (
    <ThemeContext.Provider value={{ theme, setTheme, themes: THEMES, customColors, setCustomColor, resetCustomColors }}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) {
    throw new Error("useTheme must be used within a ThemeProvider");
  }
  return ctx;
}
