/**
 * **See the bill before it prints** — audit D6, and it was never built.
 *
 * > *"No bill preview before printing. You cannot see the actual bill for the
 * > actual order before it comes out of the printer."*
 *
 * `ipc.rs` carried a comment from P08 saying *"P09 will add
 * `preview_order(order_id)` beside this"*. P09 to P31 came and went, and a grep
 * for the name found the comment and nothing else — so the only preview in the
 * product was of an invented sample, which is also why a bill printing a
 * database key where the table's name goes survived to a real install.
 *
 * This is the screen half. It costs one dialog, because the sink already
 * exists: `flows::preview_order_on` builds the **same document**
 * `queue_bill_print` builds — same template, same table label, same face, same
 * engine — and hands it here instead of to the printer.
 */

import { useCallback, useEffect, useState } from 'react';

import { Button, Modal, useReport } from '../kit';
import { call } from '../ipc/call';
import type { PreviewDoc } from '../ipc/generated/PreviewDoc';
import { Receipt } from './Receipt';

/** Which of the two pieces of paper this counter makes. */
export type Paper = 'bill' | 'kitchen';

export function Before({
  what,
  open,
  onClose,
  onPrint,
}: {
  what: Paper;
  open: boolean;
  onClose: () => void;
  /** What the button under the paper does. Printing is the caller's business. */
  onPrint?: () => void;
}) {
  const [doc, setDoc] = useState<PreviewDoc | null>(null);
  const [trouble, setTrouble] = useState<string | null>(null);
  const report = useReport();

  const draw = useCallback(async () => {
    setDoc(null);
    setTrouble(null);
    try {
      const command = what === 'bill' ? 'preview_order' : 'preview_kitchen';
      setDoc(await call(command, { orderId: null }));
    } catch (cause) {
      // **It says why, in the words Rust chose.** The commonest reason is the
      // honest one — the kitchen already has everything — and `UiError`'s own
      // tone decides whether it is a notice or a fault, which is why the
      // report goes through the same reporter every other screen uses.
      setTrouble(
        typeof cause === 'object' && cause && 'message' in cause
          ? String((cause as { message: unknown }).message)
          : 'This could not be drawn.',
      );
      report(cause);
    }
  }, [what, report]);

  useEffect(() => {
    if (open) void draw();
  }, [open, draw]);

  return (
    <Modal
      open={open}
      title={what === 'bill' ? 'The bill, before it prints' : 'The kitchen ticket'}
      onClose={onClose}
      actions={
        onPrint ? (
          <>
            <Button variant="quiet" onClick={onClose}>
              Close
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                onClose();
                onPrint();
              }}
            >
              Print it
            </Button>
          </>
        ) : undefined
      }
    >
      {doc ? <Receipt doc={doc} /> : null}
      {trouble ? <p>{trouble}</p> : null}
      {!doc && !trouble ? <p>Drawing the paper…</p> : null}
    </Modal>
  );
}
