-- 0013 — a paid bill taken back to the counter to be fixed and billed again under the SAME
-- number: the bill book keeps no gap. The order goes back to open and settles again; this is
-- the register beside it, with the bill as it was before, who took it back, why, and who
-- signed it off afterwards.
CREATE TABLE bill_reverts (
    id                TEXT    NOT NULL PRIMARY KEY,
    outlet_id         TEXT    NOT NULL REFERENCES outlets (id),
    order_id          TEXT    NOT NULL REFERENCES orders (id),
    business_day      INTEGER NOT NULL,
    reason            TEXT    NOT NULL CHECK (trim(reason) <> ''),
    reverted_at       INTEGER NOT NULL,
    reverted_by       TEXT    REFERENCES staff (id),
    -- Paise: what the bill came to before it was taken back.
    before_total      INTEGER NOT NULL,
    before_settled_at INTEGER NOT NULL,
    before_settled_by TEXT    REFERENCES staff (id),
    approved_at       INTEGER,
    approved_by       TEXT    REFERENCES staff (id)
) STRICT;

-- The bill's lines as they were, so the register can say what was removed or changed.
CREATE TABLE bill_revert_lines (
    id         TEXT    NOT NULL PRIMARY KEY,
    revert_id  TEXT    NOT NULL REFERENCES bill_reverts (id),
    seq        INTEGER NOT NULL,
    name       TEXT    NOT NULL,
    -- Thousandths, like order_lines.qty.
    qty        INTEGER NOT NULL,
    unit_price INTEGER NOT NULL,
    amount     INTEGER NOT NULL
) STRICT;

-- And how it had been paid.
CREATE TABLE bill_revert_payments (
    id        TEXT    NOT NULL PRIMARY KEY,
    revert_id TEXT    NOT NULL REFERENCES bill_reverts (id),
    seq       INTEGER NOT NULL,
    mode      TEXT    NOT NULL,
    amount    INTEGER NOT NULL
) STRICT;

CREATE INDEX idx_bill_reverts_day            ON bill_reverts (outlet_id, business_day);
CREATE INDEX idx_bill_reverts_order          ON bill_reverts (order_id);
CREATE INDEX idx_bill_revert_lines_revert    ON bill_revert_lines (revert_id);
CREATE INDEX idx_bill_revert_payments_revert ON bill_revert_payments (revert_id);

-- Taking a bill back is the cashier's job; signing the register is a manager's.
INSERT INTO permissions (code, description) VALUES
    ('bill.revert',         'Take a paid bill back to the counter to fix it and bill again'),
    ('bill.revert.approve', 'Approve a bill that was taken back and billed again');

-- The roles a shop already has get the rows the presets would have given them: whoever may
-- void may take a bill back and sign the register, and the built-in cashier may take one back.
INSERT OR IGNORE INTO role_permissions (role_id, permission_code)
    SELECT role_id, 'bill.revert' FROM role_permissions WHERE permission_code = 'bill.void';
INSERT OR IGNORE INTO role_permissions (role_id, permission_code)
    SELECT role_id, 'bill.revert.approve' FROM role_permissions WHERE permission_code = 'bill.void';
INSERT OR IGNORE INTO role_permissions (role_id, permission_code)
    SELECT id, 'bill.revert' FROM roles WHERE id = 'role_cashier';
