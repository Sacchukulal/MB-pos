# Magic Bill POS — Reaudit, 27 August 2026

How this was done: every Rust and TypeScript file in MB-pos was read (about 150,000 lines), the
real app was run on this laptop against the test shop, 32 screens and dialogs were photographed,
and the shop's database was checked against what the screens said. Nothing was changed.

The rule for every fix below: **fix the root, delete the duplicate, never patch.**

---

## 1. The verdict, on one page

**The money core is good.** `mb-core` computes a bill in one fixed order, in whole paise, with
3,000 random bills proving the totals always tie. Tax classes, discounts, charges, rounding,
liquor outside GST — all correct and tested. Keep it.

**The layer around the core is where the mistakes are.** The old build wrapped that good core in
code that does the same job in several places, shows settings that nothing reads, resets timers
it should keep, leaves tickets and print jobs stuck forever, and puts popups and folds in the
cashier's way. The UI has the right structure (one grid, permanent cart, keyboard first) but the
details are not finished.

The twelve things that matter most, in order:

| # | Problem | Where |
|---|---|---|
| 1 | A table's timer resets to zero every time a kitchen ticket prints, and its business day can flip — the floor cannot tell a late table | `flows.rs` park_open_order, complete_bill_on |
| 2 | A settled order's business day ignores the shop's day-start setting | `billing.rs:694`, `ipc.rs:130` |
| 3 | Kitchen screen never clears: tickets for settled orders stay on the board forever (7 in the test shop) and every ticket ends as "ALREADY PRINTED ON PAPER" | settle path + `kitchen.rs` fallback |
| 4 | A print job left in "printing" survives restarts forever; the bar says "1 printing" for days while Health says "nothing is stuck" | `mb-print` queue store, `health.rs` |
| 5 | The phone's order total skips tax, charges, discount and rounding — waiter and cashier see different totals | `orders.rs:557` |
| 6 | The bill is computed in 4 places and the bill paper is built in 4 places | `billing.rs`, `flows.rs` |
| 7 | 9 settings are shown to the owner that nothing reads (order-type lock, ask-before-print, no-kitchen-ticket, language, backup schedule…) | `settings/catalog.rs`, `settings/mod.rs` |
| 8 | The theme is stored in two places and `index.html` is hard-coded light — a dark shop flashes light on every start | `ThemeProvider.tsx`, `index.html` |
| 9 | Order type and table are two separate fields, so an order can be "Parcel" AND on Table 1 (one is in the test shop now) | `billing.rs` CartState, `ipc.rs` |
| 10 | Every item add opens a quantity popup; the "more actions" fold hides the TOTAL | `Keys.tsx`, `Billing.tsx` |
| 11 | Every cart add, every table open, every ticket loads the whole menu / all tables / all open orders from disk | `state.rs:872`, `flows.rs`, `ipc.rs` |
| 12 | Updates have no server, payments have no real provider, sub-tables say "still to come" — three features that look finished and are not | `state.rs:144`, `provider.rs`, `Billing.tsx:531` |

---

## 2. Bugs in the money and order path (core fixes)

### 2.1 Re-parking an order overwrites when it was created
`park_open_order` and `complete_bill_on` (`flows.rs`) rebuild the order with
`state.to_draft(at = now, …)` and then do `open.core = draft.core.clone()`. `DraftOrder::new`
stamps `created_at` and `business_day` from that `at`. So every kitchen ticket (which parks the
order twice) sets `created_at` back to *now*.

- The floor tile's minutes and its amber/red state come from `created_at` → a table that ordered
  40 minutes ago and just added a tea shows "0m" and turns white again.
- After midnight the business day flips to tomorrow's, so the bill lands in the wrong day's
  report and takes tomorrow's number series.
- mb-core has a test "a settled order keeps its original time" — the app layer breaks it anyway.

**Root fix:** the cart must carry the order's `created_at` and `business_day` once it has an
order, and `to_draft` must copy them instead of inventing new ones. One function builds an
`OrderCore` from the cart; parking and settling both call it.

### 2.2 Two ways to work out "which business day"
`flows::today()` uses the shop's day-start setting. `CartState::to_draft` (`billing.rs:694`) and
`print_test_page` (`ipc.rs:130`) use `DayRule::default()` (midnight). A shop that closes at 2 am
gets its late bills on the wrong day.

**Root fix:** delete both `DayRule::default()` calls; only `flows::today()` may decide a day.
Same for the clock: `ipc.rs:305 now_millis()` duplicates `flows::now()` — delete it.

