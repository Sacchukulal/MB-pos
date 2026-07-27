/**
 * Decoupled triggers for the order bridge. Repositories call the request
 * functions after local mutations; the bridge registers handlers when it
 * starts. Keeping this in its own module avoids db-layer -> service-layer
 * import cycles. All calls are no-ops until the bridge is running.
 */

type Handler = () => void;

let ordersPushHandler: Handler | null = null;
let catalogPushHandler: Handler | null = null;

export function registerOrdersPushHandler(fn: Handler | null): void {
  ordersPushHandler = fn;
}

export function registerCatalogPushHandler(fn: Handler | null): void {
  catalogPushHandler = fn;
}

/** A processing/finalized order changed locally — republish live orders (debounced by the bridge). */
export function requestOrdersPush(): void {
  ordersPushHandler?.();
}

/** Menu / tables / customers changed locally — recompute the catalog hash (debounced by the bridge). */
export function requestCatalogPush(): void {
  catalogPushHandler?.();
}
