import { SUPABASE_FUNCTIONS_URL, SUPABASE_ANON_KEY } from "../../config/supabase";
import { STORAGE_KEYS } from "../../config/constants";
import { getDeviceInfo } from "./device";
import * as subscriptionRepo from "../../db/repositories/subscriptionRepo";
import type { UserDetails } from "../../types";

/**
 * One-desktop licensing against the Supabase backend (MB-backend repo).
 * The POS never touches Postgres directly — it calls Edge Functions
 * (`activate-license`, `license-status`) which enforce the device binding
 * server-side. The subscription snapshot is cached in SQLite for offline
 * plan checks, same as before.
 */

export interface RemoteSubscription {
  status: string;
  planId: string;
  subscriptionId: string;
  nextBillingDate: string;
  updatedAt: string;
}

export type ActivationResult =
  | { ok: true; subscription: RemoteSubscription; user: UserDetails }
  | { ok: false; reason: "invalid-key" | "bound-elsewhere" | "bind-failed" | "network"; message: string };

export function getStoredLicenseKey(): string {
  return localStorage.getItem(STORAGE_KEYS.licenseKey) || "";
}

export function getPreviousLicenseKey(): string {
  return localStorage.getItem(STORAGE_KEYS.licenseKeyHistory) || "";
}

interface FnResponse {
  ok: boolean;
  reason?: string;
  message?: string;
  subscription?: RemoteSubscription;
  user?: UserDetails;
}

async function callFn(path: string, body: unknown): Promise<FnResponse> {
  const res = await fetch(`${SUPABASE_FUNCTIONS_URL}/${path}`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${SUPABASE_ANON_KEY}`,
      apikey: SUPABASE_ANON_KEY,
    },
    body: JSON.stringify(body),
  });
  return (await res.json()) as FnResponse;
}

function mapRemoteSubscription(sub: Partial<RemoteSubscription> | undefined): RemoteSubscription {
  return {
    status: sub?.status || "",
    planId: sub?.planId || "",
    subscriptionId: sub?.subscriptionId || "",
    nextBillingDate: sub?.nextBillingDate || "",
    updatedAt: sub?.updatedAt || "",
  };
}

function mapRemoteUser(user: Partial<UserDetails> | undefined): UserDetails {
  return {
    displayName: user?.displayName || "",
    email: user?.email || "",
    mobileNumber: user?.mobileNumber || "",
    restaurantName: user?.restaurantName || "",
  };
}

/**
 * Verifies a license key, enforces the one-device binding (claiming an unbound
 * license for this machine), persists the snapshot locally, and stores the key.
 */
export async function activateLicense(key: string): Promise<ActivationResult> {
  try {
    const device = await getDeviceInfo();
    const data = await callFn("activate-license", {
      key,
      deviceId: device.id,
      deviceName: device.name,
    });

    if (!data.ok) {
      const reason =
        data.reason === "invalid-key" || data.reason === "bound-elsewhere" || data.reason === "bind-failed"
          ? data.reason
          : "network";
      return {
        ok: false,
        reason,
        message: data.message || "Failed to verify. Please check your internet connection.",
      };
    }

    const subscription = mapRemoteSubscription(data.subscription);
    const user = mapRemoteUser(data.user);

    localStorage.setItem(STORAGE_KEYS.licenseKey, key);
    localStorage.setItem(STORAGE_KEYS.licenseKeyHistory, key);
    await subscriptionRepo.saveSubscription(subscription);
    await subscriptionRepo.saveUserDetails(user);

    return { ok: true, subscription, user };
  } catch (err: any) {
    console.error("License activation failed:", err);
    return { ok: false, reason: "network", message: err?.message || "Failed to verify. Please check your internet connection." };
  }
}

/** Removes the local license and clears the cached subscription/account snapshots. */
export async function deactivateLicense(): Promise<void> {
  localStorage.removeItem(STORAGE_KEYS.licenseKey);
  await subscriptionRepo.clearSubscription();
  await subscriptionRepo.clearUserDetails();
}

/**
 * Background sync run at app startup: refreshes the local subscription snapshot
 * and detects a license binding transferred to another machine (which locks this
 * device back to activation). Silent on network errors — offline is normal.
 */
export async function syncSubscriptionFromRemote(): Promise<void> {
  if (!navigator.onLine) return;
  const key = getStoredLicenseKey();
  if (!key) return;

  try {
    const device = await getDeviceInfo();
    const data = await callFn("license-status", { key, deviceId: device.id });

    if (data.ok) {
      await subscriptionRepo.saveSubscription(mapRemoteSubscription(data.subscription));
      return;
    }

    if (data.reason === "unbound" || data.reason === "invalid-key") {
      // Binding moved (e.g. reset by support and claimed elsewhere) or the key
      // was deleted — lock this device back to the activation screen.
      await deactivateLicense();
    }
    // Any other failure (server hiccup etc.) is non-fatal; keep the cached snapshot.
  } catch (err) {
    console.error("Subscription sync failed:", err);
  }
}
