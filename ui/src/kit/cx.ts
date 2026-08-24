/**
 * Join class names, dropping the falsy ones.
 *
 * Thirty-one copies of `[...].filter(Boolean).join(' ')` were written by hand
 * before this existed. One of them is one too many.
 */
export function cx(...names: (string | false | null | undefined)[]): string {
  return names.filter(Boolean).join(' ');
}
