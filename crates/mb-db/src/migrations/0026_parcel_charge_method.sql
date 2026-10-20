-- Keep the calculation method with the issued bill, independent of later settings.
ALTER TABLE bills ADD COLUMN parcel_charge_basis TEXT NOT NULL DEFAULT 'legacy'
    CHECK (parcel_charge_basis IN ('legacy', 'per_item', 'per_bill'));

-- Preserve existing item charges. Never turn an old self-service flat fee into an
-- automatic Parcel fee. Its amount remains available when Per bill is selected.
INSERT INTO settings (outlet_id, key, value, value_type, updated_at)
SELECT o.id, 'billing.parcel_charge_method', 'per_item', 'text', o.created_at
FROM outlets o
WHERE (EXISTS (SELECT 1 FROM items i
               WHERE i.outlet_id = o.id AND i.parcel_charge > 0)
       OR EXISTS (SELECT 1 FROM order_lines l JOIN orders r ON r.id = l.order_id
                  WHERE r.outlet_id = o.id AND l.parcel_charge > 0))
  AND NOT EXISTS (SELECT 1 FROM settings s
                  WHERE s.outlet_id = o.id AND s.key = 'billing.parcel_charge_method');
-- Shops without item charges use the new Off default; no setting row is needed.
