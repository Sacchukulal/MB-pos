# Audit implementation and verification — 8 October 2026

This is the continuation record for [the original audit](AUDIT-2026-10-08.md). Read both before continuing in another session. The audit describes the old implementation; this file records subsequent work. Use the latest owner correction below rather than the earlier free-history policy.

## Authorization and decisions

On 10 October 2026, the owner authorized committing these changes locally only. Push, deployment and release remain unauthorized. Earlier no-commit notes below describe the implementation/testing stage and are superseded only for this local commit.

The owner subsequently asked to implement the audit items one by one, use subagents if useful, and actually launch and test the latest debug app. **Do not commit, push, release, or deploy** without a later instruction. This restriction remains in force.

Use the installed shop and existing account. The installed DB pointer resolves to `C:\Data_Drive\Billing_DB\v2\v4\magicbill.db`. Do not create another POS shop/account/database for manual app testing. Automated unit/integration tests use the repository's normal temporary test fixtures. Do not change the installed license to simulate expiry.

Owner answers during implementation:

- **Latest correction after visible app review:** lock the entire Reports page, including Bills/history/editing, when the Reports entitlement is unavailable after configured grace. Show the licence message and renewal action. Keep ordinary new billing available. Remove the added top-level Bills tab and use Reports → Bills. This supersedes the earlier answer allowing free individual historical bill work. Staff permission remains an additional requirement. Safe discard/exit of a correction and operational day counting must not trap ordinary billing.
- Keep the configured license grace period. Expiry enforcement must respect it, including custom values.
- For closed-day returns, implement the best accounting approach. Chosen approach: retain the original issued bill and closed day; post a linked sales/tax return and the actual money movement on the current open day.
- Do not invent new free onboarding. Check the existing backend and website. The existing free-trial-to-shop/key flow remains the setup path; a lapsed key must not prevent ordinary billing or access to the existing shop.
- Table arrivals blink for a few seconds, not indefinitely.

## Running the actual development app

The first launch used an existing debug executable with old bundled screens. That was wrong for live UI review and was corrected. The active development setup uses the existing Vite server on port 5173 and a fresh ordinary `cargo build -p magic-bill`, **without** `custom-protocol`. No new development configuration was introduced.

The actual WebView was checked: `location.href` was `http://localhost:5173/`, and the Vite client script was present. Existing staff appeared; the owner signed in normally. UI edits hot-reload. Rust/IPC changes require a rebuilt executable and an app restart; do not claim Vite updates Rust.

Native tools must still use `tools/Invoke-HiddenCommand.ps1` from the workspace root, with the documented hidden-process behavior. Use the existing `scripts/drive.mjs` for real WebView inspection, screenshots, UI interaction, and IPC checks. Do not open external terminal windows. DevTools remote debugging is a process-scoped launch argument, not a machine-wide setting.

## D01: dashboard meaning and the additional cash-receipt finding

The original screenshot's figures were correct: 1,606 total across cash/card/UPI, and 955 cash. The dashboard now labels these as net sales and expected cash in the drawer, with text explaining their different meanings.

Implementation tests uncovered a separate real issue: cash tender above a bill's amount could be stored as retained money, overstating cash in the drawer and the amount refundable. Some drawer/delivery queries also added tips to an amount that already included the tip.

The fix belongs in the shared receipt projection, not separate adjustments in each screen. The new core projection preserves the original tender and displayed change while persisting net retained receipts. A versioned migration applies the same projection to older stored receipts. Drawer/delivery consumers count included tips once. Test overpayment, tips, mixed receipts, reloading, reprinting, correction settlement, and repeat migration before using the updated schema in the debug app.

Main files: `crates/mb-core/src/payment.rs`, `crates/mb-db/src/repo/order.rs`, migration `0023_net_receipts.sql`, DB money/delivery readers, and `src-tauri/src/refund_tests.rs`.

