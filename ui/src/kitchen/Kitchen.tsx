/** The kitchen display. */

import { useCallback, useEffect, useState } from 'react';

import { Button, EmptyState, Page, PageHeader, Scroller, useReport } from '../kit';
import { call, inApp, subscribe } from '../ipc/call';
import type { KitchenTicket } from '../ipc/generated/KitchenTicket';
import type { KitchenView } from '../ipc/generated/KitchenView';
import { useTick } from '../clock';

import './kitchen.css';

export function Kitchen() {
  const [view, setView] = useState<KitchenView | null>(null);
  const [station, setStation] = useState<string | null>(null);
  /**
   * Whether the shop has turned the kitchen screen on. The page is always listed (owner,
   * 2026-09-03); when the screen is off it says so and offers the switch, instead of hiding.
   */
  const [screenOn, setScreenOn] = useState<boolean | null>(null);
  const readStatus = useCallback(() => {
    if (!inApp()) return;
    call('app_status')
      .then((status) => setScreenOn(status.kitchenScreen))
      .catch(() => setScreenOn(true));
  }, []);
  useEffect(readStatus, [readStatus]);

  // ONE clock for the whole screen — see the note above.
  const tick = useTick();

  // One reporter for the whole product, obeying the tone the engine set — so "the kitchen
  // already has this" is not shown in the colour of a real fault.
  const report = useReport();

  const load = useCallback(() => {
    if (!inApp()) return;
    call('kitchen', { station })
      .then(setView)
      .catch(() => {
        /*
         * A kitchen screen that goes blank is the failure this whole feature exists to prevent.
         */
      });
  }, [station]);

  // The tick moves the minutes; a change arrives by push.
  useEffect(load, [load, tick]);
  useEffect(() => {
    if (!inApp()) return undefined;
    let stop: (() => void) | undefined;
    subscribe((message) => {
      if (message.kind === 'kitchen') load();
    })
      .then((off) => {
        stop = off;
      })
      .catch(() => undefined);
    return () => stop?.();
  }, [load]);

  /** Tell the counter this screen DREW the ticket — not that it arrived. */
  useEffect(() => {
    if (!view) return;
    for (const ticket of view.tickets) {
      if (ticket.tone === 'new') {
        call('kitchen_shown', { id: ticket.id }).catch(() => undefined);
      }
    }
  }, [view]);

  const act = (work: Promise<KitchenView>) => {
    work.then(setView).catch(report);
  };

  // Keyboard, for a shop that mounted a numpad instead of a touchscreen.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!view || event.ctrlKey || event.altKey) return;
      const index = Number(event.key) - 1;
      const ticket = view.tickets[index];
      if (!Number.isNaN(index) && index >= 0 && ticket) {
        event.preventDefault();
        act(
          ticket.isCancelled
            ? call('kitchen_acknowledge', { id: ticket.id })
            : call('kitchen_bump', { id: ticket.id }),
        );
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  if (screenOn === false) {
    return (
      <Page>
        <PageHeader title="Kitchen" />
        <EmptyState
          title="The kitchen screen is off"
          hint="With it on, every kitchen ticket also appears here, and the kitchen presses Done when the food goes out."
          action={
            <Button
              variant="primary"
              onClick={() =>
                call('save_settings', {
                  edits: [{ key: 'billing.kitchen_screen', value: 'true' }],
                })
                  .then(() => {
                    readStatus();
                    load();
                  })
                  .catch(report)
              }
            >
              Turn on the kitchen screen
            </Button>
          }
        />
      </Page>
    );
  }

  if (!view) return null;

  return (
    <div className="mb-kds">
      <header className="mb-kds__bar">
        <span className="mb-kds__station">{view.station}</span>
        <span className="mb-kds__headline">{view.headline}</span>
        {view.stations.length > 1 && (
          <span className="mb-kds__pick">
            {view.stations.map((name) => (
              <button
                key={name}
                type="button"
                className={
                  name === view.station ? 'mb-kds__tab mb-kds__tab--on' : 'mb-kds__tab'
                }
                onClick={() => setStation(name)}
              >
                {name}
              </button>
            ))}
          </span>
        )}
        {/* The undo, and it lives here because a cleared card is gone. */}
        {view.lastCleared && (
          <button
            type="button"
            className="mb-kds__recall"
            onClick={() =>
              act(call('kitchen_recall', { id: view.lastCleared?.id ?? '' }))
            }
          >
            Bring back {view.lastCleared.what}
          </button>
        )}
      </header>

      {/* Courses waiting to be fired. */}
      {view.waitingCourses.length > 0 && (
        <div className="mb-kds__fire">
          <span className="mb-kds__fire-label">Ready to fire:</span>
          {view.waitingCourses.map((waiting) => (
            <Button
              key={`${waiting.orderId}-${waiting.course}`}
              size="sm"
              onClick={() =>
                act(
                  call('kitchen_fire', {
                    orderId: waiting.orderId,
                    course: waiting.course,
                  }),
                )
              }
            >
              {waiting.place} · {waiting.course} ({waiting.what})
            </Button>
          ))}
        </div>
      )}

      {view.tickets.length === 0 ? (
        <p className="mb-kds__empty">Nothing waiting. The kitchen is clear.</p>
      ) : (
        <Scroller className="mb-kds__grid">
          {view.tickets.map((ticket, index) => (
            <Card
              key={ticket.id}
              ticket={ticket}
              shortcut={index < 9 ? index + 1 : null}
              onBump={() => act(call('kitchen_bump', { id: ticket.id }))}
              onAcknowledge={() => act(call('kitchen_acknowledge', { id: ticket.id }))}
              onBumpLine={(key) => act(call('kitchen_bump_line', { id: ticket.id, key }))}
            />
          ))}
        </Scroller>
      )}
    </div>
  );
}

function Card({
  ticket,
  shortcut,
  onBump,
  onAcknowledge,
  onBumpLine,
}: {
  ticket: KitchenTicket;
  shortcut: number | null;
  onBump: () => void;
  onAcknowledge: () => void;
  onBumpLine: (key: string) => void;
}) {
  return (
    <article className={`mb-kds__card mb-kds__card--${ticket.tone}`}>
      <header className="mb-kds__head">
        <span className="mb-kds__place">{ticket.place || 'Order'}</span>
        {ticket.token && <span className="mb-kds__token">#{ticket.token}</span>}
      </header>

      <div className="mb-kds__meta">
        <span className="mb-kds__waiting">{ticket.waiting}</span>
        {/* The word, beside the colour — §2. */}
        <span className="mb-kds__says">{ticket.says}</span>
        {ticket.expected && <span className="mb-kds__target">target {ticket.expected}</span>}
      </div>

      {(ticket.waiter || ticket.course) && (
        <div className="mb-kds__who">
          {ticket.waiter && <span>{ticket.waiter}</span>}
          {ticket.course && <span className="mb-kds__course">{ticket.course}</span>}
        </div>
      )}

      <ul className="mb-kds__lines">
        {ticket.lines.map((line) => (
          <li
            key={line.key}
            className={line.isDone ? 'mb-kds__line mb-kds__line--done' : 'mb-kds__line'}
          >
            {/* Tap one dish as it comes off the pass. */}
            <button type="button" className="mb-kds__tick" onClick={() => onBumpLine(line.key)}>
              <span className="mb-kds__qty">{line.qty}</span>
              <span className="mb-kds__name">
                {line.name}
                {line.isNew && <span className="mb-kds__new">NEW</span>}
              </span>
            </button>
            {line.note && <span className="mb-kds__note">{line.note}</span>}
          </li>
        ))}
      </ul>

      <footer className="mb-kds__actions">
        {ticket.isCancelled ? (
          // A cancellation cannot be dismissed, only acknowledged.
          <Button variant="danger" wide onClick={onAcknowledge}>
            Got it — cancelled
          </Button>
        ) : (
          <Button variant="primary" wide onClick={onBump}>
            Done
            {shortcut ? <kbd className="mb-kbd">{shortcut}</kbd> : null}
          </Button>
        )}
      </footer>
    </article>
  );
}
