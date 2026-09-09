/** Label over value, in columns: the shape every fact on the Account page takes. */

import type { ReactNode } from 'react';

export function Fact({
  label,
  children,
  code = false,
}: {
  label: string;
  children: ReactNode;
  /** A string read out to support: monospaced and spaced. */
  code?: boolean;
}) {
  return (
    <div className="mb-account__fact">
      <dt>{label}</dt>
      <dd className={code ? 'mb-account__code' : undefined}>{children}</dd>
    </div>
  );
}

export function Facts({ children }: { children: ReactNode }) {
  return <dl className="mb-account__facts">{children}</dl>;
}
