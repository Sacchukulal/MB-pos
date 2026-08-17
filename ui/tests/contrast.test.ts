/**
 * **T6 — every shipped theme is readable, computed rather than eyeballed.**
 *
 * P17's prompt asks for it and UI_GUIDELINES §2 is the rule:
 *
 * > *"every token pair in every theme, including a generated custom palette,
 * > must pass a minimum contrast ratio."*
 *
 * A theme is one block of values in `tokens.css` (D21), so this reads that file
 * — not a copy of it — and checks the pairs that a person actually has to read.
 * **Adding a theme therefore gets checked automatically**, which is the whole
 * point: the owner will name a different accent later, and the thing that
 * catches an unreadable one has to be the build rather than somebody's eye.
 *
 * # The custom-palette generator is deferred, and this is where that shows
 *
 * v1 had "Custom (base tint + accent, everything else auto-balanced)". An
 * auto-balancer is a colour ENGINE, and the owner's 2026-08-04 ruling asks for
 * the opposite — that the look is data they dictate. So P17 ships a fixed set
 * of themes, each checked here, and the generator waits for a session of its
 * own. When it arrives, it has to pass THIS function before it may offer a
 * palette.
 */

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
 * The pairs somebody has to read, and the minimum each one needs.
 *
 * 4.5 is WCAG AA for body text. **3.0 for muted and faint text on purpose**:
 * they are secondary by design — a hint under a field, a section note — and
 * holding them to body-text contrast would mean they were not secondary. They
 * are never the only carrier of anything (§2 rule 2).
 */
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

  /*
   * **THE LINES, and their absence here is why the owner had to report this.**
   *
   * On 2026-08-17 the owner installed the counter and said *"very light in
   * opacity, the lines and letters brightness not correctly visble."* Every
   * pair above passed. They still passed. The letters were a near miss; the
   * LINES were not close, and nothing checked them:
   *
   *   dark   --border-strong on --surface   1.68:1
   *   light  --border-strong on --surface   1.49:1
   *
   * That token outlines every input, every secondary button and every table
   * tile in the product. WCAG 1.4.11 asks 3:1 of a control's boundary for the
   * same reason a person does — a box you cannot see the edge of does not read
   * as a box. So it is held to 3:1 here, against both grounds it is ever drawn
   * on, in every theme, including the ones nobody has written yet.
   *
   * `--border` is deliberately NOT held to the same bar. It separates rows
   * that belong together; at 3:1 it would stop being a separator and start
   * being a grid, which is a different and worse screen. 1.5 is the floor for
   * "present" rather than for "prominent".
   */
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
        // A theme that does not define a token is a different failure, and the
        // test below is the one that reports it.
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
