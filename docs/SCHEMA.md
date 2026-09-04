# Magic Bill v2 — The Schema

The counter's SQLite file, table by table. Written at **P04**, 2026-08-03.

> **A test reads this file.** `crates/mb-db/tests/schema_rules.rs::t17` parses
> every `### table` heading and every `| column | type |` row and diffs them
> against the live database **in both directions**. A column in the database
> that is not here fails the build; a column here that is not in the database
> fails the build too.
>
> That is audit finding **E10** made impossible instead of discouraged. v1
> carried `bill_font_size`, `logo_opacity`, an unused `pin` column and KOT font
> settings nothing read, and the auditor's fix was *"start clean; do not carry
> any of it forward."* Starting clean is easy. Staying clean is this file.
>
> Keep the format simple enough for a twenty-line parser: one `### name`
> heading, one markdown table, first cell the column, second cell the type. The
> column list can be regenerated with
> `cargo test -p mb-db --test schema_rules -- --ignored --nocapture dump`; the
> prose stays hand-written, because a generated note says nothing and saying
> nothing is how dead columns survive review.

---

## The rules every table obeys

| | |
|---|---|
| **STRICT** | Every table. SQLite then enforces the declared type on every write — and refuses to create a table declaring `BOOLEAN`, `NUMERIC`, `DECIMAL` or `VARCHAR` at all. |
| **Three types** | TEXT, INTEGER, BLOB. Nothing else exists in this database. |
| **Money** | INTEGER paise (D2). There is no REAL column. v1 had nine, and every rupee it ever touched went through one. |
| **Quantity** | INTEGER thousandths, so 0.5 kg is `500`. |
| **Tax rate** | INTEGER basis points, so 5% is `500` and 2.5% is `250`. |
| **Instant** | INTEGER milliseconds since the Unix epoch, UTC. |
| **Business day** | INTEGER days since 1970-01-01, **stored, never derived** (D5). |
| **Id** | TEXT (D13). No `AUTOINCREMENT` anywhere: two terminals in one shop collide on integers and there is no repair. |
| **Boolean** | Name begins `is_` / `has_` / `was_` / `can_`; INTEGER, NOT NULL, `CHECK (col IN (0,1))`. |
| **Enum** | TEXT tag with a `CHECK` listing its values, spelled exactly as serde spells it, so the counter, the phone and the cloud agree. |
| **Outlet** | Every root table carries `outlet_id NOT NULL REFERENCES outlets(id)` (scope 11.4). Child tables reach it through their parent. |
| **Deletes** | Nothing in the money path cascades. A bill is never deleted — it is *voided*, which is a state, not an absence. The only cascades are pure join tables. |

The value-by-value encoding is in `crates/mb-db/src/encode.rs`, which is the
contract every later session reads.

---

## Reserved — tables a later prompt adds, and why they are not here yet

**Adding a table later is free.** SQLite writes one row in `sqlite_master`; no
existing data is read or moved, no shop is offline for it, nothing can fail
half way.

**Adding a column, or discovering a missing dimension, is not.** An
`ALTER TABLE ADD COLUMN` on 600,000 rows has to choose a value for every one of
them, and a dimension found late — an outlet, a terminal — has to be back-filled
across every table at once with a value nobody can verify.

So the rule this schema follows is: **model the dimensions and the columns now;
reserve the modules.** Each entry below names its owning prompt and the keys it
will take into the core, so that prompt *adds* tables and never *alters* one.

| Reserved for | Prompt | Foreign keys it will take into the core |
|---|---|---|
| Price lists, time-of-day and day-of-week rules, coupons, offers, customer groups | P13 | `items.id`, `categories.id`, `customers.id`, `outlets.id` |
| Loyalty points, wallet, advances and deposits | P15 | `customers.id`, `orders.id`, `outlets.id` |
| KDS stations, tickets, ticket states, bump times | P24 | `orders.id`, `order_lines.id`, `printers.id`, `outlets.id` |
| Units, unit conversions, materials, recipes, the append-only stock ledger, wastage | P25 | `items.id`, `order_lines.id`, `outlets.id` |
| Suppliers, purchases, purchase lines with input GST, supplier ledger, purchase orders, stock counts | P26 | `outlets.id`, and P25's materials |
| Shifts, attendance | P27 | `staff.id`, `terminals.id`, `outlets.id` |
| Salary structures, payroll runs, salary advances, leave types and requests | P28 | `staff.id`, `outlets.id` |
| Delivery zones, riders, saved addresses | P28 | `orders.id`, `customers.id`, `outlets.id` — the per-order columns are already on `orders` |
| Devices: scanners, scales, customer displays | P28 | `outlets.id`, `terminals.id` — the per-payment reference is already `payments.device_ref` |

An empty table nobody reads is a dead column with more punctuation. A
reservation costs a row in this table and buys the same protection.

---

## Views

#### `v_orders_readable`

Not a table, not covered by the T17 diff (which reads `###` headings only), and **nothing in the code depends on
it.** Timestamps are stored as INTEGER milliseconds, which is the right thing to
store and the wrong thing to read in a SQLite browser at 11 pm. This view
renders `business_day`, `created_at` and `settled_at` in IST beside the bill
number and the grand total, and costs nothing because it is computed.

---

## Tables

Forty-seven, including the migration ledger.

### schema_version

The migration ledger. Created by the engine itself, before any migration runs.

**One row per applied migration** — not a high-water mark. v1's whole ledger was
`schema_version(version INTEGER PRIMARY KEY)` read with
`COALESCE(MAX(version), 1)`, so a migration skipped in the middle was invisible
forever, and there was no checksum to notice a shipped migration being edited
after it ran.

| column | type | null | notes |
|---|---|---|---|
| version | INTEGER | no | Ascending, contiguous, never reused. |
| name | TEXT | no | The file, so a refusal names something findable. |
| checksum | TEXT | no | FNV-1a of the SQL with line endings normalised. |
| applied_at | INTEGER | no | Wall clock. The one place this crate reads the clock. |
| run_ms | INTEGER | no | A migration that takes four minutes on an HDD is something the next release needs to know. |

---

### outlets

Scope 11.4, multi-outlet. Seeded with exactly one row, `outlet_default`.

This is the dimension that cannot be retro-fitted, which is why it exists a
whole phase before anything reads it.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| name | TEXT | no | |
| is_active | INTEGER | no | |
| created_at | INTEGER | no | |

### store_profile

One row per outlet: everything a bill header and a GST return need. Read by P06
(the printed bill), P17 (settings) and P18 (tax reports).

| column | type | null | notes |
|---|---|---|---|
| outlet_id | TEXT | no | Primary key — one profile per outlet. |
| name | TEXT | no | |
| address | TEXT | no | |
| phone | TEXT | yes | |
| gstin | TEXT | yes | Absent for a shop below the threshold. |
| fssai | TEXT | yes | |
| state_code | TEXT | yes | Decides intra vs inter-state (scope 2.4). |
| upi_id | TEXT | yes | Scope 8.2, the QR on the bill. |
| upi_merchant_name | TEXT | yes | |
| upi_reference | TEXT | yes | Audit Part 3's third UPI field. Rides in the QR payload as `tn`. |
| registration | TEXT | no | **P33.** `unregistered` / `composition` / `regular`. Only a regular taxpayer may put GST on a bill; the boolean it replaced could not tell a shop below the threshold from a regular one. The shop-wide place of supply went with it — restaurant service is always intra-state (IGST Act s.12(4)), so that setting could only ever produce an illegal bill. |
| price_basis | TEXT | no | **0008 — the tax book.** `exclusive` / `inclusive`: whether a menu price already contains its tax, unless a slab or an item says otherwise. The bottom layer of the three (item → slab → shop) that `mb_core::TaxBook::spec_for` reads. |
| updated_at | INTEGER | no | |

### terminals

Scope 11.1 / 11.2, built at P27. Here now because `orders` and `counters`
reference it, and adding a terminal column to a populated orders table later is
exactly the migration this session exists to avoid.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| name | TEXT | no | |
| is_master | INTEGER | no | **D137** — the master owns the FLOOR. It does **not** own numbering; nothing does. Exactly one per outlet, and a person chooses it (D139). |
| last_seen_at | INTEGER | yes | 11.3 — how a shop can see that the tills are apart. |
| series_prefix | TEXT | no | **D135 — every terminal has its own series, and the series IS the terminal.** Till 1 issues `A/0001`, till 2 issues `B/0001`; the two share no value, so no partition, clock skew or restart can produce one number twice. It seeds BOTH of the till's counter rows, because tokens collide across tills exactly as bills do. Unique within the outlet — enforced by `idx_counters_prefix` where it actually prints. |
| master_since | INTEGER | yes | **D139.** A later stamp elsewhere wins, which is how an old master stands down when it comes back instead of arguing. |

**`block_start` and `block_end` were here at P04 and are DELETED at P27.** They
reserved the design P27's draft prompt asked for — a master handing out number
ranges — and that design has to answer *"what happens when a block runs out
mid-service"*. Both answers are a round trip to a machine that may be off, or a
till that stops taking money, and those are the two outcomes the session exists
to prevent. A partly-used block also leaves a permanent hole in the shop's bill
book, and every hole is a question at an audit. **A reserved column for a design
that turned out to be wrong is worse than no column, because the next session
reads it as an instruction.**
| created_at | INTEGER | no | |

### settings

**The E6 fix.** One setting is one row, written by name.

> *"Settings are saved as one giant command with 41 numbered slots. Adding one
> option means editing three lists that must stay perfectly aligned. This has
> already caused a 'reuse slot 39 for four columns' patch in the past. It is a
> silent-wrong-data machine."*

There is no positional UPDATE here and there never can be.

| column | type | null | notes |
|---|---|---|---|
| outlet_id | TEXT | no | Composite primary key with `key`. |
| key | TEXT | no | |
| value | TEXT | no | |
| value_type | TEXT | no | `int` / `bool` / `text` / `money` / `json` — so a reader can parse it without asking the writer. |
| updated_at | INTEGER | no | |
| updated_by | TEXT | yes | |

---

### tax_classes

**The fix for B10 / B11 / B14**, added at P13 — one finding from three sides:
v1 had one tax rate for the whole shop, so *"it could not bill a bar, an
AC/non-AC outlet or anyone selling packaged goods."*

A slab (the screen's word for a class) is a rate and a kind, with an optional
say on pricing. **It is the one place tax lives** (0008 — the tax book): an
item points at a slab and stores nothing else; a charge points at a slab; the
whole tax question for anything is answered by `mb_core::TaxBook::spec_for`,
which reads the item's say, then the slab's, then the shop's. **A slab never
reaches a bill** (D52) — the line froze its own copy when it was sold.

Six ship seeded — GST 0%, 5%, 18%, 40%, Exempt, and *Liquor — state VAT*,
which is not the same thing as *Exempt*: exempt is a nil-rated supply, liquor
is not a supply under GST at all and carries a state VAT rate instead, and a
return treats them differently. An owner adds, renames, re-rates and removes
slabs on Settings › Tax.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| name | TEXT | no | What an owner picks from a list. |
| rate_bp | INTEGER | no | Basis points. GST for `kind='gst'`, state VAT for `outside_gst`. |
| kind | TEXT | no | **P33.** `gst` / `exempt` / `outside_gst` / `untaxed` — what the supply is, in law. |
| basis | TEXT | yes | **P33 / 0008.** `exclusive` / `inclusive`, or NULL for "the shop decides". Liquor ships `inclusive` — a bar quotes the price paid whatever the shop does with food. |
| is_active | INTEGER | no | Retired while items still point at it; deleted outright once nothing does. |
| sort_order | INTEGER | no | |

---

### categories

The menu's top level. P13 manages it, P07 routes kitchen tickets by it.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| name | TEXT | no | |
| default_tax_class_id | TEXT | yes | A new item in this category starts here — an owner chooses "GST 5%" once, not four hundred times. Set on Settings › Tax. |
| colour | TEXT | yes | P14's grid uses it. A property of the category, not of the screen. |
| station | TEXT | yes | **P24 — which kitchen screen this category's food appears on.** NULL is the shop's one screen, the normal case, and it must stay invisible to a shop that never thinks about sections. One station per category, because paneer tikka goes to the tandoor and not also to the wok — so an owner adds, renames or removes a section by typing on the category they already edit, with no separate screen to learn. |
| sort_order | INTEGER | no | |
| is_active | INTEGER | no | |
| created_at | INTEGER | no | |
| updated_at | INTEGER | no | |

### items

The live menu. **An order never joins back to this table to print a line** —
`order_lines` carries a frozen snapshot instead.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| category_id | TEXT | yes | |
| name | TEXT | no | |
| unit_price | INTEGER | no | Paise. |
| tax_class_id | TEXT | no | **The slab the owner chose** (P13, made the only copy at 0008). The rate, the kind and the pricing default are read from the slab every time; nothing here can disagree with it. Past bills are untouched by a slab change, because a line froze its own copy (crown jewel 4, D52). |
| price_basis | TEXT | yes | **0008.** The item's own say: `exclusive` / `inclusive`, or NULL to follow the slab and then the shop. An MRP water bottle on the 18% slab is inclusive by law while the slab is not. |
| hsn | TEXT | yes | Scope 2.5. Absent for non-GST and below-threshold shops. |
| cost_price | INTEGER | yes | Scope 4.1. NULL, not zero — a shop that has not costed its menu must not be shown a 100% margin. |
| short_code | TEXT | yes | Scope 1.3 — typed at the counter instead of the name. |
| prep_minutes | INTEGER | yes | Scope 3.6 — the KDS target. A ticket's target is its slowest dish, because the order is ready when the last thing on it is. |
| course | TEXT | yes | **Scope 3.5 — which course this dish belongs to** (P24). NULL means "no course", and that is the default on purpose: when every dish is NULL the whole order fires at once, exactly as it does today, and a shop that does not serve in courses never discovers this exists. Free text rather than an enum — "starter" and "main" are the common pair, but a thali house has its own words. |
| is_open_price | INTEGER | no | Sold by weight; the cashier types the price. |
| is_available | INTEGER | no | |
| sort_order | INTEGER | no | |
| created_at | INTEGER | no | |
| updated_at | INTEGER | no | |

### item_variants

Scope 6.1 — half / full, sizes. Filled at P13.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| item_id | TEXT | no | |
| name | TEXT | no | |
| unit_price | INTEGER | no | |
| sort_order | INTEGER | no | |
| is_active | INTEGER | no | |

### modifier_groups

Scope 6.2. Filled at P13.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| name | TEXT | no | |
| min_select | INTEGER | no | |
| max_select | INTEGER | yes | NULL means no limit. |
| sort_order | INTEGER | no | |

### modifiers

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| group_id | TEXT | no | |
| name | TEXT | no | |
| price_delta | INTEGER | no | May be negative: "no cheese, minus ten rupees" is a real menu line. |
| sort_order | INTEGER | no | |
| is_active | INTEGER | no | |

### item_modifier_groups

A pure join. One of the two places a cascade is right, because the row has no
meaning without both parents.

| column | type | null | notes |
|---|---|---|---|
| item_id | TEXT | no | |
| group_id | TEXT | no | |
| sort_order | INTEGER | no | |

### combos

Scope 6.3. Filled at P13.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| name | TEXT | no | |
| unit_price | INTEGER | no | |
| is_active | INTEGER | no | |
| created_at | INTEGER | no | |
| updated_at | INTEGER | no | |

### combo_components

The tax share is on the component, not the combo, so the rate-wise summary
still adds up when the components differ in rate.

| column | type | null | notes |
|---|---|---|---|
| combo_id | TEXT | no | |
| item_id | TEXT | no | |
| qty | INTEGER | no | Thousandths. |
| share_bp | INTEGER | no | Basis points of the combo price attributed to this component. |

### printers

Scope 7.1 / 7.2 / 7.3 / 7.4. Filled at P07.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| name | TEXT | no | |
| kind | TEXT | no | `spooler` / `network` / `serial` / `none`. |
| address | TEXT | yes | A Windows printer name, an `ip:port`, or a COM port. |
| paper_mm | INTEGER | no | |
| is_default | INTEGER | no | |
| can_kick_drawer | INTEGER | no | Scope 7.4. |
| offset_x_mm | INTEGER | no | Scope 7.11. Whole millimetres, signed. Thermal printers disagree about where the first dot sits relative to the paper edge, so a document whose columns add up to exactly the paper width can still print 2–3 mm off-centre. P06 applies it once at the layout boundary so all three renderers inherit it; P07 makes it adjustable from the test print; P17 puts it on a screen. Clamped in code, not by a CHECK, because the sane range depends on `paper_mm`. |
| offset_y_mm | INTEGER | no | Scope 7.11, the vertical half — how far down the first line starts. |
| role | TEXT | no | `bill` / `kitchen` / `both`. Added at P07. What this printer may receive; routing that ignores it is how a customer's bill ends up in the tandoor. |
| engine | TEXT | no | `raster` / `text`. Added at P07 — v1's *"Print engine: Graphics (prints exactly like the preview, as a picture) or Text (the printer's own font, faster)"* (audit Part 3), kept because a shop that has chosen one should not have it chosen again for them. |
| is_bold_dark | INTEGER | no | Added at P07 — v1's "Bold & Dark": emphasis and double-strike on everything, for a head or a roll that has gone pale. |