## B01/L01: Reports and historical bill work require entitlement

The first implementation added a direct Bills navigation entry. The owner rejected this during visible app review. It has been removed; the sole historical bill screen is the existing Reports → Bills. Reports shows its licence lock before mounting bill history or day history. Historical bill commands and corrections also enforce the existing Reports feature in Rust so a direct call or an already-open correction cannot bypass expiry. Staff permissions still control which actions each cashier may perform.

The Bills response supplies totals only with report permission, and bulk exports are separately protected in Rust. Loss of entitlement now locks the whole Reports page, rather than merely hiding summaries and exports. Stale asynchronous replies must not restore protected content. Operational cash counting/day-close work outside Reports is preserved so free new billing is not trapped.

Historical invoice preview/PDF/reprint commands also require the Reports entitlement. Ordinary new-bill completion and its normal invoice printing remain available without it. Safe correction discard or parking/exit remains available so an expired draft cannot trap the counter and prevent new billing.

Licence validation runs before the report-list permission check, preventing a day-close-only staff role from using a permission fallback to bypass expiry. While a previously authorized screen is rechecked, its subtree is hidden and inert; a valid result preserves open dialogs and typed reasons, while an actual refusal removes the protected content. Initial entry never mounts it before validation. Modal keyboard handling ignores hidden/inert dialogs.

Main files: `src-tauri/src/guard.rs`, `corrections.rs`, `dayclose.rs`, `flows.rs`, `ui/src/shell/permissions.tsx`, `Shell.tsx`, `ui/src/reports/Bills.tsx`, `Days.tsx`, and `Reports.tsx`.

## B02: whole returns and actual tender amounts

The old screen always requested the whole bill total back as cash. The replacement asks Rust for the remaining amount by original payment method. A single dialog records a whole-bill return and its actual payouts atomically. A previously voided/returned bill can pay back a remaining amount without returning the full original total again.

The repository validates positive amounts, reason, cumulative remaining balance, and the original tender. Shared command validation rejects duplicate methods. Custom payment methods remain distinct from cash/card/UPI and from each other. Credit is an account adjustment, not a cash payout.

Partial money payouts carry a stable request ID. The existing refund primary keys persist it; retrying the same request is a no-op, and using it with different details is refused. The dialog also blocks repeated submission while saving. The obsolete cash-only `refund_bill` IPC is removed; test compatibility helpers delegate to the shared implementation.

Card/UPI entries record a refund actually made through the provider. They do not claim to call a bank/provider refund API. The dialog says this explicitly.

Prepared food is not restored to ingredient stock by the new whole-return action. An explicit erroneous-sale void retains the existing stock-reversal behavior. The action history records the chosen behavior. Food-cost reporting reads the stock ledger, so retained consumption remains a cost.

Main files: `src-tauri/src/refunds.rs`, `corrections.rs`, `crates/mb-db/src/repo/corrections.rs`, and `ui/src/reports/Bills.tsx`. `Reason.tsx` provides shared asynchronous submission protection; `Approval.tsx` provides shared manager input.

## B03: finishing or discarding an edit

An issued bill remains unchanged while a separate correction draft is worked on. Billing displays Save changes and Discard changes; the bill list distinguishes resuming an unfinished edit. Removing all items offers Return whole bill instead of leaving an empty correction that cannot complete.

Discard is deliberately refused if new money has already been recorded or the kitchen has already acted on an update. Dropping those records would lose physical actions. The draft remains available to finish. Safe unsent edits can be discarded without changing the issued bill or receipts.

Manager approval moves to the final save. The preview shows original/new totals, and the existing threshold applies to the larger amount. Above threshold, a distinct active manager with the existing permission must approve. Below threshold, the cashier's authorized final save is recorded. New edits do not require a separate later approval of the same work. Older pending reviews remain visible for historical records.

