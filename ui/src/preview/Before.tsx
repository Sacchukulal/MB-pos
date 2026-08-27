/** See the bill before it prints. */

import { useCallback, useEffect, useState } from 'react';

import { Button, Modal, useReport } from '../kit';
import { call } from '../ipc/call';
import type { PreviewDoc } from '../ipc/generated/PreviewDoc';
import { Receipt } from './Receipt';

export function Before({
  open,
  onClose,
  onPrint,
}: {
  open: boolean;
  onClose: () => void;
  /** What the button under the paper does. */
  onPrint?: () => void;
}) {
  const [doc, setDoc] = useState<PreviewDoc | null>(null);
  const [trouble, setTrouble] = useState<string | null>(null);
  const report = useReport();

  const draw = useCallback(async () => {
    setDoc(null);
    setTrouble(null);
    try {
      setDoc(await call('preview_order', { orderId: null }));
    } catch (cause) {
      // It says why, in the words Rust chose.
      setTrouble(
        typeof cause === 'object' && cause && 'message' in cause
          ? String((cause as { message: unknown }).message)
          : 'This could not be drawn.',
      );
      report(cause);
    }
  }, [report]);

  useEffect(() => {
    if (open) void draw();
  }, [open, draw]);

  return (
    <Modal
      open={open}
      title="The bill, before it prints"
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
