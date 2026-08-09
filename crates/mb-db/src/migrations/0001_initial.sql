-- Magic Bill v2 — the whole schema.
--
-- ONE migration, not twenty-one. Decision D11: there are no existing
-- customers, so there is nothing to preserve. BACKEND-G6 is what the
-- alternative looks like — a schema whose current truth is spread across six
-- files, where "anybody reading the folder from the top gets the wrong answer".
--
-- RULES THIS FILE OBEYS, all of them enforced by tests/schema_rules.rs:
--
--   * Every table is STRICT. SQLite's STRICT mode permits only INT, INTEGER,
--     REAL, TEXT, BLOB and ANY, and it enforces the declared type on every
--     write. Since this file declares nothing but TEXT and INTEGER, a REAL
--     cannot be created and cannot be stored. v1 declared nine REAL columns
--     and every rupee the product ever touched went through one of them (D2).
--     STRICT also makes `BOOLEAN`, `NUMERIC` and `VARCHAR` a syntax error, so
--     v1's 51 BOOLEAN columns — two of which defaulted to NULL, giving a
--     "boolean" three values — cannot happen here either.
--   * Money is INTEGER paise. Quantity is INTEGER thousandths. A tax rate is
--     INTEGER basis points. A timestamp is INTEGER milliseconds since the Unix
--     epoch, UTC. A business day is INTEGER days since 1970-01-01 (D5).
--   * Ids are TEXT (D13). There is no AUTOINCREMENT anywhere: two terminals in
--     one shop collide on integers and there is no way to repair that later.
--   * A boolean column's name begins is_ / has_ / was_ / can_, is NOT NULL,
--     and carries a CHECK (col IN (0,1)).
--   * Every root table carries outlet_id (scope 11.4). Today there is exactly
--     one outlet; the day there is not, nothing has to be back-filled.
--   * NOTHING in the money path cascades on delete. A bill is never deleted —
--     it is voided, which is a state, not an absence.
--   * Every enum is a TEXT tag with a CHECK listing its values, spelled the
--     same way serde spells it, so the counter, the phone and the cloud agree.
--
-- Column-by-column reference, and what reads each one: docs/SCHEMA.md.
-- A test diffs that document against this schema in both directions, so a
-- column added here without being documented fails the build. That is audit
-- finding E10 — v1's dead columns (bill_font_size, logo_opacity, an unused
-- pin) — made impossible rather than discouraged.

-- ===========================================================================
-- THE SHOP
-- ===========================================================================

-- Scope 11.4 — multi-outlet, DESIGN. One row today.
--
-- This is the dimension that cannot be retro-fitted. Adding a table later is
-- free; discovering that every business row needs an outlet after a year of
-- trading means back-filling every table at once with a value nobody can
-- verify.
CREATE TABLE outlets (
    id         TEXT    NOT NULL PRIMARY KEY,
    name       TEXT    NOT NULL,
    is_active  INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at INTEGER NOT NULL
) STRICT;

INSERT INTO outlets (id, name, is_active, created_at) VALUES ('outlet_default', 'Main', 1, 0);

-- One row per outlet. Everything a bill's header and a GST return need.
CREATE TABLE store_profile (
    outlet_id           TEXT    NOT NULL PRIMARY KEY REFERENCES outlets (id),
    name                TEXT    NOT NULL DEFAULT '',
    address             TEXT    NOT NULL DEFAULT '',
    phone               TEXT,
    gstin               TEXT,
    fssai               TEXT,
    state_code          TEXT,
    upi_id              TEXT,
    upi_merchant_name   TEXT,
    -- Audit Part 3, "UPI Payment: UPI ID, merchant name, payment reference".
    -- It rides in the QR payload as `tn`, so the shop's own bank statement says
    -- which counter a payment came from. Added at P17, which is the session
    -- that discovered the third field had nowhere to go.
    upi_reference       TEXT,
    -- Scope 2.10. A composition dealer charges no GST and must print a
    -- declaration instead, so this changes the bill, not just a report.
    is_composition      INTEGER NOT NULL DEFAULT 0 CHECK (is_composition IN (0, 1)),
    -- The default only. The place of supply is stored on each bill, because a
    -- B2B customer in another state changes it for that bill alone (2.4).
    default_place_of_supply TEXT NOT NULL DEFAULT 'intra'
        CHECK (default_place_of_supply IN ('intra', 'inter')),
    updated_at          INTEGER NOT NULL
) STRICT;

-- Scope 11.1 / 11.2, built at P27. Here now because orders and counters
-- reference it, and adding a terminal column to a populated orders table later
-- is exactly the migration this session exists to avoid.
CREATE TABLE terminals (
    id           TEXT    NOT NULL PRIMARY KEY,
    outlet_id    TEXT    NOT NULL REFERENCES outlets (id),
    name         TEXT    NOT NULL,
    is_master    INTEGER NOT NULL DEFAULT 1 CHECK (is_master IN (0, 1)),
    last_seen_at INTEGER,
    -- 11.2: a secondary terminal bills from a reserved block so numbers stay
    -- unique without asking the master for each one.
    block_start  INTEGER,
    block_end    INTEGER,
    created_at   INTEGER NOT NULL
) STRICT;

INSERT INTO terminals (id, outlet_id, name, is_master, created_at)
VALUES ('terminal_default', 'outlet_default', 'Counter', 1, 0);

-- THE E6 FIX.
--
-- Audit E6: "Settings are saved as one giant command with 41 numbered slots.
-- Adding one option means editing three lists that must stay perfectly
-- aligned. This has already caused a 'reuse slot 39 for four columns' patch in
-- the past. It is a silent-wrong-data machine."
--
-- One setting is one row, written by name. There is no positional UPDATE and
-- there never can be. value_type is here so a reader knows how to parse the
-- text without asking the code that wrote it.
CREATE TABLE settings (
    outlet_id  TEXT    NOT NULL REFERENCES outlets (id),
    key        TEXT    NOT NULL,
    value      TEXT    NOT NULL,
    value_type TEXT    NOT NULL CHECK (value_type IN ('int', 'bool', 'text', 'money', 'json')),
    updated_at INTEGER NOT NULL,
    updated_by TEXT,
    PRIMARY KEY (outlet_id, key)
) STRICT;

-- ===========================================================================
-- THE MENU
-- ===========================================================================

-- THE FIX FOR B10 / B11 / B14, which are one finding from three sides: v1 had
-- ONE tax rate for the whole shop, so "it could not bill a bar, an AC/non-AC
-- outlet or anyone selling packaged goods".
--
-- A class is a shop's own name for a rate and a treatment. P00 built the
-- engine; this is how an owner reaches it without choosing basis points four
-- hundred times.
--
-- **A class never reaches a bill** (D52). What reaches a bill is the frozen
-- snapshot on the line — see `items.tax_rate_bp` below, and crown jewel 4.
CREATE TABLE tax_classes (
    id         TEXT    NOT NULL PRIMARY KEY,
    outlet_id  TEXT    NOT NULL REFERENCES outlets (id),
    name       TEXT    NOT NULL,
    rate_bp    INTEGER NOT NULL CHECK (rate_bp BETWEEN 0 AND 10000),
    treatment  TEXT    NOT NULL
        CHECK (treatment IN ('exclusive', 'inclusive', 'exempt', 'non_gst')),
    -- Retired, never deleted: an item may still point at it, and an old bill
    -- names it in its own frozen values.
    is_active  INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    sort_order INTEGER NOT NULL DEFAULT 0
) STRICT;

-- Scope 6.8's tax half: some states tax the same dish differently to take
-- away. P13b owns the per-order-type PRICE and reads this rather than growing
-- a second copy of it.
CREATE TABLE tax_class_rates (
    class_id   TEXT    NOT NULL REFERENCES tax_classes (id) ON DELETE CASCADE,
    order_type TEXT    NOT NULL
        CHECK (order_type IN ('dine_in', 'parcel', 'self_service', 'delivery')),
    rate_bp    INTEGER NOT NULL CHECK (rate_bp BETWEEN 0 AND 10000),
    treatment  TEXT    NOT NULL
        CHECK (treatment IN ('exclusive', 'inclusive', 'exempt', 'non_gst')),
    PRIMARY KEY (class_id, order_type)
) STRICT;

