# What Rust could do that the shop could not reach

**Audited 2026-08-16. Closed 2026-08-17. 226 commands were registered and 29 of
them had no caller.**

Audited against the running Tauri window, not against the source: every command
was invoked through `window.__TAURI_INTERNALS__.invoke` on a genuinely fresh
install, and every screen and tab was opened and read. The harness is
`scripts/drive.mjs`; the method is in `REWIRE_PLAN.md`.

**The count is now a build step.** `node scripts/audit-wiring.mjs` runs inside
`npm run lint`, and it fails if a registered command has no screen that mentions
it. This file is the record of what it found the first time.

```
  wiring: clean — all 230 commands are reachable from a screen
```

## How this was counted

A command is **wired** if some `.tsx` outside `ipc/call.ts` names it — including
through a variable, which is how `who_owes`, `report_csv`, `report_pdf`,
`open_pairing`, `close_pairing`, `allow_device` and `refuse_device` are reached
(`Credit.tsx`, `Reports.tsx` and `Network.tsx` each dispatch on a union of
literal names). Those seven look unwired to a naive grep and are not.

`call.ts` and the Rust registration already have a compiler between them; the
gap was entirely between `call.ts` and the screens.

---

## Severity 1 — the till lost money, history or paper

| Command | What was missing | Where it is now |
|---|---|---|
| `cart_set_qty` | **A line's quantity could not be changed.** The only control on a cart line was ✕, so two dosas becoming three meant deleting the line and typing it again, on the till, mid-service | The quantity on a cart line is a button: tap it and type. `Billing.tsx` |
| — | (new) `cart_step_qty` — − and + beside it. **The arithmetic is Rust's**: a quantity is thousandths, and `0.1 + 0.2` in a JavaScript double is `0.30000000000000004` (D2) | `ipc.rs`, `Billing.tsx` |
| `cancel_order` | **"Cancel order" did not cancel the order.** It called `cart_clear`, so the reason, the audit row and the kitchen's cancellation were never written. An order that reached the kitchen and was then killed left no trace — audit B5/B6, the exact hole P12 was built to close | The button now asks for a reason and calls `cancel_order` **when the order has a bill number**; before that it is still somebody clearing the screen |
| `void_line` | The same hole from the other side: a line removed after its kitchen ticket went is a void with a reason, not a silent `cart_remove` | ✕ changes meaning once `kitchenTold` is true — a reason dialog, an audit row, and Rust prints the kitchen its cancellation slip |
| `save_menu_category` | Categories were read-only. A shop could not add one, rename one or retire one — ever, by any route | **Menu > Groups**, with `route_category` beside it: what the groups are, and where each one's food is cooked. `Groups.tsx` |
| `save_employee` | Designation, department, address, emergency contact, ID proof, employment type — **and the leaving date.** Somebody who left the shop stayed on the payroll list permanently | **Staff > People > At work**. `Employment.tsx` |
| `save_purchase_order` | Buying > Orders could advance a purchase order and **could not create one**, so the tab was a list that could only ever be empty | **Buying > Raise an order** |
| `save_supplier_adjustment` | A supplier ledger could not be corrected — a credit note, a wrong invoice, a rounding. Shops did it by entering a fake payment | **Buying > Suppliers > Account > Correct the balance** |
| `go_back_a_version` | **A shop could not go back to the version that worked.** Audit E9, I1, ANDROID-G2/G4. `main.rs` detects the roll-back state and logs *"Settings > Go back"* — a place that did not exist | **Settings > This version**. `Updates.tsx` |

## Severity 2 — a shop could not do its work

| Command | What was missing | Where it is now |
|---|---|---|
| `even_split` | "What do we each owe?" — the question a table of six asks every time | **Billing > Split**. `Split.tsx` |
| `split_order` | Moving part of a table's order onto its own bill | the same dialog |
| `set_covers` | How many people are on the table. Every per-cover figure in Reports had nothing to divide by | the same dialog |
| `save_expense_category` | Expense categories were whatever migration 0001 seeded, for ever | **Spends > Categories** |
| `adjust_leave` | A leave balance could go down and never up: no yearly grant, no carry-forward, no correction | **Staff > Leave > Grant or deduct** |
| `edit_payroll_line` | A payroll line could not be corrected before approval — and the table already printed a `*` beside an "edited" figure that nothing could produce | **Staff > Payroll > (open a draft) > Correct** |
| `save_roster` | No roster, so **every "Late by 35 minutes" was judged against nothing** | **Staff > Attendance > Roster**. `AttendanceView` now carries the shop's shifts so the choice is a list and not an id somebody has to know |
| `set_rider` | A rider could not be assigned. The empty state said *"mark somebody as a rider on the Staff screen"* and there was no such control anywhere | **Delivery > Who can ride**, beside the sentence that asks for it |
| `staff_cost` | Wages against takings — *"the second of the two numbers that decide whether a restaurant makes money"* — had no screen | **Staff > Payroll**, over the period already chosen |
| `stock_variance` | Theoretical against counted — the number that finds theft — had no screen | **Stock > What went missing** |
| `refresh_licence` | After paying, a shop could not re-check. "Check again" re-read the cached entitlement and gave the same answer | "Check again" now calls `refresh_licence` |
| `look_for_an_update` | Nothing offered an update | **Settings > This version** |
| `dismiss_update` | And nothing could turn one down | the same page |

