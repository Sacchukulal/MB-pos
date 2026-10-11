# Reports header alignment plan

## Purpose and status

Discussed and authorised for implementation on 11 October 2026. This file records the final decisions and implementation progress. Earlier proposals to retain duplicate page titles or replace period buttons with a dropdown are superseded.

Implement the changes at their source, reuse shared controls and formatters, and use theme tokens. Preserve business logic, permissions, date queries, account data and unrelated workflows. Wording changes outside Reports are presentation changes only. Do not create parallel implementations or apply arbitrary layout overrides to hide symptoms.

The goal is a compact, orderly header with consistent alignment and enough room for report content. Avoid unnecessary space, but keep dates readable and controls usable. Alignment takes priority over squeezing everything into one line.

This expands pending points 6–9 in [Printing and date control follow-up](PRINTING-AND-REPORTS-FOLLOW-UP.md). That file remains the historical record of completed printing and date-control work. Its completed tasks are not part of the new work list below. Relevant calendar details are recorded separately at the end of this plan.

The user is the developer. The installed account and database contain developer testing data, not a live restaurant operation. Continue using that environment; do not create a new account, licence or operating database for this work.

## Review before implementation

The current debug app was inspected directly on Dashboard, Bills, Sales by day, Item sales and Day open/close, in the existing dark theme and text scale of 1.15. The review also checked the Dashboard calendar icon hover. The user's three screenshots from 20:25 on 11 October provide the original examples.

| Area | Current behaviour and visible issue |
| --- | --- |
| Dashboard | Compact dates, inline From/To labels and compact presets form a clear row. This is a useful reference for Reports. |
| Bills | Title, count and totals are followed by search and filters, then presets and dates. The date labels sit above the fields, and the presets sit at a different vertical position. The filters and dates consume more vertical space than needed. |
| Other reports | Sales by day and Item sales use the same header. Dates appear before the title. Six separate actions stretch across the title row. |
| Comparison | A prominent strip says “Nothing at all the day before” with a green upward indicator. There is no previous sales baseline for a meaningful percentage comparison. |
| Day open/close | It has its own title and operations. In the inspected account, Rules is Disabled. It does not need the sales report date range or export toolbar. |
| Bills table | Long row actions extend beyond the visible right edge, with a horizontal scrollbar. This is separate from header alignment. |

Bills and Day open/close are separate components. The report entries below them share the Reports component's period controls and actions. A shared header design should give these screens consistent alignment while preserving their individual controls and state.

## Approved changes

### 1 Remove duplicate visible titles

The selected sidebar entry already identifies the page. Remove the repeated visible Bills, Day open/close and report heading, including the otherwise empty heading row. Retain an accessible heading and keep full report titles in printed, shared and exported documents.

Bills keeps its count and Taken/Net summary in a compact information row beside the supported actions. Taken and Net remain distinct even when their values match. Do not reserve an empty summary row.

Use a labelled Report info control within the action group when explanation is available. It opens the explanation on request. No floating information icon and no help buttons attached to sidebar entries.

Use the same left and right content edges for the period row, information/actions row, comparison and table. Avoid decorative header cards and extra padding.

### 2 Use compact aligned period controls

Use the shared compact date-control variant already used by Dashboard. Place From and To beside their respective fields, vertically centred. Use the same height for preset buttons and date fields in that row.

Keep all five existing presets as separate buttons: Today, Yesterday, Last 7 days, This month and Last month. The user explicitly rejected a preset dropdown. Keep their backend-defined business-day meaning.

There is an important correction to the description of Dashboard's “narrow” dates: Dashboard and Reports currently use the same width token, `--field-sm`. At the reviewed text scale both were approximately 184–185 pixels wide. Dashboard is more compact because its fields are 30 pixels high, with smaller text and inline labels. Reports uses 38-pixel fields with labels above them.

The recommended starting width is Dashboard's existing bounded width. It looks readable and fits the date, padding and calendar button. Do not stretch date fields to fill spare horizontal space. Do not make them narrower just to meet an arbitrary pixel target. A further reduction should be retained only if the full `DD-MM-YYYY` date, caret and calendar control remain comfortable at larger text sizes.

Scope any layout or width adjustment to the Reports header or an explicit shared variant. Do not change the global small-field width or shrink unrelated inputs.

### 3 Give Bills filters their own aligned row

