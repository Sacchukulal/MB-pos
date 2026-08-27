/** Every shipped theme is readable, computed rather than eyeballed. */

import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

const CSS = readFileSync('src/theme/tokens.css', 'utf8');

/** `#rrggbb` or `#rgb` to its three channels. */
function channels(hex: string): [number, number, number] | null {
  const value = hex.trim().replace('#', '');
  if (value.length === 3) {
    const [r, g, b] = [...value].map((c) => parseInt(c + c, 16));
    return r === undefined || g === undefined || b === undefined ? null : [r, g, b];
  }
  if (value.length !== 6) return null;
  return [
    parseInt(value.slice(0, 2), 16),
    parseInt(value.slice(2, 4), 16),
    parseInt(value.slice(4, 6), 16),
  ];
}

/** WCAG relative luminance. */
function luminance(hex: string): number | null {
  const rgb = channels(hex);
  if (!rgb || rgb.some(Number.isNaN)) return null;
  const [r, g, b] = rgb.map((c) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  }) as [number, number, number];
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function ratio(a: string, b: string): number | null {
  const la = luminance(a);
  const lb = luminance(b);
  if (la === null || lb === null) return null;
  const [light, dark] = la > lb ? [la, lb] : [lb, la];
  return (light + 0.05) / (dark + 0.05);
}

/** Every `[data-theme="…"]` block, as a map of token to value. */
function themes(): Map<string, Map<string, string>> {
  const found = new Map<string, Map<string, string>>();
  for (const block of CSS.matchAll(/\[data-theme="([^"]+)"\]\s*\{([\s\S]*?)\n\}/g)) {
    const name = block[1] ?? '';
    const tokens = new Map<string, string>();
    for (const line of (block[2] ?? '').matchAll(/(--[a-z0-9-]+)\s*:\s*([^;]+);/g)) {
      tokens.set(line[1] ?? '', (line[2] ?? '').trim());
    }
    found.set(name, tokens);
  }
  return found;
}

/** The pairs somebody has to read, and the minimum each one needs. */
const PAIRS: readonly { ink: string; ground: string; least: number }[] = [
  { ink: '--text', ground: '--bg', least: 4.5 },
  { ink: '--text', ground: '--surface', least: 4.5 },
  { ink: '--text', ground: '--surface-2', least: 4.5 },
  { ink: '--text', ground: '--surface-sunk', least: 4.5 },
  { ink: '--text-muted', ground: '--surface', least: 3 },
  { ink: '--text-faint', ground: '--surface', least: 3 },
  // The accent's own ink, on the accent — a primary button.
  { ink: '--accent-ink', ground: '--accent', least: 4.5 },
  // Semantic text on its own soft ground: "NOT PRINTED", "never checked".
  { ink: '--text', ground: '--ok-soft', least: 4.5 },
  { ink: '--text', ground: '--warn-soft', least: 4.5 },
  { ink: '--text', ground: '--danger-soft', least: 4.5 },
  { ink: '--text', ground: '--accent-soft', least: 4.5 },

  { ink: '--border-strong', ground: '--surface', least: 3 },
  { ink: '--border-strong', ground: '--bg', least: 3 },
  { ink: '--border', ground: '--surface', least: 1.5 },
  { ink: '--border', ground: '--bg', least: 1.5 },
];

describe('every theme is readable (T6, UI_GUIDELINES §2)', () => {
  const all = themes();

  it('finds the themes at all, so a broken parse cannot pass', () => {
    expect([...all.keys()].sort()).toEqual(['contrast', 'dark', 'light']);
    for (const [name, tokens] of all) {
      expect(tokens.size, `${name} has no tokens`).toBeGreaterThan(15);
    }
  });

  for (const [name, tokens] of all) {
    it(`"${name}" passes every pair a person has to read`, () => {
      const failures: string[] = [];
      for (const pair of PAIRS) {
        const ink = tokens.get(pair.ink);
        const ground = tokens.get(pair.ground);
        // A theme that does not define a token is a different failure, and the test below is
        // the one that reports it.
        if (!ink || !ground) continue;
        const got = ratio(ink, ground);
        if (got === null) continue;
        if (got < pair.least) {
          failures.push(
            `${pair.ink} on ${pair.ground} is ${got.toFixed(2)}:1, needs ${pair.least}:1`,
          );
        }
      }
      expect(failures, `${name}: ${failures.join('; ')}`).toEqual([]);
    });

    it(`"${name}" defines every token the pairs need`, () => {
      const missing = [...new Set(PAIRS.flatMap((p) => [p.ink, p.ground]))].filter(
        (token) => !tokens.has(token),
      );
      expect(missing, `${name} is missing: ${missing.join(', ')}`).toEqual([]);
    });
  }
});