CREATE TABLE categories (
    id         TEXT    NOT NULL PRIMARY KEY,
    outlet_id  TEXT    NOT NULL REFERENCES outlets (id),
    name       TEXT    NOT NULL,
    -- Scope 6.1's setup half: an owner setting up a restaurant chooses
    -- "food 5%" once, not four hundred times. A new item in this category
    -- starts here.
    default_tax_class_id TEXT REFERENCES tax_classes (id),
    -- P14's grid uses it; stored here because it is a property of the
    -- category, not of the screen.
    colour     TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_active  INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

-- The live menu. An order NEVER joins back to this table to print a line —
-- see order_lines, which carries a frozen snapshot instead.
CREATE TABLE items (
    id            TEXT    NOT NULL PRIMARY KEY,
    outlet_id     TEXT    NOT NULL REFERENCES outlets (id),
    category_id   TEXT    REFERENCES categories (id),
    name          TEXT    NOT NULL,
    -- Paise (D2).
    unit_price    INTEGER NOT NULL,
    -- **What the owner CHOSE** (P13). The rate below is what that choice
    -- currently resolves to.
    tax_class_id  TEXT REFERENCES tax_classes (id),
    -- **What the owner GETS, denormalised on purpose.** Basis points: 500 is
    -- 5%. Editing a class rewrites these on every item that points at it, in
    -- one transaction — so the billing path never joins to work out a rate,
    -- the same reason `payments.business_day` is denormalised.
    --
    -- Past bills are untouched by any of it: a line froze its own copy when it
    -- was added (crown jewel 4, D52).
    tax_rate_bp   INTEGER NOT NULL DEFAULT 0 CHECK (tax_rate_bp BETWEEN 0 AND 10000),
    tax_treatment TEXT    NOT NULL DEFAULT 'exclusive'
        CHECK (tax_treatment IN ('exclusive', 'inclusive', 'exempt', 'non_gst')),
    -- Scope 2.5. Optional: alcohol under non_gst has none, and a shop below the
    -- turnover threshold is not required to print one.
    hsn           TEXT,
    -- Scope 4.1, read by P13 and P18's menu-engineering report. Nullable
    -- because a shop that has not costed its menu must not be shown a margin
    -- of 100%.
    cost_price    INTEGER,
    -- Scope 1.3 — typed at the counter instead of the name.
    short_code    TEXT,
    -- Scope 3.6 — the KDS prep-time target.
    prep_minutes  INTEGER,
    -- Sold by weight; the cashier types the price (sweets, meat).
    is_open_price INTEGER NOT NULL DEFAULT 0 CHECK (is_open_price IN (0, 1)),
    is_available  INTEGER NOT NULL DEFAULT 1 CHECK (is_available IN (0, 1)),
    sort_order    INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
) STRICT;

-- Scope 6.1 — half / full, sizes.
CREATE TABLE item_variants (
    id         TEXT    NOT NULL PRIMARY KEY,
    item_id    TEXT    NOT NULL REFERENCES items (id),
    name       TEXT    NOT NULL,
    unit_price INTEGER NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_active  INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1))
) STRICT;

-- Scope 6.2.
CREATE TABLE modifier_groups (
    id         TEXT    NOT NULL PRIMARY KEY,
    outlet_id  TEXT    NOT NULL REFERENCES outlets (id),
    name       TEXT    NOT NULL,
    min_select INTEGER NOT NULL DEFAULT 0,
    max_select INTEGER,
    sort_order INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE TABLE modifiers (
    id          TEXT    NOT NULL PRIMARY KEY,
    group_id    TEXT    NOT NULL REFERENCES modifier_groups (id),
    name        TEXT    NOT NULL,
    -- May be negative: "no cheese, -10" is a real thing on a real menu.
    price_delta INTEGER NOT NULL DEFAULT 0,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    is_active   INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1))
) STRICT;

-- A pure join: the child has no meaning without both parents, so this is one
-- of the two places a cascade is right.
CREATE TABLE item_modifier_groups (
    item_id    TEXT NOT NULL REFERENCES items (id) ON DELETE CASCADE,
    group_id   TEXT NOT NULL REFERENCES modifier_groups (id) ON DELETE CASCADE,
    sort_order INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (item_id, group_id)
) STRICT;

-- Scope 6.3 — a combo's tax is apportioned across its components, which is why
-- the components carry a share rather than the combo carrying one rate.
CREATE TABLE combos (
    id         TEXT    NOT NULL PRIMARY KEY,
    outlet_id  TEXT    NOT NULL REFERENCES outlets (id),
    name       TEXT    NOT NULL,
    unit_price INTEGER NOT NULL,
    is_active  INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE TABLE combo_components (
    combo_id  TEXT    NOT NULL REFERENCES combos (id) ON DELETE CASCADE,
    item_id   TEXT    NOT NULL REFERENCES items (id),
    qty       INTEGER NOT NULL,
    -- Basis points of the combo price attributed to this component, so the
    -- rate-wise summary still adds up when the components differ in rate.
    share_bp  INTEGER NOT NULL CHECK (share_bp BETWEEN 0 AND 10000),
    PRIMARY KEY (combo_id, item_id)
) STRICT;

-- Scope 7.1 / 7.2 / 7.3, built at P07.
CREATE TABLE printers (
    id           TEXT    NOT NULL PRIMARY KEY,
    outlet_id    TEXT    NOT NULL REFERENCES outlets (id),
    name         TEXT    NOT NULL,
    kind         TEXT    NOT NULL CHECK (kind IN ('spooler', 'network', 'serial', 'none')),
    -- A Windows printer name, an ip:port, or a COM port.
    address      TEXT,
    paper_mm     INTEGER NOT NULL DEFAULT 80,
    is_default   INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    can_kick_drawer INTEGER NOT NULL DEFAULT 0 CHECK (can_kick_drawer IN (0, 1)),
    -- Scope 7.11. Thermal printers disagree about where the first dot sits
    -- relative to the paper edge, so the same correct document comes out 2-3 mm
    -- off-centre on one model and centred on another. This is NOT the same
    -- problem as text overflowing: the columns can add up to exactly the paper
    -- width and the whole bill still be shifted.
    --
    -- P06 applies the offset ONCE, at the layout boundary, so all three
    -- renderers inherit it and cannot drift. P07 makes it adjustable from the
    -- test print. P17 puts it on a screen.
    --
    -- Whole millimetres, signed, and they live here rather than arriving in a
    -- later migration because D22 says a column added to a populated table is
    -- the expensive kind. `printers` is empty today; at P07 it will not be.
    -- Clamped in code rather than by a CHECK, because the sane range depends on
    -- the paper width, which is the column next door.
    offset_x_mm  INTEGER NOT NULL DEFAULT 0,
    offset_y_mm  INTEGER NOT NULL DEFAULT 0,
    -- Added at P07, in this file rather than in a migration 0002, for the same
    -- reason the offset was: `printers` is empty on every disk that exists
    -- (D11 — there are no customers), so the columns are free today and an
    -- ALTER TABLE on a live shop tomorrow. This is the LAST cheap moment.
    --
    -- What may be sent here. Routing that ignores this is how a customer's bill
    -- ends up in the tandoor.
    role         TEXT    NOT NULL DEFAULT 'both'
        CHECK (role IN ('bill', 'kitchen', 'both')),
    -- v1's "Print engine: Graphics (prints exactly like the preview, as a
    -- picture) or Text (the printer's own font, faster)" — audit Part 3. Kept,
    -- because a shop that has chosen one should not have it chosen again for
    -- them. 'raster' is P07's name for what v1 called graphics.
    engine       TEXT    NOT NULL DEFAULT 'raster'
        CHECK (engine IN ('raster', 'text')),
    -- v1's "Bold & Dark" print option: emphasis and double-strike on
    -- everything, for a head or a roll that has gone pale.
    is_bold_dark INTEGER NOT NULL DEFAULT 0 CHECK (is_bold_dark IN (0, 1))
) STRICT;

-- Scope 3.1 — which printer a category's kitchen tickets go to.
CREATE TABLE category_printers (
    category_id TEXT NOT NULL REFERENCES categories (id) ON DELETE CASCADE,
    printer_id  TEXT NOT NULL REFERENCES printers (id) ON DELETE CASCADE,
    PRIMARY KEY (category_id, printer_id)
) STRICT;

-- Scope 7.5, built at P07. THE FIX FOR AUDIT D4:
--
--   "A failed print is only a red message on screen. Nothing remembers it. In a
--   rush the cashier misses the toast and the kitchen simply never gets the
--   order."
--
-- A job is written here BEFORE the caller is let go, so a power cut in the
-- middle of a rush loses no kitchen ticket.
--
-- *** THIS IS A SPOOL, NOT A LOG — decision D35, and it is a budget. ***
-- A finished job's row is DELETED. `payload` is the whole document as JSON, two
-- to four kilobytes of it; a bill and its ticket are two rows; 75,000 bills a
-- year would be roughly 450 MB against M5's 400 MB for the ENTIRE database, of
-- which P04 has already spent 318. Keeping print history here would cost more
-- than every bill, line, payment and tax row in the shop put together.
--
-- If "was this bill's ticket printed?" is ever wanted, the answer belongs on
-- `order_events` or in `reprints`, both of which already exist for it.
--
-- In a healthy shop this table holds between nought and three rows.
CREATE TABLE print_jobs (
    id           TEXT    NOT NULL PRIMARY KEY,
    outlet_id    TEXT    NOT NULL REFERENCES outlets (id),
    printer_id   TEXT    NOT NULL REFERENCES printers (id),
    kind         TEXT    NOT NULL
        CHECK (kind IN ('bill', 'kitchen', 'label', 'test', 'drawer')),
    -- 'done' is not a state a row can be in: a done job has no row.
    state        TEXT    NOT NULL DEFAULT 'pending'
        CHECK (state IN ('pending', 'printing', 'failed', 'parked')),
    copies       INTEGER NOT NULL DEFAULT 1,
    -- Lower is sooner. A bill queued behind forty kitchen tickets is a customer
    -- standing at the counter.
    priority     INTEGER NOT NULL DEFAULT 100,
    attempts     INTEGER NOT NULL DEFAULT 0,
    -- The Document, as JSON. Stored rather than a callback, because the whole
    -- point is that it survives the process that made it.
    payload      TEXT    NOT NULL,
    -- Why this was printed, for the queue the cashier looks at: "table 6",
    -- "reprint by Ravi", "test print".
    reason       TEXT,
    last_error   TEXT,
    -- Which sink actually drew it, once it has been tried: a shop whose receipt
    -- suddenly looks different gets an answer instead of making a support call.
    engine_used  TEXT    CHECK (engine_used IS NULL OR engine_used IN ('raster', 'text')),
    -- D5: stamped by whoever created the job, never re-derived.
    business_day INTEGER NOT NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
) STRICT;

-- ===========================================================================
-- THE FLOOR
-- ===========================================================================

CREATE TABLE sections (
    id         TEXT    NOT NULL PRIMARY KEY,
    outlet_id  TEXT    NOT NULL REFERENCES outlets (id),
    name       TEXT    NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_active  INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1))
) STRICT;