### 2.3 The kitchen board never clears
The database shows 7 kitchen deliveries still open for orders that were **settled** days ago,
and 3 for open orders from 24 August. Settling (`mb_db::settle`) and voiding never close the
order's kitchen tickets; only cancel does (`repos.kitchen().cancel_order`). The Kitchen screen
therefore shows "10 orders waiting" with cards at 5,548 minutes.

Second half: `print_kitchen_ticket_on` prints paper AND creates a kitchen-screen delivery. The
5-second fallback thread then finds it "undrawn" after 20 s and marks it printed. In a shop with
no kitchen screen (most shops) every ticket ends up on the board as "ALREADY PRINTED ON PAPER",
forever.

**Root fix:** (a) settle/void close the order's deliveries in the same transaction; (b) a
delivery is created only when the shop has a kitchen station configured (the `kitchen_ticket_off`
/ station settings decide), never as a side effect of paper; (c) the board is pushed, not polled
(see 5.2).

### 2.4 A print job can freeze the queue for ever
`print_jobs` row `job_1787594654354_0` has been in state `printing` since 24 Aug 18:04. The queue
store never resets an in-flight job at start-up and there is no per-job timeout (only a 3 s
connect timeout). The top bar says "1 printing" for 54 hours; `health.rs` reports "Nothing is
stuck in the print queue" because it only counts *parked* jobs.

**Root fix:** at start-up every `printing` row becomes `pending` (attempt + 1); a job gets a
hard deadline after which it is `failed`; Health counts anything older than a minute that is not
done.

### 2.5 The phone sees a different total
`orders.rs::view_of` sums `qty × unit price` — no tax, no charges, no discount, no rounding — and
sends that to the waiter's phone as "total". The counter shows the real total. Two totals for one
order is the kind of thing a customer notices.

**Root fix:** `view_of` calls the same one bill function as the counter (2.6).

### 2.6 The bill is computed in four places, the paper built in four places
`compute_bill(BillInput::new(...).with_order_type().with_rounding().with_charges())` is written
out in `CartState::bill` (`billing.rs:72`), `running_total` (`billing.rs:603`),
`print_open_bill_on` (`flows.rs:672`) and `bill_of` (`flows.rs:1061`). If one of them ever
differs (as `view_of` already does), the tile, the cart and the paper disagree.

`BillContext { … }` for the printer is built in `queue_bill_print`, `queue_bill_copy`,
`preview_order_on` and `bill_pdf_on` — four copies of the same 15 lines, so a preview can drift
from the real print (the exact bug found in round 10).

**Root fix:** one `bill_for(cart, order_type, config)` in the app, one `bill_paper(order, bill,
copy)` builder; every caller uses them. `default_printer` / `default_kitchen_printer` become one
function with a role argument.

### 2.7 Order type and table are two independent fields
`CartState` holds `order_type` and `table` separately. `cart_set_order_type` clears the cart's
table for non-dine-in, but `open_table_on` on a free table keeps whatever type is set — so with
the type lock on "Parcel" a cashier taps table 1 and gets a **parcel order on table 1**. The
test shop has exactly that row (`ord_mt7jpax1…`, type parcel, table tbl_…_1). The floor shows it
under AC 1, the cart shows "Parcel" lit.

**Root fix:** one enum — `DineIn { table }`, `Parcel`, `SelfService`, `Delivery` — in mb-core,
so a table without dine-in cannot be expressed. `require_a_table` disappears with it.

### 2.8 Smaller bugs in the same area
- `move_order_on` (`floor.rs:579`) updates the cart's `table` but not `table_label`, so the cart
  keeps showing the old table name.
- `merge_orders` needs the **void** permission (`guard.rs:173`), and the absorbed order is saved
  as *cancelled* with reason "merged into …" — so every merge counts as a cancellation in the
  day's totals and the control report.
- `floor_on` (`floor.rs:148`) takes "today" from the first open order's day, not from the clock.
- `open_table_on` (`ipc.rs:1316`) finds an order with no table by pretending the order id is a
  table id.
- `print_kitchen_ticket_on` is four to five transactions (park, park again, kitchen send, event)
  instead of one — a power cut between them leaves a half-told kitchen.
- `complete_bill` in the UI first calls `cart_add_payment` for the balance, then
  `complete_bill` — two round trips using a stale copy of the cart.

---

## 3. One job, many doers (duplicates to collapse)

