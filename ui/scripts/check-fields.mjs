#!/usr/bin/env node
/**
 * **THE LINT THAT KEEPS A PHONE A PHONE AND AN AMOUNT A NUMBER.**
 *
 * The owner, 2026-08-22, from a real install:
 *
 * > *"i noticed while adding a credit customer, shop details, i can enter
 * > alphabets, more than 10 numbers, fix it, this app is india only so only 10
 * > digits needed."*
 *
 * > *"user needs to enter only numbers, not alphabet, it would mess up
 * > calculations also, didn't you even consider that? … These above places are
 * > wher i found phone number and amount feild is not correct, but if there are
 * > more places in entire app, fix there also. … You should always remember
 * > these things in future when adding phone number and amount feilds."*
 *
 * **"Always remember" is not a thing a person can promise.** Eight screens each
 * collected a phone or an amount with a plain `<Input>`, and every one of them
 * was written by somebody who knew the rule and was thinking about something
 * else at the time. `check-tokens.mjs` made a raw colour impossible and
 * nineteen sessions went by without one creeping back; this is the same
 * mechanism pointed at the same kind of failure.
 *
 * It fails the build on a field whose LABEL says it holds a phone number or an
 * amount, drawn with anything but the kit component for it:
 *
 *   * a phone   → `<PhoneInput>`  (ten digits, `+91` beside the box)
 *   * an amount → `<MoneyInput>`  (digits and one dot, `₹` beside the box)
 *
 * The label is what it reads, because the label is what the person filling the
 * field reads. A box called "Amount" that takes letters is wrong whatever the
 * variable behind it is called.
 *
 * # Getting out of it
 *
 * Put `field-lint-ok:` and a reason on the line above. There are real cases —
 * a per-cent box says "Rupees" on its other setting, a licence contact is a
 * *"Mobile or email"*. An escape with a reason is a decision; a lint nobody can
 * escape is a lint somebody deletes.
 *
 * NO DEPENDENCIES, for the same reason `check-tokens.mjs` has none.
 *
 *   node scripts/check-fields.mjs            check src/
 *   node scripts/check-fields.mjs <dir>      check somewhere else
 */

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';

const root = process.argv[2] ?? 'src';

/** Words that mean "this box holds a phone number". */
const PHONE_WORDS = /\b(phone|mobile)\b/i;

/**
 * Words that mean "this box holds money".
 *
 * `rate` is in and `tax rate` is not, which is why the exclusions exist: a GST
 * rate is a percentage and a supplier's rate is rupees per kilo. When a word is
 * genuinely both, the escape hatch above is the answer.
 */
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

    // **The hint counts as well as the label.** "Biggest discount" reads like
    // money and its hint says *"Per cent"* — the field is a percentage, and the
    // only thing on screen that says so is the hint. A lint that reads less
    // than the shopkeeper does is a lint that cries wolf, and a lint that cries
    // wolf gets an escape comment on every line until it means nothing.
    const nearby = lines.slice(Math.max(0, index - 4), index + 5).join(' ');
    const hint = nearby.match(/hint=(?:"([^"]*)"|\{`([^`]*)`\})/);
    const around = `${text} ${hint?.[1] ?? hint?.[2] ?? ''}`;

    let wants = null;
    if (PHONE_WORDS.test(text)) wants = 'phone';
    else if (MONEY_WORDS.test(text) && !NOT_MONEY.test(around)) wants = 'money';
    if (!wants) return;

    // **The opening tag, which may be several lines above the label.** A field
    // is written `<Input\n  label="Amount"\n  … />` as often as on one line, so
    // walk back to the tag that owns this label.
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

    // Anything that is not a field at all — a Select of payment modes labelled
    // "How", a Checkbox. Only the text-entry components are in scope.
    if (!['Input', 'NumberInput', 'MoneyInput', 'PhoneInput'].includes(tag)) return;

    if (tag === RIGHT[wants]) return;

    // The documented way out. A few lines of room above the tag, because the
    // reason is usually a JSX comment and a sentence worth reading rarely fits
    // on one line.
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
