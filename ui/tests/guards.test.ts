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

/** Nothing polls. */
describe('nothing polls (M4)', () => {
  /** The one file allowed to tick, and it is allowed by name. */
  const MAY_TICK = ['src/clock.ts'];

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
          const path = full.split('\\').join('/');
          if (/setInterval\s*\(/.test(code) && !MAY_TICK.includes(path)) {
            offenders.push(path);
          }
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

/** Every class a screen names must exist in a stylesheet. */
describe('the classes a screen asks for (P13)', () => {
  const cssText = (() => {
    const found: string[] = [];
    const walk = (dir: string) => {
      for (const entry of readdirSync(dir)) {
        const full = join(dir, entry);
        if (statSync(full).isDirectory()) walk(full);
        else if (entry.endsWith('.css')) found.push(readFileSync(full, 'utf8'));
      }
    };
    walk('src');
    return found.join('\n');
  })();

  const asked = (() => {
    const names = new Set<string>();
    const walk = (dir: string) => {
      for (const entry of readdirSync(dir)) {
        const full = join(dir, entry);
        if (statSync(full).isDirectory()) walk(full);
        else if (entry.endsWith('.tsx')) {
          const source = readFileSync(full, 'utf8');
          // Only what is actually a class.
          for (const attribute of source.matchAll(
            /className\s*=\s*(?:"([^"]*)"|\{([^}]*)\})/g,
          )) {
            const text = attribute[1] ?? attribute[2] ?? '';
            for (const match of text.matchAll(
              /\bmb-[a-z0-9]+(?:__[a-z0-9-]+)?(?:--[a-z0-9]+)?\b/g,
            )) {
              names.add(match[0]);
            }
          }
        }
      }
    };
    walk('src');
    return [...names].sort();
  })();

  it('finds classes to check at all, so a broken walk cannot pass', () => {
    expect(asked.length).toBeGreaterThan(50);
  });

  it('has a rule for every one of them', () => {
    const missing = asked.filter((name) => !cssText.includes(`.${name}`));
    expect(missing, `no stylesheet defines: ${missing.join(', ')}`).toEqual([]);
  });
});

/** Nothing crossing INTO Rust may be a `bigint`. */
describe('the IPC boundary (P13)', () => {
  it('declares no bigint in any command argument', () => {
    const source = readFileSync('src/ipc/call.ts', 'utf8');
    const commands = source.slice(
      source.indexOf('export interface Commands'),
      source.indexOf('export type CommandName'),
    );
    expect(commands.length).toBeGreaterThan(500);

    // `args:` runs to the matching `returns:`, which is where every entry in the table puts it.
    const offenders: string[] = [];
    for (const entry of commands.matchAll(/args:([\s\S]*?)returns:/g)) {
      const args = entry[1] ?? '';
      if (args.includes('bigint')) offenders.push(args.trim());
    }
    expect(offenders, `a bigint cannot be sent: ${offenders.join(' | ')}`).toEqual([]);
  });
});