| What | Copies | Keep |
|---|---|---|
| Building a bill from a cart | 4 (see 2.6) + phone's own sum | one `bill_for` |
| Building the bill paper | 4 | one builder |
| "Which printer" for bills / kitchen | 2 near-identical fns | one with a role |
| "What is this table called" | `table_label`, `label_for`, `first_table_label`, `place_of` (kitchen), each listing all tables | one lookup by id |
| Finding a menu item | `App::find_menu_item` and `orders.rs:284` both load the whole menu; `repo.find_item` already exists and is unused by them | `find_item` |
| Business day | `flows::today` vs `DayRule::default()` ×2 | `flows::today` |
| Clock | `flows::now` vs `ipc::now_millis` | `flows::now` |
| Theme | `AppConfig.theme` (Rust, written, never read) vs `localStorage` (TS, read) | one |
| Order-type lock | setting `billing.lock_order_type` (unread) vs `Billing.tsx` local state (forgets on every screen change) | the setting |
| Table tiles | Billing grid and Floor draw the same tiles with different click behaviour | one behaviour |
| "Which orders are open" | Billing "Processing orders" panel and the "No table" tile group show the same orders | one |
| First-run role | `'role_owner'` typed in `FirstRun.tsx:180` must match a Rust preset | Rust decides |

---

## 4. Things that look finished and are not

### 4.1 Settings the owner can change that nothing reads
All visible on Settings → Billing / Appearance / Backup today:

- `billing.lock_order_type`, `billing.locked_order_type` — "Always start on one order type"
- `billing.confirm_before_kitchen`, `billing.confirm_before_bill` — "Ask before printing…"
- `billing.kitchen_ticket_off` — "This shop has no kitchen ticket"
- `appearance.language` (the language feature was removed)
- `backup.every_hours`, `backup.keep_count`, `backup.send_crash_reports`

**Fix:** either wire each one to the code that must obey it, or delete the row from the catalogue.
A setting with no reader must fail the build (extend `settings/tests.rs` to grep for each field).

### 4.2 Stubs
- **Updates**: `App::releases()` returns `NoReleaseServerYet` (`state.rs:144`). The whole
  update/rollback screen can never find an update.
- **Payments**: the only provider is `Manual` (`provider.rs`). "Card" and "UPI" just record what
  the cashier typed. No UPI QR verification, no card machine. (Owner decision X17 pending.)
- **Sub-tables (6A / 6B)**: choosing a letter shows a toast "needs the settle flow — still to
  come" (`Billing.tsx:531`). The choice is offered and does nothing.
- **Short codes typed by hand**: `search.rs` ranks item *names* only. A code typed in the box is
  found only if the scanner heuristic (`devices.rs scanned`) decides it was a scan.
- **Cloud**: `Pushed::Sync` and the `sync_outbox` table exist; whether bills actually reach the
  cloud was not verified in this audit (see §10).

### 4.3 Dev-only things that ship in the UI code
`seed_demo_shop` (debug builds only, fine) and the "Kit" gallery screen (dev only, fine) — but
the billing screen's empty state offers "Add a demo shop" to a real owner in a debug build.

---

## 5. Speed and lag

### 5.1 Whole-table reads on the hot path
- `App::find_menu_item` (`state.rs:872`) loads **every** menu item to find one — called on every
  cart add and every phone add. `MenuRepo::find_item` exists (`menu.rs:242`) and is unused here.
- `open_table_on` loads all tables and all open orders (with their carts) to open one.
- `queue_kitchen_lines` loads the printer list, the category→printer map and the whole menu per
  ticket, and `routed_printer` runs a query per line.
- `table_label` lists all tables every time a name is needed (paper, cart, kitchen).
- `list_bills_on` runs two queries per bill row (reprints, refunds) — N+1.
- `dashboard_on` runs about nine separate transactions and calls three other modules.

On a 2,000-item menu with an HDD these are the difference between "instant" and "sticky".

### 5.2 Polling instead of pushing
- The billing floor re-reads `open_orders` every 15 s (`clock.ts`, `Billing.tsx:197`) — every
  refresh recomputes a bill per open order.
- The Kitchen screen polls every 15 s while the paper fallback fires at 20 s
  (`kitchen_delivery.rs:19`) — 5 seconds of margin, and one slow poll sends paper.
- `kitchen_shown` is sent per new ticket on every poll.
- The shell re-reads `setup_list` on every screen change.
- `Billing` asks `device_manager` on mount even for a cashier who may not open Devices.

**Fix:** Rust already has a push channel; use it for floor changes, kitchen tickets and the
queue, and keep the tick only for timers.

### 5.3 Rules with no guard
`main.rs:5` has `#![allow(dead_code)]` for the whole crate, so dead functions never warn
(see §8). `unwrap_or(Money::ZERO)` and friends appear 34 times in the app layer — a money add
that overflows is silently zero on a report.

---

## 6. The UI, screen by screen (from running it)

