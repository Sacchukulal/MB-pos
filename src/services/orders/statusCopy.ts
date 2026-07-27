import type { OrderBridgeStatus } from "./orderBridge";

/**
 * The ONE place the bridge's internal state is turned into words a shop owner
 * understands. v1.3.0 rendered a bare "Offline" for four completely different
 * situations, which cost the owner an evening — every state here says what is
 * happening and, when something is wrong, what to do about it.
 *
 * `tone` reuses the existing .mo-pill classes, so no new colours are needed.
 */

export type BridgeTone = "connected" | "degraded" | "off";

export interface BridgeStatusCopy {
  tone: BridgeTone;
  /** Settings pill text. */
  label: string;
  /** Compact billing-panel text (same meaning, fewer words). */
  short: string;
  /** Plain-English explanation; "" when nothing needs explaining. */
  detail: string;
}

/** The heartbeat is 30s, so nothing newer than this is worth worrying about. */
const LAST_SYNC_STALE_MS = 90_000;

export function describeBridge(s: OrderBridgeStatus): BridgeStatusCopy {
  if (!s.featureEnabled) {
    return {
      tone: "off",
      label: "Off — enable and save to go live",
      short: "Mobile ordering · Off",
      detail: "",
    };
  }

  if (!s.cloudReachable) {
    return {
      tone: "off",
      label: "Not reaching Magic Bill",
      short: "Mobile ordering · No internet",
      detail:
        "This computer cannot reach the internet, so phones cannot send orders. " +
        "Billing at the counter is unaffected. Check the Wi-Fi or the network cable — " +
        "everything reconnects on its own once the connection is back.",
    };
  }

  if (s.fault === "misconfigured") {
    return {
      tone: "degraded",
      label: "This copy of Magic Bill is not configured correctly",
      short: "Mobile ordering · Update needed",
      detail:
        "Install the latest version of Magic Bill using the update button at the " +
        "bottom-left of the sidebar. Until then orders from phones still arrive and " +
        "still print, about 3 seconds slower than normal.",
    };
  }

  if (s.channel === "connected") {
    const n = s.phones;
    return {
      tone: "connected",
      label: `Connected · ${n} phone${n === 1 ? "" : "s"} online`,
      short: `Mobile ordering · On · ${n} phone${n === 1 ? "" : "s"}`,
      detail: "",
    };
  }

  if (s.channel === "degraded") {
    return {
      tone: "degraded",
      label: "Connected (backup mode)",
      short: "Mobile ordering · Backup mode",
      detail:
        "Orders from phones still arrive and still print — about 3 seconds slower " +
        "than usual. Nothing is lost. This clears by itself when the live " +
        "connection comes back.",
    };
  }

  // featureEnabled, cloud reachable, but the bridge has not gone live yet.
  return {
    tone: "off",
    label: "Starting…",
    short: "Mobile ordering · Starting",
    detail:
      "The counter is still connecting. If this does not clear within a minute, " +
      "close and reopen Magic Bill.",
  };
}

/** True when "Last sync" has stopped advancing — the heartbeat is not running. */
export function isLastSyncStale(lastSyncAt: string | null, now: number = Date.now()): boolean {
  if (!lastSyncAt) return false;
  const t = Date.parse(lastSyncAt);
  return Number.isFinite(t) && now - t > LAST_SYNC_STALE_MS;
}
