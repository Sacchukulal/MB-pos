#!/usr/bin/env node
/**
 * No raw colour, size or inline style outside src/theme/. Fails the build.
 *
 * Usage:
 *   node scripts/check-tokens.mjs            check src/
 *   node scripts/check-tokens.mjs <dir>      check somewhere else
 */

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';

const ROOT = process.argv[2] ?? 'src';

/** The token file is the one place these values are allowed to exist. */
const ALLOWED_DIRS = [join('src', 'theme')];

const CHECKS = [
  {
    what: 'a raw hex colour',
    // #abc, #aabbcc, #aabbccdd.
    re: /#[0-9a-fA-F]{3,8}\b/g,
  },
  {
    what: 'a raw colour function',
    re: /\b(?:rgba?|hsla?|oklch|oklab|color-mix)\s*\(/g,
  },
  {
    what: 'a raw size',
    // 12px, 1.5rem, 2em — but not 0px and not a bare 0.
    re: /(?<![\w-])(?!0(?:px|rem|em)\b)\d+(?:\.\d+)?(?:px|rem|em)\b/g,
  },
  {
    what: 'an inline style prop',
    re: /style=\{\{/g,
  },
  {
    what: 'a named CSS colour',
    re: /:\s*(?:red|green|blue|black|white|grey|gray|orange|yellow|purple|pink|brown|cyan|magenta)\s*[;,)]/gi,
  },
];

/**
 * Lines saying `mb-tokens-allow: <why>` exempt themselves, and are counted.
 *
 * **Checked against the RAW line, not the comment-stripped one** — P27.5 found
 * that the escape was unusable in a CSS file, because CSS has no line-comment
 * syntax, so the only way to write the reason is inside a `/* … *\/` that
 * `stripComments` blanks before the check ever runs. The one legitimate raw
 * value in this product is a media-query breakpoint (a media condition is
 * evaluated before the cascade, so `var()` is invalid there), and it could not
 * be declared. An escape hatch that cannot be reached is not an escape hatch.
 */
const ESCAPE = 'mb-tokens-allow:';

/**
 * Comments describe these rules constantly — "every control is 44px tall" is documentation, not
 * a hardcoded size.
 */
function stripComments(text) {
  const block = new RegExp("/\\*[\\s\\S]*?\\*/", "g");
  // `[ \t]`, not `\s`, and that was a real bug.
  const lineComment = new RegExp("^[ \\t]*//.*$", "gm");
  // Blanked rather than deleted, so a reported line number still points at the right line of
  // the real file.
  return text
    .replace(block, (found) => found.replace(new RegExp("[^\\n]", "g"), " "))
    .replace(lineComment, "");
}

function walk(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry === 'node_modules' || entry.startsWith('.')) continue;
      walk(full, out);
    } else if (/\.(tsx?|css)$/.test(entry)) {
      out.push(full);
    }
  }
  return out;
}

function isAllowed(file) {
  const path = relative('.', file);
  return ALLOWED_DIRS.some((dir) => path.startsWith(dir + sep) || path === dir);
}

const problems = [];
let escapes = 0;

for (const file of walk(ROOT)) {
  if (isAllowed(file)) continue;
  const source = readFileSync(file, 'utf8');
  const raw = source.split(/\r?\n/);
  const lines = stripComments(source).split(/\r?\n/);
  lines.forEach((line, index) => {
    if ((raw[index] ?? '').includes(ESCAPE)) {
      escapes += 1;
      return;
    }
    for (const check of CHECKS) {
      check.re.lastIndex = 0;
      const found = check.re.exec(line);
      if (found) {
        problems.push({
          file: relative('.', file),
          line: index + 1,
          what: check.what,
          text: found[0],
          source: line.trim(),
        });
      }
    }
  });
}

if (problems.length > 0) {
  console.error(
    `\n  ${problems.length} raw value(s) outside src/theme/.\n` +
      `  Every colour, size, radius and spacing value is a token — that is\n` +
      `  decision D21 and the owner's ruling of 2026-08-04. Put it in\n` +
      `  src/theme/tokens.css and use var(--…) here.\n`,
  );
  for (const p of problems) {
    console.error(`  ${p.file}:${p.line}  ${p.what}: ${p.text}`);
    console.error(`      ${p.source}`);
  }
  console.error('');
  process.exit(1);
}

console.log(
  `  tokens: clean${escapes > 0 ? ` (${escapes} documented exception(s))` : ''}`,
);