**Those three columns went into migration 0001 rather than into a 0002**, for
the same reason the offset did at P04: `printers` is empty on every disk that
exists, because there are no customers (D11). That was the last cheap moment.
Once a shop has a printer row, each of these is an `ALTER TABLE` on a live disk,
which is exactly the cost D22 exists to avoid.

### category_printers

Scope 3.1 — multi-printer kitchen routing. A pure join, so it cascades.

| column | type | null | notes |
|---|---|---|---|
| category_id | TEXT | no | |
| printer_id | TEXT | no | |

### print_jobs

Scope 7.5, built at P07. **The fix for audit D4** — *"a failed print is only a
red message on screen; nothing remembers it, and in a rush the cashier misses
the toast and the kitchen simply never gets the order."* A job is written here
before the caller is let go, so a power cut in the middle of a rush loses no
kitchen ticket.

> **This is a spool, not a log — decision D35, and it is a budget.** A finished
> job's row is *deleted*. `payload` is a whole document as JSON, two to four
> kilobytes; a bill and its ticket are two rows; 75,000 bills a year would be
> roughly 450 MB against M5's 400 MB for the entire database, of which P04 has
> already spent 318. Keeping print history here would cost more than every bill,
> line, payment and tax row in the shop put together.
>
> "Was this bill's ticket printed?" is answered by `order_events` or by
> `reprints`, both of which already exist for it. In a healthy shop this table
> holds between nought and three rows.

No index: a table that holds three rows is scanned faster than an index is
consulted, and §Indexes' rule is that one nobody named does not exist.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| printer_id | TEXT | no | |
| kind | TEXT | no | `bill` / `kitchen` / `label` / `test` / `drawer` / `day_close` / `delivery`. **P30 added the last two, and finding them missing is the point of the row:** `day_close` has existed since P18 and `delivery` since P29, so every Z-report and every rider slip was refused by this CHECK with the day still closing normally. `every_job_kind_the_queue_can_make_is_allowed_by_the_schema` walks the enum against this list so they cannot part company again. |
| state | TEXT | no | `pending` / `printing` / `failed` / `parked`. There is no `done`: a done job has no row. |
| copies | INTEGER | no | |
| priority | INTEGER | no | Lower is sooner. A bill queued behind forty kitchen tickets is a customer standing at the counter. |
| attempts | INTEGER | no | Bounded — five, then parked. v1 had both failure modes in different places: retrying for ever, and dropping silently. |
| payload | TEXT | no | The `mb-print` document, as JSON. **Opaque to this crate** (D32): storage stores, printing prints. |
| reason | TEXT | yes | Why this was printed, for the queue the cashier looks at: "table 6", "reprint by Ravi". |
| last_error | TEXT | yes | |
| engine_used | TEXT | yes | `raster` / `text`, once it has been tried. A shop whose receipt suddenly looks different gets an answer instead of making a support call. |
| business_day | INTEGER | no | D5 — stamped by whoever created the job, never re-derived. |
| created_at | INTEGER | no | |
| updated_at | INTEGER | no | |

**And it is the one table with no outbox row.** A print job is local to one
counter, worthless to a phone or to the cloud, and syncing one would spend real
egress from D16's 10 MB monthly budget on a row nobody will ever read. Said here
as well as in the code, because "make it consistent with every other repository"
is a very reasonable-looking mistake.

---

### sections

The floor's top level. P14 owns it; P09's grid groups by it.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| name | TEXT | no | |
| sort_order | INTEGER | no | |
| is_active | INTEGER | no | |

### dining_tables

Named `dining_tables`, not `tables`: `FROM tables` reads like a mistake in every
query anyone will ever write against this file.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| section_id | TEXT | yes | |
| label | TEXT | no | What the waiter says: "6", "AC 1". |
| seats | INTEGER | no | |
| pos_x | INTEGER | yes | Scope 14.1, the floor plan. NULL until placed. |
| pos_y | INTEGER | yes | |
| sort_order | INTEGER | no | |
| is_active | INTEGER | no | |

### reservations

Scope 14.4. Filled at P14.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| table_id | TEXT | yes | |
| customer_id | TEXT | yes | Unconstrained on purpose: a walk-in booking is not a customer record. |
| guest_name | TEXT | no | |
| guest_phone | TEXT | yes | |
| covers | INTEGER | no | |
| business_day | INTEGER | no | D5 — a booking for 00:30 belongs to the evening it was made for. |
| expected_at | INTEGER | no | |
| state | TEXT | no | `booked` / `seated` / `no_show` / `cancelled`. |
| note | TEXT | yes | |
| created_at | INTEGER | no | |
| created_by | TEXT | yes | |

### waitlist

Scope 14.4. Filled at P14.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| guest_name | TEXT | no | |
| guest_phone | TEXT | yes | |
| covers | INTEGER | no | |
| business_day | INTEGER | no | |
| joined_at | INTEGER | no | |
| seated_at | INTEGER | yes | |
| left_at | INTEGER | yes | |
| state | TEXT | no | `waiting` / `seated` / `left`. |

---

### orders

One row per order in **every** state. `state` is `AnyOrder`'s serde
discriminator, spelled identically, so the counter, the phone and the cloud
agree on what a cancelled order is called without a translation layer.

`business_day` is stored (D5) and nothing re-derives it. Audit B1: v1 stored
UTC, filtered by local time and grouped reports by the UTC date, so a bill at
00:15 landed on two different days in two different screens and *"your totals
will not tie."*

Table-level CHECKs on this table carry rules the type system carries in
mb-core: a draft has no numbers, everything past draft has both; a cancelled or
voided order has a non-blank reason; a dine-in order past draft has a table.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| terminal_id | TEXT | no | Scope 11.1. |
| state | TEXT | no | `draft` / `open` / `settled` / `cancelled` / `voided`. |
| business_day | INTEGER | no | **Stamped once, at creation** (D5). |
| created_at | INTEGER | no | |
| created_by | TEXT | no | |
| order_type | TEXT | no | Scope 1.5, including `delivery`. |
| table_id | TEXT | yes | Required past draft for dine-in (audit 2.3). |
| sub_table | TEXT | yes | Scope 1.6 — the 6A / 6B letter. |
| covers | INTEGER | yes | Scope 1.24. |
| customer_id | TEXT | yes | |
| note | TEXT | yes | Scope 1.26. |
| token_value | INTEGER | yes | |
| token_formatted | TEXT | yes | Stored, not formatted on demand: a **printed** number must not change when the prefix setting does. |
| bill_number_value | INTEGER | yes | Unique per outlet **and till**, forever — see `idx_orders_bill_number`. What the customer holds is `bill_number_formatted`, which carries the till's prefix, so the value alone was never the identity. |
| bill_number_formatted | TEXT | yes | Same reason as the token. |
| settled_at | INTEGER | yes | |
| settled_by | TEXT | yes | |
| cancelled_at | INTEGER | yes | |
| cancelled_by | TEXT | yes | |
| cancel_reason | TEXT | yes | Audit B6. Compulsory when cancelled, and non-blank. |
| voided_at | INTEGER | yes | |
| voided_by | TEXT | yes | |
| void_reason | TEXT | yes | Audit B5. Compulsory when voided, and non-blank. |
| merged_into | TEXT | yes | Scope 1.22, P14. Where this order's food went when two tables became one bill. The order is `cancelled` — its food was sold on the other bill — and this column is what tells a report a merge from a walkout. |
| external_order_id | TEXT | yes | Scope X1, **PENDING**. Three nullable columns so an aggregator order can become one later without a migration. |
| channel | TEXT | yes | Scope X1. |
| commission_bp | INTEGER | yes | Scope X1. |
| delivery_address | TEXT | yes | Scope 14.5, built at P29. |
| delivery_rider | TEXT | yes | Scope 14.5. |
| delivery_state | TEXT | yes | `pending` / `assigned` / `out` / `delivered` / `failed`. P29 checks the STEP as well as the value: forward only, and anything on the road may fail. |
| delivery_failure | TEXT | yes | P29. Why it did not arrive. A CHECK makes it compulsory on `failed` and refuses it everywhere else — the same shape as `cancel_reason` and `void_reason` above, because a delivery that did not arrive is a STATE with a reason (D47) and not a row somebody deletes. |

### order_lines

The line **as typed**. The snapshot columns are crown jewel 4, the one thing
the audit says v1 got right:

> *"Each order stores its items as a frozen snapshot (name, price, quantity at
> that moment). If you rename an item or change its price tomorrow, old bills
> do not change. This is correct and legally safer."*

`item_id` is therefore never read to print a line. It stays as a plain,
non-cascading reference for reporting — and because it does, **an item that has
ever been billed cannot be deleted.** `ON DELETE SET NULL` would quietly turn
last Diwali's best seller into an unattributed row in item-wise sales (10.2), so
a sold item is history in the same way a staff member who has left is (9.15).
P13 removes an item from the menu with `is_available`, not with a DELETE. An
item that was typed in by mistake and never sold deletes normally — the
constraint bites on history, not on housekeeping.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| order_id | TEXT | no | |
| seq | INTEGER | no | The sequence the waiter called items in. The kitchen ticket reads in it and the cart rebuilds in it. |
| item_id | TEXT | yes | A reference for reporting only. Never read to print a line. |
| variant_id | TEXT | yes | Scope 6.1. |
| name | TEXT | no | Snapshot. |
| unit_price | INTEGER | no | Snapshot, paise. |
| tax_rate_bp | INTEGER | no | Snapshot. |
| tax_kind | TEXT | no | Snapshot. |
| tax_basis | TEXT | no | Snapshot. |
| hsn | TEXT | yes | Snapshot. |
| category_id | TEXT | yes | Snapshot, unconstrained — a deleted category must not orphan a bill. |
| qty | INTEGER | no | Thousandths (scope 1.10, 0.5 kg is 500). |
| note | TEXT | yes | Scope 1.9. Part of the kitchen line identity. |
| course | TEXT | yes | Scope 3.5, and part of the snapshot. NULL means "with everything else", which is every line in a shop that does not use courses. |
| prep_minutes | INTEGER | yes | Scope 3.6, and part of the snapshot. How long this dish takes; NULL means no target and the screen falls back to one number for the shop. |
| discount_kind | TEXT | yes | `percent` / `amount`, the tag half. |
| discount_value | INTEGER | yes | Basis points or paise, per the tag. |
| discount_reason | TEXT | yes | Scope 1.12. |
| discount_by | TEXT | yes | Who authorised it (P11). |
| discount_applied | INTEGER | yes | What was actually taken off. |
| discount_requested | INTEGER | yes | What was asked for. |
| was_discount_capped | INTEGER | no | **D15** — a discount that could not do what was asked says so, and the flag has to reach the disk to reach the bill. NOT NULL with a default of 0: a line with no discount was not capped, and `discount_kind` already says whether there was one. A nullable boolean is v1's `subtotal_bold` again. |

