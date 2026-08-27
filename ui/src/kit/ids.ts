/** Where a new row's id comes from, on this side of the wire. */

/** How many random characters go on the end. */
const TAIL = 10;

const ALPHABET = '0123456789abcdefghijklmnopqrstuvwxyz';

/** A fresh id for a new row. */
export function freshId(prefix: string): string {
  return `${prefix}_${Date.now().toString(36)}_${tail()}`;
}

function tail(): string {
  const bytes = new Uint8Array(TAIL);
  crypto.getRandomValues(bytes);
  // Modulo 36 of a byte is very slightly biased — 256 is not a multiple of 36, so the first
  // four letters come up a shade more often.
  return Array.from(bytes, (b) => ALPHABET[b % 36] ?? '0').join('');
}
