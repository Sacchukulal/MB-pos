/**
 * The one sound the counter makes: a short two-note beep when an order lands from a phone.
 * Made here, not from a file — there is nothing to ship, nothing to load, and it plays the
 * same on every machine. The shop's switch (Settings › Billing) decides whether it is asked
 * for at all; that is the caller's business.
 */

let context: AudioContext | null = null;

/** The one AudioContext, made on the first beep — a page starts with none. */
function audio(): AudioContext | null {
  if (typeof window === 'undefined' || typeof AudioContext === 'undefined') return null;
  context ??= new AudioContext();
  // A context made before anybody clicked starts suspended; the cashier has clicked by now.
  if (context.state === 'suspended') void context.resume();
  return context;
}

/** Two rising notes, a fifth of a second — heard across a counter, over without being noticed twice. */
export function beep(): void {
  const ctx = audio();
  if (!ctx) return;
  const at = ctx.currentTime;
  const gain = ctx.createGain();
  gain.connect(ctx.destination);
  gain.gain.setValueAtTime(0.0001, at);
  gain.gain.exponentialRampToValueAtTime(0.25, at + 0.01);
  gain.gain.exponentialRampToValueAtTime(0.0001, at + 0.22);
  for (const [note, from, until] of [
    [880, 0, 0.1],
    [1175, 0.1, 0.22],
  ] as const) {
    const tone = ctx.createOscillator();
    tone.type = 'sine';
    tone.frequency.value = note;
    tone.connect(gain);
    tone.start(at + from);
    tone.stop(at + until);
  }
}
