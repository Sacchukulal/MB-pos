#!/usr/bin/env node
/**
 * R8, MADE MECHANICAL: **no business logic in React.**
 *
 * The rule everybody agrees with, and the one most likely to erode, because
 * the first violation is always three characters long:
 *
 *     const total = a + b;
 *
 * D1 and audit E3 are what it costs: *"business rules live inside screen
 * files… to answer 'what exactly happens when a bill is settled?' you must
 * read four files at once."* And D2 is why it matters more here than anywhere:
 * money is an integer count of paise and JavaScript has no integers — it has
 * doubles, and `0.1 + 0.2` is the oldest bug in the industry.
 *
 * So: every rupee is computed in Rust, crosses as paise plus a preformatted
 * string, and TypeScript displays it. This fails the build if TypeScript does
 * arithmetic on anything money-shaped, or tries to format it itself.
 *
 * NO DEPENDENCIES (see check-tokens.mjs for why).
 *
 *   node scripts/check-no-money.mjs           check src/
 *   node scripts/check-no-money.mjs <dir>     check somewhere else
 */

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

const ROOT = process.argv[2] ?? 'src';

/** Words that mean money in this product. */
const MONEY =
  '(?:total|subtotal|amount|price|paise|rupees|money|tax|gst|cgst|sgst|igst|discount|charge|change|due|balance|tip|grand_?total|round_?off)';

const CHECKS = [
  {
    what: 'arithmetic on money',
    // `total + x`, `x * price`, `amount -= …`
    //
    // **Whitespace around the operator is required**, and that is not
    // fussiness: the first version of this rule flagged `data-paise={…}` as a
    // subtraction and an import path as a division. Real arithmetic in this
    // codebase is spaced; a hyphenated JSX attribute and a `/` in a path are
    // not. Compound assignment (`+=`) is caught either way.
    re: new RegExp(
      `\\b\\w*${MONEY}\\w*\\s*[+\\-*/]=|` +
        `\\b\\w*${MONEY}\\w*\\s+[+\\-*/]\\s|` +
        `[+\\-*/]\\s+\\w*${MONEY}\\w*\\b`,
      'i',
    ),
  },
  {
    what: 'formatting money in TypeScript',
    re: new RegExp(
      `\\b\\w*${MONEY}\\w*\\s*\\.\\s*(?:toFixed|toPrecision|toLocaleString)\\b`,
      'i',
    ),
  },
  {
    what: 'parsing money in TypeScript',
    re: new RegExp(
      `\\b(?:parseFloat|parseInt|Number)\\s*\\(\\s*\\w*${MONEY}\\w*`,
      'i',
    ),
  },
  {
    what: 'a floating-point money literal',
    re: /\b(?:price|amount|total|subtotal)\s*[:=]\s*\d+\.\d+/i,
  },
];

/**
 * Lines saying `mb-money-allow: <why>` exempt themselves.
 *
 * There is one legitimate case and it is worth naming: a *count* that happens
 * to be called `total` — "total items", "total tables". Those are integers and
 * they are not money. Say so on the line.
 */
const ESCAPE = 'mb-money-allow:';

/** The generated bindings describe Rust's types; they do no arithmetic. */
const SKIP = ['src/ipc/generated'];

function walk(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry === 'node_modules' || entry.startsWith('.')) continue;
      walk(full, out);
    } else if (/\.tsx?$/.test(entry)) {
      out.push(full);
    }
  }
  return out;
}

const problems = [];

for (const file of walk(ROOT)) {
  const path = relative('.', file).split('\\').join('/');
  if (SKIP.some((s) => path.startsWith(s))) continue;

  const lines = readFileSync(file, 'utf8').split(/\r?\n/);
  lines.forEach((line, index) => {
    if (line.includes(ESCAPE)) return;
    // Comments describe the rule constantly; they do not break it.
    const code = line.replace(/\/\/.*$/, '').replace(/\/\*.*?\*\//g, '');
    if (code.trim().startsWith('*')) return;
    for (const check of CHECKS) {
      if (check.re.test(code)) {
        problems.push({
          file: relative('.', file),
          line: index + 1,
          what: check.what,
          source: line.trim(),
        });
        break;
      }
    }
  });
}

if (problems.length > 0) {
  console.error(
    `\n  ${problems.length} place(s) where TypeScript touches money.\n` +
      `  R8: React renders and collects input. Every rule, every calculation\n` +
      `  and every rupee lives in Rust (mb-core). Money crosses IPC as integer\n` +
      `  paise plus a preformatted string — JavaScript has no integers, and\n` +
      `  Money::to_plain_string is the only formatter in the product.\n`,
  );
  for (const p of problems) {
    console.error(`  ${p.file}:${p.line}  ${p.what}`);
    console.error(`      ${p.source}`);
  }
  console.error('');
  process.exit(1);
}

console.log('  money: clean — every rupee is computed in Rust');