### order_line_modifiers

Snapshot again, for the same reason as the line.

| column | type | null | notes |
|---|---|---|---|
| order_line_id | TEXT | no | |
| seq | INTEGER | no | |
| modifier_id | TEXT | yes | Reporting only. |
| name | TEXT | no | Snapshot. |
| price_delta | INTEGER | no | Snapshot, paise, may be negative. |

### bills

The **computed** bill header, one row per settled or voided order.

Stored rather than recomputed, and the reasoning matters: a printed bill went to
a customer and into a GST return, so a recomputed "old" bill silently stops
matching the paper when a rate or a rounding setting changes; requirement 7 of
the ten is provable in SQL only if the numbers are columns; and budget R2 gives
a year-long report over ~75,000 bills 2.5 seconds, which recomputation is not.

| column | type | null | notes |
|---|---|---|---|
| order_id | TEXT | no | Primary key. |
| subtotal | INTEGER | no | Sum of line gross, before any discount. |
| total_line_discount | INTEGER | no | |
| total_bill_discount | INTEGER | no | |
| total_discount | INTEGER | no | |
| total_charges | INTEGER | no | Charges before their own tax (scope 1.14). |
| was_bill_discount_capped | INTEGER | no | D15. |
| bill_discount_kind | TEXT | yes | The discount as given, so a report shows asked-for as well as taken. |
| bill_discount_value | INTEGER | yes | |
| bill_discount_reason | TEXT | yes | |
| bill_discount_by | TEXT | yes | |
| total_taxable | INTEGER | no | |
| total_cgst | INTEGER | no | |
| total_sgst | INTEGER | no | |
| total_igst | INTEGER | no | Scope 2.4 — audit B11, v1 split 50/50 always and had no IGST. |
| total_vat | INTEGER | no | **P33.** State VAT on alcohol, on its own channel. Never inside a GST total, and never in a GST return. |
| non_gst_value | INTEGER | no | Liquor, outside GST entirely. Never inside a GST total. |
| exempt_value | INTEGER | no | |
| untaxed_value | INTEGER | no | **P33.** No tax of any kind — a refundable deposit. A different box from exempt, which is a nil-rated supply. |
| round_off | INTEGER | no | Its own figure, so the printed lines always sum to the printed total. |
| grand_total | INTEGER | no | |
| place_of_supply | TEXT | no | Per bill, not per shop. |
| registration | TEXT | no | **P33.** Frozen with the bill, for the same reason as the place of supply: leaving the composition scheme must not change what last year's bills reprint as. |
| state_tax | TEXT | no | **P33.** `sgst` / `utgst`. Audit 3.10 — the old template hardcoded "SGST", so a Chandigarh shop printed the wrong word on every bill it ever issued. |
| rounding_mode | TEXT | no | Stored, so the bill reprints identically after the setting changes. |
| computed_at | INTEGER | no | |
| customer_gstin | TEXT | yes | Scope 2.6, a snapshot — the customer may correct theirs next year. |
| customer_name | TEXT | yes | Snapshot, same reason. |
| irp_irn | TEXT | yes | Scope 2.12 e-invoice, DESIGN. Nothing sends these yet. |
| irp_ack_no | TEXT | yes | Scope 2.12. |
| irp_ack_at | INTEGER | yes | Scope 2.12. |
| irp_signed_qr | TEXT | yes | Scope 2.12. |
| irp_status | TEXT | yes | `pending` / `sent` / `failed` / `cancelled`. |
| irp_error | TEXT | yes | Scope 2.12. |

### bill_lines

The computed line, one row per `order_lines` row once the bill exists. The
column order follows D4's pipeline, so the file reads like the calculation.

| column | type | null | notes |
|---|---|---|---|
| order_line_id | TEXT | no | Primary key — one computed line per typed line. |
| order_id | TEXT | no | Denormalised so a report never joins through the line. |
| gross | INTEGER | no | D4 step 1: effective unit price times quantity. |
| line_discount | INTEGER | no | Step 2. |
| bill_discount_share | INTEGER | no | Step 3, spread by floor plus largest remainder (D14). |
| net | INTEGER | no | |
| taxable | INTEGER | no | Step 4, from the discounted net. |
| cgst | INTEGER | no | |
| sgst | INTEGER | no | |
| igst | INTEGER | no | |
| vat | INTEGER | no | **P33.** State VAT on this line, on its own channel. Zero on every row written before the rework, and correctly so — no bill before it ever charged state VAT. |
| gross_including_tax | INTEGER | no | `taxable + tax`. Equals `net` exactly for an inclusive-priced line. |
| rate_bp | INTEGER | no | |
| tax_kind | TEXT | no | Snapshot of the line's kind. |
| tax_basis | TEXT | no | Snapshot of the line's pricing basis. It is what the printed inclusive/added split is recovered from. |

### bill_charges

Scope 1.14. D17: a percentage charge is taken on the discounted line total,
never compounds onto another charge, and carries its own tax rate.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| order_id | TEXT | no | |
| seq | INTEGER | no | |
| kind | TEXT | no | `service` / `packing` / `delivery` / `other`. |
| name | TEXT | no | For `kind='other'` this is also the label the enum carries. |
| basis | TEXT | no | `percent` / `flat`. |
| basis_value | INTEGER | no | Basis points when percent, paise when flat — the tag beside it decides. |
| amount | INTEGER | no | The charge before its own tax. |
| taxable | INTEGER | no | |
| cgst | INTEGER | no | |
| sgst | INTEGER | no | |
| igst | INTEGER | no | |
| gross_including_tax | INTEGER | no | What this charge adds to the grand total. |
| rate_bp | INTEGER | no | Its own rate, not the bill's. |
| tax_kind | TEXT | no | `gst` / `exempt` / `untaxed`. **No `outside_gst`** — a charge is never alcohol, which is also why there is no `vat` column here. |
| tax_basis | TEXT | no | `exclusive` / `inclusive`. |

### bill_tax_rows

The rate-wise summary, one row per rate on the bill. This is what the GSTR-1
report (scope 2.8) selects from, and it is why that report is a `GROUP BY`
rather than a recomputation of a year of bills.

| column | type | null | notes |
|---|---|---|---|
| order_id | TEXT | no | |
| rate_bp | INTEGER | no | Composite primary key with `order_id`. |
| taxable | INTEGER | no | |
| cgst | INTEGER | no | |
| sgst | INTEGER | no | |
| igst | INTEGER | no | |

### payments

**Many per order** (scope 1.15). Audit B9: v1 was one bill, one payment mode,
*"and today you must lie about it."*

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| order_id | TEXT | no | |
| seq | INTEGER | no | |
| mode | TEXT | no | `cash` / `card` / `upi` / `credit` / `other`. |
| customer_id | TEXT | yes | The payload of `credit`. A CHECK ties it to the tag. |
| mode_label | TEXT | yes | The payload of `other`. A CHECK ties it to the tag. |
| amount | INTEGER | no | Always positive. |
| tip | INTEGER | no | Scope 8.5. Not taxable, never in the GST summary. |
| reference | TEXT | yes | A UPI reference, a card approval code, a cheque number. |
| device_ref | TEXT | yes | Scope 8.3 / 8.4 — what the payment device gave back, so an auto-confirmed UPI or a card terminal reconciles later. |
| provider | TEXT | yes | P29. Which provider answered for this payment. NULL on the modes nobody has to be asked about — cash and credit. |
| confirmed_at | INTEGER | yes | **P29, and the point of the feature.** NULL means nobody has said the money arrived. Today that is every UPI and card payment, because the manual provider cannot check a bank and will not pretend to — and that is exactly the list a shop needs at close, because a shop cannot chase what it cannot list. |
| confirmed_by | TEXT | yes | P29. Who said so. A confirmation with no name on it settles no argument. |
| settles_credit | INTEGER | no | **Audit B12.** `mode` says what it *was*; this says what it *did*. v1 recorded a khata settlement as payment mode "Full Settlement", which is not a payment mode, and it polluted every payment-mode report. |
| received_at | INTEGER | no | |
| received_by | TEXT | yes | |
| business_day | INTEGER | no | Denormalised so the cash-position report and the day close never join back to `orders`. |

### payment_attempts

**Added at P29, scope 8.3 / 8.4. What the payment machine said, including when
it said no.**

Every time a provider is asked whether money arrived, the answer lands here:
approved, declined or waiting. An approved attempt has a payment row beside it
and is nearly redundant. **A declined one is the only record that the event
happened at all**, because a declined card leaves no payment and an unsettled
bill — the cashier tried, the machine refused, the customer paid cash instead,
and three weeks later nobody can explain the argument at the counter.

`order_id` is deliberately **not** a foreign key. A card is often swiped before
the draft exists — the cashier is standing at the terminal — and a constraint
forcing the other order would put a modal in the middle of the fastest screen
in the product. Same argument as `attachments.subject_id`.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| order_id | TEXT | yes | Not a foreign key, on purpose — see above. |
| provider | TEXT | no | Which provider answered. `manual` is the one that ships. |
| mode | TEXT | no | `cash` / `card` / `upi` / `credit` / `other`. |
| amount | INTEGER | no | Paise, always positive. |
| reference | TEXT | yes | What was typed, or what the machine gave back. |
| answer | TEXT | no | `approved` / `declined` / `waiting`. **Three, not two** — "I do not know" is the ordinary answer from a manual provider and from a terminal that timed out, and collapsing it into yes or no is how a shop ends up with bills it believes were paid. |
| because | TEXT | yes | The provider's own words, which a cashier reads out to a customer. |
| at | INTEGER | no | |
| business_day | INTEGER | no | The STORED business day (D5). |
| asked_by | TEXT | yes | |

### kitchen_ledger

**Crown jewel 2, on disk at last.**

> *"The delta KOT. Only what the kitchen has not seen gets printed, and what was
> printed is remembered **in the database**, not in the screen's memory."*

| column | type | null | notes |
|---|---|---|---|
| order_id | TEXT | no | |
| identity_key | TEXT | no | item + note + **sorted** modifier ids, joined with a unit separator. The sort is what stops one dish having two identities; mb-core owns the rule, `encode.rs` encodes it. |
| item_id | TEXT | yes | Kept beside the key so a ticket reprints without parsing it. |
| note | TEXT | yes | Same. |
| qty_told | INTEGER | no | Thousandths. |
| updated_at | INTEGER | no | |

### reprints

Scope 1.20 — counted, so the copy is marked DUPLICATE and report 10.5 can show
who reprinted what.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| order_id | TEXT | no | |
| printed_at | INTEGER | no | |
| printed_by | TEXT | yes | |
| business_day | INTEGER | no | Reaches its outlet through the order. |
| reason | TEXT | yes | |

### refunds

Scope 8.7, added at **P12**. Money going back to a customer after a void.

**Not a negative payment.** `payments` has `CHECK (amount > 0)` and that check
is right: audit **B12** is what happens when a table meaning "money in" starts
holding other things — *"v1 recorded a khata settlement with payment mode 'Full
Settlement', which is not a payment mode, and it polluted every payment-mode
report."* A refund is its own fact and gets its own table (D22 — adding a table
later is the cheap direction).

`mode` is deliberately **not** the payment-mode enum: money can go back in cash
that came in on a card, and the drawer count needs to see that.

Only ever against a voided order, and never for more than was taken. Both rules
live in the repository, because both need to read the order.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| order_id | TEXT | no | |
| amount | INTEGER | no | Paise, positive. The direction is the table's name. |
| mode | TEXT | no | How it went back: cash, card, upi, other. |
| reason | TEXT | no | |
| refunded_at | INTEGER | no | |
| refunded_by | TEXT | yes | |
| business_day | INTEGER | no | D5, denormalised like every other money row. |

---

### reasons

Scope 1.17–1.20, added at **P12**. The reasons a shop offers for its own
corrections.

**Editable data, not a hardcoded list** — the reasons a chai stall needs are
not the reasons a bar needs, and a list in the source is a support call. One
table for all four flows, because the reason dialog is one component with four
callers. A shop may also type free text; mb-core has refused an empty reason
since P03 either way.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| kind | TEXT | no | `void`, `cancel`, `item_void`, `reprint`, `wastage`. The fifth was added at **P25**: scope 4.7 asks for a wastage reason from an editable list, and this table already IS that mechanism — a fifth table would have been a second answer to a question already answered. |
| text | TEXT | no | |
| sort_order | INTEGER | no | |
| is_active | INTEGER | no | Retired rather than deleted, so old rows still read. |

---

### order_events

A narrow, append-only trail of what happened to one order. Distinct from
`audit_log` on purpose: this one is always written and safe to sync;
`audit_log` carries before/after JSON and is not.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| order_id | TEXT | no | |
| at | INTEGER | no | |
| business_day | INTEGER | no | |
| event | TEXT | no | |
| staff_id | TEXT | yes | |
| detail | TEXT | yes | Short text, not a document. |

---

### staff

