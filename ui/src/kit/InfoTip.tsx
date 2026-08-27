/** The info tip, in its own file because everything in the kit needs it. */

import { useId, type ReactNode } from 'react';

import { Icon } from './Icon';

/** The explanation you can ask for, instead of the one you are given. */
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
