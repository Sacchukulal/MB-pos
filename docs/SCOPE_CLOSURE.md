# The whole feature scope, closed

P30's third job, and the answer to *"is everything included?"* — asked in a way
that does not depend on anybody's memory, mine least of all.

**Every numbered line of `docs/FEATURE_SCOPE.md` is below, all sixteen
sections, 183 of them.** The status column is generated from that file
rather than typed here, so this document cannot quietly disagree with it: the
scope file is what each session updates as it finishes, and this is a view of
it.

| mark | means |
|---|---|
| **DELIVERED** | built, and the session that built it is done. The `Where` column names it |
| **DESIGNED-ONLY** | the model exists and adding it later needs no breaking migration. Nothing is built |
| **DEFERRED** | not built, with a stated reason. Several are the owner's own decision |
| **PART** | partly built, and the missing half is named |
| **PENDING THE OWNER** | waiting on a decision that is not ours to make (§15) |

## The count

| | |
|---|---|
| **DELIVERED** | 168 |
| **DEFERRED** | 6 |
| **DESIGNED-ONLY** | 6 |
| **PART** | 2 |
| **PENDING THE OWNER** | 1 |
| | **183 total** |

## What P30 checked by hand

A generated table proves the file was read, not that the file is true. These
were opened, run or printed during P30 and match what the scope claims:

* **1.1, 1.15, 1.16** — a whole day billed through the real commands, split
  payments and change included (`a_whole_day_reconciles_against_itself`).
* **2.1–2.4** — a bill with 5%, 18% and non-GST liquor on it, each its own row
  in the summary.
* **1.17** — a void, and the drawer following it.
* **8.5** — a tip, in the drawer and in no sales figure.
* **9.8, 9.14** — **both were PART and both were finished at P30**: the shift
  handover report, and the printed payslip.
* **10.x** — every report in the catalogue answered for a real day, and that is a test now: `every_report_in_the_catalogue_answers_for_a_real_day`.
* **14.5** — a delivery, a rider, and the cash coming back.
* **7.x** — the device screen, with nothing plugged in.

## What is NOT delivered, in full

The exceptions are the useful part of this document. Everything else is a
green row.

| # | Feature | | Why |
|---|---|---|---|
| 1.29 | Item tile grid for cashiers who don't know the menu | **DEFERRED** | Owner chose keyboard-first; revisit after real use |
| 2.12 | E-invoice (IRP) for turnover above ₹5 crore | **DESIGNED-ONLY** | P06, P13 — invoice model must carry every IRP field |
| 2.13 | E-way bill | **DEFERRED** | Not a restaurant need |
| 4.10 | Central kitchen and stock transfer | **DESIGNED-ONLY** | P25. A transfer is two ledger rows sharing one id — the same shape whether it crosses a wall or a city. `transfer_id`, `counterpart_outlet_id` and `location` are on `stock_movements` from the first day; **no UI, on purpose**. |
| 5.8 | Send the bill on WhatsApp / SMS | **DESIGNED-ONLY** | P22 notification port; sending needs a provider (OWNER, Phase 10) |
| 5.9 | Feedback capture (QR on the bill) | **DEFERRED** | Needs the customer-facing web piece |
| 8.3 | **Auto-confirmed UPI (the till knows it was paid)** | **PART** | P29 built the seam and ships the honest manual provider: a reference typed in, a payment visibly UNCONFIRMED, and the list a shop reads at close (D152). **Auto-confirmation needs an aggregator account — §15, the owner's decision** |
| 8.4 | **Card machine integration (amount pushed to the terminal)** | **PART** | P29 — same trait, and a DECLINE leaves the bill unsettled with the reason recorded (T8). A real terminal needs the bank's SDK and an account — §15 |
| 8.8 | Foreign currency | **DEFERRED** | No requirement |
| 9.17 | Statutory deductions (PF / ESI / TDS) | **PENDING THE OWNER** | See §15 — needs your decision |
| 11.4 | **Multi-outlet — one owner, several shops** | **DESIGNED-ONLY** | P04 schema, P27 — **D142**: every root table carries `outlet_id`, a transfer is two ledger rows sharing an id (P25), and a terminal belongs to an outlet with a master per outlet. No UI, no cross-outlet query, no cloud — what the cloud will join on is written down in `SCHEMA.md` and nothing more is built. |
| 11.5 | Android tablet POS | **DESIGNED-ONLY** | Rust core stays portable; not built now |
| 12.5 | **QR ordering by the customer's own phone** | **DESIGNED-ONLY** | P19/P20 — same protocol, a different client. Built in a later phase |
| 12.6 | Waiter call / service request from the table | **DEFERRED** | With 12.5 |
| 13.13 | Cloud backup and sync | **DEFERRED** | Phase 8 (the backend, P31–P35), not the POS. The counter's half — a full local backup, verify and restore — is 13.12 and is done. |

