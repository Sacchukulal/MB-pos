/** What the screen is, for anything drawn dot for dot. */

import { useEffect, useState } from 'react';

/** Device pixels per CSS pixel, and again whenever the window moves to another screen. */
export function useDevicePixelRatio(): number {
  const [ratio, setRatio] = useState(() =>
    typeof window === 'undefined' ? 1 : window.devicePixelRatio || 1,
  );
  useEffect(() => {
    if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return undefined;
    // The query is true only at the current ratio, so it fires when the ratio changes.
    const query = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
    const update = () => setRatio(window.devicePixelRatio || 1);
    query.addEventListener('change', update);
    return () => query.removeEventListener('change', update);
  }, [ratio]);
  return ratio;
}

/** CSS times keep their units: a production build may shorten 3000ms to 3s. */
export function cssTimeMs(value: string): number {
  const time = /^([+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:e[+-]?\d+)?)(ms|s)$/i.exec(value.trim());
  if (!time) return 0;
  const milliseconds = Number(time[1]) * (time[2]?.toLowerCase() === 's' ? 1000 : 1);
  return Number.isFinite(milliseconds) && milliseconds >= 0 ? milliseconds : 0;
}

/** The bell's full animation lifetime in milliseconds, including reduced-motion tokens. */
export function beatsFor(): number {
  const theme = getComputedStyle(document.documentElement);
  const beat = cssTimeMs(theme.getPropertyValue('--motion-beat'));
  const beats = Number.parseFloat(theme.getPropertyValue('--beats'));
  return Number.isFinite(beat) && Number.isFinite(beats) ? beat * beats : 0;
}
