-- 0012 — the two money rows that never said which drawer they landed in. Cash from a credit
-- collection is a note going into the box under one till, and a rider's handback is another;
-- every other money row already names its terminal, so the drawer figure counted these on
-- every till or on none. `customer_payments` was left out of the expected drawer entirely,
-- which is why a shop that takes a credit repayment in cash read "over" by exactly that
-- amount every time.
ALTER TABLE customer_payments ADD COLUMN terminal_id TEXT REFERENCES terminals (id);
ALTER TABLE rider_handbacks   ADD COLUMN terminal_id TEXT REFERENCES terminals (id);

-- The rows already written belong to whichever till was the shop's main one, which is the same
-- rule `cash_position_of` reads them by: COALESCE(terminal_id, the master).
