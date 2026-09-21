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

/**
 * How long something new is drawn attention to, in milliseconds — asked of the theme, never
 * decided here. One beat times the number of beats: the bell's ring, and a card that just
 * arrived. A counter with motion turned off has the beat set to zero there, so this comes
 * back zero and nothing beats at all.
 */
export function beatsFor(): number {
  const theme = getComputedStyle(document.documentElement);
  const beat = Number.parseFloat(theme.getPropertyValue('--motion-beat'));
  const beats = Number.parseFloat(theme.getPropertyValue('--beats'));
  return Number.isFinite(beat) && Number.isFinite(beats) ? beat * beats : 0;
}
