/**
 * Counting things, in shop English.
 *
 * "3 item(s)" is how a developer writes it and nobody says it. One place turns
 * a number into words so no screen has to decide again.
 */
export function plural(
  count: number | bigint,
  one: string,
  many = `${one}s`,
): string {
  return `${count} ${count === 1 || count === 1n ? one : many}`;
}
