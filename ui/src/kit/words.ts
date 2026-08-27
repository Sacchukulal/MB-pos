/** Counting things, in shop English. */
export function plural(
  count: number | bigint,
  one: string,
  many = `${one}s`,
): string {
  return `${count} ${count === 1 || count === 1n ? one : many}`;
}
