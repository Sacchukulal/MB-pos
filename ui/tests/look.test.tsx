/**
 * **P27.5 — the design session, as assertions.**
 *
 * The deliverable of a look session is what is on the glass, and no test can
 * assert "this is good". What a test CAN assert is the set of mechanical
 * claims the design rests on — the ones that were quietly untrue before, and
 * the ones a later session would break by accident:
 *
 *   1. every navigation item draws a real icon and keeps its word (§5);
 *   2. the navigation is a fixed six plus More, so the bar cannot overflow;
 *   3. the More button says where you are when you are inside it;
 *   4. one icon set, one stroke weight, one optical size;
 *   5. an icon inherits its colour, which is what makes it theme-proof (D21);
 *   6. the layout primitives put the same shape on every screen;
 *   7. the two lints that keep all of the above true actually fail.
 *
 * The look itself is judged by the owner, on the screen. That is not a gap in
 * this file; it is the correct division of labour, and `UI_AFTER.md` is where
 * the screenshots live.
 */

import { cleanup, render, screen, within } from '@testing-library/react';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';

import { Icon, Page, PageHeader, Panel, Toolbar } from '../src/kit';
import { SHIPPED_SCREENS, splitScreens } from '../src/shell/Shell';

afterEach(cleanup);

// ---------------------------------------------------------------------------
// The icon set
// ---------------------------------------------------------------------------

