import { getDb } from "../client";
import type { SubscriptionState, UserDetails } from "../../types";
import { toStr } from "../../utils/sqlite";

export async function getSubscription(): Promise<SubscriptionState | null> {
  const rows = await getDb().select<any[]>("SELECT * FROM subscription WHERE id = 1");
  if (rows.length === 0) return null;
  const r = rows[0];
  return {
    status: toStr(r.status),
    planId: toStr(r.planId),
    subscriptionId: toStr(r.subscriptionId),
    nextBillingDate: toStr(r.nextBillingDate),
    updatedAt: toStr(r.updatedAt),
    last_checked_date: toStr(r.last_checked_date),
  };
}

export async function saveSubscription(sub: {
  status: string;
  planId: string;
  subscriptionId: string;
  nextBillingDate: string;
  updatedAt: string;
}): Promise<void> {
  await getDb().execute(
    "UPDATE subscription SET status=$1, planId=$2, subscriptionId=$3, nextBillingDate=$4, updatedAt=$5 WHERE id=1",
    [sub.status, sub.planId, sub.subscriptionId, sub.nextBillingDate, sub.updatedAt]
  );
}

export async function clearSubscription(): Promise<void> {
  await getDb().execute(
    `UPDATE subscription SET status='', planId='', subscriptionId='', nextBillingDate='', updatedAt='' WHERE id=1`
  );
}

/** Advances the tamper-detection watermark, never moving it backwards. */
export async function touchLastChecked(): Promise<void> {
  await getDb()
    .execute(
      `UPDATE subscription SET last_checked_date = $1
       WHERE id = 1 AND (last_checked_date IS NULL OR last_checked_date < $1)`,
      [new Date().toISOString()]
    )
    .catch(() => {});
}

export async function getUserDetails(): Promise<UserDetails | null> {
  const rows = await getDb().select<any[]>("SELECT * FROM user_details WHERE id = 1");
  if (rows.length === 0) return null;
  const r = rows[0];
  return {
    displayName: toStr(r.displayName),
    email: toStr(r.email),
    mobileNumber: toStr(r.mobileNumber),
    restaurantName: toStr(r.restaurantName),
  };
}

export async function saveUserDetails(u: UserDetails): Promise<void> {
  await getDb().execute(
    "UPDATE user_details SET displayName=$1, email=$2, mobileNumber=$3, restaurantName=$4 WHERE id=1",
    [u.displayName, u.email, u.mobileNumber, u.restaurantName]
  );
}

export async function clearUserDetails(): Promise<void> {
  await getDb()
    .execute(`UPDATE user_details SET displayName='', email='', mobileNumber='', restaurantName='' WHERE id=1`)
    .catch(() => {});
}
