/**
 * **The kitchen display** — P24, scope 3.3.
 *
 * A screen on the kitchen wall instead of a paper ticket. Read from two metres,
 * in a bright hot room, by somebody whose hands are full.
 *
 * # Three rules this screen is built around
 *
 * **1. Huge type, and no mouse.** Everything is a big touch target. Every
 * action also has a number key, because plenty of shops mount a keyboard or a
 * numpad instead of a touchscreen — press `1` to clear the first card.
 *
 * **2. Colour is never the only signal.** UI_GUIDELINES §2, and here it is not
 * a nicety: the room is bright, the screen is across it, and a colour-blind
 * cook is not a rare event. Every card carries a WORD and a border as well as a
 * colour.
 *
 * **3. No card owns a clock.** PERFORMANCE §5 rule 10, and P14 learned it on
 * the floor grid. One shared tick re-reads the minutes Rust already computed.
 * A timer per card is budget M3's leak — and M3 exists because *"v1's KDS-style
 * timer screens are exactly where a re-render storm hides"*.
 *
 * # Nothing here decides anything
 *
 * R8. Which colour, how many minutes, what the state is called, whether a line
 * is new — all of it arrives from `kitchen.rs`. This file draws.
 */

import { useCallback, useEffect, useState } from 'react';

import { Button, Scroller, useReport } from '../kit';
import { call, inApp } from '../ipc/call';
import type { KitchenTicket } from '../ipc/generated/KitchenTicket';
import type { KitchenView } from '../ipc/generated/KitchenView';
import { useTick } from '../clock';

import './kitchen.css';

export function Kitchen() {
  const [view, setView] = useState<KitchenView | null>(null);
  const [station, setStation] = useState<string | null>(null);

  // **ONE clock for the whole screen** — see the note above.
  const tick = useTick();

  // One reporter for the whole product, obeying the tone the engine set — so
  // "the kitchen already has this" is not shown in the colour of a real fault.
  const report = useReport();

  const load = useCallback(() => {
    if (!inApp()) return;
    call('kitchen', { station })
      .then(setView)
      .catch(() => {
        /* A kitchen screen that goes blank is the failure this whole feature
           exists to prevent. It keeps what it had and tries again on the next
           tick; the counter is meanwhile printing anything nobody drew. */
      });
  }, [station]);

  // Re-read on every tick. The tickets carry their own minutes, so this is a
  // repaint of values Rust computed and not a clock running here.
  useEffect(load, [load, tick]);

  /**
   * **Tell the counter this screen DREW the ticket** — not that it arrived.
   *
   * An ack meaning "the bytes got here" lies exactly when a tablet's power
   * saver has frozen the tab, which is the case the paper fallback exists for.
   * So this runs after the cards are on screen.
   */
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

  // **Keyboard, for a shop that mounted a numpad instead of a touchscreen.**
  // `1`..`9` clears that card. Both ways in, always (T9).
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
        {/* **The undo, and it lives here because a cleared card is gone.**
            A cook who clears the wrong ticket has nothing left to press on —
            so the way back has to be somewhere still on screen. It names the
            card, so nobody brings back the wrong one. */}
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

      {/* Courses waiting to be fired (scope 3.5). Empty for the shops that do
          not use courses, which is most of them — and they never see this. */}
      {view.waitingCourses.length > 0 && (
        <div className="mb-kds__fire">
          <span className="mb-kds__fire-label">Ready to fire:</span>
          {view.waitingCourses.map((waiting) => (
            <Button
              key={`${waiting.orderId}-${waiting.course}`}
              small
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
            {/* Tap one dish as it comes off the pass — the owner asked for
                both this and clearing the whole card. Tapping again unticks
                it, because a cook who ticks the wrong dish presses it again
                and an undo behind a different button is one nobody finds. */}
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
          // **A cancellation cannot be dismissed, only acknowledged** (D107).
          // Food already cooking is thrown away; food not started is cooked for
          // nobody. It is the one thing here allowed to interrupt.
          <Button variant="danger" wide onClick={onAcknowledge}>
            Got it — cancelled
          </Button>
        ) : (
          <Button variant="primary" wide onClick={onBump}>
            Done{shortcut ? ` (${shortcut})` : ''}
          </Button>
        )}
      </footer>
    </article>
  );
}