Start Bills and other reports with the same period row. Put Bills search and filters in their own aligned row, followed by its compact count/totals and export actions. Remove the duplicate title rather than rearranging it.

Keep the existing bill-number/table/item search, cashier filter, state filter and payment filter. Search should have more room than the dropdowns. Give controls within this filter row a consistent height and align their top and bottom edges.

The filter row does not need to be as short as the compact date row. Keeping its current comfortable input height is reasonable. Different rows may use different sizes; inconsistent sizes inside one row are the problem.

This concerns the search inside Bills under Reports. Billing's separate search field is outside this work.

### 4 Group supported actions

For ordinary reports, use three visible controls: Print, Share and Export.

| Control | Existing actions it exposes |
| --- | --- |
| Print | The current report print command, directly available |
| Share | Copy, WhatsApp and Email |
| Export | CSV and PDF |

Bills gets Export with its existing CSV and PDF actions. Do not add a page-level Print or Share action to Bills when those actions are not currently supported there. Existing per-bill reprint and other bill actions remain separate.

Preserve the current export permissions and command behaviour. Grouping the controls does not permit inventing new actions, changing output formats or changing who may use them. In particular, keep the existing copy and sharing flow; do not attempt a redesign of sharing as part of alignment work.

Menus must open within the window, remain above the report table, and work with keyboard navigation. Escape should close them and return focus to their trigger. Use clear labels and visible focus. Do not replace the three controls with unexplained icons merely to save a few pixels.

### 5 Define deliberate wrapping

On the normal counter window, keep the title/actions row and period row compact. When space becomes tight, wrap whole groups predictably.

- Keep each From or To label attached to its field.
- Keep the date pair together where space permits. If it cannot fit, place each complete label-and-field unit on its own aligned line.
- Move the date group to the next line when the presets and dates cannot fit comfortably together.
- Let actions wrap as a group when they cannot fit alongside useful summary information.
- Let Bills filters wrap into aligned rows while retaining a useful search width.
- Keep the header inside the page width. Wide tables may retain their own horizontal scrolling.

Do not force a single row by reducing text to an uncomfortable size, clipping the year, hiding labels or overlapping the calendar icon. A deliberate extra row is better than an unaligned or cramped single row.

### 6 Keep selected presets clear

Use a consistent selected appearance for period presets within the Reports area. Selection must remain obvious on hover and keyboard focus, in both themes. Expose the selected state accessibly, rather than relying only on colour.

The live dark-theme hover changed the selected preset's shade; it did not reproduce a full colour inversion during this review. The requirement is to keep selection stable and readable, not to claim that every current hover is broken.

Keep the existing date-based preset behaviour. For a custom range that matches no preset, do not leave an unrelated preset looking selected. Do not introduce another permanent Custom button unless a concrete usability need emerges.

### 7 Reduce and clarify the comparison strip

The current large bordered box says “Nothing at all the day before” and shows a green arrow. Replace it with a small, plain comparison line. For sales reports with no previous sales baseline, use “No sales in the previous period” with the comparison dates and no green growth arrow. Retain valid comparisons.

Distinguish a zero previous value from an actual request failure or missing data. When both periods have zero sales, say so accurately. Preserve valid up/down percentages and their useful explanation when a comparison can be calculated.

The comparison sentence and direction currently come from Rust in `src-tauri/src/reports.rs`. The implementation must handle the underlying state explicitly; do not detect the condition by matching an English sentence in CSS or React. Keep amounts and calculations in Rust.

Use “sales” only for comparisons that actually measure sales. Any reuse for a different report metric must use appropriate wording.

### 8 Keep Day open and close distinct

Remove the repeated visible title and match the compact summary/action alignment. Retain its own date/status explanation and operational controls, with a labelled info control for its existing explanation.

Do not add sales presets, From/To fields, Share or Export simply to make the page identical to other reports. Do not enable Rules or perform open/close operations during a visual review. Its currently sparse disabled state is not a reason to add unrelated controls.

### 9 Keep the header steady during loading

The current Reports component clears the loaded report when the report or period changes. Its title and actions are rendered with the loaded report, so they can disappear while the next result loads.

Keep the period controls and action placement stable during loading. Disable output actions until a result matching the selected report and range is ready. Do not allow stale report content to be printed, shared or exported under a newly selected report or period.

Preserve clear loading, empty and error states. Do not create large blank header gaps when a comparison or subtitle is absent. Maintain usable table height and scrolling.

### 10 Standardise displayed numeric dates