Scope 9.15: an employee record is **never deleted**. Someone who left in March
is still on March's bills, March's audit trail and March's payroll.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| role_id | TEXT | yes | |
| name | TEXT | no | Scope 9.4 — printed on the bill. |
| pin_hash | TEXT | yes | **P11 chooses the algorithm.** The column and this note are all P04 has any business deciding. |
| phone | TEXT | yes | |
| joined_on | INTEGER | yes | Days since epoch. |
| status | TEXT | no | `active` / `suspended` / `left`. Never a delete. |
| designation | TEXT | yes | P28. |
| department | TEXT | yes | P28. Kitchen, counter, service — free text, because every shop draws the line differently and a coined list nobody uses is worse than the shop's own words. |
| address | TEXT | yes | P28. |
| emergency_name | TEXT | yes | P28. |
| emergency_phone | TEXT | yes | P28. |
| id_proof | TEXT | yes | P28. **A reference, not the number** — "Aadhaar ...4321". A POS has no business holding somebody's identity document, and holding one is a liability with no upside. |
| photo_file | TEXT | yes | P28. `attachments.filename`. |
| is_rider | INTEGER | no | P29, scope 14.5. **A rider is a member of staff with a flag**, not a second people table. Most riders in a small restaurant also wait tables, and a separate list nobody keeps up to date is how a shop ends up with two spellings of one name. |
| employment_type | TEXT | no | P28. `full_time` / `part_time` / `casual`. |
| left_on | INTEGER | yes | P28. A CHECK refuses a leaving date on somebody still working — payroll reading that would stop paying a person standing in the kitchen. The other direction is allowed: `left` with no date is *incomplete*, which is what P11's sign-in path writes, because ending an account is an identity decision made without knowing which day the employment ended (and re-deriving it from the clock is the mistake D5 exists to prevent). |
| created_at | INTEGER | no | |
| updated_at | INTEGER | no | |

### roles

Scope 9.2. Filled at P11.

`is_builtin` means **cannot be deleted**, not "cannot be edited". A shop whose
waiters take payments must be able to say so without asking us.

The two discount columns are **scope 1.12** and arrived at P11. D18 is why they
live on the role rather than on the bill: the policy is checked by the caller,
never inside `compute_bill`, so an old bill recomputes identically after
somebody's role changes.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| name | TEXT | no | |
| is_builtin | INTEGER | no | A shipped role the owner cannot delete out from under themselves. |
| max_discount_bp | INTEGER | yes | Basis points. NULL is no limit; 0 is a role that may not discount at all, and a waiter has exactly that. |
| max_discount_paise | INTEGER | yes | Paise. NULL is no limit. |

### permissions

**The BACKEND-G7 fix**, seeded by migration 0001.

> *"The `staff` permission map is free-form. Any key can be written; a typo in a
> permission name silently means 'denied'. There is no list of valid permissions
> anywhere in the database."*

A permission that is not a row here cannot be granted, so a typo is a foreign
key violation instead of a silent refusal nobody can debug from the counter.

| column | type | null | notes |
|---|---|---|---|
| code | TEXT | no | Primary key. Twenty-two rows seeded; P11 uses this vocabulary and no other. |
| description | TEXT | no | Shown on the roles screen. |

### role_permissions

A pure join, so it cascades from the role — but never from the permission,
because deleting a permission that roles still grant should fail loudly.

| column | type | null | notes |
|---|---|---|---|
| role_id | TEXT | no | |
| permission_code | TEXT | no | |

### audit_log

Scope 9.3. `before_json` / `after_json` are the **one** JSON in the schema: the
shape differs per action and nothing queries inside it.

**It must not sync by default.** It is unbounded, it is the widest row in the
product, and nothing on the phone reads it (D16).

**`seq`, `prev_hash` and `hash` are P11's, and they are why this log can be
believed.** Audit C4 asked for a *tamper-evident* log, and the two triggers on
this table are not that — they stop this program and every accident inside it,
and they stop nobody at all with a SQLite browser, who can drop a trigger in
one statement. So every row carries `sha256(prev_hash ‖ its own fields)`.
Editing a row changes its hash; deleting one leaves a gap in `seq` **and**
breaks the link; reordering two breaks both. `mb_auth::verify_chain` reports the
first `seq` where it stops making sense.

That is evidence rather than prevention, and evidence is the honest goal: the
shop's own machine has to be able to read the shop's own file.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| seq | INTEGER | no | Per outlet, gapless. `MAX(seq)+1` inside the writing transaction — exact, because this database has one writer. |
| at | INTEGER | no | |
| business_day | INTEGER | no | |
| staff_id | TEXT | yes | NULL for a failed login against a name nobody matched. There genuinely is no staff member, and inventing one would be a lie in the one table that must not contain any. |
| action | TEXT | no | |
| entity | TEXT | no | |
| entity_id | TEXT | yes | |
| before_json | TEXT | yes | |
| after_json | TEXT | yes | |
| prev_hash | TEXT | yes | NULL only on the first row of a shop's life. |
| hash | TEXT | no | 64 hex characters. |

---

### customers

Scope 5.1. **Note the column that is not here: there is no balance.**

v1 kept `credit_balance REAL` on this table, beside the payments that make it —
two sources of truth for what a customer owes, one of them a floating-point
number. The balance is the ledger's sum, computed, always.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| name | TEXT | no | |
| phone | TEXT | yes | Scope 5.4. |
| gstin | TEXT | yes | Scope 2.6. |
| address | TEXT | yes | |
| credit_limit | INTEGER | yes | Scope 5.2. NULL means no limit, which is not a limit of zero. |
| phone_key | TEXT | yes | P15. The last ten digits of `phone`, with a partial UNIQUE index on it: **the phone is the identity in this market**, and two rows for one number are two balances for one person. The derived copy has ONE writer (`MoneyRepo::save_customer`) — D56's rule again — and `phone` keeps what was typed. |
| birthday | INTEGER | yes | Scope 5.7, days since epoch, so the report is a range scan. |
| anniversary | INTEGER | yes | Scope 5.7. |
| note | TEXT | yes | |
| is_active | INTEGER | no | |
| created_at | INTEGER | no | |
| updated_at | INTEGER | no | |

### credit_adjustments

Scope 5.1. An opening balance, a write-off, or a correction: the movements that
are neither a sale nor a repayment.

**Every row carries a reason and a name**, because an adjustment with neither is
indistinguishable from a mistake — and this is the one door in the credit
account somebody could use to make money disappear.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| customer_id | TEXT | no | |
| amount | INTEGER | no | Always positive paise. The direction is `increases`, never a sign. |
| increases | INTEGER | no | Both directions are real: a forgotten sale added later, and money written off. |
| reason | TEXT | no | Non-blank, by CHECK. |
| at | INTEGER | no | |
| business_day | INTEGER | no | D5. |
| made_by | TEXT | yes | |

### customer_payments

The credit ledger (the owner renamed it from "khata" on 2026-08-08). Audit A3: in v1 *"payments received against khata are never
sent at all"*, so the cloud could never rebuild a shop's udhaar. Here it is an
ordinary table with an ordinary outbox entry.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| customer_id | TEXT | no | |
| amount | INTEGER | no | |
| mode | TEXT | no | `cash` / `card` / `upi` / `other` — the real mode, per audit B12. |
| mode_label | TEXT | yes | The payload of `other`. |
| reference | TEXT | yes | |
| received_at | INTEGER | no | |
| received_by | TEXT | yes | |
| business_day | INTEGER | no | |
| terminal_id | TEXT | yes | **0012.** Whose drawer the cash landed in, read as `COALESCE(terminal_id, the master)` like every other money row. Before it existed, `cash_position_of` left credit collected in cash out of the expected drawer entirely, so a shop that took a repayment in cash read "over" by exactly that amount every evening. |
| note | TEXT | yes | |

### expense_categories

Data, not a hardcoded list.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| name | TEXT | no | |
| sort_order | INTEGER | no | |
| is_active | INTEGER | no | |

### expenses

Audit A2: *"Expenses never reach the cloud — so the owner's phone shows wrong
profit."* Same table, same outbox, same treatment as a bill.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| terminal_id | TEXT | yes | **P27, D140.** Which drawer a CASH expense came out of; irrelevant for the other three modes, which never touch a box. NULL is attributed to the master, so the per-drawer figures still sum exactly to the shop's. |
| category_id | TEXT | yes | |
| description | TEXT | no | |
| amount | INTEGER | no | Always positive. |
| mode | TEXT | no | P16 replaced `is_cash` with the real mode: `cash`, `bank`, `upi`, `card`. Cash still decides the day close's expected drawer; one boolean could not tell a bank transfer from a UPI payment, and a shop reconciling a statement needs to. |
| paid_to | TEXT | yes | A NAME. Suppliers and their ledger are P26. |
| reference | TEXT | yes | The vendor's bill number. |
| gst_rate_bp | INTEGER | yes | Scope 2.9, input credit. Both this and `gst_amount` or neither, by CHECK. |
| gst_amount | INTEGER | yes | The tax INSIDE what was paid — 1,180 at 18% contains 180. Never more than `amount`, by CHECK. |
| paid_at | INTEGER | no | |
| paid_by | TEXT | yes | |
| business_day | INTEGER | no | Stored (D5). |
| note | TEXT | yes | |

### cash_movements

P16. What moved in and out of the DRAWER that is not a sale and not an expense:
the opening float, a top-up from the owner, a payout, a bank drop.

**A cash expense deliberately has no row here.** It would be a second row for
one fact, and two rows for one fact can disagree — the same argument D65 makes
about a credit balance. `MoneyRepo::cash_position` adds these to the cash sales
and subtracts the cash expenses; the duplicate cannot exist because it is never
written.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| terminal_id | TEXT | yes | **P27, D140** — a drawer is a box under ONE till. NULL is a row written before this shop had a second till, and every query attributes it to the master, so the per-drawer figures still sum EXACTLY to the shop total. |
| kind | TEXT | no | `float` / `top_up` / `payout` / `bank_drop`. |
| amount | INTEGER | no | Always positive. The direction belongs to the kind, never to a sign. |
| reason | TEXT | no | Non-blank, by CHECK. |
| at | INTEGER | no | |
| business_day | INTEGER | no | D5. |
| moved_by | TEXT | yes | |

### recurring_expenses

P16. Rent, salary, the internet bill: a TEMPLATE and a reminder.

**Nothing here is ever posted automatically.** Silently writing money into a
shop's books is not acceptable at any level of convenience; `next_due` advances
only when a reminder is confirmed, and confirming it writes an ordinary expense
with an ordinary audit row.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| category_id | TEXT | yes | |
| description | TEXT | no | |
| amount | INTEGER | no | |
| mode | TEXT | no | As `expenses.mode`. |
| paid_to | TEXT | yes | |
| every | TEXT | no | `week` or `month`. |
| next_due | INTEGER | no | Days since epoch. Rent due on the 31st lands on the 30th in April and the 28th in February — `mb_core::expense::next_due`. |
| is_active | INTEGER | no | |

### day_closes

**The drawer counts.** Scope 10.8 and requirement 9 of the ten. Audit B15: v1
had *"no opening cash, no closing cash, no expected vs actual, no Z-report. This
is how every restaurant actually closes the day and it does not exist."*

**P27, D140 — a row is one of two things, and `terminal_id` says which.** A cash
drawer is a box under one till, and counting the shop's cash as one number is
what makes *"we were ₹340 short"* unanswerable: short on which till, on whose
shift? So a **drawer count** names its terminal and its shift, and the **shop's
figure** (`terminal_id IS NULL`, `shift_no = 0`) is written automatically when
the LAST till counts — as the SUM of each till's latest count, never an
independent query that could disagree with them.

**Since 0011 (core round 2026-09-03) a count locks nothing.** The day is a thing
of its own — `business_days` — and closing it needs no count at all. Counting the
drawer is the first panel of the Day close screen, and **Close today** carries
whatever is in that grid through `count_drawer_on`, the one path a drawer is ever
counted by; **Write the count on its own** is the same path at a shift handover.
`is_locked` stays for the rows written before 0011 and is never set again.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| terminal_id | TEXT | yes | **D140.** NULL is the shop's roll-up; otherwise the till whose drawer this is. |
| shift_no | INTEGER | no | Scope 9.8's BOUNDARY half, and only that half: one till can count its drawer several times in a day. 0 on the shop row. Clock in/out, rosters, attendance, salary and leave are P28. |
| business_day | INTEGER | no | |
| opening_float | INTEGER | no | |
| expected_cash | INTEGER | no | |
| counted_cash | INTEGER | no | |
| variance | INTEGER | no | Stored, not derived, so the Z-report reprints identically years later even if a bill is voided afterwards. |
| is_locked | INTEGER | no | Historical. Before 0011 the shop's row carried the day lock; it now lives in `business_days.is_locked` and nothing sets this one. |
| closed_at | INTEGER | no | When the drawer was counted. |
| closed_by | TEXT | yes | |
| note | TEXT | yes | Why the drawer was out, when it was out by more than the shop's threshold. |

### day_close_denominations

A child table rather than a JSON column, because the note mix is a report an
owner actually asks for ("we are always short of tens") and JSON would make it a
scan.

| column | type | null | notes |
|---|---|---|---|
| day_close_id | TEXT | no | Cascades — a denomination count has no meaning without its close. |
| denomination | INTEGER | no | Paise, so a 500 rupee note is 50000 and a 50 paise coin is 50. |
| count | INTEGER | no | |

### business_days

**Added at 0011 (core round 2026-09-03, ruling 3).** A business day is a thing.
Before this there was no day entity: "closed" meant a shop roll-up row in
`day_closes` for *today*, so yesterday could never be closed once 5 am passed
(B8), a bill could be settled into a closed day (B9), and a day the shop was shut
had nowhere to be called a holiday.

One row per day the shop has said something about. **`is_locked` here is the one
lock every money path checks**, through the one function `dayclose::day_refusal` —
a settle, an order parked for the kitchen, a void, a refund, an expense, a cash
movement, a credit collection or adjustment, a supplier payment or adjustment, a
delivery received, a staff advance, a payroll approval, a rider's handback, a
stock-count approval, a purchase cancel. Nothing calls `DaysRepo::locked_at` on its
own: a path that writes a `business_day` and does not go through `day_refusal` is a
hole, and there were six of them before that function existed.

**A shop can switch the whole thing off** — `day.must_close`, on by default. Off,
`day_refusal` answers "not closed" whatever the row says, the gate never asks, and
the Day close screen has nothing to press; every figure stays where it is, so
switching it back on locks the same days again. `App::closes_days` is the one reader.

