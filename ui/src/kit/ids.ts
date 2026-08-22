/**
 * **Where a new row's id comes from, on this side of the wire.**
 *
 * The twin of `src-tauri/src/newid.rs`, and the same bug. Seventeen screens
 * built an id out of the clock — ``id: `cus_${Date.now().toString(36)}` `` and
 * its cousins — so two rows created in the same thousandth of a second got the
 * same id, and two rows cannot share one.
 *
 * # Why a screen makes an id at all
 *
 * Because the screen is where a row is *started*. "Add a customer" opens an
 * empty form, and the form needs to know what it is editing before Rust has
 * ever heard of it. That is a real need and it stays; only the source of the
 * unique part changes.
 *
 * # What it looks like
 *
 * `cus_mt4bb0ee_7fk3x9qz` — the prefix, the clock in base 36 so ids still sort
 * into the order they were made, and a random tail that is the part doing the
 * work. Deliberately the same shape as Rust's, so an id in a log line does not
 * say which side of the wire made it.
 *
 * # The randomness
 *
 * `crypto.getRandomValues`, which WebView2 has and which is the same kind of
 * source Rust uses. **Not `Math.random()`**: the one place that already had a
 * random tail used `Math.floor(Math.random() * 1000)`, and a thousand values is
 * a coin toss you lose about once in forty tries once a few rows are in play.
 */

/** How many random characters go on the end. Ten of base 36 is about 51 bits. */
const TAIL = 10;

const ALPHABET = '0123456789abcdefghijklmnopqrstuvwxyz';

/**
 * **A fresh id for a new row.** The only way a screen makes one.
 *
 * `prefix` is the short word that says what the row is: `cus`, `exp`, `itm`.
 */
export function freshId(prefix: string): string {
  return `${prefix}_${Date.now().toString(36)}_${tail()}`;
}

function tail(): string {
  const bytes = new Uint8Array(TAIL);
  crypto.getRandomValues(bytes);
  // **Modulo 36 of a byte is very slightly biased** — 256 is not a multiple of
  // 36, so the first four letters come up a shade more often. That matters for
  // a recovery code and does not matter here: this is a name tag, not a secret,
  // and the bias costs a fraction of a bit out of fifty-one.
  return Array.from(bytes, (b) => ALPHABET[b % 36] ?? '0').join('');
}