-- Named dining_tables, not tables: `FROM tables` reads like a mistake in every
-- query anyone will ever write against this file.
CREATE TABLE dining_tables (
    id         TEXT    NOT NULL PRIMARY KEY,
    outlet_id  TEXT    NOT NULL REFERENCES outlets (id),
    section_id TEXT    REFERENCES sections (id),
    label      TEXT    NOT NULL,
    seats      INTEGER NOT NULL DEFAULT 4,
    -- Scope 14.1, the floor plan. NULL until a table is placed.
    pos_x      INTEGER,
    pos_y      INTEGER,
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_active  INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1))
) STRICT;

-- Scope 14.4.
CREATE TABLE reservations (
    id           TEXT    NOT NULL PRIMARY KEY,
    outlet_id    TEXT    NOT NULL REFERENCES outlets (id),
    table_id     TEXT    REFERENCES dining_tables (id),
    customer_id  TEXT,
    guest_name   TEXT    NOT NULL,
    guest_phone  TEXT,
    covers       INTEGER NOT NULL DEFAULT 2,
    -- D5: a reservation for 00:30 belongs to the evening it was booked for.
    business_day INTEGER NOT NULL,
    expected_at  INTEGER NOT NULL,
    state        TEXT    NOT NULL DEFAULT 'booked'
        CHECK (state IN ('booked', 'seated', 'no_show', 'cancelled')),
    note         TEXT,
    created_at   INTEGER NOT NULL,
    created_by   TEXT
) STRICT;

CREATE TABLE waitlist (
    id           TEXT    NOT NULL PRIMARY KEY,
    outlet_id    TEXT    NOT NULL REFERENCES outlets (id),
    guest_name   TEXT    NOT NULL,
    guest_phone  TEXT,
    covers       INTEGER NOT NULL DEFAULT 2,
    business_day INTEGER NOT NULL,
    joined_at    INTEGER NOT NULL,
    seated_at    INTEGER,
    left_at      INTEGER,
    state        TEXT    NOT NULL DEFAULT 'waiting'
        CHECK (state IN ('waiting', 'seated', 'left'))
) STRICT;

-- ===========================================================================
-- ORDERS AND BILLS — the heart
-- ===========================================================================

-- One row per order in EVERY state. `state` is AnyOrder's serde discriminator,
-- spelled identically, so the counter, the phone and the cloud agree on what a
-- cancelled order is called without a translation layer.
--
-- business_day is STORED (D5) and nothing anywhere re-derives it. Audit B1: v1
-- stored UTC, filtered by local time and grouped reports by the UTC date, so a
-- bill at 00:15 landed on two different days in two different screens and the
-- totals did not tie.
CREATE TABLE orders (
    id           TEXT    NOT NULL PRIMARY KEY,
    outlet_id    TEXT    NOT NULL REFERENCES outlets (id),
    terminal_id  TEXT    NOT NULL REFERENCES terminals (id),
    state        TEXT    NOT NULL
        CHECK (state IN ('draft', 'open', 'settled', 'cancelled', 'voided')),
    -- Days since 1970-01-01. Stamped once, at creation.
    business_day INTEGER NOT NULL,
    created_at   INTEGER NOT NULL,
    created_by   TEXT    NOT NULL REFERENCES staff (id),
    order_type   TEXT    NOT NULL
        CHECK (order_type IN ('dine_in', 'parcel', 'self_service', 'delivery')),
    table_id     TEXT    REFERENCES dining_tables (id),
    -- Scope 1.6 — the 6A / 6B letter. The table is 6; this is the letter.
    sub_table    TEXT,
    -- Scope 1.24.
    covers       INTEGER,
    customer_id  TEXT    REFERENCES customers (id),
    note         TEXT,

    -- The claimed numbers. `formatted` is stored beside `value` and that
    -- duplication is deliberate (P03): a bill number that was PRINTED must not
    -- change because someone edited the prefix setting six months later.
    token_value        INTEGER,
    token_formatted    TEXT,
    bill_number_value  INTEGER,
    bill_number_formatted TEXT,

    settled_at   INTEGER,
    settled_by   TEXT REFERENCES staff (id),
    cancelled_at INTEGER,
    cancelled_by TEXT REFERENCES staff (id),
    -- Audit B5/B6: the reason is compulsory. mb-core takes it as a required
    -- &str, not an Option; the CHECK below is the same rule on disk.
    cancel_reason TEXT,
    voided_at    INTEGER,
    voided_by    TEXT REFERENCES staff (id),
    void_reason  TEXT,

    -- Scope 1.22 — where this order's food went when two tables became one
    -- bill (P14). The order itself is CANCELLED, because its food was sold on
    -- the other bill and counting it twice is the bug; this column is what
    -- tells a report the difference between a merge and a walkout, and gives
    -- "where did table 5's order go" an answer that is a row rather than a
    -- guess. D47 — a correction is a state, never a deletion.
    merged_into TEXT REFERENCES orders (id),

    -- Scope X1, PENDING. Three nullable columns so an aggregator order can
    -- become one later without a migration on a populated orders table. The
    -- integration itself is a commercial decision, not a code one.
    external_order_id TEXT,
    channel           TEXT,
    commission_bp     INTEGER,

    -- Scope 14.5, built at P28.
    delivery_address TEXT,
    delivery_rider   TEXT,
    delivery_state   TEXT
        CHECK (delivery_state IS NULL
               OR delivery_state IN ('pending', 'assigned', 'out', 'delivered', 'failed')),

    -- A draft has no numbers; everything past draft has both.
    CHECK ((state = 'draft') = (bill_number_value IS NULL)),
    CHECK ((bill_number_value IS NULL) = (bill_number_formatted IS NULL)),
    CHECK ((token_value IS NULL) = (token_formatted IS NULL)),
    -- A state that carries a reason carries a non-empty one.
    CHECK ((state = 'cancelled') = (cancel_reason IS NOT NULL)),
    CHECK ((state = 'voided') = (void_reason IS NOT NULL)),
    CHECK (cancel_reason IS NULL OR trim(cancel_reason) <> ''),
    CHECK (void_reason IS NULL OR trim(void_reason) <> ''),
    -- Audit 2.3: a dine-in order must have a table by the time it is open. A
    -- draft may sit there without one while the cashier is still typing.
    CHECK (state = 'draft' OR order_type <> 'dine_in' OR table_id IS NOT NULL)
) STRICT;

-- THE LINE AS TYPED.
--
-- The snapshot columns are the crown jewel the audit says v1 got right: "each
-- order stores its items as a frozen snapshot (name, price, quantity at that
-- moment). If you rename an item or change its price tomorrow, old bills do
-- not change. This is correct and legally safer."
--
-- item_id is therefore never read to print a line. It stays as a plain,
-- non-cascading reference for reporting only — and because it does, an item
-- that has ever been billed cannot be deleted. ON DELETE SET NULL would
-- quietly turn last Diwali's best seller into an unattributed row in item-wise
-- sales (scope 10.2), so a sold item is history in the same way a staff member
-- who has left is (scope 9.15). P13 removes an item from the menu with
-- is_available, not with a DELETE.
CREATE TABLE order_lines (
    id       TEXT    NOT NULL PRIMARY KEY,
    order_id TEXT    NOT NULL REFERENCES orders (id),
    -- The sequence the waiter called the items in. The kitchen ticket reads in
    -- this order and the cart must rebuild in it.
    seq      INTEGER NOT NULL,

    item_id       TEXT REFERENCES items (id),
    variant_id    TEXT REFERENCES item_variants (id),
    -- The snapshot. Read instead of items, always.
    name          TEXT    NOT NULL,
    unit_price    INTEGER NOT NULL,
    tax_rate_bp   INTEGER NOT NULL CHECK (tax_rate_bp BETWEEN 0 AND 10000),
    tax_treatment TEXT    NOT NULL
        CHECK (tax_treatment IN ('exclusive', 'inclusive', 'exempt', 'non_gst')),
    hsn           TEXT,
    category_id   TEXT,

    -- Thousandths of a unit, so 0.5 kg is 500 (scope 1.10).
    qty  INTEGER NOT NULL,
    note TEXT,
    -- Scope 3.5 — course firing. NULL means "with everything else".
    course_no INTEGER,

    -- The line discount as given (D15: the capped flag reaches the bill, so it
    -- has to reach the disk). kind/value are the tag+payload pair from
    -- encode.rs; applied and requested are what the outcome actually was.
    discount_kind      TEXT CHECK (discount_kind IS NULL OR discount_kind IN ('percent', 'amount')),
    discount_value     INTEGER,
    discount_reason    TEXT,
    discount_by        TEXT REFERENCES staff (id),
    discount_applied   INTEGER,
    discount_requested INTEGER,
    -- NOT NULL with a default of 0, rather than nullable "no discount here".
    -- A line with no discount was not capped, and 0 says so; `discount_kind`
    -- already says whether there was a discount at all. Making this nullable
    -- would be v1's `subtotal_bold` again — a boolean with three values.
    was_discount_capped INTEGER NOT NULL DEFAULT 0
        CHECK (was_discount_capped IN (0, 1)),

    CHECK ((discount_kind IS NULL) = (discount_value IS NULL)),
    -- Nothing can be capped that was never asked for.
    CHECK (was_discount_capped = 0 OR discount_kind IS NOT NULL),
    CHECK (qty > 0),
    UNIQUE (order_id, seq)
) STRICT;