The gate at sign-in asks the one question *"which days from the last locked one to
yesterday have no locked row?"* (bounded to sixty days; a shop that has never closed
anything starts counting from the first day anything happened). Somebody without
`day.close` is told and let past it — holding a waiter at the counter's bookkeeping
stops the shop taking orders.

A bill forwarded from another till into a day that is already closed is **stored, not
refused** — a refusal would bounce between two tills for ever — and
`DaysRepo::refreeze` makes the frozen figures below true again. The day stays closed.

| column | type | null | notes |
|---|---|---|---|
| outlet_id | TEXT | no | |
| business_day | INTEGER | no | Days since 1970-01-01, stored (D5). Part of the key. |
| kind | TEXT | no | `trading` or `holiday`. A holiday is refused for a day with any bill or expense; allowed for today and for days that have not come. |
| is_locked | INTEGER | no | Closed, or marked a holiday. Reopening clears it and turns a holiday back into a trading day. |
| closed_at | INTEGER | yes | When it was locked. |
| closed_by | TEXT | yes | |
| reopened_at | INTEGER | yes | The override's mark, kept across a later close. |
| reopened_by | TEXT | yes | |
| note | TEXT | yes | The reason given to reopen it. |
| bills | INTEGER | no | Frozen at the close: every bill, including the ones later voided. |
| net | INTEGER | no | Frozen at the close: gross − voids, paise. |
| cash_taken | INTEGER | no | Frozen at the close: cash on settled bills, paise. |

Backfilled by 0011 from every shop roll-up row in `day_closes`, lock intact.

### counters

**The B4 fix on disk.**

> *"Bill and token numbers are claimed in two steps, not one… One atomic
> database operation that increments and returns in a single step.
> Non-negotiable for a bill number."*

The claim is one `UPDATE … RETURNING`, and the daily reset (audit B3) is
evaluated inside it.

mb-core's `Counter` is **not** persisted as a struct — its fields are private
and `last_reset_day` has no setter, so it cannot be rebuilt from a row. It does
not need to be: the counter lives here as columns, and mb-core's `Counter`
stays the in-memory model P03 tested. P17's settings screen reads and writes
these columns.

| column | type | null | notes |
|---|---|---|---|
| outlet_id | TEXT | no | Primary key with `terminal_id` and `kind`. |
| terminal_id | TEXT | no | **D135, the series IS the terminal.** Every till has its own counter row and its own prefix, so two tills share no number and neither needs the other to issue one. |
| kind | TEXT | no | `token` / `bill`. |
| last_issued | INTEGER | yes | The number already handed out; NULL means none yet, which is not zero. Named for the **past**, like `Counter::last_issued()` — a column called `current` reads as "the number I am about to use", and that reading is the mistake B4 is made of. |
| start | INTEGER | no | What a reset returns to. |
| reset_daily | INTEGER | no | Not a boolean by the naming rule — it is a mode, and the settings screen labels it "Reset daily". |
| prefix | TEXT | no | |
| pad_width | INTEGER | no | |
| last_reset_day | INTEGER | yes | NULL means never reset. Compared inside the claim, not before it. |

---

### sync_outbox

**The A1 / A2 / A3 fix, and it is one table.** v1's outbox knew about bills.

Table-agnostic on purpose: a new synced table is a new value in `table_name`,
not a new outbox.

**The payload is not stored for an upsert.** The sender reads the row at send
time — that halves the write, keeps M5 down, and means a row edited five times
between connections syncs *once*, which is D16's 10 MB egress budget decided
here rather than at P33. A delete carries a tombstone because there is nothing
left to read.

And there is deliberately **no `is_synced` flag on the business tables**: it
looks cheaper than an outbox row and it turns every sync into a full scan of
every table for dirty rows, forever.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| table_name | TEXT | no | |
| row_id | TEXT | no | |
| op | TEXT | no | `upsert` / `delete`. |
| tombstone | TEXT | yes | Only for a delete — a CHECK enforces it. |
| created_at | INTEGER | no | |
| attempts | INTEGER | no | P33's backoff reads this. |
| last_error | TEXT | yes | |
| synced_at | INTEGER | yes | NULL is the backlog; the pending index is partial on it. |

### applied_events

Crown jewel 11: *"Every phone request is applied exactly once, guarded by a
local ledger, so a re-delivered message never double-prints a KOT."* v1 had this
(`applied_order_events`) and it was right.

| column | type | null | notes |
|---|---|---|---|
| event_id | TEXT | no | The id the phone sent. Primary key — that is the whole mechanism. |
| outlet_id | TEXT | no | |
| applied_at | INTEGER | no | |
| source | TEXT | no | Which device or channel it came from. |
| result | TEXT | yes | Short, so a retry can be answered with the original outcome. |

---

### cloud_notices

Notices from Magic Bill, brought down with every cloud check (migration 0007). The bell
reads this. `seen_at` is the counter's own record; the cloud never keeps read state.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | The cloud's id. |
| outlet_id | TEXT | no | |
| title | TEXT | no | |
| body | TEXT | no | |
| starts_at | INTEGER | no | |
| ends_at | INTEGER | yes | NULL is never. |
| updated_at | INTEGER | no | The cloud's stamp; an older copy never overwrites a newer one. |
| seen_at | INTEGER | yes | When the bell was opened. |
| is_deleted | INTEGER | no | Withdrawn by us; kept, hidden. |

---

### cloud_day_totals

One row per day whose bills are not on this computer (migration 0007). Written only by a
restore from the cloud: the last 30 days come back as real bills, every older day comes back
as one of these. The day-wise report reads a day from here only when it has no bills of its
own for that day. Nothing else reads it; nothing on the counter writes it after the restore.

| column | type | null | notes |
|---|---|---|---|
| outlet_id | TEXT | no | |
| business_day | INTEGER | no | Days since 1970-01-01. Part of the key. |
| bills | INTEGER | no | Settled bills that day. |
| voids | INTEGER | no | |
| gross | INTEGER | no | Paise. |
| discount | INTEGER | no | Paise. |
| tax | INTEGER | no | Paise. |
| charges | INTEGER | no | Paise. |
| net | INTEGER | no | Paise. |
| by_payment | TEXT | no | JSON: `{"cash": paise, "upi": paise}`. |
| expenses | INTEGER | no | Paise. |
| credit_given | INTEGER | no | Paise. |
| credit_collected | INTEGER | no | Paise. |
| is_day_closed | INTEGER | no | |
| updated_at | INTEGER | no | The cloud's stamp. |

---

### kitchen_deliveries

**Added at P24 — the kitchen display.** One row per ticket sent to one station.

The kitchen **ledger** (crown jewel 2, on the order) already knows what the
kitchen was told. This knows what became of it: did a screen draw it, did it
fall back to paper, has a cook finished it.

**Bump state is here and not in the screen's memory.** A cook bumps a ticket,
the tablet reloads, and it must not come back — and a counter that reopens that
table must see what the kitchen sees.

Three columns are worth reading twice:

* **`station`** is a column and not a lookup, because a shop can rename or
  delete a section and last Tuesday's ticket still has to say where it went. It
  is here from the first day on purpose: a shop with one screen never notices
  it, and a shop that splits the kitchen later rewrites no tickets.
* **`course`** makes a delivery a FIRING. "Fire the mains" creates a second row
  for the same order carrying only the mains, with its own clock. `NULL` is the
  whole order, which is what a shop that does not use courses always sends.
* **`shown_at` is when a SCREEN DREW it**, not when the bytes arrived. An ack
  meaning "it was received" lies when a tablet's power saver has frozen the tab
  — and that is the exact failure the paper fallback exists for.

`expected_minutes` is stored rather than recomputed, so editing an item's prep
time next month does not silently rewrite last Tuesday's kitchen-speed figures.

| column | type | null | why |
|---|---|---|---|
| id | TEXT | no | The idempotency key. The same id applied twice is applied once — D82's rule, and this session's use of it. |
| outlet_id | TEXT | no | |
| order_id | TEXT | no | |
| station | TEXT | no | Which screen this went to. A column and not a lookup: a shop can rename or delete a section and last Tuesday's ticket still has to say where it went. |
| course | TEXT | yes | Which course this firing is (3.5). NULL is the whole order, which is what a shop that does not use courses always sends. |
| expected_minutes | INTEGER | yes | The slowest dish on this firing (3.6). Stored, not recomputed. |
| state | TEXT | no | `pending`, `shown`, `bumped` or `printed`. The state machine is `mb_core::kitchen_delivery` — pure and tested; this column only stores its answer. |
| sent_at | INTEGER | no | When the counter told the kitchen. Every timer on the screen and every figure in the kitchen-speed report is measured from this. |
| business_day | INTEGER | no | **D5, and audit B1 is why.** Every report filters the STORED business day, never one re-derived from a timestamp — B1 was the day-wise report bucketing by UTC while its filter used local time, and a new report is exactly where that comes back. Stamped from the order this firing belongs to, so a ticket sent at 00:30 belongs to the day the order does. |
| shown_at | INTEGER | yes | **When a screen DREW it** — not when the bytes arrived. An ack meaning "received" lies when a tablet's power saver has frozen the tab, which is the exact failure the paper fallback exists for. |
| bumped_at | INTEGER | yes | `sent_at` → `bumped_at` IS the kitchen-speed figure. |
| bumped_by | TEXT | yes | "Who marked that done?" is a question an owner asks when a dish never reached a table. |
| bumped_on | TEXT | yes | Which device the cook was standing at. |
| bumped_lines | TEXT | no | Lines a cook ticked off individually, as a JSON array of line keys — the owner asked for both whole-card and per-dish. JSON because it is read and written only with its parent row and never queried across tickets, the same reasoning `audit_log.before_json` carries. |
| cancelled_at | INTEGER | yes | A cancellation. **The one thing on this screen allowed to interrupt**: food already cooking is thrown away, and food not started is cooked for nobody. |
| acked_at | INTEGER | yes | When somebody pressed "Got it". Until then the cancellation stays on the screen — it cannot be dismissed, only acknowledged (D107). |

### lan_devices

**D9, added at P19.** In v1 every tap on a waiter's phone travelled to Mumbai
and back, and the server refused the order if the counter had not checked in
within five minutes. From P19 the phone talks to the **counter**, over the
shop's own WiFi. This is the register of which phones are allowed to.

The shop WiFi is not trustworthy — guests are on it — so a phone is not a
device that knows the address, it is a device that a person let in and that
holds a credential the counter issued.

**The certificate and its private key are deliberately NOT here.** They live
beside the config in `%APPDATA%\MagicBill`, because a backup taken on one
machine is restored onto another (P05, D27) and a restored backup must not
resurrect another machine's private key. A certificate is machine identity;
this table is shop data.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | The device id the counter issues. |
| outlet_id | TEXT | no | |
| name | TEXT | no | What the phone calls itself. Shown to the person approving it, so a device that appears unexpectedly can be refused **by name**. Non-blank by CHECK. |
| platform | TEXT | no | `android`, `ios`, `web`. |
| secret_hash | TEXT | no | **Argon2 of the credential, never the credential.** This database is copied to a pen drive on purpose (P05); a plaintext secret in it is a database that hands out access to whoever finds the drive. |
| staff_id | TEXT | yes | Nullable on purpose: a shared tablet at the pass belongs to no one person, and each ACTION on it still names the staff member who did it. |
| paired_at | INTEGER | no | |
| paired_by | TEXT | yes | Who let it in. |
| last_seen_at | INTEGER | yes | What the panel shows, so a phone that stopped talking is visible. |
| last_ip | TEXT | yes | For a support call: "which phone is on 192.168.1.31?" |
| revoked_at | INTEGER | yes | NULL means live. **A revoked device is not deleted** — D47's rule, a correction is a state and never a deletion. An owner asking "which phone was that, and who took it off?" needs the row. |
| revoked_by | TEXT | yes | |
| install_id | TEXT | yes | **0010, one seat.** The phone names its install when it pairs; the counter keeps one row per install, so a phone that signs out and comes back takes its own seat again. NULL on rows paired before 0010. |

---

### materials

**Added at P25 — scope 4.2.** What a kitchen consumes, as opposed to what a
customer buys. Not an `items` row and never one: the day somebody sells a kilo
of rice over the counter, those are still two different things with two
different prices.

**The base unit is not a column.** It follows from `dimension` — gram,
millilitre, piece — and that is **D108**. A shop that could pick kg for rice and
g for masala has two rice figures the day somebody converts one, and a session
six months from now has to guess which. The shop's own packs (bag, tin, tray)
live in `material_units`, against the material, because a bag is 25 kg of rice
and 50 kg of flour.

**P26 will ADD `supplier_id` to this table, and that is allowed.** D22's rule —
reserve the module, never alter a table — is about tables with 600,000 rows in
them. This one holds a few hundred and an `ALTER` on it chooses a value for
nobody. `buy_from` is the free-text answer until then (**D116**), and it is also
what a shop with one owner and one scooter actually says: *Vegetable market*,
*Metro*, *the milk van*.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| name | TEXT | no | Unique within the outlet. |
| dimension | TEXT | no | `weight`, `volume` or `count`. **The base unit follows from this** (D108) and is `g`, `ml` or `piece`. |
| category | TEXT | no | Free text — a starting point a shop edits, never a list in the source (audit B14). |
| buy_from | TEXT | no | **D116.** Where you buy it. The buy list groups by this, and it stayed the answer after P26 brought suppliers — a shop with one owner and one scooter buys from "the vegetable market". |
| supplier_id | TEXT | yes | **Added at P26, and it is the ALTER the paragraph above licenses.** Who you buy it from, when the shop has said. NULL is the everyday answer; setting it is what lets a purchase order be raised. |
| reorder_level | INTEGER | no | Base units. Below this, the material is on the buy list (4.6). |
| reorder_qty | INTEGER | no | Base units. How much to buy when it is. |
| is_perishable | INTEGER | no | **D117.** A property that warns, not batch tracking. |
| shelf_life_days | INTEGER | yes | With `is_perishable`, drives "this has not moved in six days and lasts three". |
| location | TEXT | no | Scope 4.10, DESIGN. Where in the shop it is kept; a transfer between two locations is the same shape as one between two outlets. |
| avg_cost | INTEGER | no | **D118.** Paise per **1,000** base units — ₹60 a kilo is `6000`, the number the shopkeeper already says. A weighted average of what actually came in, never a typed "current price" nobody has updated since 2019. Per thousand and not per one because water at ₹5 for 20 litres is 0.025 paise a millilitre, which per base unit rounds to zero and makes every recipe using it free. |
| cost_changed_at | INTEGER | yes | So the screen can say when, which is half of whether to believe it. |
| last_counted_at | INTEGER | yes | **D115.** NULL means *never counted*, and the variance report says so out loud rather than showing a confident 0.0%. P26's physical count writes it. |
| is_active | INTEGER | no | D47: never deleted. Last month's ledger rows point at it. |
| sort_order | INTEGER | no | |
| created_at | INTEGER | no | |

