-- A closed sale is immutable. A full return carries its own current business day and
-- frozen amounts; refunds remain separate records of money actually handed back.
CREATE TABLE bill_returns (
    id TEXT NOT NULL PRIMARY KEY,
    outlet_id TEXT NOT NULL REFERENCES outlets(id),
    order_id TEXT NOT NULL UNIQUE REFERENCES orders(id),
    terminal_id TEXT NOT NULL REFERENCES terminals(id),
    business_day INTEGER NOT NULL,
    original_business_day INTEGER NOT NULL,
    returned_at INTEGER NOT NULL,
    returned_by TEXT NOT NULL REFERENCES staff(id),
    reason TEXT NOT NULL CHECK(length(trim(reason)) > 0),
    stock_disposition TEXT NOT NULL DEFAULT 'not_restocked' CHECK(stock_disposition = 'not_restocked'),
    bill_number TEXT NOT NULL,
    order_type TEXT NOT NULL,
    table_id TEXT,
    grand_total INTEGER NOT NULL,
    total_charges INTEGER NOT NULL,
    total_discount INTEGER NOT NULL,
    total_taxable INTEGER NOT NULL,
    total_cgst INTEGER NOT NULL,
    total_sgst INTEGER NOT NULL,
    total_igst INTEGER NOT NULL,
    total_vat INTEGER NOT NULL,
    CHECK(business_day != original_business_day)
) STRICT;
CREATE INDEX idx_bill_returns_day ON bill_returns(outlet_id, business_day);

CREATE TABLE bill_return_lines (
    id TEXT NOT NULL PRIMARY KEY,
    return_id TEXT NOT NULL REFERENCES bill_returns(id),
    seq INTEGER NOT NULL,
    item_id TEXT,
    name TEXT NOT NULL,
    category_id TEXT,
    hsn TEXT,
    qty INTEGER NOT NULL CHECK(qty > 0),
    gross_including_tax INTEGER NOT NULL,
    line_discount INTEGER NOT NULL,
    bill_discount_share INTEGER NOT NULL,
    taxable INTEGER NOT NULL,
    cgst INTEGER NOT NULL,
    sgst INTEGER NOT NULL,
    igst INTEGER NOT NULL,
    vat INTEGER NOT NULL,
    rate_bp INTEGER NOT NULL,
    tax_kind TEXT NOT NULL,
    UNIQUE(return_id, seq)
) STRICT;

-- These views are the shared signed reporting source. An operational bill lookup
-- still reads the original orders; reports see the original sale and its later return.
CREATE VIEW report_sale_orders AS
SELECT id, id AS source_order_id, outlet_id, terminal_id, state, business_day,
       settled_at, settled_by, order_type, table_id, 1 AS sale_count
  FROM orders
UNION ALL
SELECT id, order_id, outlet_id, terminal_id, 'settled', business_day,
       returned_at, returned_by, order_type, table_id, 0
  FROM bill_returns;

CREATE VIEW report_sale_bills AS
SELECT order_id, grand_total, total_discount, total_taxable, total_charges,
       total_cgst, total_sgst, total_igst, total_vat FROM bills
UNION ALL
SELECT id, -grand_total, -total_discount, -total_taxable, -total_charges,
       -total_cgst, -total_sgst, -total_igst, -total_vat FROM bill_returns;

CREATE VIEW report_order_lines AS
SELECT id, order_id, seq, item_id, name, category_id, hsn, qty FROM order_lines
UNION ALL
SELECT id, return_id, seq, item_id, name, category_id, hsn, -qty FROM bill_return_lines;

CREATE VIEW report_bill_lines AS
SELECT order_line_id, order_id, gross_including_tax, line_discount, bill_discount_share,
       taxable, cgst, sgst, igst, vat, rate_bp, tax_kind FROM bill_lines
UNION ALL
SELECT id, return_id, -gross_including_tax, -line_discount, -bill_discount_share,
       -taxable, -cgst, -sgst, -igst, -vat, rate_bp, tax_kind FROM bill_return_lines;

CREATE VIEW report_sale_payments AS
SELECT order_id, CASE WHEN mode = 'other' THEN 'other:' || lower(trim(mode_label)) ELSE mode END AS mode, amount FROM payments
UNION ALL
-- Attribute the full cancelled sale to its original tenders. Refund rows describe
-- actual payouts separately: a payout tomorrow must not rewrite today's sales.
SELECT r.id, CASE WHEN p.mode = 'other' THEN 'other:' || lower(trim(p.mode_label)) ELSE p.mode END, -p.amount
 FROM payments p JOIN bill_returns r ON r.order_id = p.order_id;
