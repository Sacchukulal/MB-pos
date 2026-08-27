-- A table belongs to a dine-in order only. Rows that broke that take the table's word
-- (they were opened on a table, so they are dine-in); a dine-in row with no table is a counter
-- order. A seat letter without a table means nothing.
UPDATE orders SET order_type = 'dine_in' WHERE table_id IS NOT NULL AND order_type <> 'dine_in';
UPDATE orders SET order_type = 'self_service' WHERE table_id IS NULL AND order_type = 'dine_in';
UPDATE orders SET sub_table = NULL WHERE table_id IS NULL;
