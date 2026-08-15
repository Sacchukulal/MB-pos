#!/usr/bin/env node
/**
 * THE LINT THAT KEEPS P27.5 FROM UNDOING ITSELF.
 *
 * `check-tokens.mjs` made a raw colour impossible and it worked: nineteen
 * sessions and not one hex crept back. But it could only see VALUES, and the
 * owner's complaint of 2026-08-15 — *"now it looks here and there"* — was not
 * about values. Every screen was already using the token scale. They were each
 * using it differently, because nothing said what a step MEANT and nothing
 * owned the shape of a page.
 *
 * So this is the same mechanism pointed at the failure that actually happened.
 * It fails the build on:
 *
 *   1. **a feature file that sets the page margin.** Fourteen screens each set
 *      their own — `--space-3`, `--space-4` and `--space-5` between them — so
 *      the left edge of the app moved as you walked through it. The margin is
 *      `.mb-main`'s, once, in shell.css.
 *
 *   2. **a feature file that hand-rolls a page header.** Five screens had a
 *      title-plus-actions row of their own and no two were the same. `PageHeader`
 *      is the shape.
 *
 *   3. **an `<svg>` outside the kit.** One icon set, one stroke weight, one
 *      optical size (§5). An SVG drawn in a screen is the next `▦`.
 *
 *   4. **a Unicode glyph used as an icon.** The left rail drew its whole
 *      navigation with `▦ ☰ ⌁ ⬒ ⇩` and got whatever Windows substituted, at
 *      three different weights. That is most of what "old-styled and
 *      unprofessional" meant, and it must not come back.
 *
 * NO DEPENDENCIES, for the same reason `check-tokens.mjs` has none.
 *
 *   node scripts/check-layout.mjs            check src/
 *   node scripts/check-layout.mjs <dir>      check somewhere else
 */

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';

const ROOT = process.argv[2] ?? 'src';

/**
 * The kit owns layout, icons and the page shape; the theme owns values. These
 * two directories are where the rules below are *implemented*, so they are the
 * two places the rules cannot apply to.
 */
const KIT_DIRS = [join('src', 'kit'), join('src', 'theme')];

/**
 * The shell draws the page margin, because somebody has to. It is one file and
 * one declaration, which is the whole point.
 */
const SHELL_FILES = [join('src', 'shell', 'shell.css')];

/**
 * **The glyphs that were used as icons**, and a few of the same family that
 * would be the obvious next choice. Ordinary punctuation is not here: an em
 * dash in a sentence is writing, and `—` as a placeholder for "no value" is a
 * legitimate typographic mark rather than a picture of a thing.
 */
const GLYPH_ICONS = [
  '▦', '▣', '▤', '▥', '▧', '▨', '▩', '◫', '⬒', '⬓', '◧', '◨', '◐', '◑',
  '☰', '☱', '☲', '☳', '☴', '☵', '☶', '☷', '≣', '⌁', '⇩', '⇧', '⇦', '⇨',
  '❐', '❑', '❒', '⌧', '⚙', '☺', '☹', '◇', '◈', '◉', '✚', '✕', '✖', '✓',
  '✔', '▲', '▼', '◀', '▶', '★', '☆', '🔒', '🔓', '🖨', '🖶', '☀', '☾', '☼',
];

