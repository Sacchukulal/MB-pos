ALTER TABLE items ADD COLUMN parcel_charge INTEGER NOT NULL DEFAULT 0
    CHECK (parcel_charge >= 0);
ALTER TABLE order_lines ADD COLUMN parcel_charge INTEGER NOT NULL DEFAULT 0
    CHECK (parcel_charge >= 0);
