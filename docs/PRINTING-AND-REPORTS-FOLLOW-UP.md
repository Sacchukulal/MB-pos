# Printing and date-control follow-up

## Scope approved on 11 October 2026

Implement audit points 1–5 only. Preserve print queue operations, permissions,
calendar behaviour, the installed account/database and unrelated input controls.
Use existing theme tokens. Reports page work is deferred for further discussion.

- [x] 1. Contain print-card text/actions, including long names and narrow windows.
- [x] 2. Use Queued / Printing / Retrying / Failed, Retry and Cancel print; remove duplicate document labels.
- [x] 3. Show concise current errors, retain technical details, and distinguish previous errors from current attempts.
- [x] 4. Label bulk actions with their actual counts and provide a Close button independent of cancellation.
- [x] 5. Size the complete shared date control, including its calendar trigger and hover area; replace the dashboard's input-only override with the compact variant. Keep Reports on its existing default size.
- [x] Run automated checks and inspect the rebuilt app, including both themes, hover, keyboard operation, larger text and narrow windows.

## Deferred proposals — NOT authorised for implementation yet

These preserve audit numbering. Discuss the design in depth with the user before
implementation; this document is a record of proposals, not approval to ship them.

6. Adopt compact date fields consistently in Dashboard and Reports, with inline
From/To labels, aligned controls and deliberate responsive wrapping. Dashboard
already uses a compact layout; adopting it in Reports remains deferred.

7. Give Bills and report subsections a shared Reports-owned header: title and
relevant count/summary/actions first; period presets and dates second; Bills-only
search, cashier, state and payment filters in their own aligned row. Use one
selected-preset treatment without hover colour inversion. Preserve report date
state, deep links, permissions, tables, totals, bill details, returns/refunds and
all existing filters. Day open/close has different operations: share the visual
language without adding irrelevant controls.

8. Keep Print directly available where supported. Group existing Copy, WhatsApp
and Email actions under Share; CSV/PDF under Export. Bills retains its supported
exports. Regrouping must not invent unsupported actions or alter permissions.

9. Shorten the comparison strip, for example “No sales in the previous period”,
retain the comparison dates and use a neutral state when there is no baseline.
Preserve valid comparisons and any useful explanation.

## Boundaries and verification

Do not redesign Reports, change Billing's search field or globally shrink inputs.
Only new print controls are Close and expandable Details. Use the existing print
commands; do not retry/cancel real jobs as part of verification. Validate callbacks
and state/count behaviour in automated fixtures. A timeout does not prove that no
paper printed. No new account or shop database for the debug build.

## Results

Completed points 1–5. Points 6–9 remain deferred, with no Reports header/layout changes.

- UI: `npm run check` passed type checking, all linters and 553 tests in 45 files.
- Rust: three print-queue presentation tests passed, covering active/previous error
  separation, timeout versus other errors, and duplicate document labels. The
  PrintJobView binding was regenerated; Clippy passed with warnings denied.
- Built frontend assets, then `cargo build -p magic-bill --locked --features
  custom-protocol`. Running debug process loads `index-BOM3--FD.js` and
  `index-EvZamOqQ.css` from that build.
- Real app: inspected print queue and date hover in dark and light themes.
  Dashboard date field remains 30px, with a 22px trigger inset 4px on all sides.
  Reports retains its 38px field and 30px trigger; Billing search remains 38px.
- Verified current queued/printing jobs show collapsed Previous error, and a
  naturally timed-out job shows Failed with the concise timeout message. Its
  footer correctly showed Retry failed (1) alongside Cancel all prints (8).
- All print cards and their buttons fit at normal size. At 480 x 640 and text
  scale 1.3, actions wrapped below text and footer actions stayed inside the
  dialog. Scrolling reached the final card above the footer. Long unbroken names
  also wrapped within cards (temporary DOM text only, restored afterwards).
- Details expansion, Close, focus containment and calendar arrow/Escape/focus
  return were exercised in the live app. Retry/cancel callback targets, bulk
  counts and Close independence were verified with automated fixtures; no real
  retry/cancel action was pressed. The existing queue worker continued running.
- Debug app uses the installed account and database at
  `C:/Data_Drive/Billing_DB/v2/v4/magicbill.db`. No new account/database was created.
  Left the app on Dashboard in the original dark theme and text scale 1.15.