Use DD-MM-YYYY for human-facing numeric dates, including report rows, period descriptions and comparison dates. Reuse a central presentation formatter; do not rewrite arbitrary table strings based on their appearance. Keep ISO query values, database formats, date calculations, sort keys and machine interchange formats unchanged. Preserve time-of-day where it is present.

### 11 Put report notes near the figures

Place Average bill and other report notes directly after the table, inside the same scrolling content area. Do not pin them to the bottom of a mostly empty page. Keep warnings and explanations available without covering totals.

### 12 Use concise wording across the app

Review labels, page names, loading messages and empty states for conversational or essay-like wording. Use familiar, concise UI language, such as All cashiers, All states and All payment modes. Shorten report names at the catalogue source, not with a second frontend lookup table. Keep necessary explanations in help and preserve distinctions such as returns versus refunds. Outside Reports, change wording only; retain commands, IDs, permissions and workflows.

## Intended arrangement

Ordinary report:

```text
Today  Yesterday  Last 7 days  ...    From [date]  To [date]
No sales in the previous period · [dates]   Report info  Print  Share v  Export v
[Report table]
[Average bill and other report notes]
```

Bills:

```text
Today  Yesterday  Last 7 days  ...    From [date]  To [date]
[Bill search                 ] [Cashier] [State] [Payment]
8 bills · Taken ... · Net ...                              Export v
[Bills table]
```

These illustrate hierarchy, not fixed columns or a requirement to stretch blank space. Actual wrapping follows the available width and text scale.

## Other findings and scope limits

The Bills table needs a separate review of its row actions. The live screen requires sideways scrolling to reach all of them. Header cleanup will not solve this. A possible later approach is to retain the most frequent action and group secondary actions, but that needs a separate decision because it changes bill workflows.

Sales by day has a large blank area below a short table. Empty space after a short report is not automatically wasted space. The approved correction moves its separated average-bill note next to the table; it does not stretch content to fill the page.

The Item sales screenshot supplied by the user shows a dark hover background on another sidebar entry as well as the selected Item sales entry. That does not by itself establish a selection bug. In the live review, the selected entry and report title agreed. Keep hover distinguishable from selection and check it during visual verification.

Preserve separate Bills and report date state, deep links from Dashboard, permissions, totals, tables, bill detail dialogs, returns, refunds and filters. Shared presentation must not silently combine state or change navigation behaviour.

## Existing calendar behaviour to preserve

This section records completed work that the new header will reuse. It is not a new implementation checklist.

The earlier fix sized the complete date control together: input, calendar button and the button's hover background. Previously, a Dashboard override shortened only the input, leaving the calendar trigger sized for the larger control.

The shared `.mb-date` control now uses `--date-height`. The normal variant uses `--control-md`; `.mb-date--sm` uses `--control-sm`. The input follows that height and reserves right padding for the calendar control.

The calendar button is inset by `--space-1` at the top and right. Its width and height are the date height minus two insets. With the current tokens, that gives:

| Variant | Input height | Calendar button | Inset |
| --- | --- | --- | --- |
| Compact, used by Dashboard | 30 px | 22 px | 4 px |
| Default, used by Reports before this change | 38 px | 30 px | 4 px |

This keeps the trigger and its hover background inside the input. The live Dashboard hover showed that containment working. Reuse the compact variant in Reports instead of bringing back an input-only height override.

The compact toolbar field does not imply shrinking the calendar popup or its day cells. Preserve the existing popup positioning, date entry, date validation, keyboard navigation, Escape behaviour and focus return. The earlier report records broader calendar verification; it is historical evidence, not proof that a future header change has passed its own checks.

## Implementation references and verification

Implementation files:

- `ui/src/reports/Reports.tsx`: shared report header, period selection, actions, loading and comparison presentation.
- `ui/src/reports/Bills.tsx`: Bills title, totals, filters and exports.
- `ui/src/reports/Days.tsx`: Day open/close header and its distinct operations.
- `ui/src/reports/reports.css`: Reports and Bills layout.
- `ui/src/reports/Dashboard.tsx` and `dashboard.css`: compact date layout reference.
- `ui/src/kit/DatePicker.tsx`, `date-picker.css` and related date-range controls: existing calendar behaviour and sizing.
- `ui/src/theme/tokens.css`: existing field, control, text and spacing tokens.
- `src-tauri/src/reports.rs`: comparison state and wording.

