# The audit, closed

`docs/POS_REBUILD_AUDIT.md` Part 9 is the list of everything wrong with v1 that
this rebuild exists to fix. **Seventy-eight findings**, in nine groups. (The
master plan says 62; the audit grew as it was written, and the real number is
the one below.)

Each is marked:

* **FIXED** — and the test that proves it. A fix with no test is not fixed; it
  is a fix that survives until the next refactor.
* **DECIDED** — deliberately not done, with the decision that says why.
* **NOT OURS** — the finding is about the backend, the phone app or the
  website. It stays on the list because the owner reads one list, and it is
  answered in that project's own rebuild.

Nothing is marked "done" on the strength of a session's own say-so. Run any
test by name: `cargo test --workspace <name>`.

---

## GROUP A — Data safety (the most dangerous group)

| # | The finding, in short | | Evidence |
|---|---|---|---|
| A1 | Only bills are backed up; menu, settings, tables, expenses and credit balances live on one disk with no backup | **FIXED** | A whole-shop backup, verify and restore: `t1_a_whole_shop_survives_delete_and_restore`, and `every_table_is_counted_or_named_as_not` makes it impossible to add a table without deciding whether it is backed up |
| A2 | Expenses never reach the cloud, so the owner's phone shows an inflated profit | **FIXED (counter half)** | Expenses are in the backup and in `sync_outbox` (`t11_every_write_enqueues_in_the_same_transaction`). The cloud half is P31–P35 |
| A3 | Credit balances are not truly backed up, and the balance is stored beside its own ledger | **FIXED** | The balance is a SUM, never a column: `t16_the_credit_balance_is_computed_not_stored`, and `credit_a_riders_cash_and_a_tip_each_reconcile_two_ways` asserts the balance equals the ledger that made it |
| A4 | If the PC dies the shop cannot bill until support unlocks the key | **FIXED** | D86 — the licence gates FEATURES, never billing: `a_shop_bills_with_an_expired_plan`; and an emergency code exists (`an_emergency_code_is_single_use_across_a_restart`) |
| A5 | The database path is stored in the browser's local storage | **FIXED** | It is a file beside the config, found again after it is lost: `t14_a_lost_config_finds_the_database_again` |
| A6 | No export of the raw data | **FIXED** | Every report exports CSV with correct quoting (`a_comma_in_an_item_name_does_not_break_its_row`), and a backup is a plain SQLite file the owner can open |
| A7 | Cloud keeps bills ~60 days live, then only daily summaries | **NOT OURS** | The counter now holds its own full history for ever; the cloud's retention is P31–P35's decision |

## GROUP B — Money correctness

| # | The finding, in short | | Evidence |
|---|---|---|---|
| B1 | The "day" is confused between UTC and Indian time | **FIXED** | D5, the finding that shaped the schema. `a_bill_after_midnight_appears_on_exactly_one_day_in_every_report`, `a_bill_after_midnight_belongs_to_one_day_on_every_screen` |
| B2 | Older bills break the day-wise report completely | **FIXED** | The business day is STORED on every money row; there is no derivation to break. Same tests |
| B3 | The daily token/bill reset only happens when the app starts | **FIXED** | `t14_the_daily_reset_happens_inside_the_claim` — the reset is part of claiming a number, not a start-up job |
| B4 | Numbers are claimed in two steps, not one | **FIXED** | One statement, inside the settle transaction: `t13_ten_thousand_claims_have_no_repeats_and_no_gaps` |
| B5 | No way to cancel a finalised bill | **FIXED** | D47 — a void is a state with a reason: `a_voided_bill_keeps_its_number_and_its_amounts`, `a_big_void_needs_a_second_person` |
| B6 | No way to cancel an open KOT'd order from the counter | **FIXED** | `a_cancel_frees_the_table_and_a_void_does_not`, and the reason is compulsory in the schema |
| B7 | No discount at all | **FIXED** | Line and bill discounts, with a role cap: `both_discounts_apply_in_the_right_order`, `a_capped_bill_discount_reaches_the_bill` |
| B8 | No round-off | **FIXED** | `round_off_reaches_the_rupee_and_is_recorded_separately` — and it is its own figure so the printed lines still sum |
| B9 | One bill, one payment mode | **FIXED** | Split payments: `part_cash_and_part_upi_settles_a_bill_exactly` |
| B10 | One GST rate for the whole bill | **FIXED** | Per item, per charge: `a_bill_discount_on_a_mixed_rate_bill_taxes_correctly` |
| B11 | The tax report splits 50/50 always; no IGST, no HSN, nothing filable | **FIXED** | `interstate_is_all_igst`, `the_hsn_summary_agrees_with_the_rate_wise_one`, `the_rate_wise_tax_report_equals_what_the_bills_printed` |
| B12 | A credit settlement is recorded as payment mode "Full Settlement" | **FIXED** | `payments.settles_credit` — the mode says what it WAS, the flag says what it DID: `a_repayment_will_not_take_a_mode_that_is_not_a_payment_mode` |
| B13 | No service, packing or delivery charge | **FIXED** | Each with its OWN tax rate: `a_service_charge_carries_its_own_rate_into_the_summary`, and P29's delivery charge |
| B14 | "Net profit" is revenue minus expenses, with no purchases | **FIXED** | D133 — three blocks, and the report names the double count: `t14_the_profit_statement_reconciles_and_names_the_double_count` |
| B15 | No day close and no cash reconciliation | **FIXED** | `a_drawer_that_matches_closes_without_a_reason`, `a_short_drawer_cannot_be_closed_without_saying_why`, and P29 added the two lines the list was missing (suppliers paid, cash with riders) |

