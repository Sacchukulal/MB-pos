-- 0018 — history retention. Three indexes with no reader of their own go: `idx_order_lines_order`
-- and `idx_payments_order` are prefixes of each table's UNIQUE (order_id, seq), which serves
-- every read by order already; `idx_audit_log_at` had no query on it (the history reads by seq
-- and by staff). Each one was a B-tree written on every bill for nothing.
--
-- The log tables (applied_events, order_events, payment_attempts, kitchen_ledger) gain a
-- retention in code, `mb_db::retention`, run once a business day off the billing path. Money
-- tables are never in that list.
DROP INDEX IF EXISTS idx_order_lines_order;
DROP INDEX IF EXISTS idx_payments_order;
DROP INDEX IF EXISTS idx_audit_log_at;