The final settlement rechecks the reviewed proposal, original issued bill, policy, and approving staff before committing. Approval and replacement bill commit together. A rejected/stale review must not collect new money. Partial item returns use the existing correction pricing/settlement engine rather than a second price/tax calculation.

Main files: `src-tauri/src/correction_draft.rs`, `flows.rs`, `corrections.rs`, `crates/mb-db/src/settle.rs`, and `ui/src/billing/Billing.tsx`.

## B04: closed historical bills

A full historical return freezes the exact original bill and line tax amounts in a linked current-day record. The original issued order remains settled and unchanged. The current day receives the sales/tax adjustment, actual refund rows, and any credit-account adjustment. Prepared food is not restocked.

Shared signed report sources include returns in daily, item, category, payment, cashier, table/section/terminal, tax and HSN totals. The archive includes the linked records even on a day containing only returns. Existing cloud row transport accepts the new record types; credit reductions reuse the existing persisted customer-ledger adjustment path rather than a parallel virtual balance.

The bill history shows the linked return; returned bills cannot be edited or voided again. Remaining actual payouts may be recorded separately and are capped by original receipts.

Scope limit: this implements **full** closed-day bill returns. A partial item return against a closed historical bill and a statutory credit-note document are not introduced by this change. Do not claim otherwise or bypass the closed-day edit guard.

Main files: `crates/mb-db/src/repo/returns.rs`, migration `0022_bill_returns.sql`, shared reports/archive/wire/credit code, `src-tauri/src/refunds.rs`, and historical return tests.

## L02/L03/L04: license decisions and trust

Paid-feature gates reevaluate the local signed license when the cached time boundary has passed; they do not depend on waiting for a successful network check. Grace remains supported. The background loop publishes local expiry changes; focus/license notifications refresh screens and clear stale financial responses. Ordinary billing remains outside the paid feature enum/gates.

Unsigned emergency timestamps and the old client-embedded HMAC secret are replaced by signed `MB-E1` support grants. Grants bind to the existing license and machine, use a bounded duration, and restore only existing plan features. Verification happens when entitlement is read. Failed persistence must not create an active unlock. Legacy short codes need support reissue; billing continues without a grant. Support tooling and protocol documentation are updated locally, without deployment.

Cross-repository onboarding trace: website signup creates an account; the existing trial claim creates the shop/license. POS shop selection checks for the key, not active paid status. The backend permits lapsed/trial-ended activation and supplies a signed lapsed snapshot. No second free-shop/account flow was added. Misleading website/POS wording implying all billing stops after expiry is corrected.

Main sources: POS `crates/mb-license`, `src-tauri/src/licensing.rs`/`state.rs`, website license-state copy, backend support grant tool, and `docs/LICENCE_PROTOCOL.md`.

## N01/N02: arrival alerts

The shared arrival hook now uses an authoritative order-content fingerprint, including item identity/quantity and note. Equal-price substitutions are detectable. Each order has its own restartable timer. A new generation restarts only the passive pulse layer, retaining the real button/card and keyboard focus.

Reduced motion shows a brief static highlight instead of a zero-duration cue. Floor listens to both applicable event forms and rejects stale reads. Billing, Processing, and Floor share the same alert implementation.

Actual app checks completed so far:

- Real WebView reduced-motion preference was false.
- Through the visible Billing UI, selected free AC Table 1, added Fish Egg Fry, and printed KOT. The app created test order **token 97**, initially 84.00; this was not yet an issued invoice. Existing Table 3 / token 96 was left unchanged.
- Both Table 1 and its Processing card contained the running `mb-arrive` animation, duration one second; the pulse layer disappeared after its three-second lifetime.
- Increased quantity twice, 1.8 seconds apart, through the real quantity controls. The animation age reset after the second update and remained active until about three seconds after that update. It was absent at the final 5.8-second sample.
- This left number 97 with three Fish Egg Fry items / 252.00 as test work in the existing shop. The original KOT contained one item; the additional quantities were initially unsent. Later manual tests must account for this work instead of silently deleting it.
- Screenshot captured and inspected at `%TEMP%\magicbill-audit-2026-10-08\debug-billing.png`.