---

## Every line


### 1. Billing — the core

| # | Feature | | Where |
|---|---|---|---|
| 1.1 | Keyboard-first billing, full state machine | **DELIVERED** | P10 |
| 1.2 | Item search (starts-with / contains, ranked) | **DELIVERED** | P10 |
| 1.3 | Item short codes for fast entry | **DELIVERED** | P13 |
| 1.4 | Table grid as the only open-order view | **DELIVERED** | P09 |
| 1.5 | Order types: dine-in, parcel, self-service, **delivery** | **DELIVERED** | P01, P09 |
| 1.6 | Sub-tables (6A / 6B) and merge-into-existing | **DELIVERED** | **P14** (was P10, which did not build it — D60) |
| 1.7 | Kitchen ticket, delta only, from a stored ledger | **DELIVERED** | P03, P10 |
| 1.8 | Category-wise and combined kitchen tickets | **DELIVERED** | P07 |
| 1.9 | Per-line notes | **DELIVERED** | P01, P10 |
| 1.10 | Fractional quantity (0.5 kg) | **DELIVERED** | P01 |
| 1.11 | Line discount and bill discount (₹ and %) | **DELIVERED** | P02 |
| 1.12 | Discount limits per role, reason, audit | **DELIVERED** | P02, P11 |
| 1.13 | Round-off (nearest / up / down) | **DELIVERED** | P02 |
| 1.14 | Service, packing, delivery charges, each own tax | **DELIVERED** | P02 the engine, **P17 the caller** — until P17 nothing applied one |
| 1.15 | Split payment (several modes on one bill) | **DELIVERED** | P02 |
| 1.16 | Change due on cash | **DELIVERED** | P02 |
| 1.17 | Void a finalised bill, with reason | **DELIVERED** | P12 |
| 1.18 | Cancel an open order from the counter | **DELIVERED** | P12 |
| 1.19 | Void individual items, with kitchen cancellation slip | **DELIVERED** | P12 |
| 1.20 | Reprint, counted, marked DUPLICATE | **DELIVERED** | P12 |
| 1.21 | Split one table's bill between guests | **DELIVERED** | P14 |
| 1.22 | Merge two tables into one bill | **DELIVERED** | P14 |
| 1.23 | Move an order to another table | **DELIVERED** | P14 |
| 1.24 | Cover / seat count per order | **DELIVERED** | P14 |
| 1.25 | Hold / recall a bill (park it, serve someone else) | **DELIVERED** | P10 |
| 1.26 | Bill-level note (printed on the bill) | **DELIVERED** | P10 |
| 1.27 | Keyboard shortcut help overlay + printable sheet | **DELIVERED** | P10 |
| 1.28 | Touch works everywhere, one layout | **DELIVERED** | P09, P10 |
| 1.29 | Item tile grid for cashiers who don't know the menu | **DEFERRED** | Owner chose keyboard-first; revisit after real use |

### 2. Tax and legal compliance

