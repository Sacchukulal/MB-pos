#!/usr/bin/env node
/**
 * No page margin, hand-rolled page header, raw <svg> or glyph icon in a feature file.
 *
 * Usage:
 *   node scripts/check-layout.mjs            check src/
 *   node scripts/check-layout.mjs <dir>      check somewhere else
 */

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';

const ROOT = process.argv[2] ?? 'src';

/** The kit owns layout, icons and the page shape; the theme owns values. */
const KIT_DIRS = [join('src', 'kit'), join('src', 'theme')];

/** The shell draws the page margin, because somebody has to. */
const SHELL_FILES = [join('src', 'shell', 'shell.css')];

/**
 * The glyphs that were used as icons, and a few of the same family that would be the obvious
 * next choice.
 */
const GLYPH_ICONS = [
  '▦', '▣', '▤', '▥', '▧', '▨', '▩', '◫', '⬒', '⬓', '◧', '◨', '◐', '◑',
  '☰', '☱', '☲', '☳', '☴', '☵', '☶', '☷', '≣', '⌁', '⇩', '⇧', '⇦', '⇨',
  '❐', '❑', '❒', '⌧', '⚙', '☺', '☹', '◇', '◈', '◉', '✚', '✕', '✖', '✓',
  '✔', '▲', '▼', '◀', '▶', '★', '☆', '🔒', '🔓', '🖨', '🖶', '☀', '☾', '☼',
];

/** A COMPONENT'S CLASSES MAY ONLY BE WRITTEN BY THE FILE THAT OWNS IT. */
const OWNED_CLASSES = [
  {
    prefix: 'mb-tile',
    owner: join('src', 'billing', 'TableGrid.tsx'),
    what: 'the table tile',
  },
];