-- Snapshot again, for the same reason.
CREATE TABLE order_line_modifiers (
    order_line_id TEXT    NOT NULL REFERENCES order_lines (id),
    seq           INTEGER NOT NULL,
    modifier_id   TEXT REFERENCES modifiers (id),
    name          TEXT    NOT NULL,
    price_delta   INTEGER NOT NULL,
    PRIMARY KEY (order_line_id, seq)
) STRICT;

-- THE COMPUTED BILL HEADER. One row per settled or voided order.
--
-- Stored rather than recomputed, and the reasoning matters:
--
--  * A printed bill is a legal document. It went to a customer and into a GST
--    return. If a rounding setting, a tax rate or a charge definition changes
--    next month, a recomputed "old" bill silently stops matching the paper the
--    customer is holding.
--  * Requirement 7 of the ten is "the printed bill's lines always sum to its
--    printed total, proved by a test, not by inspection". With the computed
--    values in columns that proof is one SQL statement over the whole year.
--    With a recomputation it is a proof about today's code, which is not the
--    claim being made.
--  * Budget R2 gives a year-long report across ~75,000 bills 2.5 seconds.
--    Recomputing them is not 2.5 seconds.
--
-- The rejected alternative was one JSON blob per bill. Smaller, one row, and it
-- turns every report into a full scan with a parser in the loop.
CREATE TABLE bills (
    order_id            TEXT    NOT NULL PRIMARY KEY REFERENCES orders (id),
    subtotal            INTEGER NOT NULL,
    total_line_discount INTEGER NOT NULL,
    total_bill_discount INTEGER NOT NULL,
    total_discount      INTEGER NOT NULL,
    total_charges       INTEGER NOT NULL,
    was_bill_discount_capped INTEGER NOT NULL
        CHECK (was_bill_discount_capped IN (0, 1)),
    -- The bill-level discount as given, so a report can show what was asked
    -- for as well as what was taken.
    bill_discount_kind      TEXT
        CHECK (bill_discount_kind IS NULL OR bill_discount_kind IN ('percent', 'amount')),
    bill_discount_value     INTEGER,
    bill_discount_reason    TEXT,
    bill_discount_by        TEXT REFERENCES staff (id),

    total_taxable  INTEGER NOT NULL,
    total_cgst     INTEGER NOT NULL,
    total_sgst     INTEGER NOT NULL,
    total_igst     INTEGER NOT NULL,
    -- Listed separately on the bill and never inside a GST total: liquor is
    -- outside GST entirely (scope 2.3, and the reason a bar can bill at all).
    non_gst_value  INTEGER NOT NULL,
    exempt_value   INTEGER NOT NULL,
    -- Its own figure, so the printed lines always sum to the printed total.
    round_off      INTEGER NOT NULL,
    grand_total    INTEGER NOT NULL,

    place_of_supply TEXT NOT NULL CHECK (place_of_supply IN ('intra', 'inter')),
    rounding_mode   TEXT NOT NULL
        CHECK (rounding_mode IN ('none', 'nearest_rupee', 'up', 'down')),
    computed_at     INTEGER NOT NULL,

    -- Scope 2.6 — a B2B bill carries the customer's GSTIN, and it is a
    -- snapshot: the customer may correct theirs next year.
    customer_gstin  TEXT,
    customer_name   TEXT,

    -- Scope 2.12, e-invoice (IRP), DESIGN. Nothing sends these yet; they are
    -- here because adding them to a populated bills table later is a migration
    -- on live shops, and a shop crossing the turnover threshold cannot wait for
    -- a release.
    irp_irn         TEXT,
    irp_ack_no      TEXT,
    irp_ack_at      INTEGER,
    irp_signed_qr   TEXT,
    irp_status      TEXT
        CHECK (irp_status IS NULL OR irp_status IN ('pending', 'sent', 'failed', 'cancelled')),
    irp_error       TEXT,

    CHECK ((bill_discount_kind IS NULL) = (bill_discount_value IS NULL))
) STRICT;

-- THE COMPUTED LINE. One row per order_line, once the bill exists.
CREATE TABLE bill_lines (
    order_line_id       TEXT    NOT NULL PRIMARY KEY REFERENCES order_lines (id),
    order_id            TEXT    NOT NULL REFERENCES orders (id),
    -- D4 step 1: effective unit price x quantity.
    gross               INTEGER NOT NULL,
    -- Step 2.
    line_discount       INTEGER NOT NULL,
    -- Step 3: this line's share of the bill discount, spread by floor plus
    -- largest remainder so the shares add back exactly (D14).
    bill_discount_share INTEGER NOT NULL,
    net                 INTEGER NOT NULL,
    -- Step 4, from the discounted net.
    taxable             INTEGER NOT NULL,
    cgst                INTEGER NOT NULL,
    sgst                INTEGER NOT NULL,
    igst                INTEGER NOT NULL,
    -- taxable + tax. For an inclusive-priced line this equals net exactly.
    gross_including_tax INTEGER NOT NULL,
    rate_bp             INTEGER NOT NULL CHECK (rate_bp BETWEEN 0 AND 10000),
    treatment           TEXT    NOT NULL
        CHECK (treatment IN ('exclusive', 'inclusive', 'exempt', 'non_gst'))
) STRICT;

-- Scope 1.14. D17: a percentage charge is taken on the DISCOUNTED line total,
-- never compounds onto another charge, and carries its own tax rate.
CREATE TABLE bill_charges (
    id       TEXT    NOT NULL PRIMARY KEY,
    order_id TEXT    NOT NULL REFERENCES orders (id),
    seq      INTEGER NOT NULL,
    kind     TEXT    NOT NULL
        CHECK (kind IN ('service', 'packing', 'delivery', 'other')),
    -- For kind='other' this is also the label the enum carries.
    name     TEXT    NOT NULL,
    -- One column, two meanings, disambiguated by the tag beside it: basis
    -- points when basis='percent', paise when basis='flat'.
    basis       TEXT    NOT NULL CHECK (basis IN ('percent', 'flat')),
    basis_value INTEGER NOT NULL,
    amount      INTEGER NOT NULL,
    taxable     INTEGER NOT NULL,
    cgst        INTEGER NOT NULL,
    sgst        INTEGER NOT NULL,
    igst        INTEGER NOT NULL,
    gross_including_tax INTEGER NOT NULL,
    rate_bp     INTEGER NOT NULL CHECK (rate_bp BETWEEN 0 AND 10000),
    treatment   TEXT    NOT NULL
        CHECK (treatment IN ('exclusive', 'inclusive', 'exempt', 'non_gst')),
    UNIQUE (order_id, seq)
) STRICT;

-- The rate-wise summary, one row per rate on the bill.
--
-- This is what the GSTR-1 report (scope 2.8) selects from, and it is why that
-- report is a GROUP BY rather than a recomputation of a year of bills. Audit
-- B11: v1 "splits GST 50/50 into CGST/SGST always. No IGST, no inter-state, no
-- HSN summary, and nothing that can be filed directly."
CREATE TABLE bill_tax_rows (
    order_id TEXT    NOT NULL REFERENCES orders (id),
    rate_bp  INTEGER NOT NULL CHECK (rate_bp BETWEEN 0 AND 10000),
    taxable  INTEGER NOT NULL,
    cgst     INTEGER NOT NULL,
    sgst     INTEGER NOT NULL,
    igst     INTEGER NOT NULL,
    PRIMARY KEY (order_id, rate_bp)
) STRICT;

