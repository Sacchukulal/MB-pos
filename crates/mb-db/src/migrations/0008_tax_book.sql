-- The tax book: one place for tax.
--
-- Before this, an item carried a COPY of its slab's rate, kind and basis, and the copy could
-- disagree with the slab; charges carried a rate of their own that no slab knew about; and the
-- shop had no say in whether its prices contain the tax. Now:
--
--   * a slab is a rate and a kind; its `basis` is optional (NULL = the shop decides);
--   * the shop has a `price_basis` of its own;
--   * an item points at a slab and may override the basis, and stores nothing else;
--   * a charge points at a slab too.
--
-- Foreign keys are off for the duration (the migration runner owns that).

-- --------------------------------------------------------------------------
-- store_profile: the shop's own pricing default.
-- --------------------------------------------------------------------------
ALTER TABLE store_profile ADD COLUMN price_basis TEXT NOT NULL DEFAULT 'exclusive'
    CHECK (price_basis IN ('exclusive', 'inclusive'));

-- --------------------------------------------------------------------------
-- tax_classes: basis becomes optional.
-- --------------------------------------------------------------------------
CREATE TABLE tax_classes_new (
    id         TEXT    NOT NULL PRIMARY KEY,
    outlet_id  TEXT    NOT NULL REFERENCES outlets (id),
    name       TEXT    NOT NULL,
    rate_bp    INTEGER NOT NULL CHECK (rate_bp BETWEEN 0 AND 10000),
    kind       TEXT    NOT NULL
        CHECK (kind IN ('gst', 'exempt', 'outside_gst', 'untaxed')),
    basis      TEXT    CHECK (basis IS NULL OR basis IN ('exclusive', 'inclusive')),
    is_active  INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    sort_order INTEGER NOT NULL DEFAULT 0
) STRICT;

-- An exclusive class was following the shop's (only) default, so it becomes "no opinion".
-- An inclusive one keeps its opinion.
INSERT INTO tax_classes_new (id, outlet_id, name, rate_bp, kind, basis, is_active, sort_order)
SELECT id, outlet_id, name, rate_bp, kind,
       CASE WHEN basis = 'inclusive' THEN 'inclusive' ELSE NULL END,
       is_active, sort_order
  FROM tax_classes;

DROP TABLE tax_classes;
ALTER TABLE tax_classes_new RENAME TO tax_classes;

-- Two seeded slabs said the same thing ("Restaurant food 5%" and "Packaged goods 5%"). One
-- stays; everything pointing at the other moves across.
UPDATE items SET tax_class_id = 'tax_food_5'
 WHERE tax_class_id = 'tax_goods_5'
   AND EXISTS (SELECT 1 FROM tax_classes WHERE id = 'tax_food_5');
UPDATE categories SET default_tax_class_id = 'tax_food_5'
 WHERE default_tax_class_id = 'tax_goods_5'
   AND EXISTS (SELECT 1 FROM tax_classes WHERE id = 'tax_food_5');
DELETE FROM tax_classes
 WHERE id = 'tax_goods_5'
   AND EXISTS (SELECT 1 FROM tax_classes WHERE id = 'tax_food_5');

-- Seed names say the rate, not a guess at what is sold at it. Only untouched names move.
UPDATE tax_classes SET name = 'GST 5%'  WHERE id = 'tax_food_5'      AND name = 'Restaurant food 5%';
UPDATE tax_classes SET name = 'GST 18%' WHERE id = 'tax_packaged_18' AND name = 'Packaged goods 18%';

-- The abolished 12% slab retires if nothing uses it (0004 did this too; a shop restored from
-- an older cloud copy may have it back).
UPDATE tax_classes
   SET is_active = 0
 WHERE id = 'tax_packaged_12'
   AND NOT EXISTS (SELECT 1 FROM items WHERE items.tax_class_id = tax_classes.id);

-- The two slabs the seed was missing: nil-rated, and the 40% slab of September 2025.
INSERT OR IGNORE INTO tax_classes (id, outlet_id, name, rate_bp, kind, basis, is_active, sort_order)
SELECT 'tax_gst_0', id, 'GST 0%', 0, 'gst', NULL, 1, -1 FROM outlets;
INSERT OR IGNORE INTO tax_classes (id, outlet_id, name, rate_bp, kind, basis, is_active, sort_order)
SELECT 'tax_gst_40', id, 'GST 40%', 4000, 'gst', NULL, 1, 6 FROM outlets;

-- --------------------------------------------------------------------------
-- items: every item gets a slab; the copied columns go.
-- --------------------------------------------------------------------------

-- An item with no slab keeps exactly the tax it had: a slab of that kind and rate, made if
-- the shop has none.
INSERT OR IGNORE INTO tax_classes (id, outlet_id, name, rate_bp, kind, basis, is_active, sort_order)
SELECT DISTINCT
       'tax_' || i.tax_kind || '_' || i.tax_rate_bp,
       i.outlet_id,
       CASE i.tax_kind
            WHEN 'gst'     THEN 'GST ' || CASE WHEN i.tax_rate_bp % 100 = 0
                                               THEN (i.tax_rate_bp / 100)
                                               ELSE printf('%.2f', i.tax_rate_bp / 100.0) END || '%'
            WHEN 'exempt'  THEN 'Exempt'
            WHEN 'untaxed' THEN 'No tax'
            ELSE 'Outside GST'
       END,
       i.tax_rate_bp, i.tax_kind, NULL, 1, 90
  FROM items i
 WHERE i.tax_class_id IS NULL
   AND NOT EXISTS (SELECT 1 FROM tax_classes c
                    WHERE c.outlet_id = i.outlet_id
                      AND c.kind = i.tax_kind AND c.rate_bp = i.tax_rate_bp);

