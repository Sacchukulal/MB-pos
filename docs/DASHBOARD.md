# Dashboard implementation checklist

## Scope

Make the counter dashboard understandable at a glance. Sales, payments against those sales, and expected physical drawer cash are different quantities and must remain clearly connected without being confused. Preserve existing reporting, chart interactions, permissions, and navigation. Use the installed account and database for the rebuilt debug app; never seed or create a debug shop.

The user approved implementation after a dashboard audit. Do not add a replacement Dashboard heading, invert the logo colours, redesign the lower charts, or change the appearance of unrelated screens. Keep backend money calculations authoritative and reuse existing accounting rules.

## Agreed changes

- [x] 1. Remove Business overview and Today, so far without adding another heading.
- [x] 2. Remove the logo outline; distinguish selection and hover with restrained neutral backgrounds, preserving logo/text colours and keyboard focus.
- [x] 3. Compact date presets, custom dates, Refresh, and New bill into an adaptive toolbar.
- [x] 4. Connect sales, bill count, and the payment breakdown in one summary.
- [x] 5. Include all actual payment modes, named custom modes, split payments, and separately labelled unpaid credit.
- [x] 6. Keep drawer cash separate, with opening cash, receipts, refunds, expenses, and other movements available in Check cash.
- [x] 7. Explain real differences and unavailable detail concisely; never equate sales, receipts, bank balances, or counted cash.
- [x] 8. Keep expenses/voids secondary; retain average bill and gross margin through reports/details.
- [x] 9. Compact healthy status; preserve every alert with concise text and appropriate direct navigation.
- [x] 10. Remove filler, preserve comparisons and access to useful definitions/details.
- [x] 11. Preserve graphs/rings/breakdowns and improve fitting of names, money, smaller windows, and larger text.
- [x] 12. Preserve presets, custom dates, refresh, chart controls/details, permissions, and report access.
- [x] 13. Scope changes to the dashboard and logo; shared additions must preserve existing defaults.
- [x] 14. Verify both themes, payment/credit/refund cases, empty and archived periods, accessibility, and related screens.

## Verification and launch

- [x] Run relevant Rust tests and the UI checks; test reconciliation using isolated automated test fixtures, never the installed shop.
- [x] Build frontend assets and a debug executable from the final source with the embedded frontend feature.
- [x] Confirm the installed configuration/database path and avoid a second live counter process.
- [x] Launch the freshly built executable with the existing account/database and inspect the real window in both themes.
- [x] Record completed checks and any remaining limits here.

## Verification evidence (11 October 2026)

- UI: `npm run check` passed all type, token, layout, field, ID, money, view, and wiring checks; 541 tests across 43 files passed. Includes keyboard focus in the new dashboard dialogs and Escape dismissal.
- Rust: 34 report tests and 9 dashboard tests passed; `cargo clippy -p magic-bill --locked -- -D warnings` passed. Test fixtures cover custom/split payment modes, tips, credit collections, negative historical returns, empty/multi-day/archived periods, and drawer reconciliation. These cases were not created in the installed shop.
- Actual authenticated preview: dark and light themes visually inspected at 1366 x 720; dashboard also inspected at 1024 x 640 with text scale 1.3. No dashboard horizontal overflow. Temporarily testing long payment labels and large figures in the rendered DOM found no clipping; original text restored immediately, without changing records.
- Existing shop: sales Rs 1,103.00 = cash Rs 452.00 + UPI Rs 651.00, 2 bills. Check cash showed opening cash Rs 0.00 plus cash payments Rs 452.00. Yesterday correctly showed no bills.
- Live navigation: Billing, Floor, Reports, and Account opened without error alerts. Sales report preserved the selected seven-day range. Area/Bars switching worked. Dashboard returned to Today, Area, dark theme, original text scale 1.15.
- Found and fixed a dashboard dialog focus issue during live checks. The dashboard now mounts only the requested dialog; shared modal behavior elsewhere is unchanged. UI checks passed again after the correction.
- Runtime uses the existing `%APPDATA%/MagicBill/database-location.txt`, pointing to `C:/Data_Drive/Billing_DB/v2/v4/magicbill.db`. No new account or debug shop was created.
- Build uses fresh UI assets and `cargo build -p magic-bill --locked --features custom-protocol`, embedding the UI in `target/debug/magic-bill.exe`. No release installation or unrelated application process was replaced.
- Final rebuilt process confirmed as `C:/Data_Drive/MagicBill/MB-pos/target/debug/magic-bill.exe`; its WebView loaded `index-B7UhTiDF.js`, matching the freshly built frontend rather than an older debug bundle.
- Follow-up audit reproduced unreadable selected-hover states, native calendar styling, and incomplete dialog keyboard containment/return. Static screenshots and the earlier tests did not cover these interactions adequately.

