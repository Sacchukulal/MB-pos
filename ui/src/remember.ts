/** A look preference, kept on this computer. Never a fact about the shop. */

export function remember(key: string, fallback: string): string {
  try {
    return window.localStorage.getItem(key) ?? fallback;
  } catch {
    // A webview with storage disabled still has to open and still has to be readable.
    return fallback;
  }
}

export function keep(key: string, value: string): void {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // See above. Never throw out of a look preference.
  }
}