UPDATE items
   SET tax_class_id = (SELECT c.id FROM tax_classes c
                        WHERE c.outlet_id = items.outlet_id
                          AND c.kind = items.tax_kind AND c.rate_bp = items.tax_rate_bp
                        ORDER BY c.is_active DESC, c.sort_order, c.id
                        LIMIT 1)
 WHERE tax_class_id IS NULL;

CREATE TABLE items_new (
    id            TEXT    NOT NULL PRIMARY KEY,
    outlet_id     TEXT    NOT NULL REFERENCES outlets (id),
    category_id   TEXT    REFERENCES categories (id),
    name          TEXT    NOT NULL,
    unit_price    INTEGER NOT NULL,
    tax_class_id  TEXT    NOT NULL REFERENCES tax_classes (id),
    -- NULL = the slab, then the shop, decide.
    price_basis   TEXT    CHECK (price_basis IS NULL OR price_basis IN ('exclusive', 'inclusive')),
    hsn           TEXT,
    cost_price    INTEGER,
    short_code    TEXT,
    prep_minutes  INTEGER,
    course        TEXT,
    is_open_price INTEGER NOT NULL DEFAULT 0 CHECK (is_open_price IN (0, 1)),
    is_available  INTEGER NOT NULL DEFAULT 1 CHECK (is_available IN (0, 1)),
    sort_order    INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
) STRICT;

-- The item keeps its own say only where it differed from what the slab and the shop already
-- said — so nothing prints differently tomorrow.
INSERT INTO items_new (id, outlet_id, category_id, name, unit_price, tax_class_id, price_basis,
                       hsn, cost_price, short_code, prep_minutes, course, is_open_price,
                       is_available, sort_order, created_at, updated_at)
SELECT i.id, i.outlet_id, i.category_id, i.name, i.unit_price, i.tax_class_id,
       CASE WHEN i.tax_basis = COALESCE(c.basis, 'exclusive') THEN NULL ELSE i.tax_basis END,
       i.hsn, i.cost_price, i.short_code, i.prep_minutes, i.course, i.is_open_price,
       i.is_available, i.sort_order, i.created_at, i.updated_at
  FROM items i
  JOIN tax_classes c ON c.id = i.tax_class_id;

DROP TABLE items;
ALTER TABLE items_new RENAME TO items;
CREATE INDEX idx_items_category   ON items (outlet_id, category_id);
CREATE INDEX idx_items_short_code ON items (outlet_id, short_code) WHERE short_code IS NOT NULL;

-- --------------------------------------------------------------------------
-- settings: a charge's tax is a slab now, not a rate of its own.
-- --------------------------------------------------------------------------

-- A rate a charge used that no slab carries gets a slab, so the bill does not change.
INSERT OR IGNORE INTO tax_classes (id, outlet_id, name, rate_bp, kind, basis, is_active, sort_order)
SELECT DISTINCT
       'tax_gst_' || CAST(s.value AS INTEGER),
       s.outlet_id,
       'GST ' || CASE WHEN CAST(s.value AS INTEGER) % 100 = 0
                      THEN (CAST(s.value AS INTEGER) / 100)
                      ELSE printf('%.2f', CAST(s.value AS INTEGER) / 100.0) END || '%',
       CAST(s.value AS INTEGER), 'gst', NULL, 1, 90
  FROM settings s
 WHERE s.key IN ('billing.service_charge_tax_bp', 'billing.packing_charge_tax_bp',
                 'billing.delivery_charge_tax_bp')
   AND NOT EXISTS (SELECT 1 FROM tax_classes c
                    WHERE c.outlet_id = s.outlet_id
                      AND c.kind = 'gst' AND c.rate_bp = CAST(s.value AS INTEGER));

INSERT OR REPLACE INTO settings (outlet_id, key, value, value_type, updated_at, updated_by)
SELECT s.outlet_id,
       replace(s.key, '_tax_bp', '_tax'),
       (SELECT c.id FROM tax_classes c
         WHERE c.outlet_id = s.outlet_id
           AND c.kind = 'gst' AND c.rate_bp = CAST(s.value AS INTEGER)
         ORDER BY c.is_active DESC, c.sort_order, c.id
         LIMIT 1),
       'text', s.updated_at, s.updated_by
  FROM settings s
 WHERE s.key IN ('billing.service_charge_tax_bp', 'billing.packing_charge_tax_bp',
                 'billing.delivery_charge_tax_bp');

DELETE FROM settings
 WHERE key IN ('billing.service_charge_tax_bp', 'billing.packing_charge_tax_bp',
               'billing.delivery_charge_tax_bp');