For implementation verification, check the normal 1366-pixel counter window and the supported 1024-pixel minimum window width, normal and enlarged text, and both themes. Check long report titles, hover, keyboard focus, open calendars and menus, restricted permissions, loading, errors, custom ranges, empty data and zero comparison baselines. Confirm that the table remains scrollable and all header controls remain reachable.

Run the repository's relevant checks after implementation. Use focused tests for behaviour changes such as menu actions, permissions, loading safety and comparison states; visual inspection is still needed for alignment. Prior passing tests for print work do not validate this header change.

## Development environment

Before implementation, the source was built and launched in the preceding review on 11 October 2026. The frontend build passed, followed by `cargo build -p magic-bill --locked --features custom-protocol`. The debug executable was rebuilt at approximately 20:33 and launched from `MB-pos/target/debug/magic-bill.exe`. Embedded assets are `index-BOM3--FD.js` and `index-EvZamOqQ.css`; identical filenames after rebuilding mean unchanged content, not that an old build was used.

The app uses the normal `%APPDATA%/MagicBill` configuration and its existing `database-location.txt`, pointing to `C:/Data_Drive/Billing_DB/v2/v4/magicbill.db`. Startup confirmed that database opened, and the existing sanath shetty account signed in. No replacement account, database or licence was created.

Future debug reviews should build the latest frontend and executable before launch and keep this existing configuration. Native console commands must run through the workspace's `tools/Invoke-HiddenCommand.ps1` in the managed shell, with shell profiles disabled. Do not open extra terminal windows.

## Implementation record — 11 October 2026

The earlier printing and calendar fixes remain separate. The following changes implement the approved header and wording work:

1. Added `ui/src/reports/ReportHeader.tsx` for the common header, period controls and export menu. Bills and ordinary reports reuse these controls while retaining separate date state and their existing commands. Day open/close uses only the header and its own operations.
2. Removed duplicate visible headings. Accessible headings remain; printed and exported report titles remain available. Useful Bills totals and day status remain visible.
3. Reused the existing compact date control, with inline From/To labels and the existing bounded field width. Kept all five preset buttons and exposed the selected period with `aria-pressed`. Layout uses existing spacing and size tokens, with wrapping rather than forced narrow fields.
4. Moved Bills search and its three filters into their own row. An absent summary does not create an empty summary row.
5. Grouped the existing output commands into Print, Share and Export. Bills retains only its supported Export actions. Reused the existing `RowMenu`, adding optional disabled state and keyboard navigation for these report menus; other menus retain their existing defaults.
6. Added concise report descriptions at the Rust report catalogue source. Report info opens those descriptions in the existing modal control. Day open/close shows its existing explanation through the same labelled control.
7. Kept ordinary report controls visible while loading and disabled output until the result matches the selected report and dates. Tests check that changing reports cannot leave output enabled for stale data.
8. Replaced the large comparison box with a plain line. A zero previous sales total now has an explicit neutral direction, with accurate wording for one or both periods having no sales. Valid percentage calculations remain unchanged.
9. Moved report notes immediately below their table, in the same scrolling area.
10. Added a central Rust `words::date` formatter for DD-MM-YYYY. Applied it to human-facing report dates and the shared timestamp formatter, preserving time-of-day. Daily sales dates are identified by the report's typed grouping, not by guessing whether arbitrary cell text resembles a date. ISO query values, storage, comparisons and machine formats remain unchanged.
11. Shortened report names at their catalogue source and reviewed wording across Account, setup, Settings, Staff, Attendance, Payroll, Floor, Menu, Delivery, Credit, Stock, Purchases, Expenses, device screens and billing status labels. Examples include “All cashiers”, “No matching bills”, “Payment mode”, “Leave balances”, “Loading settings” and “Expenses”. These changes affect labels and explanatory text only; command names, permissions and business rules were retained. This is a reviewed wording pass, not a claim that every message throughout the app has been rewritten.

### Automated verification

