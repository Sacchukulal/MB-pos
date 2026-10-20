-- Abandoning an unchanged draft is not approval of a completed correction.
ALTER TABLE bill_reverts ADD COLUMN discarded_at INTEGER;
ALTER TABLE bill_reverts ADD COLUMN discarded_by TEXT REFERENCES staff (id);
