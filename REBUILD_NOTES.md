# Rebuild Notes — v0.3.0 architecture

The application was rebuilt from scratch per `PROJECT_SPECIFICATION.md` (approved 2026-07-02).
All features from spec §2 are preserved. This file records the new layout, the sanctioned
behavior changes (bug fixes), and anything a future maintainer should know.

## New source layout

```
src/
├── app/           App shell: App, Sidebar, Onboarding, UpdateUI, useUpdater, screens
├── components/ui/ Modal, ConfirmDialog, PlanGate, Spinner, EmptyState, StatCard
├── config/        constants.ts, defaults.ts, supabase.ts
├── db/            client.ts, migrations.ts (schema_version), repositories/*
├── features/      billing/ dashboard/ reports/ expenses/ menu/ staff/ settings/ account/
├── hooks/         useToast, useSettings, usePlanGate, useUnsavedGuard
├── services/
│   ├── printing/  escpos.ts, image.ts, printService.ts, templates/{bill,kot,reports}.ts
│   └── license/   device.ts, licenseService.ts, planStatus.ts
├── styles/        base.css, ui.css, features.css   (replaces App.css)
├── theme/         theme.css (tokens, unchanged), ThemeContext.tsx (unchanged)
├── types/         all domain + settings types
└── utils/         format.ts, gst.ts, sqlite.ts
```

Rules: components never run SQL or emit printer bytes; SQLite boolean/number coercion
happens only in `db/repositories`; every ₹/date format comes from `utils/format.ts`;
all printer command bytes live in `services/printing`.

## Bug fixes applied (approved 2026-07-02)

1. **Reprints now use the same template as the original bill** — reprinted bills honor
   all design settings and include the UPI QR (previously skipped due to a raw-row
   `=== false` comparison).
2. **Tax Report is correct for Inclusive GST** — taxable value = subtotal − GST for
   inclusive bills (`utils/gst.ts:taxableValue`).
3. **Static UPI QR works** — prints a QR without the amount; Dynamic embeds the amount.
4. **Single paper-size setting** — `printer_settings.paper_size` (2/3/4 inch) now drives
   both printing *and* the Bill Settings preview. The legacy `bill_settings.printer_size`
   column is no longer read (kept in the schema for downgrade safety).
5. **GSTIN header visibility no longer disables GST math** — "Show GSTIN Header" is
   purely visual; only "Enable GST" controls the calculation.
6. **Table lookups no longer build regexes from user input** (`features/billing/tableUtils.ts`).
7. **First KOT of a sub-table prints the correct table number** (was read from stale state).
8. **Daily counter reset date is stored as ISO** (`YYYY-MM-DD`); migration v2 converts the
   legacy `DD/MM/YYYY` value once so the upgrade day doesn't double-reset.
9. **Token/bill numbers are claimed atomically from the DB** (`claimOrderNumbers`), not
   from possibly-stale UI state; `0` counters are no longer masked by `|| 100` fallbacks
   at claim time.
10. **ESC/POS fallback stripping matches exact command shapes** — no more eaten characters
    on the plain-text fallback path.
11. **Dashboard bucketing memo includes `timeRange`** in its dependencies.
12. `window.confirm` replaced everywhere by the styled `ConfirmDialog`; the blocking
    `alert()` on DB-init failure replaced by an error screen with a recovery action.

## Intentional behavior notes

- Billing remains usable when the plan is expired (only Dashboard/Reports are gated) —
  this matches the previous app and is now explicit.
- Esc in the billing search now clears the typed text (previously it closed the
  suggestion list but kept the text). Second Esc still starts a new order.
- "Full Settlement" is still recorded as a payment_mode for settlements (kept for
  data compatibility with existing rows).
- Rust side unchanged except it is now free of the unused `greet` command… (deferred:
  the Rust `greet` command and "EasyBill" doc-name strings still exist; safe to clean
  next time the Rust binary is rebuilt/re-signed).

## Removed

- Dead code: old `Settings.tsx` tab screen, Firebase Auth init, per-screen toast/expiry
  duplicates, `original_account.tsx`, one-off `.cjs` fix scripts, `easy_bill_backup.patch`,
  leftover HTML-template tree in `public/assets`, `src/App.css` (2,877 lines).
- Dependencies: `bootstrap`, `simplebar`, `@tabler/icons-webfont`, `@tauri-apps/plugin-shell`.

## Security follow-ups (action required — not code)

- **Rotate the Tauri updater signing key.** The private key is present in `package.json`,
  `.env`, and `tauri.key` and is committed to git history. Rotation must be coordinated
  with a release (the app validates updates against the embedded public key).
- Consider setting a real CSP in `tauri.conf.json` (currently `null`).
