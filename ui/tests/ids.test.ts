/** Where a new row's id comes from. */

import { describe, expect, it, vi } from 'vitest';

import { freshId } from '../src/kit';

describe('a fresh id', () => {
  it('wears its prefix, so a row is recognisable in a log', () => {
    expect(freshId('cus')).toMatch(/^cus_/);
  });

  /** The bug, as a test. */
  it('never repeats, however fast they are asked for', () => {
    const made = new Set(Array.from({ length: 10_000 }, () => freshId('exp')));
    expect(made.size, 'two ids collided').toBe(10_000);
  });

  it('is only lower-case letters, digits and underscores', () => {
    // An id ends up in log lines and support screenshots, so it has to be a thing somebody can
    // read out over a phone without ambiguity.
    for (let n = 0; n < 500; n += 1) {
      const id = freshId('itm');
      expect(id, id).toMatch(/^[a-z0-9_]+$/);
    }
  });

  /**
   * Sorting by id is what made the clock attractive in the first place, and it is worth keeping
   * — the clock is still in there, it is just no longer alone.
   */
  it('sorts into the order the rows were made', () => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(new Date('2026-08-22T09:00:00Z'));
      const early = freshId('ord');
      vi.setSystemTime(new Date('2026-08-22T09:00:01Z'));
      const later = freshId('ord');
      expect(early < later, `${early} should sort before ${later}`).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  /**
   * And within one millisecond the clock half is identical, so only the tail is doing the work
   * — which is the whole point of it being there.
   */
  it('keeps the clock half steady inside one millisecond', () => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(new Date('2026-08-22T09:00:00Z'));
      const a = freshId('ord');
      const b = freshId('ord');
      expect(a.split('_')[1]).toBe(b.split('_')[1]);
      expect(a).not.toBe(b);
    } finally {
      vi.useRealTimers();
    }
  });

  it('has a random half that is actually random', () => {
    // The one screen that already had a tail used `Math.random() * 1000`, and a thousand values
    // is a repeat inside one millisecond after about forty rows.
    const tails = new Set(
      Array.from({ length: 2_000 }, () => freshId('x').split('_')[2]),
    );
    expect(tails.size).toBeGreaterThan(1_990);
  });
});
