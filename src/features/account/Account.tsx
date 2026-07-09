import { useEffect, useState, type ReactNode } from "react";
import {
  Activity,
  AlertTriangle,
  Building2,
  CalendarClock,
  CheckCircle2,
  Crown,
  KeyRound,
  Loader2,
  LogOut,
  Mail,
  Phone,
  RefreshCw,
  ShieldCheck,
  UserCircle,
  XCircle,
  Zap,
} from "lucide-react";
import { RENEW_URL } from "../../config/constants";
import { getDeviceInfo, type DeviceInfo } from "../../services/license/device";
import {
  activateLicense,
  deactivateLicense,
  getPreviousLicenseKey,
  getStoredLicenseKey,
} from "../../services/license/licenseService";
import { evaluatePlan, type PlanStatus } from "../../services/license/planStatus";
import { pushUnsyncedBills } from "../../services/sync/billSync";
import * as subscriptionRepo from "../../db/repositories/subscriptionRepo";
import ConfirmDialog from "../../components/ui/ConfirmDialog";
import type { SubscriptionState, UserDetails } from "../../types";

interface AccountProps {
  dbReady: boolean;
}

const STATUS_CONFIG: Record<PlanStatus, { icon: typeof CheckCircle2; label: string; color: string; bg: string }> = {
  active: { icon: CheckCircle2, label: "ACTIVE", color: "var(--success)", bg: "var(--success-subtle)" },
  grace: { icon: AlertTriangle, label: "GRACE PERIOD", color: "var(--warning)", bg: "var(--warning-subtle)" },
  expired: { icon: XCircle, label: "EXPIRED", color: "var(--danger)", bg: "var(--danger-subtle)" },
  tampered: { icon: XCircle, label: "LOCKED", color: "var(--danger)", bg: "var(--danger-subtle)" },
};

function InfoRow({ icon, label, value }: { icon: ReactNode; label: string; value?: string }) {
  return (
    <div className="info-row">
      <div className="info-row-icon">{icon}</div>
      <div className="info-row-label">{label}</div>
      <div className="info-row-value">{value || <span className="not-set">Not set</span>}</div>
    </div>
  );
}

