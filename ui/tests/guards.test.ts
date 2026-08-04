/**
 * **T4 and T12 — the guards, and the proof that they fail.**
 *
 * A guard nobody has watched fail is a guard nobody knows is switched off. v1's
 * inline styles came back *after* a rebuild that was meant to remove them
 * (audit E11), so these run the real scripts against fixtures that must be
 * rejected, and against the real source, which must be clean.
 */

import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

import { describe, expect, it } from 'vitest';

function run(script: string, target: string): { code: number; output: string } {
  try {
    const output = execFileSync('node', [script, target], {
      encoding: 'utf8',
      stdio: 'pipe',
    });
    return { code: 0, output };
  } catch (error) {
    const failure = error as { status?: number; stderr?: string; stdout?: string };
    return {
      code: failure.status ?? 1,
      output: `${failure.stdout ?? ''}${failure.stderr ?? ''}`,
    };
  }
}

describe('the token lint (audit E11)', () => {
  it('REJECTS a file with an inline style and a raw colour', () => {
    const result = run('scripts/check-tokens.mjs', 'tests/fixtures/bad');
    expect(result.code).toBe(1);
    expect(result.output).toContain('raw hex colour');
    expect(result.output).toContain('inline style prop');
    expect(result.output).toContain('raw size');
  });

  it('passes the real source, so the rule is actually being kept', () => {
    const result = run('scripts/check-tokens.mjs', 'src');
    expect(result.code, result.output).toBe(0);
  });
});

describe('the money lint (R8)', () => {
  it('REJECTS arithmetic on money in TypeScript', () => {
    const result = run('scripts/check-no-money.mjs', 'tests/fixtures/bad');
    expect(result.code).toBe(1);
    expect(result.output).toContain('arithmetic on money');
  });

  it('passes the real source: every rupee is computed in Rust', () => {
    const result = run('scripts/check-no-money.mjs', 'src');
    expect(result.code, result.output).toBe(0);
  });
});

/**
 * T12. **Nothing polls** — budget M4, and `PERFORMANCE.md` §5 rule 6: *"a
 * 250 ms poll loop is M4 gone before a single feature is written."*
 *
 * Rust pushes and React subscribes. A `setInterval` anywhere in the screens is
 * either a poll or a second clock, and §5 rule 10 forbids the second one too:
 * *"timers are one clock — table timers, KDS timers and elapsed displays share
 * a single ticking source, not one interval per tile."*
 */
describe('nothing polls (M4)', () => {
  it('has no setInterval in any screen', () => {
    const offenders: string[] = [];

    const walk = (dir: string) => {
      for (const entry of readdirSync(dir)) {
        const full = join(dir, entry);
        if (statSync(full).isDirectory()) {
          walk(full);
        } else if (/\.tsx?$/.test(entry)) {
          const source = readFileSync(full, 'utf8');
          // Comments explain the rule constantly; they do not break it.
          const code = source.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/.*/g, '');
          if (/setInterval\s*\(/.test(code)) offenders.push(full);
        }
      }
    };
    walk('src');

    expect(
      offenders,
      'Rust pushes and React subscribes. If a screen needs a clock, use the ' +
        'one shared ticking source (§5 rule 10).',
    ).toEqual([]);
  });
});