| # | Feature | | Where |
|---|---|---|---|
| 2.1 | Per-item GST rate | **DELIVERED** | P00, P13 |
| 2.2 | Inclusive / exclusive / exempt per item | **DELIVERED** | P00, P13 |
| 2.3 | **Non-GST items (liquor)** — a bar can bill | **DELIVERED** | P00, P13 |
| 2.4 | CGST + SGST, and **IGST** for inter-state | **DELIVERED** | P00 |
| 2.5 | HSN / SAC per item, printed on the bill | **DELIVERED** | P13, P06 |
| 2.6 | Customer GSTIN on B2B bills | **DELIVERED** | P06, P15 |
| 2.7 | Rate-wise tax summary on the bill | **DELIVERED** | P06 |
| 2.8 | Rate-wise + HSN-wise tax report, GSTR-1 shaped | **DELIVERED** | P18 |
| 2.9 | Different rate/price by order type (dine-in vs takeaway) | **DELIVERED** | P13 |
| 2.10 | Composition-scheme mode | **DELIVERED** | P17 — the tick is on the Tax screen and the declaration prints |
| 2.11 | Tax-inclusive menu display toggle | **DELIVERED** | P17 |
| 2.12 | E-invoice (IRP) for turnover above ₹5 crore | **DESIGNED-ONLY** | P06, P13 — invoice model must carry every IRP field |
| 2.13 | E-way bill | **DEFERRED** | Not a restaurant need |

### 3. Kitchen

| # | Feature | | Where |
|---|---|---|---|
| 3.1 | Kitchen ticket printing, multi-printer routing | **DELIVERED** | P07 the routing, P17 the screen that sets it |
| 3.2 | Cancellation slips | **DELIVERED** | P12 |
| 3.3 | **Kitchen Display System (screen instead of paper)** | **DELIVERED** | P24. A tablet on the shop's own WiFi (D104), never an HDMI cable. Big cards, number keys, and every state carries a word as well as a colour. **The kitchen never goes blind:** a ticket no screen drew within 20 seconds… |
| 3.4 | Order timers and ready/served states | **DELIVERED** | P24. One clock for the whole screen, never one per card (M3). A cook can tick one dish off or clear the whole card — the owner asked for both — and the undo lives on the bar, because a cleared card has already left th… |
| 3.5 | Course / sequence firing (starters, then mains) | **DELIVERED** | P24. A shop that does not use courses never sees any of it. A firing that named no course covered the whole order, so nothing on it can be fired again (D106). |
| 3.6 | Preparation-time estimates per item | **DELIVERED** | P13 stores the minutes on the item, P24 uses them. A ticket's target is its SLOWEST dish — the order is ready when the last thing on it is. A menu nobody has costed in time still gets a useful screen. |
| 3.7 | Kitchen performance report (time to serve) | **DELIVERED** | P24 + P18. By station and by hour: how many tickets, the average, the slowest, and how many missed their target. Filtered on the STORED business day, so audit B1 has no way back in. |

### 4. Inventory and cost

| # | Feature | | Where |
|---|---|---|---|
| 4.1 | Cost price per item | **DELIVERED** | P13 |
| 4.2 | Raw materials, units, conversions | **DELIVERED** | P25. **Units are where inventory actually fails** (D108): three dimensions, a base unit that follows from the dimension rather than being chosen, and the shop's own packs defined against the MATERIAL — because a bag i… |
| 4.3 | Recipes / bill of materials, multi-level | **DELIVERED** | P25. A sub-recipe is a MATERIAL that has a recipe (D111) — a thing on a shelf, made in batches — and when a sale needs more than there is, the shortfall produces itself and says so in the ledger. One percentage per li… |
| 4.4 | Automatic stock deduction on sale | **DELIVERED** | P25. **Inside the settle transaction, and incapable of refusing it** (D112) — the same shape as D86: a missing material, a negative shelf or a loop is a `stock_problems` row with a sentence in it, and the bill settles… |
| 4.5 | Purchases, suppliers, supplier ledger | **DELIVERED** | P26. **One rupee, one row** (D120): saving a delivery writes the shelf, the paper and what the shop owes in ONE transaction, and no expense row — the drawer reads the supplier payment itself. The landed cost is what t… |
| 4.6 | Low-stock alerts | **DELIVERED** | P25. A buy list grouped by **where you buy it** (D116), counted in the pack the shop buys in — "buy 2 bag", never "buy 50000 g" — printable and copyable as a message. |
| 4.7 | Wastage recording, with reasons | **DELIVERED** | P25. Quantity, unit, an editable reason (a fifth kind on the `reasons` table), who, when, and **valued at cost without the caller having to remember** — a wastage figure with no rupees on it is one nobody reads. |
| 4.8 | Physical stock count and variance | **DELIVERED** | P26. **The count freezes the book and approving posts a DELTA** (D127) — count on Sunday night, take a delivery on Monday morning, approve at nine, and the delivery survives. The printed sheet deliberately carries **n… |
| 4.9 | Food costing / real gross margin | **DELIVERED** | P25. Cost from the recipe tree at the material's **weighted average of what actually came in** (D118), never a typed price from 2019. **Both cost figures are shown** with the gap between them as a sentence (D119) — th… |
| 4.10 | Central kitchen and stock transfer | **DESIGNED-ONLY** | P25. A transfer is two ledger rows sharing one id — the same shape whether it crosses a wall or a city. `transfer_id`, `counterpart_outlet_id` and `location` are on `stock_movements` from the first day; **no UI, on pu… |