---

### material_units

**Added at P25.** The shop's OWN packs — bag, tin, tray, crate.

**The standard units are not here.** kg, litre and dozen come from the
dimension and nobody types them: a shop that has to tell the software a kilo is
a thousand grams has been given a job it should not have.

| column | type | null | notes |
|---|---|---|---|
| material_id | TEXT | no | |
| name | TEXT | no | |
| base_per_unit | INTEGER | no | Thousandths of the base unit. A 25 kg bag of rice is `25000000`. |
| is_purchase_default | INTEGER | no | Two separate defaults on purpose: rice is **bought** in bags and **cooked** in grams, and a screen that offers one where the other belongs is the screen an owner abandons. |
| is_recipe_default | INTEGER | no | |
| sort_order | INTEGER | no | |

---

### recipes

**Added at P25 — scope 4.3.** What one of something is made of.

**D111 — a sub-recipe is a MATERIAL that has a recipe.** Gravy base, masala mix,
dough: things on a shelf, made in batches, capable of going off. Not a second
kind of entity, which is why `owner_kind` has three values and not four.

**There is no `variant` owner kind, and that is P13's doing rather than an
omission.** A variant is its own `items` row — *"Dosa (Half)" and "Dosa (Full)"
are two things to cook, two prices and two rows on a rate summary* — so scope
4.3's "a recipe per variant" needs no code at all.

Three nullable foreign keys rather than one loose `owner_id`, with CHECKs that
make exactly one of them set, so the database refuses a recipe for a dish that
is not on the menu.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| owner_kind | TEXT | no | `item`, `modifier` or `material`. |
| item_id | TEXT | yes | Set exactly when `owner_kind = 'item'`, enforced by a CHECK. |
| modifier_id | TEXT | yes | Extra cheese consumes cheese (4.3). |
| material_id | TEXT | yes | A made material — D111. |
| batch_yield | INTEGER | no | **How much one batch makes**, in the OWNER's base units. A dish or a modifier is one serving (`1000`) and no screen shows it; a made material is the chef's sentence — *"this batch makes 4 kg"*. |
| notes | TEXT | no | |
| updated_at | INTEGER | no | |

---

### recipe_lines

**Added at P25.**

**D110 — one percentage, and it is "how much survives".** The first draft asked
for a yield percentage *and* a wastage percentage. Two fields answering one
question is D78's `(s)` problem in arithmetic form: a shop that types 10 in both
gets a number neither of them meant, and the conventions do not even agree — 90%
yield needs 111.1 g issued, 10% wastage issues 110 g, and nothing on the screen
says which was asked for. **Compound losses are a sub-recipe**, which the
multi-level recipe already expresses exactly and which also gives the shop
something it can count on a shelf.

| column | type | null | notes |
|---|---|---|---|
| recipe_id | TEXT | no | |
| seq | INTEGER | no | |
| material_id | TEXT | no | |
| base_qty | INTEGER | no | What **reaches the dish**, in base units. What leaves the shelf is more whenever the yield is under 100. |
| yield_percent | INTEGER | no | **D110.** 1–100. Onions peeled at 90 means issuing 222 g to get 200 g into the pot. |
| typed_qty | INTEGER | no | **D109, the label half.** What the person typed, so the screen shows back "1 bag" and not "25000000". Nothing computes with it. |
| typed_unit | TEXT | no | |

---

### stock_movements

**Added at P25 — the ledger, and the reason the module is worth anything.**

Append-only, and current stock is **derived** from it. The same reasoning as
P15's credit ledger and D67's cash position: **a balance you can edit is not
evidence**. An owner buys this to find out where ₹40,000 a month of raw material
goes, and a figure somebody can type over is the figure they already have in a
diary with a login screen in front of it.

**D113 — a reversal negates the ROWS; it does not re-run the recipe.** Voiding
Tuesday's bill on Friday must put back what was actually taken on Tuesday.
Re-exploding would use Friday's recipe, Friday's yields and Friday's costs, and
if the chef changed the gravy on Wednesday the rice balance would permanently
gain the difference. Crown jewel 4 in the stock room.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| material_id | TEXT | no | |
| kind | TEXT | no | `opening`, `purchase` (P26 writes them; the kind is defined here so P26 adds no column), `sale`, `reversal`, `wastage`, `adjustment`, `production_in`, `production_out`, `transfer_in`, `transfer_out`. |
| base_qty | INTEGER | no | **Signed** thousandths of the base unit. Out is negative, in is positive, and the balance is the SUM. One column, not an in and an out — two columns is two places for a sign error to hide. |
| typed_qty | INTEGER | no | D109, the label. |
| typed_unit | TEXT | no | |
| unit_cost | INTEGER | no | Paise per 1,000 base units **at the time**. Stored for the reason `bills` is: a cost that changes next month must not silently rewrite what last month's wastage was worth. |
| total_cost | INTEGER | no | Paise, signed like `base_qty`. |
| business_day | INTEGER | no | **D5, and audit B1 is why.** Every report filters the STORED day, never one re-derived from a timestamp. |
| at | INTEGER | no | |
| staff_id | TEXT | yes | |
| order_id | TEXT | yes | The bill this came off, for a `sale` or a `reversal`. |
| order_line_id | TEXT | yes | Which line, so a per-item void reverses only its own share. |
| reason_id | TEXT | yes | `reasons`, kind `wastage` or `adjustment`. |
| note | TEXT | yes | |
| reverses_id | TEXT | yes | **D113.** The row this puts back, so "exactly what was taken" is a join and not a recomputation. |
| produced_for | TEXT | yes | Which made material a `production_out` fed, so the ledger reads *"3 kg tomato → gravy base"* instead of leaving somebody to work out why the tomato moved. |
| was_automatic | INTEGER | no | **D111.** This happened because a sale needed a made material nobody had recorded making. Visible in the ledger, never hidden. |
| transfer_id | TEXT | yes | Scope 4.10, DESIGN. A transfer is two rows sharing one id — the same shape whether it crosses a wall or a city. |
| counterpart_outlet_id | TEXT | yes | Scope 4.10, DESIGN. |
| location | TEXT | no | Scope 4.10, DESIGN. |

---

### material_balances

**Added at P25 — D114: the ledger is the truth and this is a CACHE.**

D67 says a balance is a query, and it is right; but the cash book has a few
hundred rows a month and this has **five thousand a day** (250 bills × 4 lines ×
5 materials). Summing a year to draw a screen is not a design.

So this row is written in the **same transaction** as every movement; a test
generates a month of purchases, sales, wastage and adjustments, rebuilds from
the ledger, and asserts every material agrees to the base unit; there is a
visible "rebuild from the ledger" action; and Health checks the two agree.
**A cache nobody verifies is a stored balance with extra words.**

| column | type | null | notes |
|---|---|---|---|
| outlet_id | TEXT | no | |
| material_id | TEXT | no | |
| base_qty | INTEGER | no | Signed. **A stock balance is the one quantity in this product allowed to go negative** — a shop that sold food it never recorded buying — because refusing would have refused the sale. |
| last_movement_at | INTEGER | yes | Drives D117's "perishable, three days, has not moved in six". |

---

### stock_problems

**Added at P25 — what the stock book could not do, which did not stop the sale.**

`mb_core::recipe::explode` has no error return at all (**D112**), for the same
reason `Feature` has no `Billing` variant (D86): a call that would stop a cashier
taking money must not typecheck. Every way deduction can go wrong lands here
instead, carrying **a whole sentence an owner can act on** — D100's rule.

**Grouped on `(kind, subject)` with a count, not one row per bill.** A shop with
400 items and 3 recipes would otherwise write four rows every bill for ever, and
the one thing an owner needs to read — *"Chicken Biryani has no recipe"* — would
be buried under its own repetitions.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| kind | TEXT | no | `no_recipe`, `retired_material`, `unknown_material`, `went_negative`, `zero_yield`, `too_deep`, `absurd`. **`retired_material` is the reachable one** — `recipe_lines.material_id` is a real foreign key and a material is never deleted (D47), so the only way a recipe can name something the shop no longer keeps is by it being switched off underneath. `unknown_material` survives for a database somebody has edited by hand. |
| subject | TEXT | no | A material id, or `item:itm_x`. The grouping key. |
| sentence | TEXT | no | Written by `Problem::sentence` — the fix, not the fault. |
| occurrences | INTEGER | no | |
| first_at | INTEGER | no | |
| last_at | INTEGER | no | |
| last_order_id | TEXT | yes | One example to open. |
| resolved_at | INTEGER | yes | Cleared by fixing the thing, or by the void that undid the bill. |

---

### stock_day_closes

**Added at P25.** The closing figure per material per business day, written when
the day is closed (`close_day`, since 0011 — before that nothing wrote it, B11).

It is what makes *"what did I have on the 3rd"* answerable, what a period's
theoretical-vs-actual is measured between, and what P26's physical stock count
will compare against.

| column | type | null | notes |
|---|---|---|---|
| outlet_id | TEXT | no | |
| business_day | INTEGER | no | D5, stored. |
| material_id | TEXT | no | |
| closing_qty | INTEGER | no | |
| unit_cost | INTEGER | no | Frozen with the day, so valuing last month's closing stock does not use this month's prices. |

---

### suppliers

**Added at P26**, scope 4.5. Who you buy from.

D47: retired, never deleted — last year's invoices point at this row, and a
retired supplier's ledger still opens.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| name | TEXT | no | Unique within the outlet. |
| phone | TEXT | yes | |
| gstin | TEXT | yes | Checked with P17's Luhn mod 36 before it gets here, exactly as a customer's is. A wrong GSTIN on a purchase is a wrong input-credit claim. |
| address | TEXT | yes | |
| terms_days | INTEGER | no | **Days.** 0 is cash-and-carry: due the day it arrives. **D131** makes a payment term a SHIFT OF THE DATE, so this is the only supplier-shaped input to ageing and there is no second algorithm. |
| note | TEXT | yes | |
| is_active | INTEGER | no | D47. |
| created_at | INTEGER | no | |

---

### supplier_materials

**Added at P26.** What a supplier sells and what they last charged, used to
pre-fill a purchase line and to answer *"who is cheapest for paneer"*.

`last_rate` is a memory of a price list and **never a cost** (D122). A screen
may show it; nothing values stock with it.

| column | type | null | notes |
|---|---|---|---|
| supplier_id | TEXT | no | |
| material_id | TEXT | no | |
| pack | TEXT | no | The pack the rate is quoted in, by name. D108: a pack belongs to the MATERIAL, so this is a lookup key and never a conversion. |
| last_rate | INTEGER | no | Paise per whole pack. |
| last_bought_day | INTEGER | yes | |

---

### purchases

**Added at P26**, scope 4.5. **The paper** — one row per invoice, kept as it was
typed.

A stock movement cannot answer *"what did the invoice say"*, which is the
question a CA and a supplier's clerk both ask, so the document is stored beside
the effect and not instead of it.

**D120 — one rupee, one row.** Saving one of these writes the stock movements,
this row and (when money changed hands) a `supplier_payments` row, in ONE
transaction — and **no `expenses` row and no `cash_movements` row**.
`cash_position` gains one more term instead.

**D125 — a purchase is never edited.** It has moved the average cost of five
materials and some of that stock has been cooked and sold; an edit would be a
rewrite of the past with no record. The one correction path is cancel-with-a-
reason and re-enter, and `is_cancelled` is that state (D47).

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| supplier_id | TEXT | no | **NOT NULL on purpose.** The money half of a purchase is "who do I now owe", and an invoice with nobody on it cannot be aged, paid or argued about. A shop buying loose vegetables makes one supplier called "Vegetable market" once. |
| kind | TEXT | no | `purchase` or `return`. **D126** — a return is its own document pointing at its parent, never a negative line edited onto the original. |
| parent_id | TEXT | yes | Set exactly when `kind = 'return'`. |
| invoice_no | TEXT | yes | |
| business_day | INTEGER | no | **D5, and audit B1 is why.** The day the shop counts it in, which can differ from the date printed on the paper. |
| received_at | INTEGER | no | |
| due_day | INTEGER | no | **D131**, frozen: the supplier's `terms_days` applied at entry, so changing the terms next month cannot re-age last month. |
| lines_value | INTEGER | no | Paise. Every figure stored, none re-derived — a total recomputed next year from a rate column is a total that changes when a rounding rule does. |
| line_discounts | INTEGER | no | |
| invoice_discount | INTEGER | no | Apportioned across the lines by **D14's largest remainder**. |
| charges | INTEGER | no | Transport, loading, hamali — apportioned the same way and **becoming part of the cost of the food** (D123). |
| tax_total | INTEGER | no | |
| tax_creditable | INTEGER | no | **D124.** How much of that tax the shop may claim back — zero for a 5%-scheme shop, and the difference is inside the landed cost. |
| round_off | INTEGER | no | On the grand total only, D4 step 7. |
| total | INTEGER | no | |
| stated_total | INTEGER | yes | What the paper's own total line said. Kept so a report can say "the invoice says ₹4,210 and the lines make ₹4,208" rather than silently believing either. |
| po_id | TEXT | yes | **D130** — a purchase entered with no order at all has NULL here and never meets a PO. |
| attachment_id | TEXT | yes | **D132**, the photograph. |
| note | TEXT | yes | |
| created_by | TEXT | yes | |
| is_cancelled | INTEGER | no | D47, D125. |
| cancelled_at | INTEGER | yes | Set exactly when `is_cancelled = 1`. |
| cancelled_by | TEXT | yes | |
| cancel_reason | TEXT | yes | |

