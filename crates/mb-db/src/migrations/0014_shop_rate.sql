-- The shop's own GST rate.
--
-- An item is taxed at its own slab; a category may name a rate for everything in it; and the
-- shop names the rate for everything else. The shop's rate lives beside its pricing default.

ALTER TABLE store_profile ADD COLUMN default_tax_class_id TEXT REFERENCES tax_classes (id);

-- A shop that already has a menu is on the slab most of its menu is on, so nothing it sells
-- changes; a shop with no menu yet starts on the seeded 5% slab.
UPDATE store_profile
   SET default_tax_class_id = (
       SELECT c.id
         FROM tax_classes c
        WHERE c.outlet_id = store_profile.outlet_id AND c.is_active = 1
        ORDER BY (SELECT COUNT(*) FROM items i WHERE i.tax_class_id = c.id) DESC,
                 CASE WHEN c.id = 'tax_food_5' THEN 0 ELSE 1 END,
                 c.sort_order
        LIMIT 1
   )
 WHERE default_tax_class_id IS NULL;
