import { useEffect, useState } from 'react';
import { subscribe } from '../ipc/call';

/** Recheck protected data after a licence change or return from sleep.
 * Rust checks local expiry each minute and on every protected request.
 */
export function useLicenceRevision(): number {
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    let alive = true;
    let stop: (() => void) | undefined;
    const refresh = () => { if (alive) setRevision((value) => value + 1); };
    void Promise.resolve().then(() => subscribe((message) => { if (message.kind === 'licence') refresh(); }))
      .then((unsubscribe) => { if (alive) stop = unsubscribe; else unsubscribe(); })
      .catch(() => undefined);
    window.addEventListener('focus', refresh);
    return () => {
      alive = false;
      stop?.();
      window.removeEventListener('focus', refresh);
    };
  }, []);
  return revision;
}