---

### purchase_lines

**Added at P26.** One line of the paper, and the landed cost that came out of it.

**D123 — the free bag is in the DENOMINATOR.** Buy 10 bags at ₹1,000 and get 1
free: eleven bags arrived, ₹10,000 was paid, and a bag cost ₹909.09. A product
that books the free bag at zero cost produces a material whose average sags
after every scheme and a stock valuation wrong by the same amount.

| column | type | null | notes |
|---|---|---|---|
| purchase_id | TEXT | no | |
| seq | INTEGER | no | |
| material_id | TEXT | no | |
| typed_qty | INTEGER | no | **D109 — both numbers, always.** What was typed. |
| typed_unit | TEXT | no | |
| base_qty | INTEGER | no | The truth everything computes with. |
| free_typed_qty | INTEGER | no | The scheme's free quantity, in the same unit. |
| free_base_qty | INTEGER | no | |
| rate | INTEGER | no | Paise per TYPED unit — ₹1,000 a bag is `100000` — because that is what the invoice says and what the person types. |
| line_value | INTEGER | no | |
| discount | INTEGER | no | |
| tax_rate_bp | INTEGER | no | |
| tax_amount | INTEGER | no | |
| charge_share | INTEGER | no | This line's share of the invoice charges, D14. |
| discount_share | INTEGER | no | This line's share of the whole-invoice discount, D14. |
| landed_value | INTEGER | no | The numerator: what this line really cost, all in. |
| landed_unit_cost | INTEGER | no | Paise per 1,000 base units, the same scale as `materials.avg_cost`, so ₹40 a kilo reads `4000`. |
| movement_id | TEXT | yes | The ledger row this line wrote. **What makes D126 exact**: a return goes out at what these goods cost when they came, read from here. |
| returns_seq | INTEGER | yes | On a return line: which line of the parent invoice is going back. What is left to return is then a query, not a cache anybody can drift. |

---

### supplier_payments

**Added at P26. D121 — what was bought and what was paid are two facts.**

"Paid cash at the door" is one press on the purchase screen and two rows. The
flag it replaces cannot express paying half now, paying three old invoices with
one note on Sunday, paying by UPI two days later, or paying somebody you have no
invoice from yet — and all four happen weekly.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| terminal_id | TEXT | yes | **P27, D140.** Cash handed to the vegetable man came out of one box, and D120 made this row the drawer's only record of it. |
| supplier_id | TEXT | no | |
| amount | INTEGER | no | Always positive. |
| mode | TEXT | no | `cash`, `bank`, `upi` or `card` — the real mode, audit B12. **`cash` is the one the drawer reads**, which is why one boolean was not enough here either. |
| reference | TEXT | yes | |
| purchase_id | TEXT | yes | Set when the money went over with the delivery. NULL is the normal weekly-settlement case, and the ageing applies it oldest-first (D131) rather than making anybody allocate it by hand. |
| paid_at | INTEGER | no | |
| business_day | INTEGER | no | D5. |
| paid_by | TEXT | yes | |
| note | TEXT | yes | |

---

### supplier_adjustments

**Added at P26.** An opening balance, a write-off, or a correction — the
supplier side of `credit_adjustments`, and the same shape for the same reason.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| supplier_id | TEXT | no | |
| amount | INTEGER | no | Always positive; `increases` is the direction. A negative amount in a ledger is how a subtraction becomes an addition in somebody's report six months later. |
| increases | INTEGER | no | |
| reason | TEXT | no | Cannot be blank. |
| at | INTEGER | no | |
| business_day | INTEGER | no | D5. |
| made_by | TEXT | yes | |

---

### purchase_orders

**Added at P26. D130 — a purchase order is optional, and the proof is that
nothing reads one.** The purchase screen never asks for a PO and never mentions
one. A shop that never raises one never meets this table.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| supplier_id | TEXT | no | |
| number | TEXT | no | Unique within the outlet. |
| state | TEXT | no | `draft`, `sent`, `received`, `closed`, `cancelled`. |
| expected_day | INTEGER | yes | |
| note | TEXT | yes | |
| created_at | INTEGER | no | |
| created_by | TEXT | yes | |
| sent_at | INTEGER | yes | |
| closed_at | INTEGER | yes | |

---

### purchase_order_lines

**Added at P26.**

| column | type | null | notes |
|---|---|---|---|
| po_id | TEXT | no | |
| seq | INTEGER | no | |
| material_id | TEXT | no | |
| typed_qty | INTEGER | no | D109. |
| typed_unit | TEXT | no | |
| base_qty | INTEGER | no | |
| rate | INTEGER | no | Paise per typed unit. What was AGREED — the purchase screen flags a difference as a sentence and never as a refusal. |

---

### stock_counts

**Added at P26**, scope 4.8. The monthly reality check, and the reason the whole
inventory module is worth paying for: recipes say what *should* have gone, and
only a person with a clipboard can say what *did*.

**D127 — the count freezes the book and posts a DELTA.** The manager counts at
11 pm on Sunday; the owner approves on Monday at 9 am, after Monday's 25 kg of
rice has arrived. A system that SETS the balance to Sunday's figure erases the
delivery and nobody notices for a month.

**D129** — `draft → approved`, or `draft → abandoned` with a reason. An approved
count is sealed for ever and is never deleted; its adjustments are ordinary
ledger rows carrying its id, so the variance history is a query.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| location | TEXT | no | Scope 4.10's `location`, which is why the count sheet groups by it. |
| state | TEXT | no | `draft`, `approved`, `abandoned`. |
| business_day | INTEGER | no | D5. |
| opened_at | INTEGER | no | |
| opened_by | TEXT | yes | Needs `stock.count` — a helper with a clipboard. |
| approved_at | INTEGER | yes | Set exactly when the state is `approved`. |
| approved_by | TEXT | yes | Needs **`stock.adjust`**, not a permission of its own: approving a count IS adjusting stock by hand, at scale, and a separate one would let a shop grant the big power while denying the small one. |
| ended_reason | TEXT | yes | Why it was given up on. |
| note | TEXT | yes | |

---

### stock_count_lines

**Added at P26.**

| column | type | null | notes |
|---|---|---|---|
| count_id | TEXT | no | |
| seq | INTEGER | no | Stable per material, so re-counting the same shelf corrects the line rather than adding a second one. |
| material_id | TEXT | no | |
| book_qty | INTEGER | no | **D127 — the book as it was when this line was counted.** Without this column the variance is computed against whatever the shelf says at approval time, which is the bug. |
| book_at | INTEGER | no | When it was frozen. |
| counted_qty | INTEGER | no | |
| typed_qty | INTEGER | no | D109 — what the person wrote on the sheet. |
| typed_unit | TEXT | no | |
| variance_qty | INTEGER | no | `counted − book`. |
| unit_cost | INTEGER | no | Frozen, so a report of last month's count does not change when a price does. |
| variance_value | INTEGER | no | **A variance in kilos is one nobody reads.** A variance in rupees is the one that finds the person taking the paneer home. |
| reason_id | TEXT | yes | `reasons.kind = 'count'`. |
| note | TEXT | yes | |
| movement_id | TEXT | yes | The adjustment this line posted on approval. |

---

### shift_patterns

P28, scope 9.7/9.8. What a shift is *supposed* to be: the shapes a shop's day
comes in. The roster then says who is expected on which one, and attendance is
measured against it.

Minutes from midnight, so a night shift is `start_minute > end_minute` and the
reader can see that it wraps. Two clock strings would mean parsing them in three
places and getting the wrap wrong in one of them.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| name | TEXT | no | |
| start_minute | INTEGER | no | 0–1439. |
| end_minute | INTEGER | no | 0–1439. Less than `start_minute` means it wraps past midnight. |
| break_minutes | INTEGER | no | Unpaid, subtracted from the hours worked. |
| sort_order | INTEGER | no | |
| is_active | INTEGER | no | |

### roster

P28, scope 9.7. Who is **expected**, and when. One row per person per business
day.

The roster is what makes "late" and "absent" mean anything. Without it,
attendance is a list of times with nothing to compare them against — which is
roughly what v1's staff screen amounted to.

**A weekly off is a row with `pattern_id IS NULL`.** The person is expected to
be away, so not turning up is not an absence. That is a different fact from
having *no row at all*, which means nobody has said either way.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| staff_id | TEXT | no | |
| business_day | INTEGER | no | |
| pattern_id | TEXT | yes | NULL is a rostered day OFF. |
| note | TEXT | yes | Why it is off — "Weekly off", "Festival". |
| created_at | INTEGER | no | |
| created_by | TEXT | yes | |

### attendance

P28, scope 9.7. What actually happened: one row per person per shift worked.

**The business day is stamped, not derived** (D5). A shift that starts at 21:00
and ends at 02:30 belongs to the day it *started* in — every hour of it, on the
payroll and on the handover. Re-deriving it from `ended_at` would put half a
night shift on tomorrow, and tomorrow's report would then disagree with the
drawer that was counted at the end of it.

**`terminal_id` and `shift_no` join to `day_closes`** (P27, D140). That row
already exists, already counts one drawer, and already rolls up to the till and
then to the shop. This table says who was standing at it; it does not invent a
second idea of a shift.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| staff_id | TEXT | no | |
| business_day | INTEGER | no | **The day it STARTED in** (D5). |
| terminal_id | TEXT | yes | |
| shift_no | INTEGER | no | Matches `day_closes.shift_no`. 0 is "not tied to a drawer" — a cook clocks in and never touches a till, which is the ordinary case. |
| pattern_id | TEXT | yes | What they were expected to do, if anybody said. |
| started_at | INTEGER | no | |
| ended_at | INTEGER | yes | NULL is still clocked in. Still NULL the next morning is a **missed clock-out**. |
| corrected_at | INTEGER | yes | Set when a manager corrects the row. |
| corrected_by | TEXT | yes | **Never the person's own.** |
| correction_reason | TEXT | yes | Required by a CHECK when corrected. |
| note | TEXT | yes | |

Correcting a clock-out needs `attendance.correct`, writes an audit row with
before and after (R11), and marks the row as corrected — because a correction
is a **state**, not an edit that leaves no trace (D47). It is the one control in
this table that matters: changing a clock-out after the fact is how hours get
inflated.

### leave_types

P28, scope 9.11. The kinds of leave a shop grants. Data, not a hardcoded list —
the same argument as the expense categories and the wastage reasons.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| name | TEXT | no | |
| annual_half_days | INTEGER | yes | NULL is "no entitlement". Unpaid leave has none, and neither does a weekly off. |
| is_paid | INTEGER | no | **The only column payroll reads.** An unpaid day is deducted from the month; every other kind is not. |
| sort_order | INTEGER | no | |
| is_active | INTEGER | no | |

Five are seeded (casual, sick, weekly off, festival, unpaid) as a starting
point a shop edits.

### leave_requests

P28, scope 9.11. A request, and what was decided about it.

**The request is not the balance.** Approving one writes a `taken` row into
`leave_ledger`; rejecting one writes nothing. So a rejected request cannot
affect a balance even by accident, because the balance never reads this table.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| staff_id | TEXT | no | |
| leave_type_id | TEXT | no | |
| from_day | INTEGER | no | |
| to_day | INTEGER | no | |
| half_days | INTEGER | no | Stored, not derived: the calendar it was computed against can change afterwards, and a request must not silently become a different request. |
| reason | TEXT | no | |
| state | TEXT | no | `pending` / `approved` / `rejected` / `cancelled`. |
| requested_at | INTEGER | no | |
| requested_by | TEXT | yes | Usually the person. A manager may enter one on their behalf, and then the two differ and the audit row says so. |
| decided_at | INTEGER | yes | |
| decided_by | TEXT | yes | |
| decision_note | TEXT | yes | **Required by a CHECK on a rejection.** A rejection without a reason is one nobody can appeal. |

### leave_ledger

P28, scope 9.11. **The leave balance, and it is a ledger.**

Four kinds of row, and the balance is their sum. No stored total exists anywhere
in this schema, so there is nothing that can drift:

- **`accrued`** (+) — the entitlement, granted yearly or monthly;
- **`taken`** (−) — a day off that was approved;
- **`adjusted`** (±) — a correction, with a reason and a name;
- **`lapsed`** (−) — what was not used by the end of the year.

(A list rather than a table on purpose: `t17` parses every markdown table under
a `###` heading as this table's column list, so a second table here would be
read as five more columns that do not exist. Found by writing one.)

This is the same shape as credit (D120: one rupee, one row) and stock (D127: a
count posts a delta). A stored `leave_balance` that somebody updates is a number
that will one day disagree with its own history, and that argument is unwinnable
because there is nothing to check it against.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| staff_id | TEXT | no | |
| leave_type_id | TEXT | no | |
| kind | TEXT | no | |
| half_days | INTEGER | no | **Signed here and only here** — the sign is what makes the sum work, and CHECKs tie each `kind` to its direction. |
| request_id | TEXT | yes | The request this came from. A partial UNIQUE index means a request can write exactly one `taken` row, **ever** — approving twice is a constraint violation rather than a doubled deduction somebody finds in March. |
| reason | TEXT | yes | Required by a CHECK on an `adjusted` row: the one row a person writes freehand, and therefore the one somebody could use to invent a fortnight's holiday. |
| at | INTEGER | no | |
| business_day | INTEGER | no | |
| made_by | TEXT | yes | |

