-- P33 Phase 2 — the tax rework, in the schema.
--
-- `treatment` said two things at once: inclusive-vs-exclusive (a pricing
-- convention) and exempt-vs-non-GST (a legal category). It is now `kind` plus
-- `basis`. That split is what lets liquor carry a state VAT rate at all, so
-- `vat` columns come with it.
--
-- SQLite cannot alter a CHECK, so each table is rebuilt. Foreign keys are off
-- for the duration (the migration runner owns that) and the rebuilds are
-- ordered children-first.

-- --------------------------------------------------------------------------
-- tax_classes
-- --------------------------------------------------------------------------
CREATE TABLE tax_classes_new (
    id         TEXT    NOT NULL PRIMARY KEY,
    outlet_id  TEXT    NOT NULL REFERENCES outlets (id),
    name       TEXT    NOT NULL,
    rate_bp    INTEGER NOT NULL CHECK (rate_bp BETWEEN 0 AND 10000),
    kind       TEXT    NOT NULL
        CHECK (kind IN ('gst', 'exempt', 'outside_gst', 'untaxed')),
    basis      TEXT    NOT NULL CHECK (basis IN ('exclusive', 'inclusive')),
    is_active  INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    sort_order INTEGER NOT NULL DEFAULT 0
) STRICT;

INSERT INTO tax_classes_new (id, outlet_id, name, rate_bp, kind, basis, is_active, sort_order)
SELECT id, outlet_id, name, rate_bp,
       CASE treatment WHEN 'exempt'  THEN 'exempt'
                      WHEN 'non_gst' THEN 'outside_gst'
                      ELSE 'gst' END,
       CASE treatment WHEN 'inclusive' THEN 'inclusive' ELSE 'exclusive' END,
       is_active, sort_order
  FROM tax_classes;

DROP TABLE tax_classes;
ALTER TABLE tax_classes_new RENAME TO tax_classes;

-- The per-order-type override: modelled, stored, written, and read by nobody.
-- Deleted with its code (audit 3.5).
DROP TABLE tax_class_rates;