- `npm run check`: 556 UI tests passed across 45 files, with TypeScript and all repository UI lint checks passing.
- `cargo test -p magic-bill --locked`: 847 passed, 6 ignored, no failures. The ignored tests remain ignored by the repository.
- `cargo clippy -p magic-bill --locked -- -D warnings`: passed.
- After the final empty-summary correction, the focused Reports and bill-return suites passed all 26 tests, and the frontend build passed again.
- The new focused assertions cover report menu routing, keyboard focus and Escape, date values remaining ISO, selected period state, accessible headings, report information, loading safety, no-baseline comparisons and formatted report dates.
- `npm run build` and `cargo build -p magic-bill --locked --features custom-protocol` passed. The frontend build reports its existing large-bundle advisory.
- A whole-tree whitespace check reports trailing whitespace in generated `DashboardView.ts` and `PrintJobView.ts`, both associated with earlier work. These generated types have not been manually reformatted as part of the header changes.
- `cargo fmt --all -- --check` is not clean: it reports extensive formatting differences across the existing workspace, including untouched files. No whole-repository reformat was applied; the new shared date formatter was formatted locally.

### Running build verification

The implementation debug executable was rebuilt at 21:33 on 11 October and launched from `MB-pos/target/debug/magic-bill.exe`, including the empty-summary correction. It opened the existing staff sign-in screen with sanath shetty selected and the normal database configuration still pointing to `C:/Data_Drive/Billing_DB/v2/v4/magicbill.db`. No replacement operating account, database or licence was created. The user unlocked this build for the live review recorded below.

### Completed live review and final correction

Inspected the actual running WebView on Bills, Sales by day, Item sales and margins, and Day open/close. Captured and inspected screenshots at the normal 1366 × 720 viewport with the user's dark theme and 1.15 text scale, and at a 1024 × 720 viewport with light theme and 1.3 text scale. Width and theme overrides were temporary review settings; restored the original theme and text scale after inspection.

- Preset buttons and dates align at normal width. At minimum width, the date pair wraps together, and actions wrap without overlapping the comparison or table. No document-level horizontal overflow was detected. Wide tables retain their own scrolling.
- The date inputs are 30 pixels high. The calendar buttons remain 22 × 22 pixels with four-pixel insets, including at enlarged text size. The calendar popup stays inside the viewport; Escape returns focus to the correct date trigger.
- Share and Export menus stay inside the smaller viewport. Actual browser keyboard events verified Copy → WhatsApp with Arrow Down and Escape returning focus to Share. The info dialog fits and displays the report's catalogue explanation.
- Today and Yesterday select the correct displayed dates. Yesterday showed “No sales in either period” with comparison date 09-10-2026. Entering a custom 03-10-2026 to 11-10-2026 range cleared preset selection and loaded the corresponding report; restored Today afterward.
- Sales dates display DD-MM-YYYY, the comparison is a plain line, and Average bill sits directly below the table. The repeat heading is absent visually.
- Day open/close retains its current Disabled Rules state and does not acquire sales controls. No day operation was performed.
- Found and corrected one additional layout issue: the full “All payment modes” label clipped inside the fixed small-width Bills filter. Bills selects now use their intrinsic content width with the existing small-field token as the minimum and a maximum of 100%. This is scoped to the Bills filter row, without changing global controls, abbreviating the label, or hardcoding a width. The resulting payment field measured 200 pixels at scale 1.15 and 219 pixels at scale 1.3; the other short filters kept their existing small width. Verified the correction in the running app at both viewport sizes before rebuilding.
- After this CSS-only correction, all UI lint checks and the TypeScript/frontend build passed again. The final packaged assets are `index-DWdY0Pie.js` and `index-C7Di7ksA.css`.

Final debug rebuild completed at 22:03 on 11 October 2026 and was launched from the same executable path. Inspected the relaunched WebView's loaded assets and stylesheet to confirm it contains the final filter-sizing rule. The restart returned to the normal staff PIN screen; account, licence and database configuration were retained.

The Bills table's long row-action area still requires its own horizontal scroll. This was an existing, explicitly separate finding; this work does not reorganise those bill operations. The app also displayed its existing failed-print-job indicator; this review did not submit or retry print jobs.

## Additional review and approved corrections — 11 October 2026

The user's subsequent screenshots exposed incomplete alignment and inconsistent table framing. The earlier review checked individual controls but missed the relationship between the complete rows and tables. These corrections are additional work, separate from the completed changes above.

