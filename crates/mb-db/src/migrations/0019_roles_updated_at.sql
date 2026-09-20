-- 0019 — a role has a moment. Every other row the cloud carries wears its own `updated_at`, and
-- the rule on both sides is "newest wins per row"; a role had no stamp, so one from the cloud
-- was applied unconditionally and could undo an edit made at the counter a minute ago. Existing
-- rows are stamped 0: any real stamp wins over them.
ALTER TABLE roles ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