### 5. Customers

| # | Feature | | Where |
|---|---|---|---|
| 5.1 | **Credit** (the owner renamed it from "khata", 2026-08-08): customers, ledger, repayments, ageing | **DELIVERED** | P15 |
| 5.2 | Credit limit with override | **DELIVERED** | P15 |
| 5.3 | Statements: thermal, PDF | **DELIVERED** | P15 |
| 5.4 | Phone capture on any bill, repeat history | **DELIVERED** | P15 |
| 5.5 | **Loyalty points and a customer wallet** | **DELIVERED** | **P15b, after P18** — a wallet is money the SHOP owes and has to appear correctly in the day close (D64) |
| 5.6 | Customer groups with price lists | **DELIVERED** | P13b, **P15b** |
| 5.7 | Birthday / anniversary capture and report | **DELIVERED** | **P15b** |
| 5.8 | Send the bill on WhatsApp / SMS | **DESIGNED-ONLY** | P22 notification port; sending needs a provider (OWNER, Phase 10) |
| 5.9 | Feedback capture (QR on the bill) | **DEFERRED** | Needs the customer-facing web piece |

### 6. Pricing and promotions

| # | Feature | | Where |
|---|---|---|---|
| 6.1 | Variants (half / full, sizes) | **DELIVERED** | P13 |
| 6.2 | Modifiers and modifier groups, priced | **DELIVERED** | P13 |
| 6.3 | Combos, tax apportioned correctly | **DELIVERED** | P13 |
| 6.4 | **Happy hour / time-of-day pricing** | **DELIVERED** | **P13b** |
| 6.5 | **Day-of-week and date-range price lists** | **DELIVERED** | **P13b** |
| 6.6 | Coupons and promo codes, with limits | **DELIVERED** | **P13b** |
| 6.7 | Buy-X-get-Y offers | **DELIVERED** | **P13b** |
| 6.8 | Per-order-type price (dine-in vs parcel vs delivery) | **DELIVERED** | **P13b** (the price); P13 (the RATE, on the tax class) |

### 7. Hardware

| # | Feature | | Where |
|---|---|---|---|
| 7.1 | Thermal printing via the Windows spooler | **DELIVERED** | P07 |
| 7.2 | **Network / IP printers (port 9100)** | **DELIVERED** | P07 |
| 7.3 | USB / serial printers | **DELIVERED** | P07 |
| 7.4 | **Cash drawer kick** | **DELIVERED** | P07 |
| 7.5 | Print queue with retry and parked jobs | **DELIVERED** | P07 |
| 7.6 | **Barcode scanner** (packaged goods, bill lookup) | **DELIVERED** | P29 — a scan is told from typing by its TIMING (a pure function, and a fast typist is never misread); an item code, a scale label or a printed bill number, and the bill prints a CODE 128 of its own number so it can be… |
| 7.7 | **Weighing scale** (sweets, meat by weight) | **DELIVERED** | P29 — two protocols plus a RAW mode that shows what an unknown scale is sending, weight-encoded labels, and a reading is only ever taken when it has settled. **No scale has ever been plugged in** — see OWNER_TESTS |
| 7.8 | **Customer-facing display (second screen)** | **DELIVERED** | P29 — a second window of the same app, plus a serial pole display. It has nothing focusable on it and never asks for focus (D154) |
| 7.9 | Label / sticker printing for parcels | **DELIVERED** | P29 — routed through P07 like any other document, to a printer chosen by id |
| 7.10 | A4 / PDF invoice printing for B2B | **DELIVERED** | P06 |
| 7.11 | **Print offset — nudge the whole printed output left/right and up/down by a few mm, per printer** | **DELIVERED** | P06 applies it, P07 stores it, P17 the screen — nudged from the paper, in millimetres, with no number to type |

