/**
 * **The info tip**, in its own file because everything in the kit needs it.
 *
 * It lived in `display.tsx` until 2026-08-24, and then a FIELD wanted one — the
 * owner, that day: *"i dont like you adding those sub lines below all those
 * feilds."* `controls.tsx` cannot import from `display.tsx`, because
 * `display.tsx` already imports `Button` from `controls.tsx`, and two files
 * importing each other at module scope is a cycle. One small file both can
 * import is the answer, exactly as `Icon.tsx` already is.
 */

import { useId, type ReactNode } from 'react';

import { Icon } from './Icon';

/**
 * **The explanation you can ask for, instead of the one you are given.**
 *
 * The owner, 2026-08-22: *"you are adding these explaination texts everywhere
 * in the app, it is not needed, it makes the app look cluttered and
 * unprofessional… make it a kind of popup text, when hovered on the section
 * heading… (in a small popup, which is common in many apps, its like info
 * text)"*
 *
 * And the reason, which is the part worth keeping: *"you keep forgetting that
 * i am the developer… I will sell this app to customers (restaurant owners)."*
 * The paragraphs were written to explain the product to the person building it.
 * A shopkeeper who has used the till for a week is reading them for the two
 * hundredth time.
 *
 * # No JavaScript in it
 *
 * `:hover` and `:focus-within` do the showing, so there is no open/closed state
 * to get stuck, nothing to clean up on unmount, and no way for two of these to
 * be open at once. The button is a real `<button>` so a keyboard reaches it,
 * and `aria-describedby` ties the bubble to it for a screen reader — which is
 * how this stays as accessible as the paragraph it replaces.
 */
export function InfoTip({ children, label }: { children: ReactNode; label?: string }) {
  const id = useId();
  return (
    <span className="mb-tip">
      <button
        type="button"
        className="mb-tip__ask"
        aria-label={label ?? 'What is this?'}
        aria-describedby={id}
      >
        <Icon name="info" size="sm" />
      </button>
      <span className="mb-tip__bubble" id={id} role="tooltip">
        {children}
      </span>
    </span>
  );
}
