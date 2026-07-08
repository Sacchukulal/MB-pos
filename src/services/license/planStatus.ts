import { GRACE_PERIOD_DAYS } from "../../config/constants";
import type { SubscriptionState } from "../../types";

export type PlanStatus = "active" | "grace" | "expired" | "tampered";

export interface PlanInfo {
  status: PlanStatus;
  remainingDays: number;
  /** True while the plan (incl. grace period) still permits gated features. */
  usable: boolean;
}

const DAY_MS = 24 * 60 * 60 * 1000;

/**
 * The one grace/tamper evaluation used everywhere (Dashboard, Reports, Billing, Account).
 * - active:   before nextBillingDate
 * - grace:    within GRACE_PERIOD_DAYS after it
 * - tampered: system clock is behind the last recorded check (rollback detected)
 * - expired:  otherwise
 */
export function evaluatePlan(sub: SubscriptionState | null, now = new Date()): PlanInfo {
  if (!sub || !sub.nextBillingDate) return { status: "expired", remainingDays: 0, usable: false };

  const nowMs = now.getTime();
  const nextBilling = new Date(sub.nextBillingDate).getTime();
  const lastChecked = sub.last_checked_date ? new Date(sub.last_checked_date).getTime() : 0;

  if (nowMs < lastChecked) return { status: "tampered", remainingDays: 0, usable: false };

  const diffDays = Math.ceil((nextBilling - nowMs) / DAY_MS);
  if (nowMs <= nextBilling) return { status: "active", remainingDays: diffDays, usable: true };
  if (nowMs <= nextBilling + GRACE_PERIOD_DAYS * DAY_MS) {
    return { status: "grace", remainingDays: GRACE_PERIOD_DAYS + diffDays, usable: true };
  }
  return { status: "expired", remainingDays: 0, usable: false };
}