1. **Common right edge:** measured the date controls ending roughly 144 pixels before the content edge, Bills filters 52 pixels before it, and Export at the edge. Align the compact date group to the right without widening date fields. Let Bills search absorb spare row width, keeping filter labels readable and wrapping at smaller widths.
2. **Bills loading:** three live navigation samples settled in 379–389 ms, versus 15–25 ms for sales reports. Direct read-only Bills requests took 22–23 ms for eight bills. Source inspection found a 150 ms search timer applied to initial loading and every filter change. The initial response then set Today's dates, triggering another delayed request for the same day. Totals and Export disappeared between responses. Fetch once on entry, debounce only search typing, keep controls stable, and prevent stale results or outputs while filters change. Preserve backend-defined business dates, permissions, licence checks and refreshes after bill operations.
3. **Consistent table container:** Bills already wraps the shared Table in a flush Panel; ordinary reports render the Table alone. Use the same subtle bordered container for report tables, retaining clickable bill rows, totals and scrolling. Do not change the global table component or bill operations.
4. **Consistent actions and separators:** give Report info and Print the same compact outlined button treatment as Share and Export. Separate bill count from totals and comparison wording from its dates clearly.

The user authorised implementing these corrections one by one. Verify request counts and search timing with focused tests, then check actual alignment, table framing and navigation timing in the newly built debug app using the existing account and database.

### Implementation and verification of the additional corrections

- The shared date group now aligns to the content's right edge. Dates retain their compact width. Bills search expands into available row space; the three dropdowns form one wrapping group aligned to the same edge. At minimum width, search occupies a complete row and all three filters stay together below it.
- Ordinary reports reuse the same existing flush Panel around their Table as Bills. Totals remain in the table and notes remain immediately below it. Bills uses the same header-to-table spacing token as other reports. No global table behaviour changed.
- Report info and Print now use the same compact secondary button treatment as Share and Export. Bill count/totals and comparison/date text have explicit separators.
- Bills now loads immediately. The initial business period comes from the existing Rust report catalogue where available, or from the Bills response for users without the report catalogue. That first response no longer triggers another request. The default period remains tied to the data shown, including for exports.
- Only search typing waits 150 ms. Date and dropdown changes apply immediately. A matching-filter marker and request sequence prevent stale responses, totals and exports from appearing under a newer selection. Export stays in place and is disabled during updates; the header remains mounted. Invalid/incomplete dates do not silently request Today's bills. Failed loads expose Retry while retaining the controls. Existing bill-operation refreshes continue through the same loader.
- Added regression coverage for a single immediate initial request, the no-catalogue fallback, export dates, search debounce, immediate dropdown/date changes, stale responses, persistent controls, failed loads and invalid dates.
- Full `npm run check` passed 559 tests across 45 files, plus TypeScript and all UI lint checks. After the final narrow-layout refinement and an additional failure/invalid-date test, the focused Reports and bill-return suites passed all 30 tests; UI lint and TypeScript/frontend build passed again. Rust business logic was not changed in this follow-up.
- For live checks without another PIN interruption, temporarily served the freshly built UI assets to the existing unlocked Tauri WebView through the local debugger. This reused the real backend and existing account/database. Disabled the temporary asset interception afterward. The complete executable was then rebuilt for the final launch; this frontend review does not claim that the older running executable had already been rebuilt.
- Inspected screenshots at 1366 × 720 in dark theme with scale 1.15 and at 1024 × 720 in light theme with scale 1.3. No document-level horizontal overflow. At normal width the dates, last filter and actions end at x=1354; at minimum width they end at x=1012. Restored the original theme and scale.
- Final live navigation samples: Bills 21/42/43 ms; Sales by day 10/11/9 ms. Earlier Bills samples were 379/382/389 ms. These are local measurements on the developer's current eight-bill dataset, not a guarantee for every database size.
- Final frontend assets: `index-BaRCfOm9.js` and `index-CN9pGlCl.css`. The separate existing Bills row-action overflow remains outside these corrections.
- `cargo build -p magic-bill --locked --features custom-protocol` passed. Rebuilt the debug executable at 22:27 on 11 October and launched it from the usual `MB-pos/target/debug/magic-bill.exe` path. Verified the relaunched WebView loaded those exact final assets and retained `C:/Data_Drive/Billing_DB/v2/v4/magicbill.db` as its configured database. The restart returned to the normal PIN screen; no new operating database, account or licence was created.
- After the user unlocked that final executable, repeated the live checks against its own embedded assets, without asset interception. Bills settled in 48/45/44 ms and Sales by day in 8/13/14 ms. Confirmed the header, date group, Bills filter group and Export all end at x=1354, the Bills table retains the shared bordered panel, and the page has no horizontal overflow. Inspected a fresh screenshot and left the app open on Bills.
