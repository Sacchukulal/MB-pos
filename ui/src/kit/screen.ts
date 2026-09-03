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
