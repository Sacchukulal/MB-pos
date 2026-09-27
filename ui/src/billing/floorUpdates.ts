import { useEffect } from 'react';

import { call, inApp, subscribe } from '../ipc/call';
import type { CartView } from '../ipc/generated/CartView';

/** Keep the open cart and its floor tile in step after any committed floor change. */
export function useFloorUpdates(
  showCart: (cart: CartView) => void,
  refreshFloor: () => Promise<void>,
  cartRevision: { current: number },
) {
  useEffect(() => {
    if (!inApp()) return undefined;
    let disposed = false;
    let latestRead = 0;
    let stop: (() => void) | undefined;
    subscribe((message) => {
      if (disposed || (message.kind !== 'floor' && message.kind !== 'floorChanged')) return;
      // Batches and changes other than additions emit floor, too. Rust reconciles the
      // current cart; the notification is not a line to add a second time in React.
      const read = ++latestRead;
      const revision = cartRevision.current;
      call('current_cart')
        .then((cart) => {
          if (!disposed && read === latestRead && revision === cartRevision.current) showCart(cart);
        })
        .catch(() => undefined);
      void refreshFloor();
    })
      .then((off) => {
        if (disposed) off();
        else stop = off;
      })
      .catch(() => undefined);
    return () => {
      disposed = true;
      stop?.();
    };
  }, [showCart, refreshFloor, cartRevision]);
}
