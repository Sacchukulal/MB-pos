#!/usr/bin/env node
/**
 * THE LINT THAT KEEPS A PHONE A PHONE AND AN AMOUNT A NUMBER.
 *
 * Usage:
 *   node scripts/check-fields.mjs            check src/
 *   node scripts/check-fields.mjs <dir>      check somewhere else
 */

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';

const root = process.argv[2] ?? 'src';

/** Words that mean "this box holds a phone number". */
const PHONE_WORDS = /\b(phone|mobile)\b/i;

/** Words that mean "this box holds money". */
const MONEY_WORDS =
  /\b(amount|price|cost|salary|advance|credit limit|rupees|paid|pay them|handed over|hand over|charges|discount|total on)\b/i;
const NOT_MONEY =
  /\b(per ?cent|percentage|tax rate|rate label|%|how many|count|days|minutes|paid to|paid by|paid on)\b/i;

/** The component each one has to be drawn with. */
const RIGHT = { phone: 'PhoneInput', money: 'MoneyInput' };

/** A file may say why it is an exception, on the line above. */
const ESCAPE = /field-lint-ok:/;

const problems = [];
let escapes = 0;

function walk(dir) {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) {
      // The kit DEFINES these components, so it is where they do not apply.
      if (entry === 'kit' || entry === 'generated' || entry === 'node_modules') continue;
      walk(path);
      continue;
    }
    if (!entry.endsWith('.tsx')) continue;
    check(path);
  }
}

function check(path) {
  const file = relative(process.cwd(), path).split(sep).join('/');
  const lines = readFileSync(path, 'utf8').split('\n');

  lines.forEach((line, index) => {
    const label = line.match(/label=(?:"([^"]*)"|\{`([^`]*)`\})/);
    if (!label) return;
    const text = label[1] ?? label[2] ?? '';

    // The hint counts as well as the label.
    const nearby = lines.slice(Math.max(0, index - 4), index + 5).join(' ');
    const hint = nearby.match(/hint=(?:"([^"]*)"|\{`([^`]*)`\})/);
    const around = `${text} ${hint?.[1] ?? hint?.[2] ?? ''}`;

    let wants = null;
    if (PHONE_WORDS.test(text)) wants = 'phone';
    else if (MONEY_WORDS.test(text) && !NOT_MONEY.test(around)) wants = 'money';
    if (!wants) return;

    // The opening tag, which may be several lines above the label.
    let tagLine = index;
    let tag = null;
    for (let n = index; n >= 0 && n > index - 12; n -= 1) {
      const found = lines[n].match(/<([A-Z][A-Za-z]*)/);
      if (found) {
        tagLine = n;
        tag = found[1];
        break;
      }
    }
    if (!tag) return;

    // Anything that is not a field at all — a Select of payment modes labelled "How", a
    // Checkbox.
    if (!['Input', 'NumberInput', 'MoneyInput', 'PhoneInput'].includes(tag)) return;

    if (tag === RIGHT[wants]) return;

    // The documented way out.
    const above = lines.slice(Math.max(0, tagLine - 4), tagLine).join(' ');
    if (ESCAPE.test(above) || ESCAPE.test(lines[index - 1] ?? '')) {
      escapes += 1;
      return;
    }

    problems.push({
      file,
      line: tagLine + 1,
      text,
      tag,
      wants: RIGHT[wants],
    });
  });
}

walk(root);

if (problems.length > 0) {
  console.error(
    `\n  ${problems.length} field(s) with the wrong shape.\n` +
      `  A phone is ten digits and an amount is digits — the owner, 2026-08-22.\n` +
      `  The kit components hold the rule so no screen has to remember it.\n`,
  );
  for (const p of problems) {
    console.error(`  ${p.file}:${p.line}  "${p.text}" is a <${p.tag}>`);
    console.error(
      `      Use <${p.wants}>, or put "field-lint-ok: <why>" on the line above.`,
    );
  }
  console.error('');
  process.exit(1);
}

console.log(
  `  fields: clean${escapes > 0 ? ` (${escapes} documented exception(s))` : ''}`,
);