-- MANY per order (scope 1.15). Audit B9: v1 was one bill, one payment mode,
-- "and today you must lie about it".
CREATE TABLE payments (
    id       TEXT    NOT NULL PRIMARY KEY,
    order_id TEXT    NOT NULL REFERENCES orders (id),
    seq      INTEGER NOT NULL,
    mode     TEXT    NOT NULL CHECK (mode IN ('cash', 'card', 'upi', 'credit', 'other')),
    -- The payload half of the enum.
    customer_id TEXT REFERENCES customers (id),
    mode_label  TEXT,
    amount   INTEGER NOT NULL,
    -- Not taxable, never in the GST summary (scope 8.5).
    tip      INTEGER NOT NULL DEFAULT 0,
    -- A UPI reference, a card approval code, a cheque number.
    reference TEXT,
    -- Scope 8.3 / 8.4 — the id the payment device gave back, so an
    -- auto-confirmed UPI or a card terminal can be reconciled later.
    device_ref TEXT,
    -- Audit B12: mode says what it WAS, this says what it DID. v1 recorded a
    -- credit settlement with payment mode "Full Settlement", which is not a
    -- payment mode, and it polluted every payment-mode report.
    settles_credit INTEGER NOT NULL DEFAULT 0 CHECK (settles_credit IN (0, 1)),
    received_at   INTEGER NOT NULL,
    received_by   TEXT REFERENCES staff (id),
    -- D5, denormalised onto the payment so the cash-position report and the day
    -- close never join back to orders to find out which day this was.
    business_day  INTEGER NOT NULL,

    CHECK ((mode = 'credit') = (customer_id IS NOT NULL)),
    CHECK ((mode = 'other') = (mode_label IS NOT NULL)),
    CHECK (amount > 0),
    CHECK (tip >= 0),
    UNIQUE (order_id, seq)
) STRICT;

-- CROWN JEWEL 2, on disk at last.
--
-- Audit Part 10: "The delta KOT. Only what the kitchen has not seen gets
-- printed, and what was printed is remembered IN THE DATABASE, not in the
-- screen's memory."
--
-- identity_key is item + note + SORTED modifier ids, joined with a unit
-- separator. mb-core sorts them (LineIdentity), and that sort is what stops
-- "cheese then no-onion" and "no-onion then cheese" being two different dishes
-- to the kitchen. The rule lives in mb-core and is encoded here, not rewritten.
CREATE TABLE kitchen_ledger (
    order_id     TEXT    NOT NULL REFERENCES orders (id),
    identity_key TEXT    NOT NULL,
    -- Kept alongside the key so a ticket can be reprinted without parsing it.
    item_id      TEXT,
    note         TEXT,
    -- Thousandths, like every other quantity.
    qty_told     INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    PRIMARY KEY (order_id, identity_key)
) STRICT;

-- Scope 1.20 — counted, so the reprint is marked DUPLICATE and the report
-- (10.5) can show who reprinted what.
CREATE TABLE reprints (
    id          TEXT    NOT NULL PRIMARY KEY,
    order_id    TEXT    NOT NULL REFERENCES orders (id),
    printed_at  INTEGER NOT NULL,
    printed_by  TEXT REFERENCES staff (id),
    business_day INTEGER NOT NULL,
    reason      TEXT
) STRICT;

-- Scope 8.7, added at P12: money going back to a customer.
--
-- **NOT a negative payment.** `payments` has `CHECK (amount > 0)` and that
-- check is right — audit B12 is what happens when a table that means "money in"
-- starts holding other things: "v1 recorded a credit settlement with payment
-- mode 'Full Settlement', which is not a payment mode, and it polluted every
-- payment-mode report." A refund is its own fact and gets its own table (D22:
-- adding a table later is the cheap direction).
--
-- Only ever against a voided order, and never for more than was taken. Both
-- rules are the repository's, because both need to read the order.
CREATE TABLE refunds (
    id           TEXT    NOT NULL PRIMARY KEY,
    outlet_id    TEXT    NOT NULL REFERENCES outlets (id),
    order_id     TEXT    NOT NULL REFERENCES orders (id),
    -- Paise, and positive: the direction is the table's name.
    amount       INTEGER NOT NULL CHECK (amount > 0),
    -- How it went back: 'cash', 'card', 'upi', 'other'. Deliberately not the
    -- payment-mode enum — money can go back in cash that came in on a card,
    -- and a shop needs to see that in its drawer count.
    mode         TEXT    NOT NULL,
    reason       TEXT    NOT NULL CHECK (trim(reason) <> ''),
    refunded_at  INTEGER NOT NULL,
    refunded_by  TEXT REFERENCES staff (id),
    -- D5, denormalised like every other money row, so the day's cash position
    -- never joins back to orders to find out which day this was.
    business_day INTEGER NOT NULL
) STRICT;

-- Scope 1.17-1.20, added at P12: the reasons a shop offers for its own
-- corrections.
--
-- **Editable data, not a hardcoded list.** The reasons a chai stall needs are
-- not the reasons a bar needs, and a list in the source is a support call. One
-- table for all four flows, because the reason dialog is one component with
-- four callers.
--
-- A shop may also type free text, and mb-core refuses an empty reason either
-- way — `require_reason` has been there since P03.
CREATE TABLE reasons (
    id         TEXT    NOT NULL PRIMARY KEY,
    outlet_id  TEXT    NOT NULL REFERENCES outlets (id),
    kind       TEXT    NOT NULL
        CHECK (kind IN ('void', 'cancel', 'item_void', 'reprint')),
    text       TEXT    NOT NULL CHECK (trim(text) <> ''),
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_active  INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1))
) STRICT;

-- A cheap append-only trail of what happened to one order, distinct from
-- audit_log: this one is narrow, always written, and safe to sync. audit_log
-- carries before/after JSON and is not.
CREATE TABLE order_events (
    id           TEXT    NOT NULL PRIMARY KEY,
    order_id     TEXT    NOT NULL REFERENCES orders (id),
    at           INTEGER NOT NULL,
    business_day INTEGER NOT NULL,
    event        TEXT    NOT NULL,
    staff_id     TEXT REFERENCES staff (id),
    detail       TEXT
) STRICT;

-- ===========================================================================
-- PEOPLE
-- ===========================================================================

-- Scope 9.15: an employee record is never deleted. Someone who left in March
-- is still on March's bills, March's audit trail and March's payroll.
CREATE TABLE staff (
    id         TEXT    NOT NULL PRIMARY KEY,
    outlet_id  TEXT    NOT NULL REFERENCES outlets (id),
    role_id    TEXT REFERENCES roles (id),
    name       TEXT    NOT NULL,
    code       TEXT,
    -- P11 chooses the algorithm and owns the hashing. The column and this
    -- comment are all P04 has any business deciding.
    pin_hash   TEXT,
    phone      TEXT,
    joined_on  INTEGER,
    status     TEXT    NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'suspended', 'left')),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

-- `is_builtin` means "cannot be DELETED". It does not mean "cannot be edited":
-- a shop whose waiters take payments must be able to say so without asking us.
--
-- The two discount columns are scope 1.12 and they arrived at P11. D18 is why
-- they live on the ROLE and not on the bill: the policy is checked by the
-- caller, never inside `compute_bill`, so an old bill recomputes identically
-- after somebody's role changes.
CREATE TABLE roles (
    id                   TEXT    NOT NULL PRIMARY KEY,
    outlet_id            TEXT    NOT NULL REFERENCES outlets (id),
    name                 TEXT    NOT NULL,
    is_builtin           INTEGER NOT NULL DEFAULT 0 CHECK (is_builtin IN (0, 1)),
    -- Basis points. NULL is "no limit", which is not the same as 0 — 0 is a
    -- role that may not discount at all, and a waiter has exactly that.
    max_discount_bp      INTEGER CHECK (max_discount_bp BETWEEN 0 AND 10000),
    -- Paise. NULL is "no limit".
    max_discount_paise   INTEGER CHECK (max_discount_paise >= 0)
) STRICT;

-- THE BACKEND-G7 FIX.
--
-- "The staff permission map is free-form. Any key can be written; a typo in a
-- permission name silently means 'denied'. There is no list of valid
-- permissions anywhere in the database."
--
-- One row per permission that exists, seeded below. A permission that is not a
-- row cannot be granted, so a typo is a foreign-key violation instead of a
-- silent refusal that nobody can debug from the counter.
CREATE TABLE permissions (
    code        TEXT NOT NULL PRIMARY KEY,
    description TEXT NOT NULL
) STRICT;

CREATE TABLE role_permissions (
    role_id         TEXT NOT NULL REFERENCES roles (id) ON DELETE CASCADE,
    permission_code TEXT NOT NULL REFERENCES permissions (code),
    PRIMARY KEY (role_id, permission_code)
) STRICT;

