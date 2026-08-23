-- **A kitchen ticket gets a number of its own** — P32, 2026-08-23.
--
-- A cook could not say *"KOT 14"*, because there was no such number. A ticket
-- carried a token when the code path that printed it happened to pass one — and
-- the ordinary "Send to kitchen" button passed `None`, so the common case was a
-- slip of paper with a table on it and nothing a kitchen could refer to.
--
-- The bill and the token have had a per-terminal, per-day series since 0001 and
-- the mechanism is right; the ticket simply was not in it. So this adds a third
-- kind rather than inventing a second numbering scheme beside the first, which
-- is the same argument D135 makes for the series being the terminal.
--
-- SQLite cannot alter a CHECK, so the table is rebuilt the way the SQLite manual
-- prescribes — new table, copy, drop, rename — inside the transaction
-- `apply_all` already holds. The table holds one row per till per series; a shop
-- with two tills has six rows after this.

CREATE TABLE counters_rebuilt (
    outlet_id   TEXT    NOT NULL REFERENCES outlets (id),
    terminal_id TEXT    NOT NULL REFERENCES terminals (id),
    kind        TEXT    NOT NULL CHECK (kind IN ('token', 'bill', 'kot')),
    -- NULL means nothing has been issued yet, which is not the same as zero.
    -- Named for the PAST, exactly like mb-core's `Counter::last_issued()`.
    last_issued INTEGER,
    start       INTEGER NOT NULL DEFAULT 1,
    reset_daily INTEGER NOT NULL DEFAULT 0 CHECK (reset_daily IN (0, 1)),
    prefix      TEXT    NOT NULL DEFAULT '',
    pad_width   INTEGER NOT NULL DEFAULT 0,
    last_reset_day INTEGER,
    PRIMARY KEY (outlet_id, terminal_id, kind)
) STRICT;

-- Columns named rather than `SELECT *`, so a future column added to one side
-- and not the other is an error here instead of a silent shift of every value
-- one place to the left.
INSERT INTO counters_rebuilt (
    outlet_id, terminal_id, kind, last_issued, start, reset_daily,
    prefix, pad_width, last_reset_day
)
SELECT
    outlet_id, terminal_id, kind, last_issued, start, reset_daily,
    prefix, pad_width, last_reset_day
FROM counters;

DROP TABLE counters;
ALTER TABLE counters_rebuilt RENAME TO counters;

-- **D135's guarantee, in the database.** Two tills issuing under the same
-- prefix is the ONE way a per-terminal series can still produce a number twice,
-- so it is a constraint and not a convention. Recreated because the rename does
-- not bring an index with it.
CREATE UNIQUE INDEX idx_counters_prefix
    ON counters (outlet_id, kind, prefix) WHERE prefix <> '';

-- Every till that already exists gets its KOT series, with the prefix its token
-- series already uses — so a two-till shop's tickets cannot collide either.
--
-- **It resets daily**, like the token and unlike the bill: a kitchen talks about
-- "KOT 14" within a shift, and a number that ran to 40,000 would be a number
-- nobody says out loud.
INSERT INTO counters (outlet_id, terminal_id, kind, last_issued, start, reset_daily,
                      prefix, pad_width, last_reset_day)
SELECT outlet_id, terminal_id, 'kot', NULL, 1, 1, prefix, 0, NULL
  FROM counters
 WHERE kind = 'token';
