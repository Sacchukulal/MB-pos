ALTER TABLE orders ADD COLUMN billing_account TEXT NOT NULL DEFAULT '{}';
-- Correction returns have already reduced the current payment projection. They must not
-- reduce it a second time when the corrected bill is later voided and refunded.
ALTER TABLE refunds ADD COLUMN is_adjustment INTEGER NOT NULL DEFAULT 0 CHECK (is_adjustment IN (0, 1));

-- Editing a paid bill does not withdraw its sale or payments from the books.
CREATE TABLE order_edits (
    order_id TEXT PRIMARY KEY REFERENCES orders(id),
    snapshot TEXT NOT NULL
) STRICT;

-- Complete issued versions remain available even when the current projection changes.
CREATE TABLE bill_versions (
    id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL REFERENCES orders(id),
    revision INTEGER NOT NULL,
    snapshot TEXT NOT NULL,
    UNIQUE (order_id, revision)
) STRICT;
CREATE TRIGGER bill_versions_no_update BEFORE UPDATE ON bill_versions
BEGIN SELECT RAISE(ABORT, 'issued bill versions are immutable'); END;
CREATE TRIGGER bill_versions_no_delete BEFORE DELETE ON bill_versions
BEGIN SELECT RAISE(ABORT, 'issued bill versions are immutable'); END;
