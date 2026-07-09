# POS Audit — post-Phase-3 structure check (2026-07-09)

Read-only audit of MB-pos against the new Supabase licensing + bill-sync structure.

## MUST-FIX (silent data corruption, no crash)

### 1. Payment-mode edits never re-sync to the cloud
`src/db/repositories/ordersRepo.ts:120` — `updateOrderPaymentMode()` changes
`payment_mode` on a finalized bill (used by Reports → edit payment mode, e.g.
settling a Credit bill) but does not reset `synced = 0`. Once a bill has synced,
any later edit stays local forever: the cloud `bills` row and the nightly
`daily_summaries` payment-mode splits (cash/UPI/credit totals) silently diverge.
The backend upsert on `(license_key, local_id)` was designed exactly for re-pushing
edited bills.
**Fix (one line):** `UPDATE finalized_orders SET payment_mode = $1, synced = 0 WHERE id = $2`.

## SHOULD-FIX (inconsistent, not breaking)

### 2. Docs still describe the Firebase world
`README.md`, `REBUILD_NOTES.md`, `PROJECT_SPECIFICATION.md`, `DEVICE_LICENSING.md`
still explain Firestore licensing. Code is clean — these are the only remaining
firebase/firestore mentions in the repo. Misleading for future sessions/devs.
**Fix:** short "licensing now = Supabase Edge Functions" corrections in the
licensing sections (text only, no code).

### 3. First sync after activation can wait up to 5 minutes
`billSync` ticks 20 s after app start, then every 5 min. If the app starts
unlicensed and you activate afterwards, the 129-bill back-fill waits for the next
interval tick. **Fix (optional, 2 lines):** fire `pushUnsyncedBills()` once after a
successful `activateLicense()` so sync starts immediately on activation.

## LEAVE-ALONE (verified working — do not touch)

- **licenseService ↔ Edge Functions:** request/response shapes match the deployed
  functions exactly (reasons `invalid-key` / `bound-elsewhere` / `bind-failed` /
  `network` / `unbound`; `subscription{status,planId,subscriptionId,nextBillingDate,updatedAt}`;
  `user{displayName,email,mobileNumber,restaurantName}`). `Account.tsx` handles all of them; no stale fields.
- **planStatus.ts / device.ts:** unchanged as required; field names align with
  `subscriptionRepo` (`status`, `planId`, `nextBillingDate`, `last_checked_date`).
- **billSync wiring:** started in `App.tsx` after the DB opens; every cycle no-ops
  unless a license key is stored, so it effectively "starts after license is
  active" without extra plumbing. Double-start guard + in-flight mutex present.
  `stopBillSync()` wired into effect cleanup.
- **Migrations:** `synced` / `sync_attempts` / `last_sync_error` are added via the
  idempotent `addColumn` helper. Verified (read-only) that the live DB at
  `C:\Data_Drive\Billing_DB\restaurant.db` does NOT have them yet — expected; they
  appear on next app start. New bills inherit `DEFAULT 0` (SQLite applies ALTER
  defaults to subsequent inserts). 129 rows total, all will read `synced=0`.
- **Updater:** endpoint now points at Sacchukulal/MB-pos, which has no releases
  yet — `check()` failures are caught (silent for auto-check, toast for manual).
  No crash path.
- **No leftovers:** zero firebase/firestore references in `src/` + `src-tauri/`
  (docs only, see #2); no imports of removed files; no old updater endpoint; no
  `DELETE FROM finalized_orders` anywhere (so no cloud-orphan risk).
- **Fresh-start runtime risks:** none found — `getDeviceInfo()` has a fallback,
  all fetches are wrapped, migrations are idempotent, `.env` contains
  `VITE_SUPABASE_ANON_KEY` (and functions are deployed `--no-verify-jwt`, so even a
  missing anon key would not break licensing calls).
- `customersRepo.deleteCustomer` NULLs `customer_id` on bills — that column isn't
  part of the sync payload (name/phone are copied as strings), so no divergence.