-- Scope 9.3. before/after are TEXT JSON and this is the ONE place JSON is
-- allowed in the schema: the shape differs per action and nothing queries
-- inside it.
--
-- It must NOT sync by default. It is unbounded, it is the widest row in the
-- product, and nothing on the phone reads it (D16).
-- THE THREE CHAIN COLUMNS ARE P11's, AND THEY ARE WHY THIS LOG CAN BE
-- BELIEVED.
--
-- Audit C4 did not ask for an append-only log. It asked for a **tamper-evident**
-- one, and the two triggers below are not that: they stop this program and
-- every accident inside it, and they stop nobody at all with a SQLite browser,
-- who can drop a trigger in one statement.
--
-- So every row carries sha256(prev_hash ‖ its own fields). Editing a row
-- changes its hash; deleting one leaves a gap in `seq` AND breaks the link;
-- reordering two breaks both. mb_auth::verify_chain reports the first `seq`
-- where it stops making sense, and the audit screen turns that into a sentence
-- with a date in it.
--
-- Evidence, not prevention — and evidence is the honest goal. The shop's own
-- machine must be able to read the shop's own file, so nothing here can STOP an
-- owner with a hex editor. It can make sure they cannot do it quietly.
CREATE TABLE audit_log (
    id           TEXT    NOT NULL PRIMARY KEY,
    outlet_id    TEXT    NOT NULL REFERENCES outlets (id),
    -- Per outlet, gapless, MAX(seq)+1 inside the writing transaction. Exact
    -- because P04 gave this database one writer; it does not need the
    -- `counters` machinery, which exists for numbers that survive being handed
    -- out and not used.
    seq          INTEGER NOT NULL,
    at           INTEGER NOT NULL,
    business_day INTEGER NOT NULL,
    staff_id     TEXT REFERENCES staff (id),
    action       TEXT    NOT NULL,
    entity       TEXT    NOT NULL,
    entity_id    TEXT,
    before_json  TEXT,
    after_json   TEXT,
    -- NULL only on the very first row of a shop's life.
    prev_hash    TEXT,
    hash         TEXT    NOT NULL,
    UNIQUE (outlet_id, seq)
) STRICT;

-- Append-only, at the storage layer (P11 T5). Two triggers, because an UPDATE
-- and a DELETE are two different ways to rewrite history and SQLite needs to be
-- told about each one.
CREATE TRIGGER audit_log_is_append_only_update
BEFORE UPDATE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'the history cannot be changed');
END;

CREATE TRIGGER audit_log_is_append_only_delete
BEFORE DELETE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'the history cannot be deleted');
END;

-- ===========================================================================
-- MONEY OUTSIDE THE BILL
-- ===========================================================================

-- NOTE THE COLUMN THAT IS NOT HERE: there is no balance.
--
-- v1 kept `credit_balance REAL` on this table, beside the payments that make
-- it. That is two sources of truth for what a customer owes, one of them a
-- floating-point number. The balance is the ledger's sum, computed, always.
CREATE TABLE customers (
    id           TEXT    NOT NULL PRIMARY KEY,
    outlet_id    TEXT    NOT NULL REFERENCES outlets (id),
    name         TEXT    NOT NULL,
    phone        TEXT,
    gstin        TEXT,
    address      TEXT,
    -- Scope 5.2. NULL means no limit, which is not the same as a limit of zero.
    credit_limit INTEGER,
    -- The last ten digits of `phone`, and the reason it is stored rather than
    -- computed in the WHERE clause is the UNIQUE INDEX below it: **the phone
    -- is the identity in this market**, and two rows for one number are two
    -- balances for one person. The derived copy has ONE writer (P15,
    -- `MoneyRepo::save_customer`), which is D56's rule applied again.
    -- `phone` keeps what was typed.
    phone_key    TEXT,
    -- Scope 5.7, as days-since-epoch so the report is a range scan.
    birthday     INTEGER,
    anniversary  INTEGER,
    note         TEXT,
    is_active    INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
) STRICT;

-- The credit ledger (scope 5.1). Audit A3: in v1 "payments received against
-- credit are never sent at all", so the cloud could never rebuild a shop's
-- udhaar. Here it is an ordinary table with an ordinary outbox entry.
CREATE TABLE customer_payments (
    id           TEXT    NOT NULL PRIMARY KEY,
    outlet_id    TEXT    NOT NULL REFERENCES outlets (id),
    customer_id  TEXT    NOT NULL REFERENCES customers (id),
    amount       INTEGER NOT NULL,
    mode         TEXT    NOT NULL CHECK (mode IN ('cash', 'card', 'upi', 'other')),
    mode_label   TEXT,
    reference    TEXT,
    received_at  INTEGER NOT NULL,
    received_by  TEXT REFERENCES staff (id),
    business_day INTEGER NOT NULL,
    note         TEXT,
    CHECK ((mode = 'other') = (mode_label IS NOT NULL)),
    CHECK (amount <> 0)
) STRICT;

-- Scope 5.1. An opening balance, a write-off, or a correction: the movements
-- that are neither a sale nor a repayment. Every one carries a reason and a
-- name, because an adjustment with neither is indistinguishable from a
-- mistake — and this is the one table in the credit account somebody could
-- use to make money disappear.
CREATE TABLE credit_adjustments (
    id           TEXT    NOT NULL PRIMARY KEY,
    outlet_id    TEXT    NOT NULL REFERENCES outlets (id),
    customer_id  TEXT    NOT NULL REFERENCES customers (id),
    -- Always positive paise. The direction is `increases`, never a sign: a
    -- negative amount in a ledger is how a subtraction becomes an addition in
    -- somebody's report six months later.
    amount       INTEGER NOT NULL CHECK (amount > 0),
    increases    INTEGER NOT NULL CHECK (increases IN (0, 1)),
    reason       TEXT    NOT NULL,
    at           INTEGER NOT NULL,
    business_day INTEGER NOT NULL,
    made_by      TEXT REFERENCES staff (id),
    CHECK (trim(reason) <> '')
) STRICT;

-- Data, not a hardcoded list.
-- P16. What moved in and out of the DRAWER that is not a sale and not an
-- expense: the opening float, a top-up from the owner, a payout, a bank drop.
--
-- **A cash expense is deliberately NOT in here.** It would be a second row for
-- one fact, and two rows for one fact can disagree — the same argument D65
-- makes about a credit balance. The cash position is a query that adds these
-- to the cash sales and subtracts the cash expenses; the duplicate cannot
-- exist because it is never written.
CREATE TABLE cash_movements (
    id           TEXT    NOT NULL PRIMARY KEY,
    outlet_id    TEXT    NOT NULL REFERENCES outlets (id),
    kind         TEXT    NOT NULL
        CHECK (kind IN ('float', 'top_up', 'payout', 'bank_drop')),
    -- Always positive paise. The direction belongs to the kind, never to a
    -- sign — a negative amount in a ledger is how a subtraction becomes an
    -- addition in somebody's report six months later.
    amount       INTEGER NOT NULL CHECK (amount > 0),
    reason       TEXT    NOT NULL,
    at           INTEGER NOT NULL,
    business_day INTEGER NOT NULL,
    moved_by     TEXT REFERENCES staff (id),
    CHECK (trim(reason) <> '')
) STRICT;

-- P16. Rent, salary, the internet bill. A TEMPLATE and a reminder — nothing
-- here is ever posted automatically, because silently writing money into a
-- shop's books is not acceptable at any level of convenience.
CREATE TABLE recurring_expenses (
    id           TEXT    NOT NULL PRIMARY KEY,
    outlet_id    TEXT    NOT NULL REFERENCES outlets (id),
    category_id  TEXT REFERENCES expense_categories (id),
    description  TEXT    NOT NULL,
    amount       INTEGER NOT NULL CHECK (amount > 0),
    mode         TEXT    NOT NULL DEFAULT 'cash'
        CHECK (mode IN ('cash', 'bank', 'upi', 'card')),
    paid_to      TEXT,
    every        TEXT    NOT NULL CHECK (every IN ('week', 'month')),
    -- Days since epoch. Advanced only when a reminder is CONFIRMED.
    next_due     INTEGER NOT NULL,
    is_active    INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1))
) STRICT;

CREATE TABLE expense_categories (
    id         TEXT    NOT NULL PRIMARY KEY,
    outlet_id  TEXT    NOT NULL REFERENCES outlets (id),
    name       TEXT    NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    is_active  INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1))
) STRICT;