export default function Account({ dbReady }: AccountProps) {
  const [licenseKey, setLicenseKey] = useState(getStoredLicenseKey);
  const [previousKey] = useState(getPreviousLicenseKey);
  const [inputKey, setInputKey] = useState("");
  const [loading, setLoading] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [error, setError] = useState("");
  const [subscription, setSubscription] = useState<SubscriptionState | null>(null);
  const [userDetails, setUserDetails] = useState<UserDetails | null>(null);
  const [deviceInfo, setDeviceInfo] = useState<DeviceInfo | null>(null);
  const [confirmDeactivate, setConfirmDeactivate] = useState(false);

  useEffect(() => {
    getDeviceInfo().then(setDeviceInfo).catch(() => {});
  }, []);

  const loadLocal = async () => {
    try {
      setSubscription(await subscriptionRepo.getSubscription());
      setUserDetails(await subscriptionRepo.getUserDetails());
    } catch (e) {
      console.error("Error reading local account data:", e);
    }
  };

  const verify = async (key: string, silent = false) => {
    if (!dbReady) return;
    if (!silent) setSyncing(true);
    setLoading(true);
    setError("");
    try {
      const result = await activateLicense(key);
      if (result.ok) {
        setLicenseKey(key);
        setUserDetails(result.user);
        await loadLocal();
        // Kick the outbox immediately so back-fill starts now, not next interval.
        pushUnsyncedBills().catch(() => {});
      } else {
        setError(result.message);
        // A key that is invalid or moved to another device de-authorizes this one.
        if ((result.reason === "invalid-key" || result.reason === "bound-elsewhere" || result.reason === "bind-failed") && licenseKey === key) {
          await handleDeactivate();
        }
      }
    } finally {
      setLoading(false);
      setSyncing(false);
    }
  };

  useEffect(() => {
    if (!dbReady) return;
    loadLocal();
    if (licenseKey) verify(licenseKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dbReady]);

  const handleActivate = () => {
    const key = inputKey.trim();
    if (!key) {
      setError("Please enter a valid User ID.");
      return;
    }
    verify(key);
  };

  const handleDeactivate = async () => {
    await deactivateLicense();
    setLicenseKey("");
    setInputKey("");
    setSubscription(null);
    setUserDetails(null);
  };

  const planInfo = evaluatePlan(subscription);
  const status = STATUS_CONFIG[planInfo.status];
  const StatusIcon = status.icon;

  const initials = (() => {
    const name = userDetails?.displayName || userDetails?.restaurantName || "";
    return (
      name
        .split(" ")
        .map((w) => w[0])
        .join("")
        .slice(0, 2)
        .toUpperCase() || "??"
    );
  })();

  /* ---------------------- Activation view ---------------------------- */
  if (!licenseKey) {
    return (
      <div className="page settings-page" style={{ maxWidth: 520, justifyContent: "center" }}>
        <div style={{ textAlign: "center" }}>
          <div
            style={{
              width: 72,
              height: 72,
              borderRadius: "50%",
              background: "var(--accent-subtle)",
              border: "2px solid var(--accent-muted)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              margin: "0 auto var(--space-4)",
            }}
          >
            <ShieldCheck size={36} color="var(--accent)" strokeWidth={1.5} />
          </div>
          <h2 style={{ fontSize: "var(--text-2xl)", fontWeight: 800, marginBottom: "var(--space-2)" }}>Activate Magic Bill</h2>
          <p className="text-secondary">Enter your User ID to unlock all features</p>
        </div>

        <div className="section" style={{ gap: "var(--space-4)" }}>
          {error && (
            <div
              style={{
                color: "var(--danger)",
                padding: "var(--space-3) var(--space-4)",
                background: "var(--danger-subtle)",
                borderRadius: "var(--radius-md)",
                fontSize: "var(--text-sm)",
                display: "flex",
                alignItems: "center",
                gap: "var(--space-2)",
              }}
            >
              <XCircle size={16} />
              {error}
            </div>
          )}

          <div className="field">
            <label>License Key / User ID</label>
            <input
              type="text"
              className="input"
              placeholder="Paste your key here…"
              value={inputKey}
              onChange={(e) => {
                setInputKey(e.target.value);
                setError("");
              }}
              onKeyDown={(e) => e.key === "Enter" && handleActivate()}
            />
          </div>

          <button className="btn btn--primary btn--lg btn--block" onClick={handleActivate} disabled={loading}>
            {loading ? <Loader2 size={18} className="ui-spinner-rotate" /> : <KeyRound size={18} />}
            {loading ? "Activating…" : "Activate Device"}
          </button>

          {previousKey && (
            <button className="btn btn--ghost btn--block" onClick={() => verify(previousKey)} disabled={loading}>
              <RefreshCw size={16} /> Restore Previous Key
            </button>
          )}
        </div>
      </div>
    );
  }

  /* ------------------------ Logged-in view --------------------------- */
  return (
    <div className="page settings-page">
      {/* Hero */}
      <div className="account-hero">
        <div className="account-avatar">{initials}</div>
        <div className="account-hero-info">
          <h2>{userDetails?.displayName || userDetails?.restaurantName || "Magic Bill Account"}</h2>
          <p>{userDetails?.email || "Account & Licensing Dashboard"}</p>
          <div className="account-chips">
            <span className="account-chip" style={{ background: status.bg, color: status.color }}>
              <StatusIcon size={12} strokeWidth={2.5} />
              {status.label}
            </span>
            <span className="account-chip">
              <Crown size={12} />
              {subscription?.planId ? `${subscription.planId} Plan` : "Free Plan"}
            </span>
          </div>
        </div>
        <div className="account-hero-actions">
          <button className="btn btn--ghost" onClick={() => verify(licenseKey)} disabled={syncing || loading}>
            <RefreshCw size={16} className={syncing ? "ui-spinner-rotate" : ""} />
            {syncing ? "Syncing…" : "Sync Data"}
          </button>
          <button className="btn btn--danger" onClick={() => setConfirmDeactivate(true)} disabled={loading}>
            <LogOut size={16} /> Deactivate
          </button>
        </div>
      </div>

      {error && (
        <div
          style={{
            color: "var(--danger)",
            padding: "var(--space-3) var(--space-4)",
            background: "var(--danger-subtle)",
            borderRadius: "var(--radius-md)",
            fontSize: "var(--text-sm)",
          }}
        >
          {error}
        </div>
      )}

      <div className="account-grid">
        {/* Identity */}
        <div className="section">
          <div className="section-head">
            <UserCircle size={15} /> Identity
          </div>
          <div>
            <InfoRow icon={<UserCircle size={15} />} label="Name" value={userDetails?.displayName} />
            <InfoRow icon={<Building2 size={15} />} label="Business" value={userDetails?.restaurantName} />
            <InfoRow icon={<Phone size={15} />} label="Mobile" value={userDetails?.mobileNumber} />
            <InfoRow icon={<Mail size={15} />} label="Email" value={userDetails?.email} />
          </div>
        </div>

        {/* Plan status */}
        <div className="section">
          <div className="section-head">
            <Activity size={15} /> Plan Status
          </div>
          {subscription ? (
            <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-4)" }}>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                <div>
                  <div className="stat-card-label">Current Plan</div>
                  <div style={{ fontSize: "var(--text-xl)", fontWeight: 700, textTransform: "capitalize" }}>
                    {subscription.planId || "Free"} Plan
                  </div>
                </div>
                <span className="account-chip" style={{ background: status.bg, color: status.color }}>
                  <StatusIcon size={12} strokeWidth={2.5} /> {status.label}
                </span>
              </div>

              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "var(--space-3)" }}>
                <div className="plan-stat">
                  <div className="plan-stat-label">
                    <CalendarClock size={14} /> Next Billing
                  </div>
                  <div style={{ fontWeight: 600 }}>
                    {subscription.nextBillingDate
                      ? new Date(subscription.nextBillingDate).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" })
                      : "N/A"}
                  </div>
                </div>
                <div className="plan-stat">
                  <div className="plan-stat-label">
                    <Zap size={14} /> {planInfo.status === "grace" ? "Grace Days" : "Days Left"}
                  </div>
                  <div style={{ fontSize: "var(--text-2xl)", fontWeight: 800, color: status.color, lineHeight: 1 }}>
                    {planInfo.remainingDays}
                  </div>
                </div>
              </div>

              {planInfo.status === "grace" && (
                <div
                  style={{
                    display: "flex",
                    alignItems: "flex-start",
                    gap: "var(--space-3)",
                    padding: "var(--space-3) var(--space-4)",
                    background: "var(--warning-subtle)",
                    borderRadius: "var(--radius-md)",
                    fontSize: "var(--text-sm)",
                    color: "var(--warning)",
                  }}
                >
                  <AlertTriangle size={16} style={{ flexShrink: 0, marginTop: 1 }} />
                  Your subscription is in the grace period. Renew soon to avoid interruption.
                </div>
              )}

              {planInfo.status === "tampered" && (
                <div
                  style={{
                    padding: "var(--space-3) var(--space-4)",
                    background: "var(--danger-subtle)",
                    borderRadius: "var(--radius-md)",
                    fontSize: "var(--text-sm)",
                    color: "var(--danger)",
                  }}
                >
                  System time tampering detected. Please correct your clock and sync again.
                </div>
              )}

              {planInfo.status === "expired" && (
                <a href={RENEW_URL} target="_blank" rel="noopener noreferrer" className="btn btn--primary" style={{ justifyContent: "center" }}>
                  <Zap size={16} /> Renew Plan
                </a>
              )}
            </div>
          ) : (
            <div className="text-secondary" style={{ fontSize: "var(--text-sm)", padding: "var(--space-4) 0" }}>
              No subscription data found. Try syncing.
            </div>
          )}
        </div>
      </div>

      {/* Device panel */}
      <div className="section device-panel" style={{ flexDirection: "row", alignItems: "center" }}>
        <div
          style={{
            width: 48,
            height: 48,
            borderRadius: "var(--radius-lg)",
            flexShrink: 0,
            background: "var(--success-subtle)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          <ShieldCheck size={22} color="var(--success)" />
        </div>
        <div style={{ flex: 1 }}>
          <div style={{ fontWeight: 700, marginBottom: 4 }}>Device Locked {deviceInfo?.name ? `· ${deviceInfo.name}` : ""}</div>
          <div className="text-secondary" style={{ fontSize: "var(--text-sm)" }}>
            This license is locked to this desktop only. One key works on a single device at a time.
          </div>
        </div>
        <div className="device-id-chip" title={deviceInfo?.id || licenseKey}>
          {(deviceInfo?.id || licenseKey).slice(0, 24)}…
        </div>
      </div>

      {confirmDeactivate && (
        <ConfirmDialog
          title="Deactivate this device?"
          danger
          message="This removes the license from this PC. You can activate it again later with the same key."
          confirmLabel="Deactivate"
          onConfirm={async () => {
            setConfirmDeactivate(false);
            await handleDeactivate();
          }}
          onCancel={() => setConfirmDeactivate(false)}
        />
      )}
    </div>
  );
}
