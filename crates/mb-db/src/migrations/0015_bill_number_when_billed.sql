-- 0015 — a bill number is born when the bill is, not when the food is ordered.
--
-- Until now an order took its bill number the moment it went on disk: the
-- kitchen ticket, or a phone opening the table. A party that ordered, waited,
-- and walked out before any bill existed then left a cancelled order holding
-- a bill number, and the bill book had a hole with nothing printed to show for
-- it. Three numbers, three moments, the way every bill book works:
--
--   the token   — when the order is taken            (unchanged)
--   the KOT     — when the kitchen ticket prints     (unchanged)
--   the bill    — the FIRST time the bill is printed or paid, never before
--
-- So an open order may have no bill number, and a cancelled one has a number
-- only if a bill had been printed for it — and then the number is used up, the
-- same as a void, because that paper is in the customer's hand. A paid bill
-- always has one. A draft never does.
--
-- SQLite cannot alter a CHECK, so the table is rebuilt the way the manual
-- prescribes — new table, copy, drop, rename — inside the transaction
-- `apply_all` already holds with foreign keys off. The columns are the ones
-- 0001 made; no later migration touched this table. The indexes and the
-- readable view are recreated, because a rename brings neither with it, and
-- the view is dropped first: a rename checks every view in the schema, and one
-- pointing at a table that is momentarily gone would refuse it.

CREATE TABLE orders_rebuilt (
    id           TEXT    NOT NULL PRIMARY KEY,
    outlet_id    TEXT    NOT NULL REFERENCES outlets (id),
    terminal_id  TEXT    NOT NULL REFERENCES terminals (id),
    state        TEXT    NOT NULL
        CHECK (state IN ('draft', 'open', 'settled', 'cancelled', 'voided')),
    business_day INTEGER NOT NULL,
    created_at   INTEGER NOT NULL,
    created_by   TEXT    NOT NULL REFERENCES staff (id),
    order_type   TEXT    NOT NULL
        CHECK (order_type IN ('dine_in', 'parcel', 'self_service', 'delivery')),
    table_id     TEXT    REFERENCES dining_tables (id),
    sub_table    TEXT,
    covers       INTEGER,
    customer_id  TEXT    REFERENCES customers (id),
    note         TEXT,

    token_value        INTEGER,
    token_formatted    TEXT,
    bill_number_value  INTEGER,
    bill_number_formatted TEXT,

    settled_at   INTEGER,
    settled_by   TEXT REFERENCES staff (id),
    cancelled_at INTEGER,
    cancelled_by TEXT REFERENCES staff (id),
    cancel_reason TEXT,
    voided_at    INTEGER,
    voided_by    TEXT REFERENCES staff (id),
    void_reason  TEXT,

    merged_into TEXT REFERENCES orders (id),

    external_order_id TEXT,
    channel           TEXT,
    commission_bp     INTEGER,

    delivery_address TEXT,
    delivery_rider   TEXT,
    delivery_state   TEXT
        CHECK (delivery_state IS NULL
               OR delivery_state IN ('pending', 'assigned', 'out', 'delivered', 'failed')),
    delivery_failure TEXT,

    -- A draft has no bill number; a paid bill always has one; in between, the
    -- number arrives with the bill.
    CHECK (state <> 'draft' OR bill_number_value IS NULL),
    CHECK (state NOT IN ('settled', 'voided') OR bill_number_value IS NOT NULL),
    CHECK ((bill_number_value IS NULL) = (bill_number_formatted IS NULL)),
    CHECK ((token_value IS NULL) = (token_formatted IS NULL)),
    CHECK ((state = 'cancelled') = (cancel_reason IS NOT NULL)),
    CHECK ((state = 'voided') = (void_reason IS NOT NULL)),
    CHECK (cancel_reason IS NULL OR trim(cancel_reason) <> ''),
    CHECK (void_reason IS NULL OR trim(void_reason) <> ''),
    CHECK ((COALESCE(delivery_state, '') = 'failed') = (delivery_failure IS NOT NULL)),
    CHECK (delivery_failure IS NULL OR trim(delivery_failure) <> ''),
    CHECK (state = 'draft' OR order_type <> 'dine_in' OR table_id IS NOT NULL)
) STRICT;

-- Columns named rather than `SELECT *`, so a column on one side and not the
-- other is an error here instead of a silent shift of every value one place.
INSERT INTO orders_rebuilt (
    id, outlet_id, terminal_id, state, business_day, created_at, created_by,
    order_type, table_id, sub_table, covers, customer_id, note,
    token_value, token_formatted, bill_number_value, bill_number_formatted,
    settled_at, settled_by, cancelled_at, cancelled_by, cancel_reason,
    voided_at, voided_by, void_reason, merged_into,
    external_order_id, channel, commission_bp,
    delivery_address, delivery_rider, delivery_state, delivery_failure
)
SELECT
    id, outlet_id, terminal_id, state, business_day, created_at, created_by,
    order_type, table_id, sub_table, covers, customer_id, note,
    token_value, token_formatted, bill_number_value, bill_number_formatted,
    settled_at, settled_by, cancelled_at, cancelled_by, cancel_reason,
    voided_at, voided_by, void_reason, merged_into,
    external_order_id, channel, commission_bp,
    delivery_address, delivery_rider, delivery_state, delivery_failure
FROM orders;

DROP VIEW v_orders_readable;
DROP TABLE orders;
ALTER TABLE orders_rebuilt RENAME TO orders;

CREATE INDEX idx_orders_day        ON orders (outlet_id, business_day);
CREATE INDEX idx_orders_state      ON orders (outlet_id, state);
CREATE INDEX idx_orders_table      ON orders (table_id) WHERE table_id IS NOT NULL;
CREATE INDEX idx_orders_customer   ON orders (customer_id) WHERE customer_id IS NOT NULL;
CREATE INDEX idx_orders_created_by ON orders (created_by, business_day);
CREATE UNIQUE INDEX idx_orders_bill_number
    ON orders (outlet_id, terminal_id, bill_number_value)
    WHERE bill_number_value IS NOT NULL;
CREATE UNIQUE INDEX idx_orders_token
    ON orders (outlet_id, terminal_id, business_day, token_value)
    WHERE token_value IS NOT NULL;

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