-- Audit A2: "Expenses never reach the cloud — so the owner's phone shows wrong
-- profit." Same table, same outbox, same treatment as a bill.
CREATE TABLE expenses (
    id           TEXT    NOT NULL PRIMARY KEY,
    outlet_id    TEXT    NOT NULL REFERENCES outlets (id),
    category_id  TEXT REFERENCES expense_categories (id),
    description  TEXT    NOT NULL,
    amount       INTEGER NOT NULL,
    -- P16 replaced `is_cash` with the real mode. Whether it came out of the
    -- till still decides the day close's expected cash — that is `mode =
    -- 'cash'` — but a shop reconciling a bank statement needs to tell a
    -- transfer from a UPI payment, and one boolean cannot.
    mode         TEXT    NOT NULL DEFAULT 'cash'
        CHECK (mode IN ('cash', 'bank', 'upi', 'card')),
    -- A NAME, not a supplier record: suppliers and their ledger are P26.
    paid_to      TEXT,
    reference    TEXT,
    -- Scope 2.9, input credit. A shop that buys 1,180 of vegetables at 18%
    -- has 180 it can claim, and v1 could not record it at all. Both columns
    -- or neither.
    gst_rate_bp  INTEGER,
    gst_amount   INTEGER,
    paid_at      INTEGER NOT NULL,
    paid_by      TEXT REFERENCES staff (id),
    business_day INTEGER NOT NULL,
    note         TEXT,
    CHECK (amount > 0),
    CHECK ((gst_rate_bp IS NULL) = (gst_amount IS NULL)),
    -- The tax cannot be more than the money that was spent.
    CHECK (gst_amount IS NULL OR (gst_amount >= 0 AND gst_amount <= amount))
) STRICT;

-- Scope 10.8 and requirement 9 of the ten: "The day can be closed: expected
-- cash vs counted cash, and locked."
--
-- Audit B15: v1 had "no opening cash, no closing cash, no expected vs actual,
-- no Z-report. This is how every restaurant actually closes the day and it does
-- not exist."
CREATE TABLE day_closes (
    id            TEXT    NOT NULL PRIMARY KEY,
    outlet_id     TEXT    NOT NULL REFERENCES outlets (id),
    business_day  INTEGER NOT NULL,
    opening_float INTEGER NOT NULL DEFAULT 0,
    expected_cash INTEGER NOT NULL,
    counted_cash  INTEGER NOT NULL,
    -- counted - expected. Stored rather than derived so the Z-report reprints
    -- identically years later even if a bill is voided afterwards.
    variance      INTEGER NOT NULL,
    is_locked     INTEGER NOT NULL DEFAULT 0 CHECK (is_locked IN (0, 1)),
    closed_at     INTEGER NOT NULL,
    closed_by     TEXT REFERENCES staff (id),
    note          TEXT,
    UNIQUE (outlet_id, business_day)
) STRICT;

-- A child table rather than a JSON column, because the note mix is a report an
-- owner actually asks for ("we are always short of tens") and JSON would make
-- it a scan.
CREATE TABLE day_close_denominations (
    day_close_id TEXT    NOT NULL REFERENCES day_closes (id) ON DELETE CASCADE,
    -- Paise, so a 500 rupee note is 50000 and a 50 paise coin is 50.
    denomination INTEGER NOT NULL,
    count        INTEGER NOT NULL CHECK (count >= 0),
    PRIMARY KEY (day_close_id, denomination)
) STRICT;

-- THE B4 FIX ON DISK.
--
-- Audit B4: "Bill and token numbers are claimed in two steps, not one. The app
-- reads the current number, then increases it in a separate command. A phone
-- order arriving at the exact moment the cashier presses Complete Bill could
-- get the same number." The fix is "one atomic database operation that
-- increments and returns in a single step. Non-negotiable for a bill number."
--
-- Audit B3 lives here too: the daily reset is evaluated INSIDE the claim, not
-- once at app start on a PC that never restarts.
--
-- NOTE: mb-core's `Counter` is NOT persisted as a struct — its fields are
-- private and `last_reset_day` has no setter, so it cannot be rebuilt from a
-- row. It does not need to be. The counter lives here as columns, the claim is
-- one SQL statement, and mb-core's Counter stays the in-memory model P03
-- tested. P17's settings screen reads and writes these columns.
CREATE TABLE counters (
    outlet_id   TEXT    NOT NULL REFERENCES outlets (id),
    terminal_id TEXT    NOT NULL REFERENCES terminals (id),
    kind        TEXT    NOT NULL CHECK (kind IN ('token', 'bill')),
    -- NULL means nothing has been issued yet, which is not the same as zero.
    -- Named for the PAST, exactly like mb-core's `Counter::last_issued()`:
    -- a column called `current` reads as "the number I am about to use", and
    -- that reading is the mistake audit B4 is made of.
    last_issued INTEGER,
    start       INTEGER NOT NULL DEFAULT 1,
    reset_daily INTEGER NOT NULL DEFAULT 0 CHECK (reset_daily IN (0, 1)),
    prefix      TEXT    NOT NULL DEFAULT '',
    pad_width   INTEGER NOT NULL DEFAULT 0,
    last_reset_day INTEGER,
    PRIMARY KEY (outlet_id, terminal_id, kind)
) STRICT;

INSERT INTO counters (outlet_id, terminal_id, kind, last_issued, start, reset_daily, prefix, pad_width)
VALUES ('outlet_default', 'terminal_default', 'token', NULL, 1, 1, '', 0),
       ('outlet_default', 'terminal_default', 'bill', NULL, 1, 0, '', 4);

-- ===========================================================================
-- SYNC AND IDEMPOTENCY
-- ===========================================================================

-- THE A1 / A2 / A3 FIX, and it is one table.
--
-- v1's outbox knew about bills. That is why the owner's phone shows zero
-- expenses forever (A2), why credit repayments have never been backed up (A3),
-- and why A1 says the shop's real asset lives on one hard disk.
--
-- Table-agnostic on purpose: a new synced table is a new value in table_name,
-- not a new outbox.
--
-- THE PAYLOAD IS NOT STORED FOR AN UPSERT. The sender reads the row at send
-- time. That halves the write, keeps M5 down, and means a row edited five times
-- before the next connection syncs ONCE instead of five times — which is D16's
-- 10 MB egress budget, decided here rather than at P33. A delete carries a
-- tombstone because there is nothing left to read.
--
-- And there is deliberately no `is_synced` flag on the business tables: it
-- looks cheaper than an outbox row and it turns every sync into a full scan of
-- every table for dirty rows, forever.
CREATE TABLE sync_outbox (
    id         TEXT    NOT NULL PRIMARY KEY,
    outlet_id  TEXT    NOT NULL REFERENCES outlets (id),
    table_name TEXT    NOT NULL,
    row_id     TEXT    NOT NULL,
    op         TEXT    NOT NULL CHECK (op IN ('upsert', 'delete')),
    tombstone  TEXT,
    created_at INTEGER NOT NULL,
    attempts   INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    synced_at  INTEGER,
    CHECK ((op = 'delete') OR tombstone IS NULL)
) STRICT;

-- Crown jewel 11: "Every phone request is applied exactly once, guarded by a
-- local ledger, so a re-delivered message never double-prints a KOT." v1 had
-- this (applied_order_events) and it was right.
CREATE TABLE applied_events (
    event_id   TEXT    NOT NULL PRIMARY KEY,
    outlet_id  TEXT    NOT NULL REFERENCES outlets (id),
    applied_at INTEGER NOT NULL,
    source     TEXT    NOT NULL,
    result     TEXT
) STRICT;

-- ===========================================================================
-- INDEXES
--
-- Every column a report groups by, filters on or joins through — and nothing
-- else. Each index costs write time on the billing path and bytes against
-- budget M5, so one that nobody named does not exist.
-- ===========================================================================

CREATE INDEX idx_orders_day        ON orders (outlet_id, business_day);
CREATE INDEX idx_orders_state      ON orders (outlet_id, state);
CREATE INDEX idx_orders_table      ON orders (table_id) WHERE table_id IS NOT NULL;
CREATE INDEX idx_orders_customer   ON orders (customer_id) WHERE customer_id IS NOT NULL;
CREATE INDEX idx_orders_created_by ON orders (created_by, business_day);

-- Requirement: a bill number is never reused. mb-core's Counter saturates at
-- the top of its range rather than wrapping, and its comment says in as many
-- words that a repeat "is caught by P04's uniqueness constraint rather than
-- silently reused". This is that constraint.
CREATE UNIQUE INDEX idx_orders_bill_number
    ON orders (outlet_id, bill_number_value)
    WHERE bill_number_value IS NOT NULL;
-- A token resets daily, so it is unique within a business day, not forever.
CREATE UNIQUE INDEX idx_orders_token
    ON orders (outlet_id, business_day, token_value)
    WHERE token_value IS NOT NULL;

CREATE INDEX idx_order_lines_order ON order_lines (order_id);
CREATE INDEX idx_order_lines_item  ON order_lines (item_id) WHERE item_id IS NOT NULL;
CREATE INDEX idx_bill_lines_order  ON bill_lines (order_id);
CREATE INDEX idx_bill_charges_order ON bill_charges (order_id);
CREATE INDEX idx_payments_order    ON payments (order_id);
CREATE INDEX idx_payments_day_mode ON payments (business_day, mode);
CREATE INDEX idx_payments_customer ON payments (customer_id) WHERE customer_id IS NOT NULL;

-- **One phone number is one customer** (P15, scope 5.4). Partial, because a
-- walk-in with no number is normal and a shop has many of them.
CREATE UNIQUE INDEX idx_customers_phone_key
    ON customers (outlet_id, phone_key) WHERE phone_key IS NOT NULL;

CREATE INDEX idx_credit_adjustments_customer ON credit_adjustments (customer_id);

CREATE INDEX idx_items_category    ON items (outlet_id, category_id);
CREATE INDEX idx_items_short_code  ON items (outlet_id, short_code) WHERE short_code IS NOT NULL;

CREATE INDEX idx_expenses_day      ON expenses (outlet_id, business_day);
CREATE INDEX idx_customer_payments_customer ON customer_payments (customer_id, business_day);
CREATE INDEX idx_audit_log_at      ON audit_log (at);
CREATE INDEX idx_audit_log_staff   ON audit_log (staff_id, business_day);
CREATE INDEX idx_order_events_order ON order_events (order_id);
-- reprints reaches its outlet through the order, so the day alone is the key
-- the reprint report (10.5) groups by.
CREATE INDEX idx_reprints_day      ON reprints (business_day);
CREATE INDEX idx_reservations_day  ON reservations (outlet_id, business_day);

-- Partial, so it stays the size of the backlog rather than the size of history.
CREATE INDEX idx_sync_outbox_pending ON sync_outbox (created_at) WHERE synced_at IS NULL;

-- ===========================================================================
-- P19: THE PHONES THIS COUNTER SERVES.
--
-- **D9.** In v1 every tap on a waiter's phone travelled to Mumbai and back, and
-- the server refused the order if the counter had not checked in within five
-- minutes. From P19 the phone talks to the COUNTER, over the shop's own WiFi,
-- and this is the register of which phones are allowed to.
--
-- The shop WiFi is not trustworthy — guests are on it — so a phone is not a
-- device that knows the address, it is a device that has been let in by a
-- person and holds a credential we issued.
--
-- **The certificate and its private key are NOT here.** They live beside the
-- config, in `%APPDATA%\MagicBill`, because a backup taken on one machine is
-- restored onto another (P05) and a restored backup must not resurrect another
-- machine's private key. A certificate is machine identity; this table is shop
-- data.
-- ===========================================================================
CREATE TABLE lan_devices (
    id           TEXT    NOT NULL PRIMARY KEY,
    outlet_id    TEXT    NOT NULL REFERENCES outlets (id),
    -- What the phone calls itself. Shown to the person who approves it, so a
    -- device that appears unexpectedly can be refused by name.
    name         TEXT    NOT NULL CHECK (trim(name) <> ''),
    platform     TEXT    NOT NULL,
    -- **Argon2 of the credential, never the credential.** This database is
    -- copied to a pen drive on purpose (P05); a plaintext secret in it is a
    -- database that hands out access to whoever finds the drive.
    secret_hash  TEXT    NOT NULL,
    -- Nullable on purpose: a shared tablet at the pass belongs to no one
    -- person, and each ACTION on it still names the staff member who did it.
    staff_id     TEXT REFERENCES staff (id),
    paired_at    INTEGER NOT NULL,
    paired_by    TEXT REFERENCES staff (id),
    last_seen_at INTEGER,
    last_ip      TEXT,
    -- **A revoked device is not deleted** — D47's rule, which is that a
    -- correction is a state and never a deletion. An owner asking "which phone
    -- was that, and who took it off?" needs the row to still be here.
    revoked_at   INTEGER,
    revoked_by   TEXT REFERENCES staff (id)
) STRICT;

-- The authentication path reads this on EVERY request (revocation has to bite
-- on the next request, not the next login), so it is an index and not a scan.
CREATE INDEX idx_lan_devices_live ON lan_devices (outlet_id) WHERE revoked_at IS NULL;

-- ===========================================================================
-- VIEWS
--
-- Timestamps are INTEGER milliseconds, which is the right thing to store and
-- the wrong thing to read in a SQLite browser at 11 pm. These views buy the
-- readable half back for nothing — they are computed, never stored, and no
-- code depends on them.
-- ===========================================================================

CREATE VIEW v_orders_readable AS
SELECT o.id,
       o.state,
       o.order_type,
       o.bill_number_formatted,
       date(o.business_day * 86400, 'unixepoch')                 AS business_day_ist,
       datetime(o.created_at / 1000, 'unixepoch', '+05:30')      AS created_at_ist,
       datetime(o.settled_at / 1000, 'unixepoch', '+05:30')      AS settled_at_ist,
       b.grand_total
FROM orders o
LEFT JOIN bills b ON b.order_id = o.id;

-- ===========================================================================
-- SEED: the permission list (BACKEND-G7).
--
-- A permission that is not a row here cannot be granted. P11 builds the roles
-- and the checks; this is the vocabulary they are allowed to use.
-- ===========================================================================

INSERT INTO permissions (code, description) VALUES
    ('bill.create',        'Take an order and settle a bill'),
    ('bill.discount.line', 'Give a discount on one line'),
    ('bill.discount.bill', 'Give a discount on the whole bill'),
    ('bill.void',          'Void a bill that was already settled'),
    ('bill.reprint',       'Reprint a bill'),
    ('order.cancel',       'Cancel an open order'),
    ('order.item.void',    'Void one item after the kitchen was told'),
    ('drawer.open',        'Open the cash drawer without a sale'),
    ('menu.manage',        'Add, edit and price menu items'),
    ('tables.manage',      'Add and arrange tables and sections'),
    ('customers.manage',   'Add customers and change credit limits'),
    ('credit.collect',      'Receive a credit repayment'),
    ('expenses.manage',    'Record and edit expenses'),
    ('reports.view',       'See sales reports'),
    ('reports.export',     'Export reports and raw data'),
    ('day.close',          'Close and lock the business day'),
    ('settings.store',     'Change the store profile and bill design'),
    ('settings.printer',   'Change printer setup'),
    ('settings.tax',       'Change tax settings and numbering'),
    ('staff.manage',       'Add staff, set roles and PINs'),
    ('audit.view',         'Read the audit trail'),
    ('backup.run',         'Take and restore a backup'),
    -- P19. Letting a phone onto the counter is its own decision: the person who
    -- may take an order is not automatically the person who may add a device to
    -- the shop's network.
    ('devices.pair',       'Let a phone onto this counter, and take it off again');

-- ===========================================================================
-- SEED: the reasons a shop starts with (P12, scope 1.17-1.20).
--
-- A starting point, not a law: the table exists so a shop can edit them, and
-- P12's own test asserts a shop's edits survive.
-- ===========================================================================

-- P16, audit B14: v1's expense categories were a hardcoded six-item list in
-- the source, so a shop that spent money on anything else recorded it as
-- "Other" forever. These are a STARTING POINT and every one of them can be
-- renamed, reordered or hidden.
INSERT INTO expense_categories (id, outlet_id, name, sort_order) VALUES
    ('exc_vegetables',  'outlet_default', 'Vegetables',      0),
    ('exc_milk',        'outlet_default', 'Milk',            1),
    ('exc_meat',        'outlet_default', 'Meat and fish',   2),
    ('exc_groceries',   'outlet_default', 'Groceries',       3),
    ('exc_gas',         'outlet_default', 'Gas',             4),
    ('exc_salary',      'outlet_default', 'Salary',          5),
    ('exc_rent',        'outlet_default', 'Rent',            6),
    ('exc_electricity', 'outlet_default', 'Electricity',     7),
    ('exc_water',       'outlet_default', 'Water',           8),
    ('exc_maintenance', 'outlet_default', 'Maintenance',     9),
    ('exc_transport',   'outlet_default', 'Transport',      10),
    ('exc_other',       'outlet_default', 'Other',          11);

INSERT INTO reasons (id, outlet_id, kind, text, sort_order) VALUES
    ('rsn_void_wrong',    'outlet_default', 'void',      'Wrong items billed',        0),
    ('rsn_void_double',   'outlet_default', 'void',      'Billed twice',              1),
    ('rsn_void_price',    'outlet_default', 'void',      'Wrong price',               2),
    ('rsn_void_cust',     'outlet_default', 'void',      'Customer changed their mind', 3),
    ('rsn_void_test',     'outlet_default', 'void',      'Test bill',                 4),

    ('rsn_can_left',      'outlet_default', 'cancel',    'Customer left',             0),
    ('rsn_can_wait',      'outlet_default', 'cancel',    'Waited too long',           1),
    ('rsn_can_stock',     'outlet_default', 'cancel',    'Item not available',        2),
    ('rsn_can_wrong',     'outlet_default', 'cancel',    'Opened on the wrong table', 3),

    ('rsn_itm_wrong',     'outlet_default', 'item_void', 'Ordered by mistake',        0),
    ('rsn_itm_stock',     'outlet_default', 'item_void', 'Not available',             1),
    ('rsn_itm_change',    'outlet_default', 'item_void', 'Customer changed it',       2),
    ('rsn_itm_quality',   'outlet_default', 'item_void', 'Sent back',                 3),

    ('rsn_rep_customer',  'outlet_default', 'reprint',   'Customer asked for a copy', 0),
    ('rsn_rep_jam',       'outlet_default', 'reprint',   'Printer jammed',            1),
    ('rsn_rep_faded',     'outlet_default', 'reprint',   'Print came out faded',      2),
    ('rsn_rep_lost',      'outlet_default', 'reprint',   'Bill was lost',             3);

-- ===========================================================================
-- SEED: the tax classes a shop starts with (P13, audit B10/B11/B14).
--
-- Five, and the one that matters commercially is the liquor line: state excise
-- is not GST at all, and v1 having no way to say so is why a bar could not use
-- it. `mb_auth`-style seeding — a starting point a shop edits, not a list in
-- the source. `mb_core::taxclass::starting_classes()` is the same five, and a
-- test asserts they agree.
-- ===========================================================================

INSERT INTO tax_classes (id, outlet_id, name, rate_bp, treatment, sort_order) VALUES
    ('tax_food_5',       'outlet_default', 'Restaurant food 5%',   500,  'exclusive', 0),
    ('tax_packaged_12',  'outlet_default', 'Packaged goods 12%',   1200, 'exclusive', 1),
    ('tax_packaged_18',  'outlet_default', 'Packaged goods 18%',   1800, 'exclusive', 2),
    ('tax_liquor',       'outlet_default', 'Liquor — outside GST', 0,    'non_gst',   3),
    ('tax_exempt',       'outlet_default', 'Exempt',               0,    'exempt',    4);