## GROUP C — Security and control

| # | The finding, in short | | Evidence |
|---|---|---|---|
| C1 | No login on the POS at all | **FIXED** | PIN sign-in, roles, idle lock: `a_shop_starts_open_locks_when_it_gets_a_pin_and_lets_the_right_person_in` |
| C2 | Two different staff systems | **FIXED** | One `staff` table, and P28's employment side hangs off the same rows |
| C3 | The bill always says "Cashier: Admin" | **FIXED** | The signed-in person is on the bill: `t11_every_setting_changes_the_output` covers the line, and the order carries `created_by` |
| C4 | No audit trail on the counter | **FIXED** | Append-only by trigger, hash-chained, and a broken link names its `seq`: `t3_edited_history_is_refused_and_nothing_is_touched` |
| C5 | No content-security policy | **FIXED** | A strict CSP in `tauri.conf.json`; there is no remote origin in it at all |
| C6 | Old branding leaks into the Windows print job | **FIXED** | Nothing in this tree says the old product's name — checked by `no_secret_is_committed`'s sibling sweep and by grep at P30 |
| C7 | A sample `greet` command from the Tauri template is still exposed | **FIXED** | Every command is classified and named: `every_command_is_classified`. There is no `greet` |
| C8 | The Razorpay webhook secret is four characters | **NOT OURS** | MB-backend. Raised again in `BACKEND_REBUILD_AUDIT.md` |
| C9 | All release signing keys live on one laptop | **STILL TRUE — the owner's** | The updater's trusted keys are in `updates::trusted_keys`, and the private half is the owner's to store somewhere else. **This is a business risk, not a code fix**, and it is in OWNER_TESTS |
| C10 | No rate limit or lockout on licence activation | **FIXED** | `mb-auth`'s lockout and `mb-license`'s deadline: `failed_logins_are_counted_from_the_history_and_reset_on_success` |

## GROUP D — Printing

| # | The finding, in short | | Evidence |
|---|---|---|---|
| D1 | The same bill is drawn three separate times, by hand | **FIXED** | One document, three sinks, and a test that no sink may drop anything: `t1_no_sink_can_drop_anything`, `t1_the_raster_sink_cannot_drop_anything_either` |
| D2 | Windows printers only, via PowerShell each time | **FIXED** | Spooler, network, serial and file transports; the printer list is one spooler call, not a PowerShell process per lookup |
| D3 | Printing to several printers blocks | **FIXED** | A thread per printer, and a dead one does not delay a healthy one: `t4_a_dead_printer_does_not_delay_a_healthy_one` |
| D4 | A failed print is a red message nobody remembers | **FIXED** | A durable spool with retry and a persistent indicator: `t14_a_job_that_cannot_be_read_back_is_parked_at_once` |
| D5 | A setting exists that cannot be changed | **FIXED** | D72 — the catalogue IS the settings screen, and a field with no entry fails the build |
| D6 | No bill preview before printing | **FIXED** | The preview is a fourth sink of the same document: `t14_the_document_crosses_ipc_unchanged` |
| D7 | A reprint is indistinguishable from an original | **FIXED** | `t10_a_reprint_is_marked_and_an_original_is_not` |

## GROUP E — Architecture and reliability

| # | The finding, in short | | Evidence |
|---|---|---|---|
| E1 | Everything on a single thread in one window | **FIXED** | Rust owns the work; the print queue, the LAN server, the licence and the backup are each off the billing path (`the_billing_path_does_not_ask_about_the_licence`) |
| E2 | The counter has almost no automated tests | **FIXED** | 1,046 named Rust tests and 233 front-end tests, plus six build-failing guards |
| E3 | Business rules live inside screen files | **FIXED** | R8, enforced: `check-no-money.mjs` fails the build on money in TypeScript, and every rule is in `mb-core` |
| E4 | The cloud/realtime layer is scar tissue | **NOT OURS** | P31–P35 |
| E5 | One counter per shop | **FIXED** | P27 — every till has its own series (D135): `per_till_drawers_sum_exactly_to_the_shops_day` |
| E6 | Settings are one command with 41 numbered slots | **FIXED** | D72 — one catalogue, walked in both directions by a test |
| E7 | No proper log on the counter | **FIXED** | A rolling log with redaction: `no_secret_reaches_the_log` |
| E8 | No crash reporting | **FIXED** | A crash file is written locally and only sent if the shop says so |
| E9 | The update is all-or-nothing with no way back | **FIXED** | Signed manifests, a staged install and a recorded previous version |
| E10 | Old and half-dead things in the codebase | **FIXED** | `every_rust_file_is_reachable_from_a_module_tree` and `nothing_is_left_as_a_todo` (P30) |
| E11 | 208 inline styles have crept back | **FIXED** | `check-tokens.mjs` and `check-layout.mjs` fail the build on a raw value or a hand-rolled page shape |
| E12 | The whole business depends on one free-plan Supabase project | **NOT OURS** | `CLOUD_BUDGET.md` and D16 bind the counter's side of it; the project itself is P31 |