## Severity 3 — support and convenience

| Command | What was missing | Where it is now |
|---|---|---|
| `reveal_logs` | Health could write a diagnostics bundle and could not open the log folder. Audit E7 asks a shopkeeper to send us that file | **Health > Open the log folder** |
| `list_print_jobs` | The print-queue panel was filled **only** by a pushed event, so after a window reload it showed nothing even with jobs parked — and D4 is precisely that the cashier must SEE a print that did not happen | fetched once when the shell mounts |
| `share_report` | Scope 10.13 — sending a report to somebody | **Reports > Copy / WhatsApp / Email**, with Rust's own caveat shown (D134) |
| `reload_settings` | Re-reading settings a second till changed | **Settings > Read these settings again** |
| `use_existing_shop` | The first run adopted a found database by calling `create_shop` with its path | it calls the command written for it, so the log says which happened |

## Severity 4 — superseded, and two were deleted

| Old | What happened |
|---|---|
| `list_printers` | **Deleted.** P17's `printer_setup` answers the same question and three more. Two commands for one question is audit E6 |
| `nudge_print_offset` | **Deleted.** A one-line wrapper around `nudge_offset_on`, and `settings::printers::nudge_printer` is another one around the same body. The body stays — D46 put the clamp in one place on purpose |
| `print_test_page` | **Kept and wired.** It is *not* a duplicate: P07's slip carries the ALIGNMENT RULER the four nudge arrows are read against, and it is the only thing that prints with no shop open. **Settings > Printers > Print the alignment slip** |

---

## Defects found in the same audit that were not a missing wire

| What | Fixed |
|---|---|
| **The item column collapsed to one character** whenever the item size was Large or Extra large. `Block::Columns` asked `cap_scale` whether *four columns* fit, when the question is whether the columns' *widths* fit. On 48-column paper at 2×: 24 usable, 23 taken by qty/rate/amount, **1 left for the item name** — a real shop printed one letter per line and burned a roll per bill | `layout.rs` weighs the columns' widths, with a minimum for a fill column. Two tests: the bill comes down to 1×, and a kitchen ticket (five fixed columns) keeps its big food |
| **A barcode never reached the on-screen preview.** `Receipt.tsx`'s switch had no `barcode` arm, returned `undefined`, and React drew nothing | the arm, plus a `never` check so the compiler fails on the next variant, plus a test keyed on `PreviewLine['kind']` so the fixture cannot go stale again |
| **There was no way to choose a logo.** `mb-print` has drawn one since P07 and `BillContext.logo` was `None` at every call site, so `receipt.logo` and `receipt.logo_width_pct` were two settings pointing at a picture nothing could supply | `logo.rs`, `Logo.tsx`. D37's split kept: Rust picks the file, the browser decodes and thresholds it and **shows the shopkeeper the actual dots**, Rust stores them beside the database so they travel with the shop |
| **One typeface, and sizes named Normal / Large / Extra large** | six faces read out of the system font folder (no installer cost), one for the bill and one for the kitchen ticket; sizes say **24 px / 48 px / 72 px**. See `REWIRE_PLAN.md` for why those are the only three |
| **The shop folder could not be browsed** on the first run, and there was no file-dialog dependency in the tree | `tauri-plugin-dialog`, Rust side only — no new JS permission. Browse for a folder, and "my shop's data is somewhere else — find it" |
| **"The shop's data could not be read"** was what a cashier saw when they pressed Kitchen ticket on a dine-in order with no table — the commonest mistake on the whole screen, reported as a data-file failure | `park_open_order` has the same guard `complete_bill` already had, in words that say what to do |
| **The token lint's escape hatch silently did not work.** `^\s*//` ate a blank line before a comment, so the stripped text was shorter than the file and `mb-tokens-allow` was read off the wrong line | `check-tokens.mjs` uses `[ \t]` |
| **The Groups dialog scrolled sideways and shredded the group names**, and the cart's new − + ✕ left 66 px of a 324 px row for the item name | both found by screenshotting the running window and measuring the DOM; both fixed, both explained in the CSS |

## What was sound

Every screen and every tab, on a fresh install: **no console errors, no
unhandled rejections, no stuck spinners, no error toasts** — before this round
and after it. The 197 wired commands answered. The gap was reach, not
correctness.
