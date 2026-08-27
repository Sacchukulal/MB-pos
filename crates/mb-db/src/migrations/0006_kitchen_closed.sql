-- A kitchen ticket can be CLOSED: the counter is done with the order (a void, a cancellation
-- somebody saw, or the day closed on it), so it is nothing more for the kitchen. The CHECK on
-- `state` has to admit the word, and SQLite cannot change a CHECK, so the table is rebuilt.

CREATE TABLE kitchen_deliveries_rebuilt (
    id           TEXT    NOT NULL PRIMARY KEY,
    outlet_id    TEXT    NOT NULL REFERENCES outlets (id),
    order_id     TEXT    NOT NULL REFERENCES orders (id),
    station      TEXT    NOT NULL,
    course       TEXT,
    expected_minutes INTEGER,
    -- See mb-core's `kitchen_delivery`: the state machine is there, and this only stores its answer.
    state        TEXT    NOT NULL
        CHECK (state IN ('pending', 'shown', 'bumped', 'printed', 'closed')),
    sent_at      INTEGER NOT NULL,
    business_day INTEGER NOT NULL,
    shown_at     INTEGER,
    bumped_at    INTEGER,
    bumped_by    TEXT REFERENCES staff (id),
    bumped_on    TEXT REFERENCES lan_devices (id),
    bumped_lines TEXT    NOT NULL DEFAULT '[]',
    cancelled_at INTEGER,
    acked_at     INTEGER
) STRICT;

INSERT INTO kitchen_deliveries_rebuilt (
    id, outlet_id, order_id, station, course, expected_minutes, state, sent_at,
    business_day, shown_at, bumped_at, bumped_by, bumped_on, bumped_lines, cancelled_at, acked_at
)
SELECT
    id, outlet_id, order_id, station, course, expected_minutes, state, sent_at,
    business_day, shown_at, bumped_at, bumped_by, bumped_on, bumped_lines, cancelled_at, acked_at
FROM kitchen_deliveries;

DROP TABLE kitchen_deliveries;
ALTER TABLE kitchen_deliveries_rebuilt RENAME TO kitchen_deliveries;

CREATE INDEX idx_kitchen_live ON kitchen_deliveries (outlet_id, station)
    WHERE state NOT IN ('bumped', 'closed');
CREATE INDEX idx_kitchen_done ON kitchen_deliveries (outlet_id, bumped_at)
    WHERE bumped_at IS NOT NULL;

-- Tickets for orders the counter finished hours ago: nothing more for the kitchen.
UPDATE kitchen_deliveries
   SET state = 'closed'
 WHERE state NOT IN ('bumped', 'closed')
   AND sent_at < (CAST(strftime('%s', 'now') AS INTEGER) - 6 * 3600) * 1000
   AND order_id IN (SELECT id FROM orders WHERE state <> 'open');
