/**
 * **The instrumentation hook — `PERFORMANCE.md` §3.2, and budget B1.**
 *
 * > *"P08 builds a small instrumentation hook: mark the input event, mark the
 * > paint, report the delta. P09/P10 assert against B1/B3/B8 with it. React
 * > profiling in development only — it must compile out of a release build
 * > entirely."*
 *
 * P09 deliberately left this to P10, because a hook shaped by a session that
 * does not use it measures the wrong thing. This one is used by the thing it
 * was built for.
 *
 * # What it measures, and why that is the honest boundary
 *
 * From the **input event** to the **next paint** — not to the end of a React
 * render, which is a lie a profiler would happily tell. A cashier does not
 * experience a commit; they experience a frame appearing. `requestAnimationFrame`
 * fires before paint, so a second one lands after it, which is as close as the
 * platform allows.
 *
 * **B1 is the product.** *"A trained cashier on v1 was faster than touch-first
 * competitors, and that came entirely from the keyboard. One dropped frame per
 * keystroke is the whole advantage gone."*
 *
 * # It compiles out
 *
 * Every call sits behind `import.meta.env.DEV`, which Vite replaces with a
 * literal — so the whole module is dead code in a release build and the bundler
 * removes it. Measurement must never be a thing the shipped counter pays for.
 */

export interface Measurement {
  what: string;
  ms: number;
}

const recorded: Measurement[] = [];

/**
 * Mark an input event and report how long it took to reach the screen.
 *
 * ```ts
 * onKeyDown={(e) => { const done = mark('keystroke'); handle(e); done(); }}
 * ```
 */
export function mark(what: string): () => void {
  if (!import.meta.env.DEV) return () => undefined;
  const started = performance.now();
  return () => {
    // Two frames: the first fires BEFORE paint, the second after it. The gap
    // between them is where the pixels actually changed.
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        const ms = performance.now() - started;
        recorded.push({ what, ms });
        // Kept small. A counter that has been open eight hours must not be
        // holding a measurement per keystroke (budget M3).
        if (recorded.length > 200) recorded.shift();
      });
    });
  };
}

/** Everything measured so far, for a test or the health panel (P22). */
export function measurements(): readonly Measurement[] {
  return recorded;
}

/**
 * The worst reading for one label — which is what a budget is about.
 *
 * §3.1 rule 3 says assert the CEILING rather than the budget, and a ceiling is
 * a statement about the worst case, not the average. An average keystroke
 * being fast is no comfort to the cashier who felt the slow one.
 */
export function worst(what: string): number | null {
  const readings = recorded.filter((m) => m.what === what).map((m) => m.ms);
  return readings.length === 0 ? null : Math.max(...readings);
}

export function reset(): void {
  recorded.length = 0;
}