### salary_structures

P28, scope 9.9. What a person is paid, and from when.

**Effective-dated, and that is the whole point.** A raise is a new row with a
later `effective_from`, never an edit of the old one — so last month's payroll
run recomputes to the figure it printed, for ever. That is what makes a payslip
something a person can be shown a year later, and it is the same argument D52
makes about a bill line freezing its own price.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| staff_id | TEXT | no | |
| effective_from | INTEGER | no | Business day. |
| basis | TEXT | no | `monthly` / `daily` / `hourly`. |
| amount | INTEGER | no | Paise — per month, per day worked, or per hour worked, by `basis`. |
| created_at | INTEGER | no | |
| created_by | TEXT | yes | |
| note | TEXT | yes | |

### salary_components

P28. The fixed parts on top of, or off, the basis: a food allowance, a room
deduction. Rows rather than columns, so a shop that gives four allowances does
not need a migration.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| structure_id | TEXT | no | Cascades. |
| kind | TEXT | no | `allowance` / `deduction`. |
| name | TEXT | no | |
| amount | INTEGER | no | **Always positive paise.** `kind` carries the direction, never a sign — the rule `credit_adjustments.increases` follows, for the same reason. |
| sort_order | INTEGER | no | |

### salary_advances

P28, scope 9.10. Money handed over before payday, recovered from the next run.
Extremely common and completely normal in a restaurant.

**It moves the drawer on the day it is given**, through `cash_movements` —
because it does. An advance that only appears at month end is a drawer that is
short all month and nobody knows why.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| staff_id | TEXT | no | |
| amount | INTEGER | no | Paise. |
| instalments | INTEGER | no | Over how many runs it comes back. 1 — all of it next month — is the common case. |
| reason | TEXT | yes | |
| given_at | INTEGER | no | |
| business_day | INTEGER | no | |
| given_by | TEXT | yes | |
| cash_movement_id | TEXT | yes | The drawer row this wrote, so the two can never be reconciled by hand. |

### payroll_runs

P28, scope 9.9. One month's payroll.

**Computed, reviewed, then posted**, and the three are different states. The
first thing an owner does with a payroll figure is disagree with one line of it,
so a run is editable while it is a `draft` and a fact once it is `approved`.
Changing an approved run means **reversing** it, which is its own fact with its
own audit row (D47).

Approving writes the cash movement and the expense **in the same transaction**
(D82), so payroll and the cash position cannot disagree — there is only one of
them.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| from_day | INTEGER | no | Business days (D5), inclusive. |
| to_day | INTEGER | no | |
| state | TEXT | no | `draft` / `approved` / `reversed`. |
| computed_at | INTEGER | no | |
| computed_by | TEXT | yes | |
| approved_at | INTEGER | yes | |
| approved_by | TEXT | yes | |
| cash_movement_id | TEXT | yes | |
| expense_id | TEXT | yes | Against the `Salary` category, which P16 already seeds. |
| paid_by | TEXT | no | `cash` / `bank`. |
| reversed_at | INTEGER | yes | |
| reversed_by | TEXT | yes | |
| reversal_reason | TEXT | yes | Required by a CHECK when reversed. |
| note | TEXT | yes | |

### advance_recoveries

P28. What each run took back off which advance. An advance's outstanding balance
is its amount minus the sum of these — a ledger again, and for the third time in
this schema the reason is that a stored `outstanding` column is a column that
will one day be wrong.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| advance_id | TEXT | no | |
| run_id | TEXT | no | |
| amount | INTEGER | no | Paise. |

### payroll_lines

P28. One person's line in one run.

**Every figure is stored rather than recomputed on read.** The structure, the
attendance and the leave calendar can all change after a run is approved, and a
payslip must say the same thing in a year's time as it said on the day it was
handed over. Same argument as D52.

The arithmetic is one column per step so that a payslip can print it and an
owner can add it up by hand — because payroll an owner cannot check by hand is
payroll they will keep doing in a notebook.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| run_id | TEXT | no | Cascades. |
| staff_id | TEXT | no | |
| basis | TEXT | no | Frozen from the structure that applied on the day. |
| basis_amount | INTEGER | no | |
| days_worked_half | INTEGER | no | Half-days. |
| minutes_worked | INTEGER | no | For an hourly basis. |
| unpaid_half_days | INTEGER | no | |
| earned | INTEGER | no | |
| allowances | INTEGER | no | |
| deductions | INTEGER | no | |
| unpaid_leave_deduction | INTEGER | no | |
| advance_recovered | INTEGER | no | |
| net | INTEGER | no | |
| edited | INTEGER | no | True when a person changed a figure by hand before approving. The screen shows it, so a reviewer knows which lines are the computer's and which are somebody's. |
| note | TEXT | yes | |

### rider_handbacks

**Added at P29, scope 14.5. The one table delivery actually needed.**

A delivery paid in cash is settled when the rider hands the food over. So the
bill is paid, the sale is real — and the money is in somebody's pocket on a bike
three kilometres away. Every other cash payment in this product is in the drawer
the moment it is taken. These are not.

A drawer that counts them is a drawer that is short all evening for a reason
nobody can name, and the shortfall walks back in at nine o'clock, which makes it
look like a theft that resolved itself. So `cash_position` subtracts what riders
are carrying, and this table is how it comes back.

A handback is a **ledger row** — how much a rider handed over, when, and who
took it. "What is Kumar carrying" is then:

    cash on his delivered orders today  −  what he has handed back today

and never a stored figure. Third time in this schema, same reason (D120): a
running total on a person is a number that can disagree with the rows that made
it, and the day it does, nobody can tell which one is lying.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| rider_id | TEXT | no | `staff.id`. A rider is a member of staff with `is_rider` set, not a second people table. |
| business_day | INTEGER | no | The STORED business day (D5), like every other money row. |
| amount | INTEGER | no | Paise. Must be positive: a negative handback is money going the other way, which is a payout and has its own table. |
| at | INTEGER | no | |
| taken_by | TEXT | yes | Who was at the till when the money changed hands. **Not the rider** — the whole point of a handback is that two people saw it. |
| terminal_id | TEXT | yes | **0012.** Which till took the money, read as `COALESCE(terminal_id, the master)`. `cash_position_of` scopes `collected` by terminal, so it has to scope the handback the same way — otherwise one till's drawer is short by a handback the other till took. |
| note | TEXT | yes | |

### attachments

**Added at P26. D132 — D69 answered: the photograph is a FILE beside the
database, and this row is its metadata.**

A photographed invoice, downscaled in the webview (D37's precedent — canvas,
1600 px, JPEG 0.7, and therefore **no image dependency in Rust**), is ~200 KB. A
shop takes ~100 deliveries a month, so inside the database that is 240 MB a
year: `VACUUM INTO` would copy all of it and R5/R6 would blow up on the
reference machine's 5400 rpm disk, every second of it spent copying pictures no
query reads.

So the bytes live in `attachments\` beside the database, named by content hash,
and **a backup is now the .db plus that folder plus a manifest**. A row here
with no file is then a DETECTABLE fact and not a mystery, which is the whole
reason the metadata is in the database at all.

| column | type | null | notes |
|---|---|---|---|
| id | TEXT | no | |
| outlet_id | TEXT | no | |
| kind | TEXT | no | `purchase`. |
| subject_id | TEXT | yes | What it is a picture of. **Not a foreign key**: the attachment is written before the purchase row exists — a person photographs the paper, then types the lines — and a constraint forcing the other order would put a modal in the middle of the fastest screen in the module. |
| filename | TEXT | no | `<sha256>.jpg`. Stored rather than derived, so a future format change cannot orphan today's files. |
| byte_count | INTEGER | no | Capped at 500 KB, refused in words above it. |
| sha256 | TEXT | no | What `verify` checks. |
| created_at | INTEGER | no | |
| created_by | TEXT | yes | |

---

## Indexes

Every column a report groups by, filters on or joins through — and nothing else.
Each index costs write time on the billing path and bytes against budget M5, so
one that nobody named does not exist. `t8` asserts the list **in both
directions**: a missing index fails, and so does a stray one.

| index | on | for |
|---|---|---|
| `idx_orders_day` | orders (outlet_id, business_day) | every day report |
| `idx_orders_state` | orders (outlet_id, state) | the table grid's open orders |
| `idx_orders_table` | orders (table_id) partial | opening a table into the cart (B7) |
| `idx_orders_customer` | orders (customer_id) partial | a customer's history (5.4) |
| `idx_orders_created_by` | orders (created_by, business_day) | sales by cashier (9.6) |
| `idx_orders_bill_number` | orders (outlet_id, terminal_id, bill_number_value) **unique**, partial | a bill number is never reused. The terminal is in the key because every till issues out of its OWN series (D135), so two tills both have a bill number 1 and they print as `A/0001` and `B/0001` |
| `idx_orders_token` | orders (outlet_id, terminal_id, business_day, token_value) **unique**, partial | a token is unique within its day and its till, not forever |
| `idx_order_lines_order` | order_lines (order_id) | loading an order |
| `idx_order_lines_item` | order_lines (item_id) partial | item-wise sales (10.2) |
| `idx_bill_lines_order` | bill_lines (order_id) | printing and reprinting |
| `idx_bill_charges_order` | bill_charges (order_id) | printing |
| `idx_payments_order` | payments (order_id) | settlement |
| `idx_payments_day_mode` | payments (business_day, mode) | payment-mode report and the day close |
| `idx_payments_customer` | payments (customer_id) partial | credit outstanding (10.7) |
| `idx_items_category` | items (outlet_id, category_id) | the menu screen (R4) |
| `idx_items_short_code` | items (outlet_id, short_code) partial | scope 1.3, typed at the counter |
| `idx_expenses_day` | expenses (outlet_id, business_day) | expenses and cash position (10.6) |
| `idx_customer_payments_customer` | customer_payments (customer_id, business_day) | statements and ageing (5.3, 10.7) |
| `idx_audit_log_at` | audit_log (at) | the audit trail |
| `idx_audit_log_staff` | audit_log (staff_id, business_day) | who did what |
| `idx_order_events_order` | order_events (order_id) | one order's history |
| `idx_reprints_day` | reprints (business_day) | the reprint report (10.5) |
| `idx_reservations_day` | reservations (outlet_id, business_day) | tonight's bookings (14.4) |
| `idx_sync_outbox_pending` | sync_outbox (created_at) where synced_at is null | the backlog, and it stays the size of the backlog |
| `idx_lan_devices_live` | lan_devices (outlet_id) where revoked_at is null | P19 authenticates on EVERY request, because revocation has to bite on the next request and not on the next login. Partial, so it is the size of the phones in use rather than of every phone ever paired |
| `idx_lan_devices_install` | lan_devices (outlet_id, install_id) where install_id is not null | one seat per install: the pairing looks the install up before it adds a row (0010) |
| `idx_kitchen_live` | kitchen_deliveries (outlet_id, station) where state <> 'bumped' | P24's screen asks "what is outstanding at my station" several times a minute and must not scan a year of tickets. Partial, so it is the size of the kitchen's current work |
| `idx_kitchen_done` | kitchen_deliveries (outlet_id, bumped_at) where bumped_at is not null | the kitchen-speed report (3.7) reads finished tickets by day; an unfinished ticket has no time to report |
| `idx_recipes_item` | recipes (outlet_id, item_id) **unique**, partial | one recipe per dish. Partial rather than a UNIQUE constraint because SQLite treats NULLs as distinct and two of the three owner columns are always NULL |
| `idx_recipes_modifier` | recipes (outlet_id, modifier_id) **unique**, partial | one recipe per modifier |
| `idx_recipes_material` | recipes (outlet_id, material_id) **unique**, partial | one recipe per made material (D111) |
| `idx_recipe_lines_material` | recipe_lines (material_id) | *"what uses this?"* — asked before deactivating a material, and by the where-used panel |
| `idx_stock_movements_material` | stock_movements (outlet_id, material_id, at) | one material's history, newest first. Without it, opening a material is a full scan of the largest table in the module |
| `idx_stock_movements_day` | stock_movements (outlet_id, business_day) | every period report, filtering the STORED day (D5) |
| `idx_stock_movements_order` | stock_movements (order_id) partial | **the void path.** D113 finds the rows a bill wrote and negates them, and must not scan a year to do it |
| `idx_purchases_supplier` | purchases (supplier_id, business_day) | a supplier's ledger, oldest first — read every time somebody asks "what do I owe him" |
| `idx_purchases_day` | purchases (outlet_id, business_day) | every buying report, on the STORED day (D5) |
| `idx_purchase_lines_material` | purchase_lines (material_id) | *"what did I pay for onions, and when did it go up"* — the price-trend report, which is the finding an owner acts on fastest |
| `idx_supplier_payments_supplier` | supplier_payments (supplier_id, business_day) | the ledger and the ageing |
| `idx_supplier_adjustments_supplier` | supplier_adjustments (supplier_id, business_day) | the ledger |
| `idx_stock_counts_day` | stock_counts (outlet_id, business_day) | the variance history |
| `idx_attachments_subject` | attachments (subject_id) partial | finding the photograph of one invoice. Partial, because a row with no subject is a stray this must not index |
| `idx_counters_prefix` | counters (outlet_id, kind, prefix) **unique**, partial | **D135 in the database.** Two tills issuing under one prefix is the ONE way a per-terminal series can still produce a number twice, so it is a constraint and not a convention. The empty-prefix gap is closed in code, in words, because SQLite cannot express "unique unless there is only one row" |
| `idx_day_closes_drawer` | day_closes (outlet_id, business_day, terminal_id, shift_no) **unique**, partial | one close per drawer per shift (D140) |
| `idx_day_closes_shop` | day_closes (outlet_id, business_day) **unique**, partial | exactly one shop roll-up per day. Partial because the shop row's terminal is NULL and SQLite treats NULLs as distinct |
