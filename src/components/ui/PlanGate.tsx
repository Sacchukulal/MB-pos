import type { ReactNode } from "react";
import { Lock } from "lucide-react";
import { RENEW_URL } from "../../config/constants";
import type { PlanInfo } from "../../services/license/planStatus";
import Spinner from "./Spinner";

interface PlanGateProps {
  checking: boolean;
  plan: PlanInfo;
  children: ReactNode;
}

/** Shared gate for subscription-locked screens (Dashboard, Reports). */
export default function PlanGate({ checking, plan, children }: PlanGateProps) {
  if (checking) {
    return (
      <div className="loading-center">
        <Spinner size={28} />
      </div>
    );
  }

  if (!plan.usable) {
    return (
      <div className="expired-plan-overlay">
        <div className="expired-plan-card">
          <div className="expired-plan-icon">
            <Lock size={36} color="var(--danger)" />
          </div>
          <h2 className="expired-plan-title">You don't have an active plan</h2>
          <p className="expired-plan-text">
            Your subscription has expired or hasn't been activated. Upgrade your plan to restore access to the
            Dashboard, Reports, and all premium features.
          </p>
          <a href={RENEW_URL} target="_blank" rel="noopener noreferrer" className="upgrade-btn">
            Activate Plan Now
          </a>
        </div>
      </div>
    );
  }

  return <>{children}</>;
}
