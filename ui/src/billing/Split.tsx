/**
 * **"We'll pay separately"** — P14's scope 1.21, 1.22 and 1.23, wired at P31.
 *
 * Three commands lived in Rust from P14 with nothing calling them:
 *
 * | | |
 * |---|---|
 * | `even_split` | *"what do we each owe?"* — the question a table of six asks every time |
 * | `split_order` | some of the food onto its own bill |
 * | `set_covers` | how many people are on the table |
 *
 * They are one dialog because they are one conversation. A party says "split
 * it" and means one of two things, and the screen should not make somebody
 * work out which word we chose for theirs.
 *
 * # Even is a QUESTION, and by-item is an ACT
 *
 * That difference is the whole shape of this file, and Rust already draws it:
 * `even_split` returns figures and changes nothing — inventing six orders to
 * answer "what do we each owe?" would litter the day with bills nobody asked
 * for. `split_order` really does make a second order.
 *
 * So the even side has no confirm button, and the by-item side does.
 *
 * # Nothing here divides anything
 *
 * The shares come from `mb_core::even_shares` already formatted, remainder and
 * all — *"₹33.34 each, and one of you pays a paisa more"*. A screen that
 * divided a total by six would be the second answer D2 forbids, and it would
 * be the wrong one on the third of every rupee that does not divide.
 */

import { useCallback, useEffect, useState } from 'react';

import { Button, Input, Modal, Notice } from '../kit';
import { call } from '../ipc/call';
import type { CartView } from '../ipc/generated/CartView';
import type { EvenSplitView } from '../ipc/generated/EvenSplitView';

export function Split({
  cart,
  onClose,
  onSplit,
  onFailed,
}: {
  cart: CartView;
  onClose: () => void;
  /** A by-item split really changed the order, so the cart must be re-read. */
  onSplit: (said: string) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [ways, setWays] = useState(2);
  const [even, setEven] = useState<EvenSplitView | null>(null);
  /** How much of each line goes to the new bill, by line index, as typed. */
  const [moving, setMoving] = useState<Record<number, string>>({});
  const [covers, setCovers] = useState(cart.covers === null ? '' : String(cart.covers));
  const [busy, setBusy] = useState(false);

  // Ask again whenever the number of guests changes. No debounce: it is one
  // division in Rust, and a timer here would be a second clock (§5 rule 10).
  useEffect(() => {
    if (ways < 2) {
      setEven(null);
      return;
    }
    call('even_split', { ways }).then(setEven).catch(onFailed);
  }, [ways, onFailed]);

  /**
   * **How many people are on this table.**
   *
   * Saved as they type rather than behind a button, because it is one number
   * and a button for one number is a button somebody forgets. Every per-cover
   * figure in Reports has nothing to divide by until this is set, which is why
   * it is here — in front of the person who can see the table — rather than
   * three screens away.
   */
  const saveCovers = useCallback(
    (text: string) => {
      setCovers(text);
      const trimmed = text.trim();
      if (trimmed === '') {
        call('set_covers', { covers: null }).catch(onFailed);
        return;
      }
      const n = Number(trimmed);
      if (!Number.isInteger(n) || n < 1 || n > 999) return;
      call('set_covers', { covers: n }).catch(onFailed);
    },
    [onFailed],
  );

  const picked = Object.entries(moving).filter(([, qty]) => qty.trim() !== '');

  return (
    <Modal open title="Split this bill" onClose={onClose} wide>
      <div className="mb-split__covers">
        <Input
          label="How many people on this table"
          hint="Used by the reports — average spend per head, and how busy the room really was."
          value={covers}
          inputMode="numeric"
          onChange={(event) => saveCovers(event.target.value.replace(/[^0-9]/g, ''))}
        />
      </div>

      <h3 className="mb-split__heading">Split it evenly</h3>
      <p className="mb-split__note">
        This only tells you what each person owes. It does not make separate
        bills — the table still pays one.
      </p>

      <div className="mb-split__ways">
        <Button
          small
          variant="quiet"
          disabled={ways <= 2}
          onClick={() => setWays(ways - 1)}
          aria-label="One fewer person"
        >
          −
        </Button>
        <span className="mb-split__count">{ways} ways</span>
        <Button
          small
          variant="quiet"
          disabled={ways >= 50}
          onClick={() => setWays(ways + 1)}
          aria-label="One more person"
        >
          +
        </Button>
      </div>

      {even ? (
        <Notice tone="info">
          {/* Rust's own sentence, remainder included. */}
          <strong>{even.note}</strong>
          <p className="mb-split__shares">
            {even.shares.map((share, at) => (
              <span key={at} className="mb-mono">
                {share.text}
              </span>
            ))}
          </p>
        </Notice>
      ) : null}

      <h3 className="mb-split__heading">Or put some of the food on its own bill</h3>
      <p className="mb-split__note">
        Type how many of each item move across. The new bill gets its own
        number and sits at the same table as a second seat; the kitchen is not
        told again, because the food is already cooking.
      </p>

      {cart.orderId === null ? (
        <Notice tone="warn">
          This order has not been sent to the kitchen or put on a table yet, so
          there is nothing to split off it. Print the kitchen ticket first, or
          open the table.
        </Notice>
      ) : (
        <>
          <ul className="mb-split__lines">
            {cart.lines.map((line) => (
              <li key={line.index} className="mb-split__line">
                <span className="mb-split__name">{line.name}</span>
                <span className="mb-mono">{line.qty}</span>
                <Input
                  label="Move"
                  value={moving[line.index] ?? ''}
                  inputMode="decimal"
                  placeholder="0"
                  onChange={(event) =>
                    setMoving({ ...moving, [line.index]: event.target.value })
                  }
                />
              </li>
            ))}
          </ul>

          <div className="mb-row mb-row--end">
            <Button
              variant="primary"
              disabled={busy || picked.length === 0}
              onClick={() => {
                setBusy(true);
                call('split_order', {
                  request: {
                    orderId: cart.orderId ?? '',
                    // The quantities go across AS TYPED. `Qty::parse` is what
                    // decides whether "0.5" is a quantity, and it already has
                    // the sentence for when it is not.
                    lines: picked.map(([index, qty]) => [Number(index), qty.trim()]),
                    toTable: null,
                    seat: null,
                  },
                })
                  // **The counter goes back to nothing, and the words say so.**
                  // Rust clears the cart when the order it was holding has
                  // been split — there are two orders now and it must not
                  // guess which one the cashier meant. Without this sentence
                  // the screen simply empties, which reads as the bill having
                  // been lost. Found by doing it and looking.
                  .then(() =>
                    onSplit(
                      'Split. There are two bills on that table now — open the one you are settling from the floor.',
                    ),
                  )
                  .catch(onFailed)
                  .finally(() => setBusy(false));
              }}
            >
              Split it off
            </Button>
          </div>
        </>
      )}

      <div className="mb-row mb-row--end">
        <Button variant="quiet" onClick={onClose}>
          Close
        </Button>
      </div>
    </Modal>
  );
}
