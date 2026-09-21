/** The small pieces the Account page's panels share: aligned lines and a folder path. */

import type { ReactNode } from 'react';

/** A run of label-beside-value lines, every label the same width, every action in one column. */
export function Lines({ children }: { children: ReactNode }) {
  return <dl className="mb-account__lines">{children}</dl>;
}

/** One line: the label, whatever sits beside it, and the button that acts on it. */
export function Line({
  label,
  children,
  action,
}: {
  label: string;
  children: ReactNode;
  /** The Change (or Copy) at the end of the line: right-aligned, so every line's lines up. */
  action?: ReactNode;
}) {
  return (
    <div className="mb-account__line">
      <dt>{label}</dt>
      <dd>
        <span className="mb-account__value">{children}</span>
        {action ? <span className="mb-account__act">{action}</span> : null}
      </dd>
    </div>
  );
}

/** How many characters of a path fit before its middle is folded away. */
const PATH_ROOM = 48;

/** The drive and the last two folders of a long path; the whole path on hover. */
export function shorten(path: string): string {
  if (path.length <= PATH_ROOM) return path;
  const parts = path.split(/[\\/]/).filter((p) => p !== '');
  if (parts.length < 4) return path;
  const sep = path.includes('\\') ? '\\' : '/';
  return [parts[0], '…', ...parts.slice(-2)].join(sep);
}

/** A folder path on one line. */
export function FolderPath({ path }: { path: string }) {
  if (path === '') return <span className="mb-muted">Not set</span>;
  return (
    <code className="mb-account__path" title={path}>
      {shorten(path)}
    </code>
  );
}