const CHECKS = [
  {
    what: 'a page margin in a feature file',
    // `padding: <two or four values>` on a rule — the shape of a page margin.
    re: /^\s*padding\s*:\s*var\(--(?:space|page-pad|gap)[^;]*\)\s+var\([^;]*;/,
    only: 'css',
    // Only the outermost element of a screen is the page.
    when: (line) => line.includes('--page-pad'),
    fix: 'The page margin belongs to .mb-main in shell.css. Delete this.',
  },
  {
    what: 'a hand-rolled page header',
    // No leading `\b`: the class is written `mb-menu__pagehead`, and `_` is a word character,
    // so a word boundary never appears before `page`.
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
    /**
     * A screen that scrolls itself puts the bar INSIDE its content, so the table ends 12px
     * short of the header above it.
     */
    what: 'a screen deciding for itself how it scrolls',
    re: /^\s*overflow(-y)?\s*:\s*(auto|scroll)\s*;/,
    only: 'css',
    fix: 'Use <Scroller> from the kit (add `inset` if it is not at the page edge).',
  },
  {
    /** `SectionHeader` is the shape; five screens had their own. */
    what: 'a hand-rolled section heading',
    re: /<h[23] className="mb-[a-z]+__(heading|title|sectiontitle)"/,
    only: 'tsx',
    fix: 'Use <SectionHeader> from the kit.',
  },
  {
    /** One place turns a count into words. */
    what: 'a developer plural',
    re: /\b[a-z]+\(s\)/,
    only: 'tsx',
    fix: "Use plural(n, 'item') from the kit.",
  },
  {
    /** The layer contract in tokens.css says what is above what. */
    what: 'a raw z-index',
    // The lookahead swallows the space itself: `:\s*(?!var\()` backtracks to zero spaces and
    // then "does not start with var(" is trivially true.
    re: /^\s*z-index\s*:(?!\s*var\()/,
    only: 'css',
    fix: 'Use a --layer-* token. See THE LAYER CONTRACT in tokens.css.',
  },
  {
    /**
     * Setting either of these switches Chromium to the OS scrollbar and silently drops
     * every::-webkit-scrollbar rule — stepper arrows, square thumb, Windows grey on a dark
     * page.
     */
    what: 'a scrollbar property that kills the themed scrollbar',
    re: /^\s*scrollbar-(width|color)\s*:/,
    only: 'css',
    fix: 'Delete it. The scrollbar is themed with ::-webkit-scrollbar in tokens.css.',
  },
  {
    what: 'a ring drawn around a box instead of on its edge',
    re: /outline-offset\s*:/,
    only: 'css',
    when: (line) =>
      !line.includes('var(--focus-ring-inset)') && !/outline-offset\s*:\s*0\s*;/.test(line),
    fix: 'Focus and selection use --focus-ring-inset, or change border-color. See :focus-visible in tokens.css.',
  },
  {
    what: 'an empty state with a paragraph',
    re: /<EmptyState\b[^>]*\bbody=/,
    only: 'tsx',
    fix: 'One line. Pass the explanation as `hint` (a tip) or a live sentence as `says`.',
  },
  {
    what: 'the old small-button prop',
    re: /<Button\b[^>]*\bsmall\b/,
    only: 'tsx',
    fix: 'Use size="sm".',
  },
  {
    what: 'small capitals outside a form caption',
    re: /text-transform\s*:\s*uppercase/,
    only: 'css',
    fix: 'Sentence case. The one caption that may be capitals is the kit Caption (mb-layout-allow: if it IS a form caption).',
  },
  {
    what: 'a scrollbar gutter reserved outside the kit',
    re: /scrollbar-gutter\s*:/,
    only: 'css',
    fix: 'Only the page body keeps a gutter (kit/layout.css).',
  },
  {
    what: 'a glyph used as an icon',
    re: new RegExp(`[${GLYPH_ICONS.join('')}]`, 'u'),
    only: 'both',
    fix: 'Use <Icon name="…" /> — one set, one stroke weight, one optical size.',
  },
];

/** The explanation under a heading. */
const EXPLAINING = {
  heading: /<(PageHeader|SectionHeader|h[1-3])[\s>]/,
  paragraph: /<(p|span) className="mb-(muted|[a-z]+__(note|sub|hint|blurb|lede))"\s*>/,
  /** How many lines after the heading still count as "under" it. */
  within: 6,
};

function explainingUnderHeadings(lines) {
  const found = [];
  let sinceHeading = Infinity;
  lines.forEach((line, index) => {
    if (EXPLAINING.heading.test(line)) sinceHeading = 0;
    else sinceHeading += 1;
    // A `{` means the words change with the data — a live message, not an explanation of the
    // screen.
    if (sinceHeading <= EXPLAINING.within && !line.includes('{')) {
      const hit = EXPLAINING.paragraph.exec(line);
      if (hit) found.push({ line: index + 1, text: hit[0] });
    }
  });
  return found;
}

/** Lines saying `mb-layout-allow: <why>` exempt themselves, and are counted. */
const ESCAPE = 'mb-layout-allow:';

/**
 * Comments describe these rules constantly — this file's own header names every glyph it bans.
 */
function stripComments(text) {
  const block = new RegExp('/\\*[\\s\\S]*?\\*/', 'g');
  // `[ \t]`, NOT `\s`: `\s` matches a newline, so `^\s*//` starting on a blank line swallowed
  // the line break and merged two `//` lines into one.
  const lineComment = new RegExp('^[ \\t]*//.*$', 'gm');
  const jsxComment = new RegExp('\\{/\\*[\\s\\S]*?\\*/\\}', 'g');
  const blank = (found) => found.replace(new RegExp('[^\\n]', 'g'), ' ');
  return text
    .replace(block, blank)
    .replace(jsxComment, blank)
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

  // Raw, not stripped — see the note on ESCAPE in `check-tokens.mjs`: an escape written inside
  // a CSS comment is blanked before it can be read, so the hatch has to be looked for in the
  // original text.
  const source = readFileSync(file, 'utf8');
  const raw = source.split(/\r?\n/);
  const lines = stripComments(source).split(/\r?\n/);

  if (kind === 'tsx' && !isKit) {
    for (const hit of explainingUnderHeadings(lines)) {
      // The line above too: JSX has no trailing comment, so the escape for a `<p>` has to sit
      // on its own line over it.
      const marked = [raw[hit.line - 1], raw[hit.line - 2]].some((l) =>
        (l ?? '').includes(ESCAPE),
      );
      if (marked) { escapes += 1; continue; }
      problems.push({
        file: path,
        line: hit.line,
        what: 'an explanation printed under a heading',
        text: hit.text,
        fix: 'Pass it as `note` on the heading — it becomes an InfoTip you can ask for.',
      });
    }
  }

  lines.forEach((line, index) => {
    if ((raw[index] ?? '').includes(ESCAPE)) {
      escapes += 1;
      return;
    }
    for (const check of CHECKS) {
      if (check.only !== 'both' && check.only !== kind) continue;
      // The kit is where these shapes are implemented; the shell is where the one page margin
      // lives.
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

    // A second author for one component's markup — see OWNED_CLASSES.
    if (kind !== 'tsx') return;
    for (const owned of OWNED_CLASSES) {
      if (path === owned.owner) continue;
      // Written inside a className, not merely mentioned.
      const re = new RegExp(
        `class(?:Name)?=["'\`{][^>]{0,200}?${owned.prefix}(?:__|--|[\\s"'\`])`,
      );
      const hit = re.exec(line);
      if (hit) {
        problems.push({
          file: path,
          line: index + 1,
          what: `${owned.what}, drawn outside the file that owns it`,
          text: hit[0].trim().slice(0, 60),
          fix: `Import it from ${owned.owner.replace(/\\/g, '/')} instead of drawing a second copy.`,
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