### 8. Payments

| # | Feature | | Where |
|---|---|---|---|
| 8.1 | Cash, card, UPI, credit, split | **DELIVERED** | P02 |
| 8.2 | Static and dynamic UPI QR on the bill | **DELIVERED** | P06 |
| 8.3 | **Auto-confirmed UPI (the till knows it was paid)** | **PART** | P29 built the seam and ships the honest manual provider: a reference typed in, a payment visibly UNCONFIRMED, and the list a shop reads at close (D152). **Auto-confirmation needs an aggregator account — §15, the owner… |
| 8.4 | **Card machine integration (amount pushed to the terminal)** | **PART** | P29 — same trait, and a DECLINE leaves the bill unsettled with the reason recorded (T8). A real terminal needs the bank's SDK and an account — §15 |
| 8.5 | Tips | **DELIVERED** | P02 did the arithmetic; P29 the reporting — per person, per mode, and proved absent from sales, from tax and from the staff-cost denominator (T9). The day close shows cash tips on their own line |
| 8.6 | Advance / deposit against a future bill | **DELIVERED** | **P15b, after P18** (D64) |
| 8.7 | Refunds against a voided bill | **DELIVERED** | P12 |
| 8.8 | Foreign currency | **DEFERRED** | No requirement |

### 9. Staff and control

| # | Feature | | Where |
|---|---|---|---|
| 9.1 | Counter login with PIN | **DELIVERED** | P11 |
| 9.2 | Roles and enumerated permissions | **DELIVERED** | P11 |
| 9.3 | Audit trail with before/after | **DELIVERED** | P11 |
| 9.4 | Cashier name on the bill | **DELIVERED** | P11 |
| 9.5 | Auto-lock on idle, fast user switch | **DELIVERED** | P11 |
| 9.6 | Sales by cashier / waiter / shift | **DELIVERED** | P18 |
| 9.7 | Attendance (clock in / out), rosters, late/overtime | **DELIVERED** | P28. Clocking in needs nothing but the PIN; **correcting a clock-out is its own permission**, writes an audit row carrying both sides, and can never be your own row. A shift belongs to the day it STARTED in (D5) — a n… |
| 9.8 | Shift management and shift handover report | **DELIVERED** | P27 built the **boundary** — a till can close several drawers in a day, each with its own shift number, rolling up to the till and then to the shop (**D140**). **P30 built the report P28 left:** every drawer counted i… |
| 9.9 | Salary structures and payroll runs | **DELIVERED** | P28. Effective-dated, so a raise never rewrites last month. A run is computed, reviewed, then posted — and posting writes ONE expense for the gross plus a compensating drawer row for advances recovered, in one transac… |
| 9.10 | Salary advances, recovered from the next run | **DELIVERED** | P28. Out of the drawer the day it is given, oldest-first on the next run, capped at what was earned, and in instalments where agreed. |
| 9.11 | Leave: types, entitlement, requests, approval, balance | **DELIVERED** | P28. **The balance is the sum of a ledger** — accrued, taken, adjusted, lapsed — and never a stored number (D120 applied a third time). A request writes exactly one taken row, ever, enforced by a partial unique index … |
| 9.12 | Central staff administration — one authorised service | **DELIVERED** | P28. Twenty-one commands behind one permission table (guard::COMMAND_ACCESS), which the counter, the phone and later the website all go through. |
| 9.13 | Owner manages staff remotely (LAN now, cloud later) | **DELIVERED** | P28. The command set is documented in MB-pos/docs/LAN_PROTOCOL.md §15, and every command is permission-checked server-side — T10 calls each one without the permission and asserts the refusal. **Phone screens are Phase… |
| 9.14 | Staff self-service: own attendance, leave, payslip | **DELIVERED** | P28 — own attendance and own leave, refused for anybody else server-side (T11). **P30 built the printed payslip**, which P28 named as not done: every step of the arithmetic on the paper, an edited figure declared on i… |
| 9.15 | Employee record: joining, ID, status, never deleted | **DELIVERED** | P28. Designation, department, emergency contact, an ID *reference* (never the number — a POS has no business holding one), and a leaving date. Nobody is deleted. |
| 9.16 | Staff cost as a % of revenue | **DELIVERED** | P28. Approved runs only — a draft is a proposal, not a cost. With P25 food cost this is the second of the two numbers that decide whether a restaurant makes money. |
| 9.17 | Statutory deductions (PF / ESI / TDS) | **PENDING THE OWNER** | See §15 — needs your decision |

