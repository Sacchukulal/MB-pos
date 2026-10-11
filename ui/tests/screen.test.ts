import { afterEach, describe, expect, it } from 'vitest';

import { beatsFor, cssTimeMs } from '../src/kit/screen';

afterEach(() => document.documentElement.removeAttribute('style'));

describe('CSS time conversion', () => {
  it.each([
    ['3000ms', 3000], ['3s', 3000], ['250ms', 250], ['.25s', 250],
    [' 1.5s ', 1500], ['1e3ms', 1000], ['0s', 0], ['0ms', 0],
  ])('reads %s as %i milliseconds', (value, milliseconds) => {
    expect(cssTimeMs(value)).toBe(milliseconds);
  });

  it.each(['', '3', '3px', '3seconds', '-1s', 'Infinityms', '1e999s'])('rejects invalid duration %s', (value) => {
    expect(cssTimeMs(value)).toBe(0);
  });
});

describe('theme animation lifetime', () => {
  it.each(['1000ms', '1s'])('keeps three beats for three seconds with %s', (duration) => {
    document.documentElement.style.setProperty('--motion-beat', duration);
    document.documentElement.style.setProperty('--beats', '3');
    expect(beatsFor()).toBe(3000);
  });

  it.each(['0ms', '0s'])('respects reduced motion with %s', (duration) => {
    document.documentElement.style.setProperty('--motion-beat', duration);
    document.documentElement.style.setProperty('--beats', '0');
    expect(beatsFor()).toBe(0);
  });
});
