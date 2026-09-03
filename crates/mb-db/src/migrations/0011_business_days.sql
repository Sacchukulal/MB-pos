-- 0011 — a business day is a thing. Until now "closed" meant a shop roll-up row in day_closes
-- for TODAY: yesterday could never be closed once 5 am passed, a bill could still be settled
-- into a closed day, and a day the shop was shut had nowhere to be called a holiday. The day
-- gets its own row: what kind of day it was, whether it is locked, who closed or reopened it,
-- and what it came to. day_closes keeps the drawer counts only; its is_locked stays for the
-- rows already written and is never set again.
CREATE TABLE business_days (
    outlet_id    TEXT    NOT NULL REFERENCES outlets (id),
    -- Days since 1970-01-01, the STORED business day (D5).
    business_day INTEGER NOT NULL,
    kind         TEXT    NOT NULL DEFAULT 'trading' CHECK (kind IN ('trading', 'holiday')),
    is_locked    INTEGER NOT NULL DEFAULT 0 CHECK (is_locked IN (0, 1)),
    closed_at    INTEGER,
    closed_by    TEXT REFERENCES staff (id),
    reopened_at  INTEGER,
    reopened_by  TEXT REFERENCES staff (id),
    note         TEXT,
    -- Frozen when the day is closed, so the figure the owner saw is the figure that stays.
    bills        INTEGER NOT NULL DEFAULT 0,
    net          INTEGER NOT NULL DEFAULT 0,
    cash_taken   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (outlet_id, business_day)
) STRICT;

-- Every shop roll-up row so far becomes the day it stood for, locked or not, with the day's
-- figures read from the bills as they are now.
INSERT INTO business_days (outlet_id, business_day, kind, is_locked, closed_at, closed_by, note,
                           bills, net, cash_taken)
SELECT d.outlet_id,
       d.business_day,
       'trading',
       d.is_locked,
       d.closed_at,
       d.closed_by,
       d.note,
       (SELECT COUNT(*)
          FROM orders o JOIN bills b ON b.order_id = o.id
         WHERE o.outlet_id = d.outlet_id AND o.business_day = d.business_day
           AND o.state IN ('settled', 'voided')),
       (SELECT COALESCE(SUM(b.grand_total), 0)
          FROM orders o JOIN bills b ON b.order_id = o.id
         WHERE o.outlet_id = d.outlet_id AND o.business_day = d.business_day
           AND o.state = 'settled'),
       (SELECT COALESCE(SUM(p.amount), 0)
          FROM payments p JOIN orders o ON o.id = p.order_id
         WHERE o.outlet_id = d.outlet_id AND o.business_day = d.business_day
           AND o.state = 'settled' AND p.mode = 'cash')
  FROM day_closes d
 WHERE d.terminal_id IS NULL;
