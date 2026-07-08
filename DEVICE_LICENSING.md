# One-Desktop License Locking

Each license key (`users/{key}` in Firestore) can be active on **one desktop at a
time**. This is enforced by binding the key to a stable hardware device id.

## How it works

1. **Device id (Rust):** `get_device_id` in `src-tauri/src/lib.rs` returns the
   motherboard **SMBIOS UUID** (`HWID-...`) — it survives an OS reinstall and is
   hard to spoof. If the firmware reports a blank/placeholder UUID it falls back
   to the first physical adapter **MAC address** (`MAC-...`).

2. **Binding (app):** on activation/sync (`src/components/Account.tsx`,
   `src/App.tsx`) the app reads `users/{key}`:
   - **Unbound** → it writes the `device` map and claims the key.
   - **Same device** → it refreshes `device.lastSeen`.
   - **Different device** → activation is **blocked** with a message to contact
     support. An already-activated device that loses the binding (after a
     transfer) is reverted to the activation screen on next sync.

3. **Security rules (`firestore.rules`):** the client can only *claim an unbound
   key* or *refresh its own binding*. It can never steal a key already bound to
   another machine, nor edit subscription/profile fields. **Deploy these rules —
   without them the lock can be overwritten from the client.**

   ```bash
   firebase deploy --only firestore:rules
   ```
   (or paste `firestore.rules` into Firebase console → Firestore → Rules.)

## Firestore shape

```jsonc
// users/{licenseKey}
{
  "subscription": { /* ... existing ... */ },
  "displayName": "...", "email": "...", "mobileNumber": "...", "restaurantName": "...",
  "device": {
    "id": "HWID-XXXXXXXX-....",
    "name": "SHOP-PC",
    "platform": "windows",
    "boundAt": "2026-06-20T10:00:00.000Z",
    "lastSeen": "2026-06-20T10:00:00.000Z"
  }
}
```

## Transferring a license to a new PC

The app cannot move a binding (by design). To transfer:

1. Firebase console → Firestore → `users` → open the customer's key document.
2. **Delete the `device` field** (or the whole map).
3. The customer opens Magic Bill on the new PC and activates — it claims the key.

> Tip: you can see which machine currently holds a key via `device.name` /
> `device.lastSeen` before transferring.

## Notes / limitations

- `device.id` changes only if the **motherboard** is replaced (or, on the MAC
  fallback, the network adapter) — those legitimately need a support transfer.
- Virtual machines may report duplicate/blank SMBIOS UUIDs; the MAC fallback
  covers most of those, but VMs are inherently weaker to lock.
