import { useState } from 'react';

/** A counter's navigation preference stays on this device. */
export function useRailState(name: string) {
  const key = `magicbill.rail.${name}`;
  const [collapsed, setCollapsed] = useState(() => {
    try {
      const saved = localStorage.getItem(key);
      return saved === null ? window.innerWidth < 1450 : saved === 'collapsed';
    } catch { return false; }
  });
  const change = (next: boolean) => {
    setCollapsed(next);
    try { localStorage.setItem(key, next ? 'collapsed' : 'expanded'); } catch { /* Optional preference. */ }
  };
  return [collapsed, change] as const;
}