## Shared-control follow-up

Approved scope: stable selected colours on hover; one themed calendar for all date fields; shared dialog focus containment/return; consistent chart money formatting. Preserve account/database, date semantics, reporting, and existing non-toggle button behaviour.

- [x] Put complete selected/hover states in the UI kit; remove dashboard/chart colour overrides.
- [x] Replace every native date popup with one shared themed calendar, preserving typing, validation, date navigation, Today, clearing, keyboard use, and edge fitting.
- [x] Fix shared dialog focus for Tab, Shift+Tab, nesting, Escape, and returning to the opener.
- [x] Format chart money through the existing Rust money formatter.
- [x] Run regression checks, then verify actual mouse/keyboard/calendar interactions in both themes and at larger text sizes.
- [x] Rebuild and launch the current source with the existing account/database; record actual results and remaining limits.

Follow-up evidence:

- Shared `DatePicker` supplies the range fields used by Dashboard, Reports and Bills, plus the holiday-date field in Days. No native `type="date"` inputs remain in the UI source. No new dependency was added.
- Shared `useFocusScope` handles dialog/calendar Tab boundaries, nested scopes and return to the opener after React finishes closing a popup. Removed the earlier dashboard-only conditional-mount workaround.
- `npm run check`: all linters/type checks and 551 tests across 45 files passed. An additional holiday integration test was then added; all 13 day-close UI tests passed, including its ISO-date request check. Date tests cover leap days, month boundaries, adjacent-month clicks, typed invalid dates, limits, direct month/year selection, clearing, independent ranges and nested dialogs.
- All 34 focused report tests and 9 dashboard tests passed. Rust Clippy passed with warnings denied. Money formatting changes only the display strings through `Money::to_indian_string`; calculations and exported report tables retain their existing behavior.
- Live WebView checks measured exactly identical selected preset/sales-trend background, text and border colours before and during hover in both themes. The calendars use the app's Inter font and theme surface; both were visually inspected, including month/year selection.
- Live keyboard checks: eight consecutive Tabs stayed inside Sales details, Escape returned to Details, Shift+Tab stayed inside Check cash, and calendar Escape returned to its date button. Calendar arrow keys and shared calendar rendering in Bills were checked.
- At 1024 x 640 with text scale 1.3, the calendar remained fully inside the viewport and the dashboard had no horizontal overflow. Shared field sizing was tightened to the existing field-width token after visual inspection; no dashboard-specific date width remains.
- The installed account currently hides holiday planning; its integration was verified with the existing automated Days fixture, without changing shop settings or marking any real holiday.
- Final build confirmed in the running WebView: `index-ClRKDrK6.js`, with the freshly built `index-C_UU5N7U.css`. Repeated the live hover, theme, calendar, modal and smaller-window checks on that saved build with no temporary stylesheet overrides.
- Final interaction checks selected 10 October through the calendar, rejected the impossible typed date 31 February with no stale sales left visible, and cleared the date successfully. Selected calendar-day colours also stayed unchanged on hover. Restored Today, dark theme and text scale 1.15; left the rebuilt debug app open with the installed account/database.
