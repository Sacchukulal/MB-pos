import type { OrderBridgeStatus } from "./orderBridge";

/**
 * The ONE place the bridge's internal state is turned into words a shop
 * owner understands. v1.3.0 rendered a bare "Offline" for four completely
 * different situations, which cost the owner an evening — every state here
 * says what is happening and, when something is wrong, what to do about it.
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

/**
 * The counter proves it is alive with a 60-second beat, so nothing newer
 * than this is worth worrying about.
 */
const LAST_SYNC_STALE_MS = 180_000;

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
        "bottom-left of the sidebar. Until then phones cannot send orders to this " +
        "counter.",
    };
  }

  if (s.fault === "budget") {
    return {
      tone: "degraded",
      label: "Paused a background task (safety limit)",
      short: "Mobile ordering · Safety limit",
      detail:
        "Magic Bill stopped a background task that was repeating far more often " +
        "than it should. Orders from phones still arrive and still print. This " +
        "clears by itself; if the message keeps coming back, send a screenshot to " +
        "support.",
    };
  }

  if (s.fault === "flapping") {
    return {
      tone: "degraded",
      label: "Live connection keeps dropping",
      short: "Mobile ordering · Unstable connection",
      detail:
        "The connection to Magic Bill keeps dropping and reconnecting, which usually " +
        "means an unreliable internet line at the counter. Magic Bill has stopped " +
        "retrying every second and will settle on its own. Orders from phones still " +
        "arrive — they may take up to a minute while this lasts.",
    };
  }

  if (s.channel === "connected") {
    const n = s.phones;
    // "connected", not "online". From 2.4.5 a phone holds its presence for
    // as long as Magic Bill is open in the foreground, not only while a
    // waiter is looking at the Orders screen — that is what stopped the
    // count flickering between 1 and 0 on every table tap. The number is
    // still true; the word "online" was the part that would have become a
    // lie, because a phone in this count is not necessarily taking orders
    // this second.
    return {
      tone: "connected",
      label: `Connected · ${n} phone${n === 1 ? "" : "s"} connected`,
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
        "The instant connection is down, so orders from phones arrive within about " +
        "a minute instead of straight away. They still arrive and still print, and " +
        "nothing is lost. This clears by itself.",
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

/** True when "Last sync" has stopped advancing — the counter is not talking. */
export function isLastSyncStale(lastSyncAt: string | null, now: number = Date.now()): boolean {
  if (!lastSyncAt) return false;
  const t = Date.parse(lastSyncAt);
  return Number.isFinite(t) && now - t > LAST_SYNC_STALE_MS;
}

/**
 * 5.4 — the owner's own usage readout, so he never has to log in to
 * Supabase to know whether Magic Bill is behaving.
 */
export function describeUsage(s: OrderBridgeStatus): string {
  const { lastHour, last24h } = s.usage;
  if (lastHour === 0 && last24h === 0) {
    return "Cloud usage: nothing in the last 24 hours (normal — the counter only " +
      "registers with Magic Bill once).";
  }
  const h = `${lastHour} in the last hour`;
  const d = `${last24h} in the last 24 hours`;
  return `Cloud usage: ${h}, ${d}.`;
}
