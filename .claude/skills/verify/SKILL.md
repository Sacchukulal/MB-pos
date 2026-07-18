---
name: verify
description: Verify MB-pos UI changes by driving the real Vite frontend in headless Chromium with Tauri IPC stubbed
---

# Verifying MB-pos (Tauri + Vite React)

Full `tauri dev` needs a Rust build. For UI/layout changes, drive the real frontend in a browser instead:

1. `npm run dev` in MB-pos → Vite serves at **http://localhost:1420/**.
2. Use `playwright-core` (install in scratchpad, NOT the project) with the cached browser at
   `%LOCALAPPDATA%\ms-playwright\chromium-*\chrome-win64\chrome.exe` (`executablePath`, `headless: true`).
3. In `page.addInitScript`, stub the Tauri side before app code runs:
   - `localStorage.dbFolderPath = "C:\\MockData"` — skips the folder-picker onboarding gate.
   - `localStorage.magicbill_license_key = "POS-DEV-1"` — renders the logged-in Account view.
   - `window.__TAURI_INTERNALS__ = { invoke, transformCallback, metadata, plugins: {} }` where `invoke` handles:
     `plugin:path|join` → joined string; `plugin:sql|load` → any string; `plugin:sql|execute` → `[0, 0]`;
     `plugin:sql|select` → rows keyed off the query text (`FROM subscription`, `FROM user_details` need one row each,
     else `[]`); `get_device_id` → `{ id, name }`.
   - Wrap `window.fetch`: intercept URLs containing `supabase` and return mock JSON
     (`activate-license` / `license-status` expect `{ ok, subscription, user }`). **Never let the browser hit the real
     Supabase functions — activation binds devices server-side.**
4. Navigate: `.account-nav-btn` is the sidebar Account button; other screens via their sidebar labels.
5. Screenshot at multiple viewport widths (1600 / 1100 / 760) — layout breakpoints are 1000px and 720px.

Gotchas:
- Migrations run many `execute` calls on startup; the generic stubs above satisfy them.
- Collect `pageerror` / console errors in the script — a broken stub surfaces as the "Database Error" screen.