These checks demonstrate the repaired POS event path in the actual WebView. A real second-device phone order has not yet been exercised; automated event/identity tests do not replace that claim.

## Verification ledger

- Alert/floor/billing focused UI tests: 73 passed at the alert checkpoint.
- License protocol: 96 Rust license tests and two support-tool protocol tests passed at the signed-grant checkpoint; additional failed-save test added afterward requires the final run.
- UI Bills return tests: four passed, covering original tender/remainder, single atomic submission, historical routing, and approval before writes.
- Full UI run: 524 passed, two failed due to concurrent changes/stale expectations (six-item navigation and manager-input test). Both affected files were subsequently rerun: 74 passed. Another final focused run will cover later UI changes.
- B04 DB checks passed at their checkpoint: returns, schema, wire, archive, credit, and migration tests. Further cash-receipt migration work is checked separately before app restart.
- At this checkpoint the final checks were pending; subsequent checks and the completed authenticated payment/edit/refund walkthrough are recorded below.

Additional completed checkpoints:

- Final full UI suite passed: 43 files, 529 tests. TypeScript and all seven UI lint checks passed, including the 278-command wiring check. A subsequent permission-route refinement passed its affected 18 tests and type checking.
- The full application Rust run completed with 828 passed, six ignored and four failures. These were outdated dashboard-label expectations, the new command module missing from the test scanner (two tests), and an invalid stock-unit fixture. The fixtures/scanner were corrected. All affected reruns passed: 14 guard tests, six historical-return tests, and the dashboard-period test. The first full run itself was not entirely green.
- The remaining workspace crates completed with one failure in a recovery test fixture. That fixture deleted the migration ledger and produced an incoherent backup, so the new version-aware backup check correctly rejected it before the intended restore-failure stage. The fixture now preserves a coherent schema version and corrupts a migration checksum; it verifies that the restore reaches the intended failure and restores the safety copy. Its focused rerun passed. All other crate targets and documentation tests passed, including the 97 license tests, seven DB return tests, backup compatibility, receipt migration, printing, queue and LAN checks.
- Final `cargo clippy --workspace --all-targets` completed successfully without warnings.
- Final chart review found that positive-only donut construction hid negative payment/order-type buckets on return days. The shared builder now uses the existing bars view when any bucket is negative, retaining signed amounts and explaining the returns. A focused regression passed for return-only, mixed zero-net, and ordinary positive sales.
- The rebuilt development app opened the installed database and applied migrations 22 and 23 successfully. A read-only `PRAGMA quick_check` returned `ok`. Original bills BILL//0108 through BILL//0111 remained settled with their unchanged totals and payment modes: 294 UPI, 530 cash, 357 card, and 425 cash. The WebView was again verified at `http://localhost:5173/` with the Vite client loaded.
- Authored-file whitespace checks passed in all three repositories (generated ts-rs output excluded from this check because of its existing generator formatting).
- A real backup was taken and verified through the existing app at 18:22. A preserved pre-migration copy is `C:\Data_Drive\Billing_DB\v2\v4\backups\audit-before-returns-2026-10-08-1822.db`. It is a backup only; the running shop still uses the original database path.
- Final code review fixed a delayed-payout problem before migration: sales-payment attribution now reverses the full original receipts on the return date. Actual payouts remain on their own payment dates. Paying later cannot rewrite a closed return day's totals.
- Void and refund operations recheck relevant closed-day/state conditions within the write transaction, protecting against another writer changing the bill or closing the day after the initial screen check.

## Continuing safely

