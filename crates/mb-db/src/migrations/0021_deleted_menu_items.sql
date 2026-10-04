-- Deleted dishes stay here for old bills and for Put back. Existing unavailable
-- dishes remain unavailable: their original reason cannot be inferred.
ALTER TABLE items ADD COLUMN is_deleted INTEGER NOT NULL DEFAULT 0 CHECK (is_deleted IN (0, 1));
CREATE INDEX items_menu_state ON items(outlet_id, is_deleted, is_available, sort_order, name);
