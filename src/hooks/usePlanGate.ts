import { useEffect, useState } from "react";
import { getSubscription, touchLastChecked } from "../db/repositories/subscriptionRepo";
import { evaluatePlan, type PlanInfo } from "../services/license/planStatus";

interface PlanGateState {
  checking: boolean;
  plan: PlanInfo;
}

/**
 * The single subscription check used by gated screens (Dashboard, Reports) and
 * the Billing autofocus. Advances the tamper watermark while the plan is usable.
 */
export function usePlanGate(dbReady: boolean): PlanGateState {
  const [state, setState] = useState<PlanGateState>({
    checking: true,
    plan: { status: "expired", remainingDays: 0, usable: false },
  });

  useEffect(() => {
    if (!dbReady) return;
    let cancelled = false;
    (async () => {
      try {
        const sub = await getSubscription();
        const plan = evaluatePlan(sub);
        if (plan.usable) await touchLastChecked();
        if (!cancelled) setState({ checking: false, plan });
      } catch (err) {
        console.error("Plan check failed:", err);
        if (!cancelled) setState({ checking: false, plan: { status: "expired", remainingDays: 0, usable: false } });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [dbReady]);

  return state;
}