-- --------------------------------------------------------------------------
-- items
-- --------------------------------------------------------------------------
CREATE TABLE items_new (
    id            TEXT    NOT NULL PRIMARY KEY,
    outlet_id     TEXT    NOT NULL REFERENCES outlets (id),
    category_id   TEXT    REFERENCES categories (id),
    name          TEXT    NOT NULL,
    unit_price    INTEGER NOT NULL,
    tax_class_id  TEXT REFERENCES tax_classes (id),
    tax_rate_bp   INTEGER NOT NULL DEFAULT 0 CHECK (tax_rate_bp BETWEEN 0 AND 10000),
    tax_kind      TEXT    NOT NULL DEFAULT 'gst'
        CHECK (tax_kind IN ('gst', 'exempt', 'outside_gst', 'untaxed')),
    tax_basis     TEXT    NOT NULL DEFAULT 'exclusive'
        CHECK (tax_basis IN ('exclusive', 'inclusive')),
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

INSERT INTO items_new
SELECT id, outlet_id, category_id, name, unit_price, tax_class_id, tax_rate_bp,
       CASE tax_treatment WHEN 'exempt'  THEN 'exempt'
                          WHEN 'non_gst' THEN 'outside_gst'
                          ELSE 'gst' END,
       CASE tax_treatment WHEN 'inclusive' THEN 'inclusive' ELSE 'exclusive' END,
       hsn, cost_price, short_code, prep_minutes, course, is_open_price,
       is_available, sort_order, created_at, updated_at
  FROM items;

DROP TABLE items;
ALTER TABLE items_new RENAME TO items;
CREATE INDEX idx_items_category   ON items (outlet_id, category_id);
CREATE INDEX idx_items_short_code ON items (outlet_id, short_code) WHERE short_code IS NOT NULL;

-- --------------------------------------------------------------------------
-- order_lines
-- --------------------------------------------------------------------------
CREATE TABLE order_lines_new (
    id       TEXT    NOT NULL PRIMARY KEY,
    order_id TEXT    NOT NULL REFERENCES orders (id),
    seq      INTEGER NOT NULL,
    item_id       TEXT REFERENCES items (id),
    variant_id    TEXT REFERENCES item_variants (id),
    name          TEXT    NOT NULL,
    unit_price    INTEGER NOT NULL,
    tax_rate_bp   INTEGER NOT NULL CHECK (tax_rate_bp BETWEEN 0 AND 10000),
    tax_kind      TEXT    NOT NULL
        CHECK (tax_kind IN ('gst', 'exempt', 'outside_gst', 'untaxed')),
    tax_basis     TEXT    NOT NULL CHECK (tax_basis IN ('exclusive', 'inclusive')),
    hsn           TEXT,
    category_id   TEXT,
    qty  INTEGER NOT NULL,
    note TEXT,
    course        TEXT,
    prep_minutes  INTEGER CHECK (prep_minutes IS NULL OR prep_minutes >= 0),
    discount_kind      TEXT CHECK (discount_kind IS NULL OR discount_kind IN ('percent', 'amount')),
    discount_value     INTEGER,
    discount_reason    TEXT,
    discount_by        TEXT REFERENCES staff (id),
    discount_applied   INTEGER,
    discount_requested INTEGER,
    was_discount_capped INTEGER NOT NULL DEFAULT 0
        CHECK (was_discount_capped IN (0, 1)),
    CHECK ((discount_kind IS NULL) = (discount_value IS NULL)),
    CHECK (was_discount_capped = 0 OR discount_kind IS NOT NULL),
    CHECK (qty > 0),
    UNIQUE (order_id, seq)
) STRICT;

INSERT INTO order_lines_new
SELECT id, order_id, seq, item_id, variant_id, name, unit_price, tax_rate_bp,
       CASE tax_treatment WHEN 'exempt'  THEN 'exempt'
                          WHEN 'non_gst' THEN 'outside_gst'
                          ELSE 'gst' END,
       CASE tax_treatment WHEN 'inclusive' THEN 'inclusive' ELSE 'exclusive' END,
       hsn, category_id, qty, note, course, prep_minutes, discount_kind,
       discount_value, discount_reason, discount_by, discount_applied,
       discount_requested, was_discount_capped
  FROM order_lines;

DROP TABLE order_lines;
ALTER TABLE order_lines_new RENAME TO order_lines;
CREATE INDEX idx_order_lines_order ON order_lines (order_id);
CREATE INDEX idx_order_lines_item  ON order_lines (item_id) WHERE item_id IS NOT NULL;

-- --------------------------------------------------------------------------
-- bill_lines. `vat` is 0 on every existing row and that is correct: no bill
-- written before this migration ever charged state VAT.
-- --------------------------------------------------------------------------
CREATE TABLE bill_lines_new (
    order_line_id       TEXT    NOT NULL PRIMARY KEY REFERENCES order_lines (id),
    order_id            TEXT    NOT NULL REFERENCES orders (id),
    gross               INTEGER NOT NULL,
    line_discount       INTEGER NOT NULL,
    bill_discount_share INTEGER NOT NULL,
    net                 INTEGER NOT NULL,
    taxable             INTEGER NOT NULL,
    cgst                INTEGER NOT NULL,
    sgst                INTEGER NOT NULL,
    igst                INTEGER NOT NULL,
    vat                 INTEGER NOT NULL DEFAULT 0,
    gross_including_tax INTEGER NOT NULL,
    rate_bp             INTEGER NOT NULL CHECK (rate_bp BETWEEN 0 AND 10000),
    tax_kind            TEXT    NOT NULL
        CHECK (tax_kind IN ('gst', 'exempt', 'outside_gst', 'untaxed')),
    tax_basis           TEXT    NOT NULL CHECK (tax_basis IN ('exclusive', 'inclusive'))
) STRICT;

INSERT INTO bill_lines_new
SELECT order_line_id, order_id, gross, line_discount, bill_discount_share, net,
       taxable, cgst, sgst, igst, 0, gross_including_tax, rate_bp,
       CASE treatment WHEN 'exempt'  THEN 'exempt'
                      WHEN 'non_gst' THEN 'outside_gst'
                      ELSE 'gst' END,
       CASE treatment WHEN 'inclusive' THEN 'inclusive' ELSE 'exclusive' END
  FROM bill_lines;

DROP TABLE bill_lines;
ALTER TABLE bill_lines_new RENAME TO bill_lines;
CREATE INDEX idx_bill_lines_order ON bill_lines (order_id);

-- --------------------------------------------------------------------------
-- bill_charges. A charge is never alcohol, so it has no vat column.
-- --------------------------------------------------------------------------
CREATE TABLE bill_charges_new (
    id       TEXT    NOT NULL PRIMARY KEY,
    order_id TEXT    NOT NULL REFERENCES orders (id),
    seq      INTEGER NOT NULL,
    kind     TEXT    NOT NULL
        CHECK (kind IN ('service', 'packing', 'delivery', 'other')),
    name     TEXT    NOT NULL,
    basis       TEXT    NOT NULL CHECK (basis IN ('percent', 'flat')),
    basis_value INTEGER NOT NULL,
    amount      INTEGER NOT NULL,
    taxable     INTEGER NOT NULL,
    cgst        INTEGER NOT NULL,
    sgst        INTEGER NOT NULL,
    igst        INTEGER NOT NULL,
    gross_including_tax INTEGER NOT NULL,
    rate_bp     INTEGER NOT NULL CHECK (rate_bp BETWEEN 0 AND 10000),
    tax_kind    TEXT    NOT NULL
        CHECK (tax_kind IN ('gst', 'exempt', 'untaxed')),
    tax_basis   TEXT    NOT NULL CHECK (tax_basis IN ('exclusive', 'inclusive')),
    UNIQUE (order_id, seq)
) STRICT;

INSERT INTO bill_charges_new
SELECT id, order_id, seq, kind, name, basis, basis_value, amount, taxable,
       cgst, sgst, igst, gross_including_tax, rate_bp,
       CASE treatment WHEN 'exempt'  THEN 'exempt'
                      WHEN 'non_gst' THEN 'exempt'
                      ELSE 'gst' END,
       CASE treatment WHEN 'inclusive' THEN 'inclusive' ELSE 'exclusive' END
  FROM bill_charges;

DROP TABLE bill_charges;
ALTER TABLE bill_charges_new RENAME TO bill_charges;
CREATE INDEX idx_bill_charges_order ON bill_charges (order_id);

-- --------------------------------------------------------------------------
-- bills. The registration and the state-tax name are frozen with the bill for
-- the same reason the place of supply already is: leaving the composition
-- scheme must not change what last year's bills reprint as.
-- --------------------------------------------------------------------------
ALTER TABLE bills ADD COLUMN total_vat INTEGER NOT NULL DEFAULT 0;
ALTER TABLE bills ADD COLUMN untaxed_value INTEGER NOT NULL DEFAULT 0;
ALTER TABLE bills ADD COLUMN registration TEXT NOT NULL DEFAULT 'regular'
    CHECK (registration IN ('unregistered', 'composition', 'regular'));
ALTER TABLE bills ADD COLUMN state_tax TEXT NOT NULL DEFAULT 'sgst'
    CHECK (state_tax IN ('sgst', 'utgst'));

-- --------------------------------------------------------------------------
-- store_profile. One three-way registration replaces the boolean, and the
-- shop-wide place of supply goes: restaurant service is always intra-state
-- (IGST Act s.12(4)), so the setting could only ever produce an illegal bill.
-- --------------------------------------------------------------------------
CREATE TABLE store_profile_new (
    outlet_id           TEXT    NOT NULL PRIMARY KEY REFERENCES outlets (id),
    name                TEXT    NOT NULL DEFAULT '',
    address             TEXT    NOT NULL DEFAULT '',
    phone               TEXT,
    gstin               TEXT,
    fssai               TEXT,
    state_code          TEXT,
    upi_id              TEXT,
    upi_merchant_name   TEXT,
    upi_reference       TEXT,
    registration        TEXT    NOT NULL DEFAULT 'regular'
        CHECK (registration IN ('unregistered', 'composition', 'regular')),
    updated_at          INTEGER NOT NULL
) STRICT;

INSERT INTO store_profile_new
SELECT outlet_id, name, address, phone, gstin, fssai, state_code, upi_id,
       upi_merchant_name, upi_reference,
       CASE WHEN is_composition = 1 THEN 'composition' ELSE 'regular' END,
       updated_at
  FROM store_profile;

DROP TABLE store_profile;
ALTER TABLE store_profile_new RENAME TO store_profile;

-- --------------------------------------------------------------------------
-- Reseed the tax vocabulary.
--
-- The 12% slab was abolished on 22 September 2025, so the old seed shipped a
-- class that no longer exists. Liquor now carries a state VAT rate the shop
-- sets — 0 means "not told yet", which the settings screen says out loud.
--
-- Classes an item already points at are left alone; only untouched seeds move.
-- --------------------------------------------------------------------------
UPDATE tax_classes
   SET is_active = 0
 WHERE id = 'tax_packaged_12'
   AND NOT EXISTS (SELECT 1 FROM items WHERE items.tax_class_id = tax_classes.id);

UPDATE tax_classes
   SET kind = 'outside_gst', basis = 'inclusive', name = 'Liquor — state VAT'
 WHERE id = 'tax_liquor';

INSERT OR IGNORE INTO tax_classes (id, outlet_id, name, rate_bp, kind, basis, is_active, sort_order)
SELECT 'tax_goods_5', id, 'Packaged goods 5%', 500, 'gst', 'exclusive', 1, 5
  FROM outlets;
