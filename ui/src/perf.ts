/** The instrumentation hook. */

export interface Measurement {
  what: string;
  ms: number;
}

const recorded: Measurement[] = [];

/** Mark an input event and report how long it took to reach the screen. */
export function mark(what: string): () => void {
  if (!import.meta.env.DEV) return () => undefined;
  const started = performance.now();
  return () => {
    // Two frames: the first fires BEFORE paint, the second after it.
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        const ms = performance.now() - started;
        recorded.push({ what, ms });
        // Kept small. A counter that has been open eight hours must not be holding a
        // measurement per keystroke.
        if (recorded.length > 200) recorded.shift();
      });
    });
  };
}

/** Everything measured so far, for a test or the health panel. */
export function measurements(): readonly Measurement[] {
  return recorded;
}

/** The worst reading for one label — which is what a budget is about. */
export function worst(what: string): number | null {
  const readings = recorded.filter((m) => m.what === what).map((m) => m.ms);
  return readings.length === 0 ? null : Math.max(...readings);
}

export function reset(): void {
  recorded.length = 0;
}
