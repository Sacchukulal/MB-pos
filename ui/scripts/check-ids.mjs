#!/usr/bin/env node
/**
 * **THE LINT THAT KEEPS AN ID OUT OF THE CLOCK.**
 *
 * Twenty-one places in Rust and seventeen on the screens built a row's id from
 * the current millisecond — `` `cus_${Date.now().toString(36)}` `` and
 * `format!("adj_{}", at.millis())`. Two rows made in the same thousandth of a
 * second got the same id, and two rows cannot share one.
 *
 * Most writes then refused, and the shopkeeper read *"The shop's data could not
 * be read"* — ugly, nothing lost. **`expenses` is an upsert**, so there the
 * second spend silently replaced the first: money quietly gone from a day's
 * list with nothing on screen to say why.
 *
 * Nobody wrote that on purpose. Each one was written by somebody who needed an
 * id, reached for the clock because it was to hand, and moved on. That is what
 * a lint is for — the same argument `check-tokens.mjs` and `check-fields.mjs`
 * make, and the same mechanism.
 *
 * It fails the build on:
 *
 *   1. **`Date.now()` used to build a string** in a screen. Use `freshId` from
 *      the kit. `Date.now()` on its own is fine — timing a keystroke burst is
 *      not an id.
 *   2. **`at.millis()` or `now().millis()` inside a `format!` in Rust.** Use
 *      `crate::newid::fresh_at`.
 *
 * # What is allowed, and why there has to be a way out
 *
 * An id that is DERIVED from something real must stay derived. `close_{day}`
 * and `float_{day}` are built from the business day on purpose: one close per
 * day, and a second one has to collide so the database refuses it. Randomness
 * there would quietly allow two closes for one day — the opposite of a fix.
 *
 * Those are the only two, and they are named below rather than escaped, so
 * adding a third is a decision somebody makes in this file.
 *
 * NO DEPENDENCIES, like its two siblings.
 *
 *   node scripts/check-ids.mjs
 */

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';

/**
 * The screens, and the Rust that serves them.
 *
 * A directory may be given instead, which is how the test in `look.test.tsx`
 * points it at a deliberately broken file — **a lint nobody has watched fail is
 * a lint nobody knows works.**
 */
const ROOTS = process.argv[2] ? [process.argv[2]] : ['src', '../src-tauri/src'];

/** An id that is derived on purpose. See the note above. */
const DERIVED = [
  /format!\("close_\{\}", *day\./,
  /format!\("float_\{\}", *day\./,
];

/** A file may say why it is an exception, on the line above. */
const ESCAPE = /id-lint-ok:/;

const problems = [];
let escapes = 0;

function walk(dir) {
  let entries;
  try {
    entries = readdirSync(dir);
  } catch {
    return; // A root that is not there is not a failure — `ui` alone still lints.
  }
  for (const entry of entries) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) {
      if (['node_modules', 'generated', 'target'].includes(entry)) continue;
      walk(path);
      continue;
    }
    if (entry.endsWith('.tsx') || entry.endsWith('.ts')) check(path, 'ts');
    else if (entry.endsWith('.rs')) check(path, 'rs');
  }
}

function check(path, kind) {
  const file = relative(process.cwd(), path).split(sep).join('/');
  // The two files that DEFINE the helpers, and the tests that drive them.
  if (/kit[/\\]ids\.ts$|newid\.rs$|check-ids\.mjs$/.test(file)) return;

  const lines = readFileSync(path, 'utf8').split('\n');

  lines.forEach((line, index) => {
    // A comment explaining the old bug is not the old bug.
    const code = line.replace(/\/\/.*$/, '').replace(/\/\*.*?\*\//g, '');
    if (/^\s*(\*|\/\/|--)/.test(line)) return;

    let what = null;
    if (kind === 'ts') {
      // Inside a template string or joined onto one: that is an id being built.
      if (/\$\{\s*Date\.now\(\)/.test(code) || /Date\.now\(\)\.toString\(/.test(code)) {
        what = { found: 'Date.now() in a string', use: "freshId('…') from the kit" };
      }
    } else if (/format!\([^)]*\)/.test(code) && /\b(at|now\(\))\.millis\(\)/.test(code)) {
      if (DERIVED.some((rule) => rule.test(code))) return;
      what = { found: 'a clock reading inside format!', use: 'crate::newid::fresh_at' };
    }
    if (!what) return;

    // Eight lines of room, because a reason worth writing is usually a
    // paragraph and the marker belongs at the top of it where it is read.
    const above = lines.slice(Math.max(0, index - 8), index).join(' ');
    if (ESCAPE.test(above) || ESCAPE.test(line)) {
      escapes += 1;
      return;
    }

    problems.push({ file, line: index + 1, text: code.trim().slice(0, 70), ...what });
  });
}

for (const root of ROOTS) walk(root);

if (problems.length > 0) {
  console.error(
    `\n  ${problems.length} id(s) built from the clock.\n` +
      `  Two rows saved in the same millisecond get the same id — and on the\n` +
      `  Spends screen the second one silently replaced the first.\n`,
  );
  for (const p of problems) {
    console.error(`  ${p.file}:${p.line}  ${p.found}: ${p.text}`);
    console.error(`      Use ${p.use}, or put "id-lint-ok: <why>" above it.`);
  }
  console.error('');
  process.exit(1);
}

console.log(
  `  ids: clean${escapes > 0 ? ` (${escapes} documented exception(s))` : ''}`,
);