describe('the icon set', () => {
  /**
   * **One stroke weight, one geometry, no fills.**
   *
   * The rail used to draw `▦ ☰ ⌁ ⬒ ⇩` and got whatever Windows substituted —
   * three different weights, three different optical sizes, three different
   * vertical positions. That is most of what the owner meant by "old-styled
   * and unprofessional", and this is the assertion that stops it returning by
   * a different route: an icon that arrives with its own stroke or its own
   * fill is an icon that will drift away from the others.
   */
  it('draws every icon on the same grid, at the same weight, unfilled', () => {
    const { container } = render(
      <>
        <Icon name="receipt" />
        <Icon name="grid" />
        <Icon name="settings" />
        <Icon name="printer" />
        <Icon name="warning" />
      </>,
    );

    const svgs = [...container.querySelectorAll('svg')];
    expect(svgs).toHaveLength(5);
    for (const svg of svgs) {
      expect(svg.getAttribute('viewBox')).toBe('0 0 24 24');
      expect(svg.getAttribute('stroke-width')).toBe('1.75');
      expect(svg.getAttribute('stroke-linecap')).toBe('round');
      expect(svg.getAttribute('stroke-linejoin')).toBe('round');
      expect(svg.getAttribute('fill')).toBe('none');
    }
  });

  /**
   * **`currentColor`, which is what makes an icon theme-proof.**
   *
   * D21 says a theme is one block of token values and nothing else. An icon
   * carrying its own colour would be a second place a theme has to reach, and
   * the swap test in `theme.test.tsx` would not catch it — that test asserts
   * the DOM is identical, and a hardcoded stroke is identical under every
   * theme. That is exactly the bug it would hide.
   */
  it('inherits its colour rather than carrying one', () => {
    const { container } = render(<Icon name="wallet" />);
    const svg = container.querySelector('svg');
    expect(svg?.getAttribute('stroke')).toBe('currentColor');
    expect(container.innerHTML).not.toMatch(/#[0-9a-f]{3,8}/i);
  });

  /**
   * **Sized off the type scale.** A shopkeeper who turns the text size up gets
   * bigger icons too; an icon that stayed at 20px beside 24px text is the
   * thing that looks broken.
   */
  it('takes its size from a class, not from an attribute', () => {
    const { container } = render(
      <>
        <Icon name="check" size="sm" />
        <Icon name="check" />
        <Icon name="check" size="lg" />
      </>,
    );
    const classes = [...container.querySelectorAll('svg')].map((s) => s.getAttribute('class'));
    expect(classes).toEqual([
      'mb-icon mb-icon--sm',
      'mb-icon mb-icon--md',
      'mb-icon mb-icon--lg',
    ]);
    // Nothing hardcodes width or height, so the token decides.
    for (const svg of container.querySelectorAll('svg')) {
      expect(svg.hasAttribute('width')).toBe(false);
      expect(svg.hasAttribute('height')).toBe(false);
    }
  });

  /**
   * **Decorative by default, labelled on purpose.** §7: an icon-only control
   * needs a name a screen reader can say; an icon beside a visible word must
   * NOT have one, or the reader says everything twice.
   */
  it('is hidden from a screen reader unless it is given a label', () => {
    const { container } = render(
      <>
        <Icon name="lock" />
        <Icon name="lock" label="Locked" />
      </>,
    );
    const [plain, labelled] = [...container.querySelectorAll('svg')];
    expect(plain?.getAttribute('aria-hidden')).toBe('true');
    expect(plain?.hasAttribute('role')).toBe(false);
    expect(labelled?.getAttribute('role')).toBe('img');
    expect(labelled?.getAttribute('aria-label')).toBe('Locked');
    expect(labelled?.hasAttribute('aria-hidden')).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// The layout primitives
// ---------------------------------------------------------------------------

describe('the layout primitives', () => {
  /**
   * **The count is beside the title, not inside it.**
   *
   * "Menu (43)" is a string a translator cannot move and a number the eye has
   * to dig out of a word. Two elements, so the number can be styled as one.
   */
  it('sets a page title, its sentence and its count apart from each other', () => {
    render(<PageHeader title="Credit" subtitle="Who owes this shop money." count={5} />);

    const heading = screen.getByRole('heading', { level: 1 });
    expect(heading.textContent).toBe('Credit');
    expect(screen.getByText('5')).toBeTruthy();
    expect(screen.getByText('Who owes this shop money.')).toBeTruthy();
  });

  /**
   * **A panel is raised once.** The elevation contract in tokens.css has three
   * levels and no fourth, and nothing in this product is elevated twice — a
   * card inside a card is the thing §5 calls out by name. The nesting rule is
   * in CSS, so what is asserted here is that the structure it keys on is
   * actually produced.
   */
  it('gives a panel one head and one body, so the nesting rule can find them', () => {
    const { container } = render(
      <Panel title="Today" note="so far">
        <p>content</p>
      </Panel>,
    );
    const panel = container.querySelector('.mb-panel');
    expect(panel).toBeTruthy();
    expect(panel?.querySelectorAll('.mb-panel__head')).toHaveLength(1);
    expect(panel?.querySelectorAll('.mb-panel__body')).toHaveLength(1);
    expect(within(panel as HTMLElement).getByText('Today')).toBeTruthy();
    expect(within(panel as HTMLElement).getByText('so far')).toBeTruthy();
  });

  /**
   * **A page does not set its own margin.**
   *
   * Fourteen screens each set their own and they disagreed, which is what the
   * owner was describing. The margin is `.mb-main`'s, in the shell, once. If
   * `Page` ever grows one back, this fails.
   */
  it('leaves the page margin to the shell', () => {
    const { container } = render(
      <Page>
        <PageHeader title="Stock" />
      </Page>,
    );
    const page = container.querySelector('.mb-page');
    expect(page).toBeTruthy();
    expect(page?.getAttribute('style')).toBeNull();
    // The scrolling variant is opt-out, not opt-in: most screens are documents.
    expect(page?.className).toContain('mb-page--scroll');
  });

  it('gives a toolbar a start and an end, so filters and views cannot merge', () => {
    const { container } = render(
      <Toolbar end={<button type="button">Everything</button>}>
        <button type="button">All</button>
      </Toolbar>,
    );
    expect(container.querySelector('.mb-toolbar__start')).toBeTruthy();
    expect(container.querySelector('.mb-toolbar__end')).toBeTruthy();
  });
});

// ---------------------------------------------------------------------------
// The lints
// ---------------------------------------------------------------------------

/**
 * **A guard nobody has watched fail is a guard nobody knows works.**
 *
 * `check-layout.mjs` is what keeps P28, P29 and every session after them from
 * re-introducing exactly what this session removed. So it gets pointed at a
 * deliberately broken file and asked to complain — the same argument D55 makes
 * about screens, applied to a build script.
 */
describe('check-layout.mjs', () => {
  const cases: { name: string; file: string; body: string; says: string }[] = [
    {
      name: 'a page margin in a feature file',
      file: 'bad.css',
      body: '.mb-bad {\n  padding: var(--page-pad-y) var(--page-pad-x);\n}\n',
      says: 'a page margin in a feature file',
    },
    {
      name: 'a hand-rolled page header',
      file: 'Bad.tsx',
      body: 'export const Bad = () => <div className="mb-bad__pagehead" />;\n',
      says: 'a hand-rolled page header',
    },
    {
      name: 'an svg outside the kit',
      file: 'Bad.tsx',
      body: 'export const Bad = () => <svg viewBox="0 0 24 24" />;\n',
      says: 'an svg outside the kit',
    },
    {
      name: 'a glyph used as an icon',
      file: 'Bad.tsx',
      body: 'export const Bad = () => <span>▦</span>;\n',
      says: 'a glyph used as an icon',
    },
  ];

  for (const bad of cases) {
    it(`fails the build on ${bad.name}`, () => {
      const dir = mkdtempSync(join(tmpdir(), 'mb-layout-'));
      const src = join(dir, 'src', 'bad');
      mkdirSync(src, { recursive: true });
      writeFileSync(join(src, bad.file), bad.body, 'utf8');

      let output = '';
      let failed = false;
      try {
        execFileSync(process.execPath, ['scripts/check-layout.mjs', join(dir, 'src')], {
          encoding: 'utf8',
        });
      } catch (cause) {
        failed = true;
        const e = cause as { stderr?: string; stdout?: string };
        output = `${e.stdout ?? ''}${e.stderr ?? ''}`;
      } finally {
        rmSync(dir, { recursive: true, force: true });
      }

      expect(failed, `the lint accepted ${bad.name}`).toBe(true);
      expect(output).toContain(bad.says);
    });
  }

  /** And it passes on the real tree, which is the other half of the claim. */
  it('passes on the product', () => {
    const out = execFileSync(process.execPath, ['scripts/check-layout.mjs'], {
      encoding: 'utf8',
    });
    expect(out).toContain('layout: clean');
  });
});

/**
 * **The escape hatch is reachable from a CSS file**, which it was not until
 * P27.5. CSS has no line-comment syntax, so the only place to write a reason is
 * inside a block comment — and both lints blanked block comments before looking
 * for the marker. The one legitimate raw value in this product (a media-query
 * breakpoint; a media condition is evaluated before the cascade, so `var()` is
 * invalid there) therefore could not be declared at all.
 */
describe('the documented-exception hatch', () => {
  it('can be written in a CSS comment', () => {
    const dir = mkdtempSync(join(tmpdir(), 'mb-escape-'));
    const src = join(dir, 'src', 'bad');
    mkdirSync(src, { recursive: true });
    writeFileSync(
      join(src, 'bad.css'),
      '@media (max-width: 1200px) { /* mb-tokens-allow: media queries cannot use var() */\n  .x { color: var(--text); }\n}\n',
      'utf8',
    );

    const out = execFileSync(process.execPath, ['scripts/check-tokens.mjs', join(dir, 'src')], {
      encoding: 'utf8',
    });
    rmSync(dir, { recursive: true, force: true });

    expect(out).toContain('tokens: clean');
    expect(out).toContain('1 documented exception');
  });
});

// ---------------------------------------------------------------------------
// The top bar
// ---------------------------------------------------------------------------

describe('the top navigation', () => {
  /**
   * **The bar is a fixed six and cannot grow.**
   *
   * Thirteen destinations do not fit across 1366px with a readable word under
   * each, and §5 rules out the usual escape — *"icon-only is fast for a daily
   * user and hostile to a new one"*. So the split is by how often a shop opens
   * the screen, and it does not move with where you are: the first version put
   * the current screen in the bar too, which made it eight items wide and ran
   * "More" over the signed-in name.
   */
  it('keeps the same six in the bar wherever you are', () => {
    const onBilling = splitScreens(SHIPPED_SCREENS, 'billing');
    const onStock = splitScreens(SHIPPED_SCREENS, 'stock');

    expect(onBilling.inBar.map((s) => s.id)).toEqual(onStock.inBar.map((s) => s.id));
    expect(onBilling.inBar).toHaveLength(6);
    expect(onBilling.inBar.map((s) => s.id)).toEqual([
      'billing',
      'floor',
      'credit',
      'expenses',
      'bills',
      'reports',
    ]);
  });

  /** And the More button says where you are when you are behind it. */
  it('names the current screen on the More button when it came from More', () => {
    expect(splitScreens(SHIPPED_SCREENS, 'billing').elsewhere).toBeNull();
    expect(splitScreens(SHIPPED_SCREENS, 'stock').elsewhere?.label).toBe('Stock');
    expect(splitScreens(SHIPPED_SCREENS, 'settings').elsewhere?.label).toBe('Settings');
  });

  /**
   * **Every destination is reachable and none is in both places.** A screen
   * that is neither `daily` nor in the sheet is a screen a shop cannot open.
   */
  it('puts every screen in exactly one of the two places', () => {
    const { inBar, inMore } = splitScreens(SHIPPED_SCREENS, 'billing');
    const ids = [...inBar, ...inMore].map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
    expect(ids.sort()).toEqual([...SHIPPED_SCREENS].map((s) => s.id).sort());
  });

  /**
   * **Every item has a real icon and keeps its word** (§5). The icon names are
   * a union type, so a typo is a compile error rather than a hole in the
   * navigation — which is what `▦` was, silently, for whichever shop's Windows
   * had no glyph for it.
   */
  it('gives every screen an icon and a label', () => {
    for (const screen of SHIPPED_SCREENS) {
      expect(screen.label.trim(), `${screen.id} has no label`).not.toBe('');
      expect(screen.icon.trim(), `${screen.id} has no icon`).not.toBe('');
      // Not a glyph: a name from the set.
      expect(screen.icon).toMatch(/^[a-z-]+$/);
    }
  });
});

// ---------------------------------------------------------------------------
// The cart column
// ---------------------------------------------------------------------------

/**
 * **Complete bill must never go below the fold.**
 *
 * P09 wrote three paragraphs about fixing this and P27.5 broke it again within
 * an hour, by adding a second floor to the cart's row template. Grid honours
 * floors by OVERFLOWING when their sum does not fit, and that column is
 * `overflow: hidden`, so what overflowed was the bottom — the button on the
 * one path PERFORMANCE §2.2 calls sacred. Found by billing a real table for
 * cash and looking at the screen (D55).
 *
 * **jsdom has no layout engine**, so this cannot be asserted by measuring
 * anything; `getBoundingClientRect` returns zeros. What it CAN assert is that
 * the mechanism which makes the promise is still the one in the file: a flex
 * column with an explicit shrink ORDER, and no absolute floor to overflow
 * against. That is a weaker test than measuring, and it is written down as
 * weaker rather than dressed up — but it fails the moment somebody reaches for
 * `grid-template-rows` and a `minmax()` again, which is exactly how this bug
 * arrives both times.
 */
describe('the cart column', () => {
  // Comments stripped first: the block below EXPLAINS why it is not a grid,
  // by naming the thing it is not — so a raw text search finds the words in
  // the very comment that documents their absence. The lints strip comments
  // for the same reason and it is the same trap.
  // Comments stripped FIRST. The block below explains why it is not a grid by
  // naming the thing it is not, so a raw text search finds those words inside
  // the very comment that documents their absence. Both lints strip comments
  // for the same reason; it is the same trap.
  const raw = readFileSync(join('src', 'billing', 'billing.css'), 'utf8');
  const css = raw.replace(new RegExp('/\\*[\\s\\S]*?\\*/', 'g'), '');
  const cart = css.slice(
    css.indexOf('.mb-billing__cart {'),
    css.indexOf('.mb-floor__section {'),
  );

  it('is a flex column, not a row template with floors', () => {
    expect(cart).toContain('display: flex');
    expect(cart).not.toContain('grid-template-rows');
    expect(cart).not.toContain('minmax(');
  });

  it('never shrinks the payment block or the actions', () => {
    for (const rule of ['.mb-payment {', '.mb-actions {']) {
      const at = css.indexOf(rule);
      expect(at, `${rule} is missing`).toBeGreaterThan(-1);
      const block = css.slice(at, css.indexOf('}', at));
      expect(block, `${rule} may be shrunk`).toContain('flex: 0 0 auto');
    }
  });

  /** The item list gives way before the totals do — a number's only job here
   *  is to be bigger than the other one. */
  it('gives the item list a much higher shrink than the totals', () => {
    const lines = css.slice(css.indexOf('.mb-cart__lines {'));
    const linesShrink = /flex:\s*1\s+(\d+)/.exec(lines)?.[1];
    const totals = css.slice(css.indexOf('.mb-totals {'));
    expect(linesShrink).toBeDefined();
    expect(Number(linesShrink)).toBeGreaterThan(10);
    expect(totals).toContain('flex: 0 1 auto');
  });
});
