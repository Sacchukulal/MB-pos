import { Button, Modal, useAction } from '../kit';
import { call } from '../ipc/call';
import type { CartView } from '../ipc/generated/CartView';

/** A conflicting local draft stays intact until the cashier explicitly chooses to reload. */
export function OrderConflict({ onClose, onReloaded, onFailed }: {
  onClose: () => void;
  onReloaded: (cart: CartView) => void;
  onFailed: (cause: unknown) => void;
}) {
  const [act, busy] = useAction();
  const reload = () => act(async () => {
    try {
      onReloaded(await call('reload_current_order'));
    } catch (cause) {
      onFailed(cause);
    }
  });
  return (
    <Modal open title="Order changed on both devices" onClose={() => { if (!busy) onClose(); }}
      actions={<>
        <Button disabled={busy} onClick={onClose}>Keep editing</Button>
        <Button variant="danger" disabled={busy} onClick={reload}>Reload saved order</Button>
      </>}>
      <p>The saved order has changes from another device that conflict with edits at this counter.</p>
      <p>Reloading discards unsaved local item changes and keeps the saved changes from the phone.
        Check the updated bill before continuing. This does not take payment or send a kitchen ticket.</p>
    </Modal>
  );
}
