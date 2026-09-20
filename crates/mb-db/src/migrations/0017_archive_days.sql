-- 0017 — the day file's ledger. The permanent history of a shop is one gzip file per business
-- day in the cloud's Storage, built from this database once the day is sealed (locked, or
-- three days old) and uploaded once under the counter's own login. This table is the only
-- record of that: when the day sealed, when a bill of it was last saved (dirty), when its file
-- last went up and what that file was. Nothing here is money; a row is derived from the bills
-- and can be rebuilt by uploading again.
CREATE TABLE archive_days (
    outlet_id    TEXT    NOT NULL REFERENCES outlets (id),
    -- Days since 1970-01-01, the STORED business day (D5).
    business_day INTEGER NOT NULL,
    -- When the day was first seen sealed by the archive step. NULL while it is open.
    sealed_at    INTEGER,
    -- The last moment a bill, refund or revert of this day was saved. Set at the one place
    -- every bill save passes through, so a change after the upload makes the day dirty.
    dirty_at     INTEGER,
    -- When the file last went up. NULL is never. Pending is: sealed AND (never uploaded OR
    -- dirty after the upload).
    uploaded_at  INTEGER,
    -- SHA-256 of the gzip that went up, lowercase hex.
    sha256       TEXT,
    bills        INTEGER NOT NULL DEFAULT 0,
    bytes        INTEGER NOT NULL DEFAULT 0,
    last_error   TEXT,
    PRIMARY KEY (outlet_id, business_day)
) STRICT;