### 6.1 Everywhere
- **Popups.** Adding an item opens a quantity dialog every time (`Keys.tsx` QuantityPopup). Clearing
  an unsaved cart asks "Cancel this order? … cannot be undone" (`Billing.tsx:1111`). Floor
  move/merge, edit table, every correction, tax class edit, categories, bulk prices, import — all
  modals. Modals can stack on each other (preview opened over the discount dialog).
- **Explaining on screen.** Devices banner ("Every one of these is optional…"), Buying ("GST on
  what you buy can be claimed back…"), Spends drawer formula strip, Stock/Buying/Delivery empty
  states, Categories dialog paragraph, Staff "Nobody is ever deleted…", Bills header note, Bulk
  prices paragraph. The owner's rule: hover tips, not paragraphs.
- **Theme.** Stored twice (§3). `index.html` says `data-theme="light"` so a dark shop flashes
  light on start. Accent and semantic colours are fine and token-driven; no raw colours found.
- **Layout at the minimum window size (1024).** The top bar's "More" collides with the printer
  and bell icons, and the cart's right edge clips (screenshot 04).
- **Custom title bar.** Minimise / maximise / close are drawn by the app; the OS snap layouts and
  double-click-to-maximise on the bar do not work.
- **Words.** "Read these settings again", "Write these settings out", "Look again" ×2 on Health,
  "Put this section back to standard" — developer sentences on owner buttons.
- **Raw ids on screen.** History shows `staff_msvyqof2` in "Which one" (`ipc.rs:2237`).

### 6.2 Billing (the screen that matters)
- The cart region: with "more actions" open the totals block (Subtotal/Tax/TOTAL) disappears and
  the payment buttons overlap the cart lines (screenshot 24). A cashier must never lose the total.
- Totals show 0.00 / 0.00 / 0.00 on an empty cart — noise.
- "Processing orders 3" dropdown duplicates the "No table" tile group below it.
- Order-type segment and the lock: the lock is a local toggle that forgets itself when you leave
  the screen; the setting that should hold it is unread (§4.1).
- Every add = name, Enter, Enter. Two keystrokes plus a dialog for the most frequent action in
  the product. Add 1 directly; quantity is edited inline on the line (the line already has − qty +).
- "Each pays" (even split) sits in the fold with Preview / Reprint / Discount / Separate bill /
  Cancel — seven quiet buttons in two columns. Split, discount and cancel are different jobs from
  preview and reprint.
- After settling, the floor is re-read and the cart cleared, but the "Cash given" answer text
  ("Change …") stays until the next keystroke.
- Item search matches names only (§4.2).

### 6.3 Floor
- For an owner (who may arrange tables) pressing a busy table does nothing; you must tick it and
  then press "Move or merge". For a cashier, pressing opens the modal. Two behaviours for one tile.
- The second timer ("food 54h") is truncated to "foo…" on every busy tile.
- The "+" arrange button has no label.
- Rooms, tables and timers live in a side fold on this screen; table *editing* is also here
  (pencil on hover) — while the setup list sends people to Settings for printers and to Floor
  for tables. Decide where the shop is configured.

### 6.4 Kitchen
- Board full of dead cards (§2.3), every one "ALREADY PRINTED ON PAPER".
- Table 1 appears three times with tokens #1, #1, #2 — one order, three firings, no grouping.
- "Done (1)…(9)" numbers are keyboard shortcuts printed on the buttons.
- No station picker until a category names a station; a shop cannot see how to get a second screen.

### 6.5 Menu
- Three buttons on every row (Edit · Sizes & choices · Sold out) and a row height that fits 8 items
  at 1366×768. A 300-item menu is 40 screens.
- Export copies CSV to the clipboard; Import is a paste box. Settings import uses a file picker.
  One way.
- Tax classes, modifier groups and combos are all appended under the item table on one long
  scroll.

### 6.6 Settings
- Good shape (catalogue-driven, live paper preview, search). But: five dead controls on Billing
  (§4.1), "Appearance" with a dead language row, backup schedule fields that do nothing.
- Shop name on Settings ("Laptop Test") and on Account ("Anna's Kitchen") differ — licence name
  vs store name, never reconciled.

### 6.7 Bills, Reports, Day close
- Bills: three large stat cards for 0.00 above an empty list.
- Reports: the 27 reports are one generic table — fine. Dashboard is slow to build (§5.1).
- Day close: `count_cash` round-trips to Rust on every keystroke of the denomination grid; fine
  on SSD, sticky on the reference HDD.

### 6.8 Staff, History
- Staff has six tabs (People, Attendance, Leave, Salary, Payroll, Roles). Payroll in a POS is a
  lot of surface; every tab is a separate data load.
- History: the "When" column wraps to four lines, "Which one" shows raw ids, the Change column
  overflows with strike-through text (screenshot 12).

### 6.9 Credit, Spends, Stock, Buying, Delivery, Devices, Health, Account
- Mostly clean tables and empty states. Health has two "Look again" buttons and disagrees with
  the print queue (§2.4). Devices opens with an explanatory banner. Account shows a grace period
  for a trial that ended — the licence flow works; wording is fine.

### 6.10 First run and lock
- First run: shop file → details → PIN → items. No printer step, no tables step; items are
  created without a category. The setup list then nags for them. Put printer and tables in the
  wizard, or drop the wizard and keep only the list.
- Lock screen: good. Recovery flow: good.

---

## 7. Missing or next-required features (what a paying restaurant will ask first)

1. **Real UPI** — a QR on the bill/customer display with payment confirmation (decision X17).
2. **Update server** — without it no shop can ever be updated (§4.2).
3. **Kitchen station setup** as a first-class settings page (today: type a station name on a
   category).
4. **Sub-tables** finished or removed from the keyboard flow.
5. **Open-order handling at day close** — the test shop has three open orders from 24 August; day
   close does not mention them, and the floor shows "58h".
6. **Short code typing** at the counter (§4.2).
7. **Customer on the bill** (name/phone for a dine-in bill, not only credit customers) — the
   paper has a `customer` slot that is always `None`.
8. **Bill number on the cart** once an order is parked (the tile has it, the cart does not).
9. **Refund modes** — refund always records `mode: 'cash'` (`Bills.tsx:239`).
10. **Real hardware tests** — printer only so far (owner decision X18).

---

## 8. Dead code and bloat (delete list)

- `delete:` `#![allow(dead_code)]` on the whole crate (`main.rs:5`). Remove it and delete what
  then warns.
- `delete:` `billing.rs` `Cart_` type alias, `pending_for_kitchen` wrapper, `TERMINAL` const.
- `delete:` `floor.rs::business_day_now`, `corrections.rs::stamp` (both kept alive by `allow`).
- `delete:` `ipc.rs::now_millis` → `flows::now`.
- `delete:` `settings` rows with no reader (§4.1) or wire them.
- `delete:` `AppConfig.theme` / `set_appearance` **or** the localStorage copy — one survives.
- `delete:` `look_demo.rs` (1,665 lines) if the design work it served is finished; otherwise move
  it to `examples/`.
- `yagni:` `Employment` payroll (2,512 lines Rust + 1,530 TS) — keep only if a shop asked for it;
  it is the largest module in the app and the least POS-like.
- `shrink:` the four `BillContext` builders → one; the four `compute_bill` call sites → one.
- `shrink:` `Shell.tsx` custom window buttons duplicated in `BareBar` and `TopBar`.

net: roughly −5,000 lines possible, −0 deps.

---

## 9. The fix plan, in order

**Phase A — the order is one thing (core).** Order type + table become one enum in mb-core;
the cart keeps `created_at`/`business_day` once it has an order; one `bill_for`, one paper
builder; `flows::today` is the only day; `find_item` replaces every full-menu scan. Tests:
re-parking keeps the time; parcel-on-table cannot compile; phone total == counter total.

**Phase B — nothing gets stuck.** Settle/void close kitchen deliveries; deliveries only when a
station exists; print queue resets in-flight jobs at start and has a job deadline; Health reads
the same queue state the bar shows; day close lists open orders.

**Phase C — push, don't poll.** Floor, kitchen and queue changes come over the existing
`mb://push` channel; the 15 s tick only moves clocks.

**Phase D — settings tell the truth.** Wire or delete the nine dead settings; a build-time test
that every catalogue key has a reader; one theme store; the order-type lock comes from the setting.

**Phase E — the counter flow.** Add without a popup; totals always visible; one actions row
grouped by job (pay / correct / paper); no confirm for an unsaved cart; "Processing orders"
merged into the grid; short-code typing; sub-tables finished or removed.

**Phase F — the rest of the screens.** Explaining text → tips; History columns; Menu density
and one import/export way; Floor tile has one behaviour; Health/queue wording; first-run steps.

**Phase G — the missing features** (§7), each as its own whole piece of work.

---

## 10. What was not checked

- The LAN phone flow end to end (needs a phone on the same WiFi).
- Cloud sync of bills and the licence server (needs the live Supabase project).
- Real printer output (the TVS printer is set up; no paper was printed during this audit).
- Employment/payroll arithmetic — read for structure only.
- Windows installer, update, rollback — no release server exists to test against.
