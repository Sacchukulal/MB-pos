import { useLayoutEffect, useRef, type RefObject } from 'react';

const scopes: HTMLElement[] = [];
const selector = 'button, input:not([type="hidden"]), select, textarea, a[href], [tabindex]';

function available(element: HTMLElement): boolean {
  return !element.closest('[hidden], [inert], [aria-hidden="true"]') &&
    !element.matches(':disabled') && getComputedStyle(element).display !== 'none' &&
    getComputedStyle(element).visibility !== 'hidden';
}

/** Nested dialogs and popovers share one keyboard boundary and return to their opener. */
export function useFocusScope(
  panel: RefObject<HTMLElement | null>,
  active: boolean,
  initial?: () => HTMLElement | null | undefined,
) {
  const first = useRef(initial);
  first.current = initial;
  useLayoutEffect(() => {
    const root = panel.current;
    if (!active || !root) return;
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    scopes.push(root);
    const items = () => [...root.querySelectorAll<HTMLElement>(selector)]
      .filter((element) => element.tabIndex >= 0 && available(element));
    const requested = first.current?.();
    const focus = requested && available(requested) ? requested :
      items().find((element) => element.matches('input, select, textarea')) ?? root;
    focus.focus();
    const keys = (event: KeyboardEvent) => {
      if (event.key !== 'Tab' || event.defaultPrevented || scopes.at(-1) !== root || !available(root)) return;
      const choices = items();
      const index = choices.indexOf(document.activeElement as HTMLElement);
      if (!choices.length || index < 0 || (event.shiftKey ? index === 0 : index === choices.length - 1)) {
        event.preventDefault();
        (event.shiftKey ? choices.at(-1) : choices[0])?.focus();
        if (!choices.length) root.focus();
      }
    };
    document.addEventListener('keydown', keys);
    return () => {
      document.removeEventListener('keydown', keys);
      const wasTop = scopes.at(-1) === root;
      const index = scopes.indexOf(root);
      if (index >= 0) scopes.splice(index, 1);
      const parent = scopes.at(-1);
      // React finishes removing the dialog after layout-effect cleanup. Restore after that
      // commit, without stealing focus from a newer dialog opened in the same update.
      queueMicrotask(() => {
        if (wasTop && scopes.at(-1) === parent && opener?.isConnected && available(opener)) opener.focus();
      });
    };
  }, [active, panel]);
}
