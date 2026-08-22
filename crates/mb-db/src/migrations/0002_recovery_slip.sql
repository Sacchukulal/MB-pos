-- **The recovery slip is a thing the queue can hold.**
--
-- `mb_auth::recovery` has said since it was written that the shop's recovery
-- code is *"shown once, and printed on the shop's own printer — there is a
-- printer, and paper in a drawer is a better place for this than a
-- screenshot."* The audit log says so out loud too: `recovery.issued` reads
-- back to a shopkeeper as **"New recovery code printed"**.
--
-- Nothing printed it. The code was handed to the screen, shown once, and that
-- was the whole of it — so a shop that closed the dialog without a pen to hand
-- had lost its only way back into its own counter, while the history claimed a
-- slip had come out. Found on 2026-08-22, going through the sign-in path after
-- the owner asked for it to be gone through properly.
--
-- This is the same family as the `day_close` and `delivery` kinds that P30
-- found missing from this CHECK: a job kind the code can make and the database
-- refuses. `every_job_kind_the_queue_can_make_is_allowed_by_the_schema` is what
-- keeps the two lists together, and it fails the moment `JobKind::Recovery`
-- exists without this file.
--
-- SQLite cannot alter a CHECK, so the table is rebuilt the way the SQLite
-- manual prescribes — new table, copy, drop, rename — inside the transaction
-- `apply_all` already holds. **The cost is nothing**: D35 makes this a spool
-- rather than a log, a finished job's row is deleted, and 0001's own comment
-- says a healthy shop holds "between nought and three rows" here.

CREATE TABLE print_jobs_rebuilt (
    id           TEXT    NOT NULL PRIMARY KEY,
    outlet_id    TEXT    NOT NULL REFERENCES outlets (id),
    printer_id   TEXT    NOT NULL REFERENCES printers (id),
    -- The list is here as well as in `JobKind` on purpose, the same argument
    -- the `permissions` table makes: a typo becomes a constraint violation
    -- rather than a silent "unknown kind".
    kind         TEXT    NOT NULL
        CHECK (kind IN ('bill', 'kitchen', 'label', 'test', 'drawer', 'day_close', 'delivery', 'recovery')),
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

-- Columns named rather than `SELECT *`, so a future column added to one side
-- and not the other is a compile-time-shaped error here instead of a silent
-- shift of every value one place to the left.
INSERT INTO print_jobs_rebuilt (
    id, outlet_id, printer_id, kind, state, copies, priority, attempts,
    payload, reason, last_error, engine_used, business_day, created_at, updated_at
)
SELECT
    id, outlet_id, printer_id, kind, state, copies, priority, attempts,
    payload, reason, last_error, engine_used, business_day, created_at, updated_at
FROM print_jobs;

DROP TABLE print_jobs;
ALTER TABLE print_jobs_rebuilt RENAME TO print_jobs;