const CHECKS = [
  {
    what: 'a page margin in a feature file',
    // `padding: <two or four values>` on a rule — the shape of a page margin.
    // A one-value padding is a control's own inset and is fine.
    re: /^\s*padding\s*:\s*var\(--(?:space|page-pad|gap)[^;]*\)\s+var\([^;]*;/,
    only: 'css',
    // Only the outermost element of a screen is the page. We cannot parse CSS
    // here, so the test is narrower and honest: a padding whose FIRST value is
    // the page's own vertical margin token.
    when: (line) => line.includes('--page-pad'),
    fix: 'The page margin belongs to .mb-main in shell.css. Delete this.',
  },
  {
    what: 'a hand-rolled page header',
    // No leading `\b`: the class is written `mb-menu__pagehead`, and `_` is a
    // word character, so a word boundary never appears before `page`. Found by
    // pointing the lint at a deliberately broken file — a lint nobody has seen
    // fail is a lint nobody knows works.
    re: /class(?:Name)?=["'`][^"'`]*(?:page-?head|screen-?head|page-?title|screen-?title)/,
    only: 'tsx',
    fix: 'Use <PageHeader> from the kit.',
  },
  {
    what: 'an svg outside the kit',
    re: /<svg[\s>]/,
    only: 'tsx',
    fix: 'Add it to kit/Icon.tsx and use <Icon name="…" />.',
  },
  {
    what: 'a glyph used as an icon',
    re: new RegExp(`[${GLYPH_ICONS.join('')}]`, 'u'),
    only: 'both',
    fix: 'Use <Icon name="…" /> — one set, one stroke weight, one optical size.',
  },
];

/** Lines saying `mb-layout-allow: <why>` exempt themselves, and are counted. */
const ESCAPE = 'mb-layout-allow:';

/**
 * Comments describe these rules constantly — this file's own header names
 * every glyph it bans. Stripping them is what keeps the lint from punishing
 * the thing that explains it. Blanked rather than deleted, so a reported line
 * number still points at the right line of the real file.
 */
function stripComments(text) {
  const block = new RegExp('/\\*[\\s\\S]*?\\*/', 'g');
  const lineComment = new RegExp('^\\s*//.*$', 'gm');
  const jsxComment = new RegExp('\\{/\\*[\\s\\S]*?\\*/\\}', 'g');
  return text
    .replace(block, (found) => found.replace(new RegExp('[^\\n]', 'g'), ' '))
    .replace(jsxComment, (found) => found.replace(new RegExp('[^\\n]', 'g'), ' '))
    .replace(lineComment, '');
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

function inside(file, dirs) {
  const path = relative('.', file);
  return dirs.some((dir) => path.startsWith(dir + sep) || path === dir);
}

const problems = [];
let escapes = 0;

for (const file of walk(ROOT)) {
  const path = relative('.', file);
  const isKit = inside(file, KIT_DIRS);
  const isShell = SHELL_FILES.includes(path);
  const kind = /\.css$/.test(file) ? 'css' : 'tsx';

  // Raw, not stripped — see the note on ESCAPE in `check-tokens.mjs`: an
  // escape written inside a CSS comment is blanked before it can be read, so
  // the hatch has to be looked for in the original text.
  const source = readFileSync(file, 'utf8');
  const raw = source.split(/\r?\n/);
  const lines = stripComments(source).split(/\r?\n/);
  lines.forEach((line, index) => {
    if ((raw[index] ?? '').includes(ESCAPE)) {
      escapes += 1;
      return;
    }
    for (const check of CHECKS) {
      if (check.only !== 'both' && check.only !== kind) continue;
      // The kit is where these shapes are implemented; the shell is where the
      // one page margin lives.
      if (isKit) continue;
      if (isShell && check.what !== 'a glyph used as an icon') continue;
      if (check.when && !check.when(line)) continue;

      const found = check.re.exec(line);
      check.re.lastIndex = 0;
      if (found) {
        problems.push({
          file: path,
          line: index + 1,
          what: check.what,
          text: found[0].trim().slice(0, 60),
          fix: check.fix,
        });
      }
    }
  });
}

if (problems.length > 0) {
  console.error(
    `\n  ${problems.length} layout problem(s).\n` +
      `  A screen does not own the shape of a page — that is P27.5, and it is\n` +
      `  the fix for "it looks here and there". The kit owns it.\n`,
  );
  for (const p of problems) {
    console.error(`  ${p.file}:${p.line}  ${p.what}: ${p.text}`);
    console.error(`      ${p.fix}`);
  }
  console.error('');
  process.exit(1);
}

console.log(
  `  layout: clean${escapes > 0 ? ` (${escapes} documented exception(s))` : ''}`,
);
