-- Notices from Magic Bill, brought down with every cloud check. The bell reads this; seen_at is
-- the counter's own record, which the cloud never keeps.

CREATE TABLE cloud_notices (
    id         TEXT    NOT NULL PRIMARY KEY,
    outlet_id  TEXT    NOT NULL REFERENCES outlets (id),
    title      TEXT    NOT NULL,
    body       TEXT    NOT NULL DEFAULT '',
    starts_at  INTEGER NOT NULL,
    ends_at    INTEGER,
    updated_at INTEGER NOT NULL,
    seen_at    INTEGER,
    is_deleted INTEGER NOT NULL DEFAULT 0 CHECK (is_deleted IN (0, 1))
) STRICT;

CREATE INDEX idx_cloud_notices_unseen ON cloud_notices (outlet_id, starts_at DESC) WHERE seen_at IS NULL AND is_deleted = 0;

-- The owner's switch for a staff member's phone login. Edited on the counter or on the owner's
-- phone; the cloud carries it either way.
ALTER TABLE staff ADD COLUMN can_login_on_phone INTEGER NOT NULL DEFAULT 0 CHECK (can_login_on_phone IN (0, 1));

-- Day totals for days whose bills are not on this computer: a shop brought down from the cloud
-- gets the last 30 days of bills back as bills, and every older day as one row here, so the
-- day-wise report shows the whole year. The report reads a day from here ONLY when it has no
-- bills of its own for that day. Money in paise, days as days since 1970-01-01.
CREATE TABLE cloud_day_totals (
    outlet_id        TEXT    NOT NULL REFERENCES outlets (id),
    business_day     INTEGER NOT NULL,
    bills            INTEGER NOT NULL DEFAULT 0,
    voids            INTEGER NOT NULL DEFAULT 0,
    gross            INTEGER NOT NULL DEFAULT 0,
    discount         INTEGER NOT NULL DEFAULT 0,
    tax              INTEGER NOT NULL DEFAULT 0,
    charges          INTEGER NOT NULL DEFAULT 0,
    net              INTEGER NOT NULL DEFAULT 0,
    by_payment       TEXT    NOT NULL DEFAULT '{}',
    expenses         INTEGER NOT NULL DEFAULT 0,
    credit_given     INTEGER NOT NULL DEFAULT 0,
    credit_collected INTEGER NOT NULL DEFAULT 0,
    is_day_closed    INTEGER NOT NULL DEFAULT 0 CHECK (is_day_closed IN (0, 1)),
    updated_at       INTEGER NOT NULL,
    PRIMARY KEY (outlet_id, business_day)
) STRICT;