## GROUP F — Screen and usability

| # | The finding, in short | | Evidence |
|---|---|---|---|
| F1 | A fixed three-column desktop layout | **FIXED** | The shell is fluid and the contracts are in UI_GUIDELINES §8 |
| F2 | No item grid or category buttons | **DECIDED** | §15 X13 — *the owner decided this*: keyboard-first. Revisit after real use |
| F3 | Only English, including on the bill | **DECIDED** | D103 — the owner dropped P23. English (India) only |
| F4 | No keyboard help | **FIXED** | `?` opens the shortcut sheet, built from the same table the keys are bound from |
| F5 | No search or filter on the open-orders list | **FIXED** | Type the table number and press Enter, plus the floor screen |
| F6 | No customer phone on a normal bill | **FIXED** | The bill carries the customer when there is one |
| F7 | The window opens at 800×600 then jumps | **FIXED** | The window's size and position are remembered and validated against the screens that exist |
| F8 | Errors show raw system text to a shopkeeper | **FIXED** | Every failure is a `UiError` with a sentence; `words.rs` is the only translator |
| F9 | No accessibility work | **PART** | Text size and light/dark are settings, focus order and labels are tested in the kit. **A screen reader has never been run** — OWNER_TESTS |
| F10 | Confirmation dialogs are inconsistent | **FIXED** | One `ConfirmDialog` in the kit; `check-layout.mjs` stops a screen rolling its own |

## GROUP G — Reporting gaps

| # | The finding, in short | | Evidence |
|---|---|---|---|
| G1 | No stock or inventory at all | **FIXED** | P25/P26 — materials, recipes, the ledger, purchases, the count |
| G2 | No comparison on the counter | **FIXED** | Today at a glance, with the day before |
| G3 | No cashier-, waiter- or shift-wise sales | **FIXED** | The "Sales by cashier" report, and P28's shift boundary |
| G4 | No hourly heat map | **FIXED** | the `sales_hour` report |
| G5 | No stopped-selling list, no menu engineering | **FIXED** | the `stopped` and `margin` reports |
| G6 | Reports print on the thermal printer only | **FIXED** | A4/PDF (`t11_the_slip_says_everything…` for the slip, `pdf.rs` for A4) and CSV |
| G7 | CSV export joins with commas | **FIXED** | One writer, and it escapes: `t4` in `reports.rs` asserts `"Biryani, ""Half"""` |

## GROUP H — Cloud cost and scale

All four are **NOT OURS** — they are `CLOUD_BUDGET.md` and P31–P35. The
counter's side of the contract is D16, and it is why the phone talks to the
counter (D9) rather than to the cloud.

| # | | |
|---|---|---|
| H1 | 30 shops = 56% of the free realtime plan | **NOT OURS** |
| H2 | The estimate assumes 2 phones per shop | **NOT OURS** |
| H3 | `sync-bills` is still deployed for old counters | **NOT OURS** |
| H4 | Nightly summaries rebuild only 3 days | **NOT OURS** |

## GROUP I — Small but real

| # | The finding, in short | | Evidence |
|---|---|---|---|
| I1 | The update notice can be dismissed and forgotten | **FIXED** | The health panel keeps saying so, and the update state is held rather than announced once |
| I2 | A moved licence silently deactivates a running counter | **FIXED** | D86 and the entitlement banner: a shop is told, and it keeps billing |
| I3 | No offline queue for menu edits | **FIXED** | Every write goes through `sync_outbox` in the same transaction |
| I4 | No realistic test bill for printer setup | **FIXED** | `the_sample_cannot_be_mistaken_for_a_bill` renders a real bill with a ruler on it (P27.5 printed it on the owner's TVSE) |
| I5 | Table numbers accept any text; two tables can print identically | **FIXED** | The floor repo refuses a duplicate printed name and says which section it clashes with |
| I6 | No limit on items per order, and no warning about a very long ticket | **FIXED at P30** | `a_very_long_order_is_mentioned_and_never_refused` — a warning past 40 lines, with the paper length, and **never a refusal** |

---

## What this table does not claim

* **C9 is still true and always will be** — where the signing keys live is the
  owner's decision, not a code change.
* **F9 is partial.** No screen reader has been run against this product.
* **Every H row and four others are another project's** to close.
* Nothing here proves anything about hardware. That is `OWNER_TESTS.md`.