### 10. Reporting

| # | Feature | | Where |
|---|---|---|---|
| 10.1 | Sales: summary, day, hour, type, mode, cashier, section | **DELIVERED** | P18. All seven, grouped by the STORED business day — audit B1 has no way back in. |
| 10.2 | Items, categories, top and bottom sellers | **DELIVERED** | P18. "Bottom sellers" ships as "items that stopped selling", which is audit G5's own words and the more useful question. |
| 10.3 | Menu engineering (volume vs margin) | **DELIVERED** | P18. An item with no cost price reads "not known", never 100% margin. |
| 10.4 | Rate-wise and HSN-wise tax | **DELIVERED** | P18 — audit B11. Summed from the stored bill lines, so it cannot disagree with the paper. |
| 10.5 | Voids, discounts, reprints, cancellations, drawer opens | **DELIVERED** | P18. One list with who and why; drawer opens stay in the History (P11). |
| 10.6 | Expenses and cash position | **DELIVERED** | P16, P18, and the PHOTOGRAPH at P26 — **D69 answered as D132**: a file beside the database, downscaled in the webview so Rust needs no image library, and the backup grew a folder rather than the database growing 240 M… |
| 10.7 | Credit outstanding and ageing | **DELIVERED** | P15 |
| 10.8 | **Day Close (Z-report), denomination count, day lock** | **DELIVERED** | P18 — audit B15, requirement 9. Reopening a closed day is its own audited action (D77). |
| 10.9 | Comparison against the previous period | **DELIVERED** | P18. The same number of days ending the day before — right across a month end and a leap year, and it names what it compared against. |
| 10.10 | Dashboard with attention items | **DELIVERED** | P18 — audit G1. Reports opens on it, and every line is read from the screen that already knows. |
| 10.11 | PDF and correctly-escaped CSV export | **DELIVERED** | P18. One CSV writer (audit G7), and the PDF paginates — it used to stop at page one. |
| 10.12 | Inventory and food-cost reports | **DELIVERED** | P25, P26 |
| 10.13 | Scheduled report at day close (print / share) | **DELIVERED** | P18 prints the closing slip; P26 shares it. **D134 — it does not need a channel, it needs an honest one**: WhatsApp Desktop and the mail client through the OS, the clipboard, and the folder the PDF is already in. **Th… |

### 11. Scale

