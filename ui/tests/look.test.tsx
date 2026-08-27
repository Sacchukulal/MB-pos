import { cleanup, render, screen, within } from '@testing-library/react';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';

import { Icon, Page, PageHeader, Panel, Toolbar } from '../src/kit';
import { SHIPPED_SCREENS, splitScreens } from '../src/shell/Shell';

afterEach(cleanup);

// The icon set.

describe('the icon set', () => {
  /** One stroke weight, one geometry, no fills. */
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

  /** `currentColor`, which is what makes an icon theme-proof. */
  it('inherits its colour rather than carrying one', () => {
    const { container } = render(<Icon name="wallet" />);
    const svg = container.querySelector('svg');
    expect(svg?.getAttribute('stroke')).toBe('currentColor');
    expect(container.innerHTML).not.toMatch(/#[0-9a-f]{3,8}/i);
  });

  /** Sized off the type scale. */
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

  /** Decorative by default, labelled on purpose. */
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

// The layout primitives.

describe('the layout primitives', () => {
  /** The count is beside the title, not inside it. */
  it('sets a page title, its sentence and its count apart from each other', () => {
    render(<PageHeader title="Credit" subtitle="Who owes this shop money." count={5} />);

    const heading = screen.getByRole('heading', { level: 1 });
    expect(heading.textContent).toBe('Credit');
    expect(screen.getByText('5')).toBeTruthy();
    expect(screen.getByText('Who owes this shop money.')).toBeTruthy();
  });

  /** A panel is raised once. */
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

  /** A page does not set its own margin. */
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

// The lints.

/** A guard nobody has watched fail is a guard nobody knows works. */
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

// The top bar.

describe('the top navigation', () => {
  /** The bar is a fixed six and cannot grow. */
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

  /** Every destination is reachable and none is in both places. */
  it('puts every screen in exactly one of the two places', () => {
    const { inBar, inMore } = splitScreens(SHIPPED_SCREENS, 'billing');
    const ids = [...inBar, ...inMore].map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
    expect(ids.sort()).toEqual([...SHIPPED_SCREENS].map((s) => s.id).sort());
  });

  /** Every item has a real icon and keeps its word (§5). */
  it('gives every screen an icon and a label', () => {
    for (const screen of SHIPPED_SCREENS) {
      expect(screen.label.trim(), `${screen.id} has no label`).not.toBe('');
      expect(screen.icon.trim(), `${screen.id} has no icon`).not.toBe('');
      // Not a glyph: a name from the set.
      expect(screen.icon).toMatch(/^[a-z-]+$/);
    }
  });
});

// The cart column.

/** Complete bill must never go below the fold. */
describe('the cart column', () => {
  // Comments stripped first: the block below EXPLAINS why it is not a grid, by naming the thing
  // it is not — so a raw text search finds the words in the very comment that documents their
  // absence.
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

  /**
   * The item list gives way; the totals never do — a cashier must never lose the total.
   */
  it('lets the item list shrink and never the totals', () => {
    const lines = css.slice(css.indexOf('.mb-cart__lines {'));
    const linesShrink = /flex:\s*1\s+(\d+)/.exec(lines)?.[1];
    const totals = css.slice(css.indexOf('.mb-totals {'));
    expect(linesShrink).toBeDefined();
    expect(Number(linesShrink)).toBeGreaterThan(10);
    expect(totals).toContain('flex: 0 0 auto');
  });
});

/** The lint that keeps an id out of the clock. */
describe('check-ids.mjs', () => {
  const cases: { name: string; file: string; body: string; says: string }[] = [
    {
      name: 'a screen building an id out of the clock',
      file: 'Bad.tsx',
      body: 'const id = `cus_${Date.now().toString(36)}`;\nexport const Bad = () => id;\n',
      says: 'Date.now() in a string',
    },
    {
      name: 'Rust building an id out of the clock',
      file: 'bad.rs',
      body: 'fn bad(at: Timestamp) -> String { format!("adj_{}", at.millis()) }\n',
      says: 'a clock reading inside format!',
    },
  ];

  for (const bad of cases) {
    it(`fails the build on ${bad.name}`, () => {
      const dir = mkdtempSync(join(tmpdir(), 'mb-ids-'));
      const src = join(dir, 'src');
      mkdirSync(src, { recursive: true });
      writeFileSync(join(src, bad.file), bad.body, 'utf8');

      let output = '';
      let failed = false;
      try {
        execFileSync(process.execPath, ['scripts/check-ids.mjs', src], { encoding: 'utf8' });
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

  /**
   * An id derived from the business day is not the bug — `close_{day}` is one per day on
   * purpose, and a second one has to collide so the database refuses it.
   */
  it('leaves an id that is derived from the business day alone', () => {
    const dir = mkdtempSync(join(tmpdir(), 'mb-ids-ok-'));
    const src = join(dir, 'src');
    mkdirSync(src, { recursive: true });
    writeFileSync(
      join(src, 'fine.rs'),
      'fn ok(day: BusinessDay) -> String { format!("close_{}", day.days_since_epoch()) }\n',
      'utf8',
    );
    try {
      const out = execFileSync(process.execPath, ['scripts/check-ids.mjs', src], {
        encoding: 'utf8',
      });
      expect(out).toContain('ids: clean');
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  /** And it passes on the real tree, which is the other half of the claim. */
  it('passes on the product', () => {
    const out = execFileSync(process.execPath, ['scripts/check-ids.mjs'], {
      encoding: 'utf8',
    });
    expect(out).toContain('ids: clean');
  });
});

/** The cart's controls, after the 2026-08-27 round: nothing in a row is taller than a button. */
describe('the cart controls', () => {
  const strip = (raw: string) => raw.replace(new RegExp('/\\*[\\s\\S]*?\\*/', 'g'), '');
  const kit = strip(readFileSync(join('src', 'kit', 'kit.css'), 'utf8'));
  const billing = strip(readFileSync(join('src', 'billing', 'billing.css'), 'utf8'));
  const block = (css: string, rule: string) => {
    const at = css.indexOf(rule);
    expect(at, `${rule} is missing`).toBeGreaterThan(-1);
    return css.slice(at, css.indexOf('}', at));
  };

  it('makes a small button SMALLER than a full one, by its own token', () => {
    // It was `--space-7` — three rem, taller than the 44px it was meant to sit under.
    const small = block(kit, '.mb-button--small {');
    expect(small).toContain('min-height: var(--target-small)');
    expect(small).not.toContain('min-height: var(--space-');
  });

  it('pays through the segmented control, not three styled buttons', () => {
    expect(billing).not.toContain('.mb-payment__mode {');
    expect(billing).not.toContain('.mb-payment__mode--on');
    expect(kit).toContain('.mb-segment--fill');
  });

  it('wraps the fold rather than letting a button run into the next', () => {
    const fold = block(billing, '.mb-actions--more {');
    expect(fold).toContain('flex-wrap: wrap');
    expect(fold).not.toContain('repeat(3');
  });

  it('gives the processing column its width from one token, and folds only up and down', () => {
    expect(block(billing, '.mb-billing__side {')).toContain('var(--queue-width)');
    expect(readFileSync(join('src', 'theme', 'tokens.css'), 'utf8')).toContain('--queue-width:');
    // The head is as wide as the list under it.
    expect(block(billing, '.mb-processing__head {')).toContain('width: 100%');
    // Rows fold, with the product's own motion — never a number of milliseconds here.
    const fold = block(billing, '.mb-processing__fold {');
    expect(fold).toContain('transition: grid-template-rows var(--motion-normal) var(--ease)');
    expect(fold).not.toMatch(/\d+ms/);
    expect(billing).not.toContain('grid-template-columns var(--motion');
  });
});