Re-read current source and this ledger before starting another task. Keep a single implementation of receipt allocation, refund caps, approval policy, and arrival tracking. Reuse the normal repositories and cloud/archive transport. Preserve the same installed account/database for visible testing. Do not use an old embedded debug executable or switch the DB pointer. Keep commits, push, deployment and release disabled until the owner explicitly asks.

### Completed real-app bill walkthrough

The owner signed in normally to the rebuilt debug app. `app_status` confirmed the original `C:\Data_Drive\Billing_DB\v2\v4\magicbill.db` path. No new account or operating database was created. The visible version string remains 1.8.0 because this is unreleased working-tree code; the Vite URL and fresh executable establish which code is running.

Through the real visible controls, using only the new Table 1 / token 97 test order:

1. Verified Fish Egg Fry quantity three / total 252. Entered cash 300: the screen showed 48 change. Completed the bill as **BILL//0112**. Dashboard showed net sales **1,858** and expected cash **1,207**, proving retained cash increased by 252 rather than 300.
2. Opened **Reports → Bills → Edit / return items**, chose Test bill as the reason, reduced quantity to two / total 168, and pressed Save changes. Final review showed original 252, corrected 168, and a prefilled **84 cash return**. Confirmed it. Dashboard showed **1,774 net sales / 1,123 expected cash**. The invoice retained BILL//0112.
3. Chose **Return whole bill** on BILL//0112. The dialog offered only **168 remaining cash**, not the original 252. Chose Test bill and confirmed. Dashboard returned to **1,606 net sales / 955 expected cash**, with 168 shown as the voided corrected bill. The original four invoices and existing Table 3 / token 96 were untouched.
4. Reopened Return money on the same test bill: remaining refundable amount was **0**, and the submit button was disabled. The row retained Edited/Approved history and **252 given back**.
5. Read-only DB inspection confirmed exactly two refund records: adjustment 8,400 paise and ordinary return 16,800 paise, both cash. Corrected retained receipt was 16,800 paise, order state voided. `PRAGMA quick_check` returned **ok**. The test audit trail was retained.
6. Captured and visually inspected `%TEMP%\magicbill-audit-2026-10-08\debug-correction-review.png` and `debug-whole-return.png`. Actual focus recheck also preserved the same return dialog DOM after the fix, rather than losing it.

After the owner's navigation/licence correction, the final UI suite passed **537 tests in 43 files**, TypeScript passed, and layout lint passed. New Rust policy checks passed **24 licence, 31 day-close, six correction-approval, nine refund, six historical-return and 14 guard tests: 90 total**. These cover expired-history refusals, grace access, free new-bill completion/printing, and refusal of an already-open correction before taking money. The final licence run also passed preview/printing of a numbered unpaid order after expiry.

That regression run caught a real integration issue: operational day closing wrote successfully and then attempted to return the now-licensed history view. The shared view builder now returns only operational status without history for expired shops; the public Days history endpoint still refuses. Its regression passed. A subsequent preview review distinguished issued settled/voided bills and corrections from ordinary unpaid orders that may already have an allocated number, preserving free preview/printing for the latter.

Final `cargo build -p magic-bill` succeeded after all policy corrections. The rebuilt debug app was relaunched and verified at `http://localhost:5173/` with Vite loaded and `app_status.shopPath` still pointing to the installed database. It is left at the normal PIN screen after restart. UI changes continue to hot-reload; Rust changes still require rebuilding/restarting. Test/build processes finished; only the debug GUI and its Vite server are intentionally left running. No commits, push, deployment or release were performed.

The actual shop has an active licence and was not changed to fake expiry. Expired/grace behavior was verified in the automated Rust/UI cases above; the actual-shop money walkthrough verified the active-licence path. Do not describe this as a manual expired-shop test.

Physical printer output and a real second-device phone arrival have not been verified. Existing failed print jobs are visible; do not erase unrelated jobs to make the status look clean. Closed historical full-return behavior is covered by integration tests; the live existing-shop walkthrough above does not imply a historical day was closed or altered for testing.