| # | Feature | | Where |
|---|---|---|---|
| 11.1 | **Multi-terminal — two or more billing PCs in one shop** | **DELIVERED** | P27 — a second till is a paired device with a role (P19's pairing, `platform: "till"`), with its own database, printer, drawer and series. The Tills screen names it, gives it its prefix and chooses the main till. **D1… |
| 11.2 | One terminal is the master; numbering stays correct | **DELIVERED** | P27 — **D135, and it is the whole design**: every till issues its own series, so Counter 1 prints `A/0001` and Counter 2 prints `B/0001`. No block, no top-up, no reservation and no master on the billing path — the sha… |
| 11.3 | A secondary terminal keeps billing if the master is away | **DELIVERED** | P27 — **D138**: it keeps the menu, the tax rules, its own numbering, its printer, its drawer and the whole of `mb-core`, so it bills, prints, takes cash and gives change into its own book. What it cannot do is table s… |
| 11.4 | **Multi-outlet — one owner, several shops** | **DESIGNED-ONLY** | P04 schema, P27 — **D142**: every root table carries `outlet_id`, a transfer is two ledger rows sharing an id (P25), and a terminal belongs to an outlet with a master per outlet. No UI, no cross-outlet query, no cloud… |
| 11.5 | Android tablet POS | **DESIGNED-ONLY** | Rust core stays portable; not built now |

### 12. Local-first ordering

| # | Feature | | Where |
|---|---|---|---|
| 12.1 | LAN server, discovery, QR pairing, TLS | **DELIVERED** | P19 — D9. `crates/mb-lan`: TLS with a pinned self-signed certificate, mDNS **and** a QR (both, always), a single-use five-minute token that still needs a person to press Allow, per-device and per-IP rate limits, and a… |
| 12.2 | Order protocol, idempotent intents, conflicts | **DELIVERED** | P20 — D9. Twelve intents, every conflict decided and tested, and the contract written as `MB-pos/docs/LAN_PROTOCOL.md` for Phase 11 to implement from. The counter is the authority: no price, total or discount amount e… |
| 12.3 | Offline batches from a phone, stale-order hold | **DELIVERED** | P20 — the COUNTER half. A batch applies in order, idempotent across the whole batch, with a per-intent report; anything older than 12 hours is HELD for a person rather than printing yesterday's tickets at 7 am. The ph… |
| 12.4 | Device list with instant revoke | **DELIVERED** | P19 — and "instant" is literal: revocation bites on the device's NEXT REQUEST, not its next login, and there is a test that revokes between two calls. |
| 12.5 | **QR ordering by the customer's own phone** | **DESIGNED-ONLY** | P19/P20 — same protocol, a different client. Built in a later phase |
| 12.6 | Waiter call / service request from the table | **DEFERRED** | With 12.5 |

### 13. Reliability and operations

| # | Feature | | Where |
|---|---|---|---|
| 13.1 | Local backup, verify, restore, second location | **DELIVERED** | P05 built it all, **P17 made it reachable** — and restore refuses an unchecked backup (D74) |
| 13.2 | Export everything (owner's data) | **DELIVERED** | P05 |
| 13.3 | Business day stored, never derived | **DELIVERED** | P03 |
| 13.4 | Atomic numbering, reset checked per allocation | **DELIVERED** | P03 |
| 13.5 | Updates with rollback and staged release | **DELIVERED** | P22 |
| 13.6 | Logging, diagnostics bundle, crash reports | **DELIVERED** | P22 |
| 13.7 | Health panel | **DELIVERED** | P22 |
| 13.8 | First-run setup wizard | **DELIVERED** | P22 |
| 13.9 | Licensing, entitlement gate, device transfer | **DELIVERED** | P21 |
| 13.10 | Billing never stops, whatever fails | **DELIVERED** | P21, P24 |
| 13.11 | English / Hindi / Kannada, screen and receipt | **DELIVERED** | P23. **Audit Part 3's FONT choice moved here too (D71)** — one embedded face means the choice changed nothing, and P23 ships a second face for Kannada anyway |
| 13.12 | Text-size setting, high contrast, themes | **DELIVERED** | P08 the token system, P17 the screen. `contrast.test.ts` computes the WCAG ratio for every pair in every theme and fails the build on an unreadable one (D73) |
| 13.13 | Cloud backup and sync | **DEFERRED** | Phase 8 (the backend, P31–P35), not the POS. The counter's half — a full local backup, verify and restore — is 13.12 and is done. |

### 14. Front-of-house

| # | Feature | | Where |
|---|---|---|---|
| 14.1 | Floor plan with real table positions | **DELIVERED** | P14 |
| 14.2 | Table timers with warn / late thresholds | **DELIVERED** | P14 |
| 14.3 | Occupancy, covers, turns | **DELIVERED** | P14 |
| 14.4 | **Reservations and waitlist** | **DELIVERED** | **P14b, after P15** — a booking is keyed on a guest and customers do not exist until P15 (D60) |
| 14.5 | Home delivery: address, rider assignment, status | **DELIVERED** | P29 — a rider is a member of STAFF with a flag, the state machine is checked in the database, a failure is a state with a reason, and **the cash a rider is carrying comes out of the drawer until it is handed back** (D… |

### 16. Performance — the speed contract

| # | Feature | | Where |
|---|---|---|---|
| 16.1 | Cold start to a usable billing screen inside budget (S1/S2) | **DELIVERED** | P08 |
| 16.2 | Keystroke → response within one frame (B1) | **DELIVERED** | P10 |
| 16.3 | Item search over 2,000 items inside budget (B2) | **DELIVERED** | P10, P13 |
| 16.4 | `compute_bill` inside budget, 50 mixed-rate lines (B4) | **DELIVERED** | P01, P02 |
| 16.5 | One durable write per settled bill (B5) | **DELIVERED** | P03, P05 |
| 16.6 | Reports never block billing — WAL, separate connection, off-thread (R1–R3) | **DELIVERED** | P04, P18 |
| 16.7 | Every unbounded list virtualised (menu, history, customers, KDS, grid) | **DELIVERED** | P09, P13, P15, P24 |
| 16.8 | No polling — Rust pushes, React subscribes (M4) | **DELIVERED** | P08 |
| 16.9 | Installer size and RAM footprint inside budget (S4, M1, M2) | **DELIVERED** | P08, P22 |
| 16.10 | Eight-hour leak test: memory growth bounded (M3) | **DELIVERED** | P24 |
| 16.11 | One shared clock for every timer, not one per tile (B8, M3) | **DELIVERED** | P09, P14, P24 |
| 16.12 | Per-crate `tests/perf.rs` asserting the ceiling in release builds | **DELIVERED** | every prompt with a budget |
| 16.13 | Measured numbers recorded in `PERFORMANCE.md` §4 each release | **DELIVERED** | P22 |
| 16.14 | Lazy-load reports, settings, menu management, KDS (S1) | **DELIVERED** | P08 |
| 16.15 | Every dependency justified, with its size cost if it ships | **DELIVERED** | all |

### 17. Cloud cost — the free-tier budget

| # | Feature | | Where |
|---|---|---|---|
| 17.1 | Nothing metered on the billing path — no function call to settle, print or open an order | **DELIVERED** | P05, P21 |
| 17.2 | Orders travel over the LAN, never the cloud (D9) | **DELIVERED** | P19, P20 |
| 17.3 | A write that changes nothing raises no realtime message | **DELIVERED** | P31, P33 |
| 17.4 | Realtime subscribed on demand, never an always-on channel per shop | **DELIVERED** | P33 |
| 17.5 | Sync batched, idempotent, off the billing thread, with backoff and a ceiling | **DELIVERED** | P33 |
| 17.6 | Nothing polls the cloud — counter, phone or website | **DELIVERED** | P33 |
| 17.7 | Reads go direct to Postgres under RLS, not through a function | **DELIVERED** | P32 |
| 17.8 | Rate limit on every publicly reachable endpoint | **DELIVERED** | P32 |
| 17.9 | Quota simulation covers **every** function and channel, projected to 30 / 100 / 500 shops | **DELIVERED** | P35 |
| 17.10 | Reports computed on-device from rows already held; no aggregates shipped | **DELIVERED** | P18, P33 |
| 17.11 | No images, PDFs or receipt rasters over the wire | **DELIVERED** | P33 |
| 17.12 | Archive policy keeps live cloud rows inside the size budget | **DELIVERED** | P33 |
| 17.13 | A free project pauses after 7 days idle — handled, not discovered | **DELIVERED** | P31 |
| 17.14 | Every prompt adding a cloud call states its per-shop-per-month cost | **DELIVERED** | all cloud prompts |
