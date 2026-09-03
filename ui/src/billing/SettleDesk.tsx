/**
 * The settle desk: what the phones asked the counter to settle.
 *
 * A waiter presses "Settle bill" on a phone; the request lands HERE, over whichever screen is
 * up, as a list — every table that asked, oldest first. Enter settles the highlighted one in
 * the highlighted payment mode; the arrows move; Escape puts the desk away until the next
 * request arrives. Nothing is settled anywhere else: the phone only asks.
 */

import { useCallback, useEffect, useRef, useState } from 'react';

import { Button, Modal, cx, useReport, useToast } from '../kit';
import { call, inApp, subscribe } from '../ipc/call';
import type { SettleRequestView } from '../ipc/generated/SettleRequestView';
import { formatMinutes } from './TableGrid';

/** The counter's own mode words, in the order the keys cycle them. */
const MODES = ['Cash', 'Card', 'UPI'] as const;
type Mode = (typeof MODES)[number];

export function SettleDesk() {
  const [requests, setRequests] = useState<readonly SettleRequestView[]>([]);
  const [index, setIndex] = useState(0);
  /** The mode per order, once the cashier has moved it off what the waiter said. */
  const [modes, setModes] = useState<Readonly<Record<string, Mode>>>({});
  /** Put away with Escape; comes back when a request the desk has not seen arrives. */
  const [away, setAway] = useState(false);
  const seen = useRef<Set<string>>(new Set());
  const [acting, setActing] = useState(false);
  const toast = useToast();
  const report = useReport();

  const load = useCallback(async () => {
    try {
      const fresh = await call('settle_requests');
      setRequests(fresh);
      // A new request wakes the desk, even one that was put away.
      if (fresh.some((r) => !seen.current.has(r.orderId))) setAway(false);
      for (const r of fresh) seen.current.add(r.orderId);
    } catch {
      // A lock screen, a shop still opening: the desk is simply empty.
      setRequests([]);
    }
  }, []);

  // Once on arrival, and again whenever the floor moves.
  useEffect(() => {
    if (!inApp()) return undefined;
    void load();
    let stop: (() => void) | undefined;
    subscribe((message) => {
      if (message.kind === 'floor' || message.kind === 'floorChanged') void load();
    })
      .then((off) => {
        stop = off;
      })
      .catch(() => undefined);
    return () => stop?.();
  }, [load]);

  // Keep the highlight on a row that still exists.
  useEffect(() => {
    setIndex((was) => Math.min(was, Math.max(0, requests.length - 1)));
  }, [requests.length]);

  const open = requests.length > 0 && !away;
  const current = requests[index];
  const modeOf = (r: SettleRequestView): Mode =>
    modes[r.orderId] ?? (MODES.find((m) => m === r.payment) ?? 'Cash');

  const settle = useCallback(
    async (r: SettleRequestView) => {
      if (acting) return;
      setActing(true);
      try {
        const number = await call('settle_from_floor', { orderId: r.orderId, mode: modeOf(r) });
        toast.show('ok', `Bill ${number} settled — ${r.place.toLowerCase()}, by ${modeOf(r).toLowerCase()}.`);
        await load();
      } catch (cause) {
        report(cause);
      } finally {
        setActing(false);
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [acting, load, modes, report, toast],
  );

  const decline = useCallback(
    async (r: SettleRequestView) => {
      if (acting) return;
      setActing(true);
      try {
        await call('decline_settle', { orderId: r.orderId });
        await load();
      } catch (cause) {
        report(cause);
      } finally {
        setActing(false);
      }
    },
    [acting, load, report],
  );

  // The keys, taken before the billing screen's own listener sees them: Enter here settles a
  // table, and must never also complete the cashier's bill.
  useEffect(() => {
    if (!open) return undefined;
    const onKey = (event: KeyboardEvent) => {
      const keys = ['Enter', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'];
      if (!keys.includes(event.key)) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      if (event.key === 'ArrowDown') setIndex((i) => Math.min(i + 1, requests.length - 1));
      if (event.key === 'ArrowUp') setIndex((i) => Math.max(i - 1, 0));
      if ((event.key === 'ArrowLeft' || event.key === 'ArrowRight') && current) {
        const at = MODES.indexOf(modeOf(current));
        const next = MODES[(at + (event.key === 'ArrowRight' ? 1 : MODES.length - 1)) % MODES.length] ?? 'Cash';
        setModes((was) => ({ ...was, [current.orderId]: next }));
      }
      if (event.key === 'Enter' && current) void settle(current);
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, current, requests.length, modes, settle]);

  return (
    <Modal
      open={open}
      title={requests.length === 1 ? 'Settle this bill?' : `Settle these bills?`}
      note="Asked from the floor. Enter settles the highlighted one; the arrows move and change how it was paid; Escape puts this away."
      onClose={() => setAway(true)}
      wide={requests.length > 1}
      actions={
        <>
          <Button variant="quiet" onClick={() => setAway(true)}>
            Later
          </Button>
          {current ? (
            <Button variant="primary" disabled={acting} onClick={() => void settle(current)}>
              Settle {current.place.toLowerCase()} by {modeOf(current)}
            </Button>
          ) : null}
        </>
      }
    >
      <ul className="mb-settle">
        {requests.map((r, i) => (
          <li
            key={r.orderId}
            className={cx('mb-settle__row', i === index && 'mb-settle__row--on')}
            onClick={() => setIndex(i)}
            aria-current={i === index ? 'true' : undefined}
          >
            <div className="mb-settle__head">
              <span className="mb-settle__place">
                {r.place}
                {r.token ? <span className="mb-settle__token"> · #{r.token}</span> : null}
              </span>
              <span className="mb-settle__total">{r.total}</span>
            </div>
            <div className="mb-settle__says">
              {r.says}
              <span className="mb-settle__ago"> · {r.minutes === 0 ? 'just now' : `${formatMinutes(r.minutes)} ago`}</span>
            </div>
            <div className="mb-settle__row-actions">
              <div className="mb-settle__modes" role="radiogroup" aria-label="Paid by">
                {MODES.map((m) => (
                  <button
                    key={m}
                    type="button"
                    role="radio"
                    aria-checked={modeOf(r) === m}
                    className={cx('mb-settle__mode', modeOf(r) === m && 'mb-settle__mode--on')}
                    onClick={() => {
                      setIndex(i);
                      setModes((was) => ({ ...was, [r.orderId]: m }));
                    }}
                  >
                    {m}
                  </button>
                ))}
              </div>
              <Button size="sm" variant="quiet" disabled={acting} onClick={() => void decline(r)}>
                Decline
              </Button>
              {i === index ? null : (
                <Button size="sm" disabled={acting} onClick={() => void settle(r)}>
                  Settle
                </Button>
              )}
            </div>
          </li>
        ))}
      </ul>
    </Modal>
  );
}
