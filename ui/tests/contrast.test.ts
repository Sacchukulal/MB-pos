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

/**
 * A token's value, following `var(--other)` as far as it goes. A theme is allowed to say
 * `--pick-ink: var(--text)` — that is the point of a palette — and the pair it makes is still
 * a pair somebody has to read.
 */
function value(tokens: Map<string, string>, name: string): string | undefined {
  let found = tokens.get(name);
  // Six hops is far more than any chain in the file; it is here so a loop cannot hang the run.
  for (let hop = 0; hop < 6; hop += 1) {
    const points = found?.match(/^var\(\s*(--[a-z0-9-]+)\s*\)$/);
    if (!points) return found;
    found = tokens.get(points[1] ?? '');
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
  // The chosen row of a list, and the row under the hand on the way to it.
  { ink: '--pick-ink', ground: '--pick-bg', least: 4.5 },
  { ink: '--pick-ink', ground: '--pick-bg-hover', least: 4.5 },
  { ink: '--text', ground: '--pick-hover', least: 4.5 },
  // The marker down its left edge, on the fill it is drawn over.
  { ink: '--pick-line', ground: '--pick-bg', least: 3 },

  { ink: '--border-strong', ground: '--surface', least: 3 },
  { ink: '--border-strong', ground: '--bg', least: 3 },
  { ink: '--border', ground: '--surface', least: 1.5 },
  { ink: '--border', ground: '--bg', least: 1.5 },
];

/**
 * The other half of readable, and the half a contrast table misses: a state has to be SEEN.
 * The chosen row of a list used to wear `--surface-2`, which passed every pair above and was
 * still invisible on a white sheet — being 4.5:1 against your own ink says nothing about
 * whether anybody can find you.
 */
const SEEN: readonly { state: string; on: string; least: number }[] = [
  { state: '--pick-bg', on: '--surface', least: 1.25 },
  { state: '--pick-bg', on: '--pick-hover', least: 1.1 },
  { state: '--pick-hover', on: '--surface', least: 1.1 },
];

describe('every theme is readable (T6, UI_GUIDELINES §2)', () => {
  const all = themes();

  it('finds the themes at all, so a broken parse cannot pass', () => {
    expect([...all.keys()].sort()).toEqual(['dark', 'light']);
    for (const [name, tokens] of all) {
      expect(tokens.size, `${name} has no tokens`).toBeGreaterThan(15);
    }
  });

  for (const [name, tokens] of all) {
    it(`"${name}" passes every pair a person has to read`, () => {
      const failures: string[] = [];
      for (const pair of PAIRS) {
        const ink = value(tokens, pair.ink);
        const ground = value(tokens, pair.ground);
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

    it(`"${name}" shows a chosen row against the sheet it sits on`, () => {
      const failures: string[] = [];
      for (const pair of SEEN) {
        const state = value(tokens, pair.state);
        const on = value(tokens, pair.on);
        expect(state, `${name} has no ${pair.state}`).toBeTruthy();
        expect(on, `${name} has no ${pair.on}`).toBeTruthy();
        const got = ratio(state ?? '', on ?? '');
        if (got === null) continue;
        if (got < pair.least) {
          failures.push(
            `${pair.state} on ${pair.on} is ${got.toFixed(2)}:1, needs ${pair.least}:1`,
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
