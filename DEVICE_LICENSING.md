# One-Desktop License Locking

Each license key (a row in the Supabase `licenses` table) can be active on **one
desktop at a time**. This is enforced by binding the key to a stable hardware
device id. Enforcement happens **server-side** in Supabase Edge Functions
(MB-backend repo) — the POS never talks to the database directly.

## How it works

1. **Device id (Rust):** `get_device_id` in `src-tauri/src/lib.rs` returns the
   motherboard **SMBIOS UUID** (`HWID-...`) — it survives an OS reinstall and is
   hard to spoof. If the firmware reports a blank/placeholder UUID it falls back
   to the first physical adapter **MAC address** (`MAC-...`).

2. **Binding (app → Edge Function):** on activation/sync
   (`src/features/account/Account.tsx`, `src/app/App.tsx` via
   `src/services/license/licenseService.ts`) the app POSTs
   `{ key, deviceId, deviceName }` to `activate-license` / `license-status`:
   - **Unbound** → the function claims the key for this device (race-safe:
     the update only applies while `device_id IS NULL`).
   - **Same device** → it refreshes `device_last_seen`.
   - **Different device** → activation is **blocked** (`bound-elsewhere`) with a
     message to contact support. An already-activated device that loses the
     binding (after a transfer) gets `unbound` from `license-status` on next
     sync and reverts to the activation screen.

3. **Security:** clients only hold the public anon key; the `licenses` table has
   read-only RLS for owner logins and no client write path at all. All binding
   writes go through the Edge Functions, which run with the service-role key —
   the lock cannot be overwritten from a client.

## Cloud shape (Supabase `licenses` row)

```jsonc
{
  "key": "<license key>",
  "display_name": "...", "email": "...", "mobile_number": "...", "restaurant_name": "...",
  "status": "active", "plan_id": "...", "subscription_id": "sub_...",
  "next_billing_date": "2026-08-01T00:00:00Z",
  "device_id": "HWID-XXXXXXXX-....",
  "device_name": "SHOP-PC",
  "device_platform": "windows",
  "device_bound_at": "2026-06-20T10:00:00.000Z",
  "device_last_seen": "2026-06-20T10:00:00.000Z"
}
```

## Transferring a license to a new PC

The app cannot move a binding (by design). To transfer:

1. Supabase dashboard → Table Editor → `licenses` → open the customer's row
   (or run SQL: `UPDATE licenses SET device_id=NULL, device_name=NULL,
   device_bound_at=NULL, device_last_seen=NULL WHERE key='<key>';`).
2. **Clear the `device_*` columns.**
3. The customer opens Magic Bill on the new PC and activates — it claims the key.

> Tip: you can see which machine currently holds a key via `device_name` /
> `device_last_seen` before transferring.

## Notes / limitations

- `device_id` changes only if the **motherboard** is replaced (or, on the MAC
  fallback, the network adapter) — those legitimately need a support transfer.
- Virtual machines may report duplicate/blank SMBIOS UUIDs; the MAC fallback
  covers most of those, but VMs are inherently weaker to lock.
