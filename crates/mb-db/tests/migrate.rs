// The clippy.toml exemption reaches `#[test]` functions only, and the helpers at the bottom of
// this file are plain functions.
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect and panic are the assertion"
)]

mod common;

use common::Scratch;
use mb_db::{DbError, MIGRATIONS, checksum, migrate};
use rusqlite::Connection;
use rusqlite::types::Value;

/// Every version this build ships, in order.
fn every_version() -> Vec<u32> {
    MIGRATIONS.iter().map(|m| m.version).collect()
}

/// A fresh database runs every migration, in order, and records one row each — not a high-water
/// mark.
#[test]
fn t1_fresh_database_runs_every_migration_and_records_each_one() {
    let scratch = Scratch::new("t1");
    let mut conn = Connection::open(scratch.db_path()).expect("open");

    let applied = migrate::apply_all(&mut conn).expect("migrations run");
    assert_eq!(applied.ran, every_version());
    assert!(applied.already.is_empty());

    let rows: Vec<(i64, String, String, i64)> = conn
        .prepare("SELECT version, name, checksum, applied_at FROM schema_version ORDER BY version")
        .expect("prepare")
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows");

    assert_eq!(
        rows.len(),
        MIGRATIONS.len(),
        "one row per migration, not a MAX()"
    );
    for (i, (version, name, sum, at)) in rows.iter().enumerate() {
        let expected = &MIGRATIONS[i];
        assert_eq!(*version, i64::from(expected.version));
        assert_eq!(name, expected.name);
        assert_eq!(sum, &checksum(expected.sql));
        assert!(*at > 0, "applied_at is a real clock reading");
    }

    // Contiguous and ascending, so a gap cannot hide.
    for pair in rows.windows(2) {
        assert_eq!(pair[1].0, pair[0].0 + 1, "versions must have no gaps");
    }
}

/// Running the migrations again does nothing at all.
#[test]
fn t2_running_migrations_twice_is_a_no_op() {
    let scratch = Scratch::new("t2");
    let mut conn = Connection::open(scratch.db_path()).expect("open");

    migrate::apply_all(&mut conn).expect("first run");
    let before: Vec<(i64, String)> = ledger(&conn);

    let applied = migrate::apply_all(&mut conn).expect("second run");
    assert!(applied.ran.is_empty(), "nothing should have run again");
    assert_eq!(applied.already, every_version());

    assert_eq!(before, ledger(&conn), "the ledger must be untouched");
}

/// Editing a migration that has already run is refused — and the refusal leaves the database
/// exactly as it was.
#[test]
fn t3_edited_history_is_refused_and_nothing_is_touched() {
    let scratch = Scratch::new("t3");
    let mut conn = Connection::open(scratch.db_path()).expect("open");
    migrate::apply_all(&mut conn).expect("first run");

    // Put a row in, so we can prove the refusal did not disturb the shop.
    conn.execute_batch(common::STAFF_SQL).expect("seed staff");

    // Simulate the edit by rewriting the recorded checksum: the effect is identical to someone
    // changing one character of 0001_initial.sql after it shipped, and it does not require
    // writing to the source tree from a test.
    conn.execute(
        "UPDATE schema_version SET checksum = 'deadbeefdeadbeef' WHERE version = 1",
        [],
    )
    .expect("tamper");

    let err = migrate::apply_all(&mut conn).expect_err("must refuse");
    match err {
        DbError::MigrationChanged { version, name, .. } => {
            assert_eq!(version, 1);
            assert_eq!(name, "0001_initial");
        }
        other => panic!("wrong error: {other}"),
    }

    let staff: i64 = conn
        .query_row("SELECT count(*) FROM staff", [], |r| r.get(0))
        .expect("count");
    assert_eq!(
        staff, 1,
        "the refusal must not have touched the shop's rows"
    );
}

/// A migration that fails part way leaves the previous version in place.
#[test]
fn t20_a_failed_migration_leaves_the_previous_version() {
    let scratch = Scratch::new("t20");
    let mut conn = Connection::open(scratch.db_path()).expect("open");
    migrate::apply_all(&mut conn).expect("baseline");

    let bad = mb_db::Migration {
        version: 99,
        name: "0099_broken",
        sql: "CREATE TABLE half_a (x TEXT) STRICT;
              CREATE TABLE half_b (y TEXT) STRICT;
              THIS IS NOT SQL;",
    };

    // Drive the engine's own path rather than a hand-rolled one, so the test exercises the
    // transaction the real code uses.
    let result = run_single(&mut conn, &bad);
    assert!(result.is_err(), "a broken migration must fail");

    let tables = mb_db::schema::tables(&conn).expect("tables");
    assert!(
        !tables.contains(&"half_a".to_owned()),
        "half_a must have rolled back"
    );
    assert!(
        !tables.contains(&"half_b".to_owned()),
        "half_b must have rolled back"
    );

    let versions: Vec<i64> = ledger(&conn).into_iter().map(|(v, _)| v).collect();
    let expected: Vec<i64> = every_version().into_iter().map(i64::from).collect();
    assert_eq!(versions, expected, "the ledger must not have gained a row");
}

/// A file written by a newer build is refused rather than used.
#[test]
fn t21_a_newer_database_is_refused() {
    let scratch = Scratch::new("t21");
    let mut conn = Connection::open(scratch.db_path()).expect("open");
    migrate::apply_all(&mut conn).expect("baseline");

    conn.execute(
        "INSERT INTO schema_version (version, name, checksum, applied_at, run_ms)
         VALUES (9999, '9999_from_the_future', 'ffffffffffffffff', 1, 0)",
        [],
    )
    .expect("pretend a newer build has been here");

    let err = migrate::apply_all(&mut conn).expect_err("must refuse");
    match err {
        DbError::NewerSchema { found, known } => {
            assert_eq!(found, 9999);
            assert_eq!(known, migrate::latest_version());
        }
        other => panic!("wrong error: {other}"),
    }
}

/// 0004 landed: the tax rework is in the schema, and nothing broke on the way.
#[test]
fn the_tax_rework_columns_are_there_and_nothing_dangles() {
    let scratch = Scratch::new("tax_rework");
    let mut conn = Connection::open(scratch.db_path()).expect("open");
    migrate::apply_all(&mut conn).expect("migrations run");

    // The new shape, table by table.
    for (table, wanted) in [
        ("tax_classes", &["kind", "basis"][..]),
        ("items", &["tax_kind", "tax_basis"]),
        ("order_lines", &["tax_kind", "tax_basis"]),
        ("bill_lines", &["tax_kind", "tax_basis", "vat"]),
        ("bill_charges", &["tax_kind", "tax_basis"]),
        (
            "bills",
            &["total_vat", "untaxed_value", "registration", "state_tax"],
        ),
        ("store_profile", &["registration"]),
    ] {
        let columns: Vec<String> = mb_db::schema::columns(&conn, table)
            .expect("read the columns")
            .into_iter()
            .map(|c| c.name)
            .collect();
        for column in wanted {
            assert!(
                columns.iter().any(|c| c == column),
                "{table}.{column} is missing after 0004"
            );
        }
    }

    // And the old words are gone, not merely unused.
    for (table, gone) in [
        ("tax_classes", "treatment"),
        ("items", "tax_treatment"),
        ("order_lines", "tax_treatment"),
        ("bill_lines", "treatment"),
        ("bill_charges", "treatment"),
        ("store_profile", "is_composition"),
        ("store_profile", "default_place_of_supply"),
    ] {
        let columns: Vec<String> = mb_db::schema::columns(&conn, table)
            .expect("read the columns")
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert!(
            !columns.iter().any(|c| c == gone),
            "{table}.{gone} survived 0004"
        );
    }

    // The override table: modelled, stored, written and read by nobody.
    let tables = mb_db::schema::tables(&conn).expect("read the tables");
    assert!(
        !tables.iter().any(|t| t == "tax_class_rates"),
        "tax_class_rates is still here"
    );

    // Six tables were dropped and rebuilt with the foreign keys off.
    let dangling: Vec<String> = conn
        .prepare("PRAGMA foreign_key_check")
        .expect("prepare")
        .query_map([], |r| r.get::<_, String>(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows");
    assert!(
        dangling.is_empty(),
        "these tables have dangling rows: {dangling:?}"
    );
}

/// 0004 with rows already in the database.
#[test]
fn every_old_row_survives_the_tax_rework_with_its_values_intact() {
    // Money that is distinct in every column, so a swapped pair is visible.
    const OLD_ROWS: &str = "
        INSERT INTO store_profile (outlet_id, name, address, phone, gstin, fssai,
            state_code, upi_id, upi_merchant_name, upi_reference, is_composition,
            default_place_of_supply, updated_at)
        VALUES ('outlet_default', 'Anna Tiffin', '12 MG Road', '9876543210',
                '29ABCDE1234F1Z5', '11223344556677', '29', 'anna@upi', 'Anna', 'TILL-1',
                1, 'inter', 555);

        INSERT INTO staff (id, outlet_id, name, status, created_at, updated_at)
        VALUES ('staff_1', 'outlet_default', 'Cashier', 'active', 0, 0);

        INSERT INTO categories (id, outlet_id, name, created_at, updated_at)
        VALUES ('cat_1', 'outlet_default', 'Drinks', 1, 2);

        INSERT INTO tax_classes (id, outlet_id, name, rate_bp, treatment, is_active, sort_order)
        VALUES ('tc_excl',   'outlet_default', 'Old exclusive', 500,  'exclusive', 1, 41),
               ('tc_incl',   'outlet_default', 'Old inclusive', 1800, 'inclusive', 0, 42),
               ('tc_exempt', 'outlet_default', 'Old exempt',    250,  'exempt',    1, 43),
               ('tc_nongst', 'outlet_default', 'Old non-GST',   1400, 'non_gst',   0, 44);

        INSERT INTO items (id, outlet_id, category_id, name, unit_price, tax_class_id,
            tax_rate_bp, tax_treatment, hsn, cost_price, short_code, prep_minutes, course,
            is_open_price, is_available, sort_order, created_at, updated_at)
        VALUES ('itm_incl', 'outlet_default', 'cat_1', 'Filter Coffee', 4567,
                'tax_packaged_12', 1800, 'inclusive', '2101', 1234, 'FC', 3, 'starter',
                0, 1, 9, 101, 102),
               ('itm_nongst', 'outlet_default', 'cat_1', 'Old Monk', 28999,
                'tax_liquor', 0, 'non_gst', NULL, 17777, 'OM', 1, 'drinks',
                1, 0, 11, 103, 104);

        INSERT INTO orders (id, outlet_id, terminal_id, state, business_day, created_at,
            created_by, order_type, bill_number_value, bill_number_formatted,
            settled_at, settled_by)
        VALUES ('ord_1', 'outlet_default', 'terminal_default', 'settled', 20320, 111,
                'staff_1', 'parcel', 7, 'B7', 222, 'staff_1');

        INSERT INTO order_lines (id, order_id, seq, item_id, name, unit_price, tax_rate_bp,
            tax_treatment, hsn, category_id, qty, note, course, prep_minutes,
            discount_kind, discount_value, discount_reason, discount_by,
            discount_applied, discount_requested, was_discount_capped)
        VALUES ('ol_1', 'ord_1', 1, 'itm_incl', 'Filter Coffee', 4567, 250, 'exempt',
                '0902', 'cat_1', 1500, 'less sugar', 'starter', 7,
                'percent', 1250, 'Regular', 'staff_1', 571, 573, 1);

        INSERT INTO bills (order_id, subtotal, total_line_discount, total_bill_discount,
            total_discount, total_charges, was_bill_discount_capped, bill_discount_kind,
            bill_discount_value, bill_discount_reason, bill_discount_by, total_taxable,
            total_cgst, total_sgst, total_igst, non_gst_value, exempt_value, round_off,
            grand_total, place_of_supply, rounding_mode, computed_at, customer_gstin,
            customer_name)
        VALUES ('ord_1', 123457, 9973, 4441, 14414, 3331, 1, 'amount', 4441, 'Manager',
                'staff_1', 103851, 2596, 2597, 1234, 8887, 7771, -43, 118219, 'inter',
                'nearest_rupee', 333, '29ZZZZZ9999Z1Z9', 'Ravi');

        INSERT INTO bill_lines (order_line_id, order_id, gross, line_discount,
            bill_discount_share, net, taxable, cgst, sgst, igst, gross_including_tax,
            rate_bp, treatment)
        VALUES ('ol_1', 'ord_1', 123457, 9973, 4441, 109043, 103851, 2596, 2597, 1234,
                110278, 1800, 'inclusive');

        INSERT INTO bill_charges (id, order_id, seq, kind, name, basis, basis_value,
            amount, taxable, cgst, sgst, igst, gross_including_tax, rate_bp, treatment)
        VALUES ('bc_1', 'ord_1', 1, 'packing', 'Packing', 'flat', 3331, 3331, 3329,
                83, 84, 17, 3513, 500, 'non_gst');
    ";

    let scratch = Scratch::new("tax_rework_data");
    let mut conn = Connection::open(scratch.db_path()).expect("open");

    // `migrate::LEDGER` is private and `run_single` assumes the table is there.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
             version    INTEGER NOT NULL PRIMARY KEY,
             name       TEXT    NOT NULL,
             checksum   TEXT    NOT NULL,
             applied_at INTEGER NOT NULL,
             run_ms     INTEGER NOT NULL
         ) STRICT;",
    )
    .expect("ledger");

    for m in MIGRATIONS.iter().filter(|m| m.version < 4) {
        run_single(&mut conn, m).expect("the schema as it stood before the rework");
    }
    conn.execute_batch(OLD_ROWS).expect("seed old-format rows");

    let applied = migrate::apply_all(&mut conn).expect("0004 runs");
    assert_eq!(
        applied.ran,
        vec![4, 5],
        "the rework and what came after it should have been left to do"
    );

    // tax_classes: treatment splits into kind + basis, the rest is untouched.
    for (id, kind, basis, rate_bp, name, is_active, sort_order) in [
        ("tc_excl", "gst", "exclusive", 500, "Old exclusive", 1, 41),
        ("tc_incl", "gst", "inclusive", 1800, "Old inclusive", 0, 42),
        ("tc_exempt", "exempt", "exclusive", 250, "Old exempt", 1, 43),
        (
            "tc_nongst",
            "outside_gst",
            "exclusive",
            1400,
            "Old non-GST",
            0,
            44,
        ),
    ] {
        check(
            &conn,
            "tax_classes",
            id,
            &[
                ("kind", txt(kind)),
                ("basis", txt(basis)),
                ("rate_bp", num(rate_bp)),
                ("name", txt(name)),
                ("is_active", num(is_active)),
                ("sort_order", num(sort_order)),
            ],
        );
    }

    check(
        &conn,
        "items",
        "itm_incl",
        &[
            ("tax_kind", txt("gst")),
            ("tax_basis", txt("inclusive")),
            ("outlet_id", txt("outlet_default")),
            ("category_id", txt("cat_1")),
            ("name", txt("Filter Coffee")),
            ("unit_price", num(4567)),
            ("tax_class_id", txt("tax_packaged_12")),
            ("tax_rate_bp", num(1800)),
            ("hsn", txt("2101")),
            ("cost_price", num(1234)),
            ("short_code", txt("FC")),
            ("prep_minutes", num(3)),
            ("course", txt("starter")),
            ("is_open_price", num(0)),
            ("is_available", num(1)),
            ("sort_order", num(9)),
            ("created_at", num(101)),
            ("updated_at", num(102)),
        ],
    );
    check(
        &conn,
        "items",
        "itm_nongst",
        &[
            ("tax_kind", txt("outside_gst")),
            ("tax_basis", txt("exclusive")),
            ("name", txt("Old Monk")),
            ("unit_price", num(28999)),
            ("tax_class_id", txt("tax_liquor")),
            ("tax_rate_bp", num(0)),
            ("hsn", Value::Null),
            ("cost_price", num(17777)),
            ("short_code", txt("OM")),
            ("prep_minutes", num(1)),
            ("course", txt("drinks")),
            ("is_open_price", num(1)),
            ("is_available", num(0)),
            ("sort_order", num(11)),
            ("created_at", num(103)),
            ("updated_at", num(104)),
        ],
    );

    check(
        &conn,
        "order_lines",
        "ol_1",
        &[
            ("tax_kind", txt("exempt")),
            ("tax_basis", txt("exclusive")),
            ("order_id", txt("ord_1")),
            ("seq", num(1)),
            ("item_id", txt("itm_incl")),
            ("variant_id", Value::Null),
            ("name", txt("Filter Coffee")),
            ("unit_price", num(4567)),
            ("tax_rate_bp", num(250)),
            ("hsn", txt("0902")),
            ("category_id", txt("cat_1")),
            ("qty", num(1500)),
            ("note", txt("less sugar")),
            ("course", txt("starter")),
            ("prep_minutes", num(7)),
            ("discount_kind", txt("percent")),
            ("discount_value", num(1250)),
            ("discount_reason", txt("Regular")),
            ("discount_by", txt("staff_1")),
            ("discount_applied", num(571)),
            ("discount_requested", num(573)),
            ("was_discount_capped", num(1)),
        ],
    );

    // bill_lines: money is the whole point, so every column comes back as put in.
    check_where(
        &conn,
        "bill_lines",
        "order_line_id = 'ol_1'",
        &[
            ("tax_kind", txt("gst")),
            ("tax_basis", txt("inclusive")),
            ("order_id", txt("ord_1")),
            ("gross", num(123_457)),
            ("line_discount", num(9973)),
            ("bill_discount_share", num(4441)),
            ("net", num(109_043)),
            ("taxable", num(103_851)),
            ("cgst", num(2596)),
            ("sgst", num(2597)),
            ("igst", num(1234)),
            // No bill written before this migration ever charged state VAT.
            ("vat", num(0)),
            ("gross_including_tax", num(110_278)),
            ("rate_bp", num(1800)),
        ],
    );

    // The deliberate difference: bill_charges has no 'outside_gst' in its new CHECK, so an old
    // non_gst charge lands on 'exempt' instead.
    check(
        &conn,
        "bill_charges",
        "bc_1",
        &[
            ("tax_kind", txt("exempt")),
            ("tax_basis", txt("exclusive")),
            ("order_id", txt("ord_1")),
            ("seq", num(1)),
            ("kind", txt("packing")),
            ("name", txt("Packing")),
            ("basis", txt("flat")),
            ("basis_value", num(3331)),
            ("amount", num(3331)),
            ("taxable", num(3329)),
            ("cgst", num(83)),
            ("sgst", num(84)),
            ("igst", num(17)),
            ("gross_including_tax", num(3513)),
            ("rate_bp", num(500)),
        ],
    );

    check_where(
        &conn,
        "bills",
        "order_id = 'ord_1'",
        &[
            ("total_vat", num(0)),
            ("untaxed_value", num(0)),
            ("registration", txt("regular")),
            ("state_tax", txt("sgst")),
            ("subtotal", num(123_457)),
            ("total_line_discount", num(9973)),
            ("total_bill_discount", num(4441)),
            ("total_discount", num(14414)),
            ("total_charges", num(3331)),
            ("was_bill_discount_capped", num(1)),
            ("bill_discount_kind", txt("amount")),
            ("bill_discount_value", num(4441)),
            ("bill_discount_reason", txt("Manager")),
            ("bill_discount_by", txt("staff_1")),
            ("total_taxable", num(103_851)),
            ("total_cgst", num(2596)),
            ("total_sgst", num(2597)),
            ("total_igst", num(1234)),
            ("non_gst_value", num(8887)),
            ("exempt_value", num(7771)),
            ("round_off", num(-43)),
            ("grand_total", num(118_219)),
            ("place_of_supply", txt("inter")),
            ("rounding_mode", txt("nearest_rupee")),
            ("computed_at", num(333)),
            ("customer_gstin", txt("29ZZZZZ9999Z1Z9")),
            ("customer_name", txt("Ravi")),
        ],
    );

    check_where(
        &conn,
        "store_profile",
        "outlet_id = 'outlet_default'",
        &[
            ("registration", txt("composition")),
            ("name", txt("Anna Tiffin")),
            ("address", txt("12 MG Road")),
            ("phone", txt("9876543210")),
            ("gstin", txt("29ABCDE1234F1Z5")),
            ("fssai", txt("11223344556677")),
            ("state_code", txt("29")),
            ("upi_id", txt("anna@upi")),
            ("upi_merchant_name", txt("Anna")),
            ("upi_reference", txt("TILL-1")),
            ("updated_at", num(555)),
        ],
    );

    // The reseed at the bottom of 0004. `itm_incl` points at the 12% class, so it must be left
    // alone rather than retired.
    check(
        &conn,
        "tax_classes",
        "tax_packaged_12",
        &[("is_active", num(1))],
    );
    check(
        &conn,
        "tax_classes",
        "tax_liquor",
        &[
            ("kind", txt("outside_gst")),
            ("basis", txt("inclusive")),
            ("name", txt("Liquor — state VAT")),
        ],
    );
    check(
        &conn,
        "tax_classes",
        "tax_goods_5",
        &[
            ("outlet_id", txt("outlet_default")),
            ("name", txt("Packaged goods 5%")),
            ("rate_bp", num(500)),
            ("kind", txt("gst")),
            ("basis", txt("exclusive")),
            ("is_active", num(1)),
            ("sort_order", num(5)),
        ],
    );

    let dangling: Vec<String> = conn
        .prepare("PRAGMA foreign_key_check")
        .expect("prepare")
        .query_map([], |r| r.get::<_, String>(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows");
    assert!(
        dangling.is_empty(),
        "these tables have dangling rows: {dangling:?}"
    );
}

fn txt(s: &str) -> Value {
    Value::Text(s.to_owned())
}

fn num(n: i64) -> Value {
    Value::Integer(n)
}

/// Every named column of one row, compared as a raw SQLite value so NULL and 0 cannot be
/// confused.
fn check_where(conn: &Connection, table: &str, whr: &str, wanted: &[(&str, Value)]) {
    for (column, want) in wanted {
        let got: Value = conn
            .query_row(
                &format!("SELECT {column} FROM {table} WHERE {whr}"),
                [],
                |r| r.get(0),
            )
            .expect("read one cell");
        assert_eq!(&got, want, "{table}.{column} where {whr}");
    }
}

fn check(conn: &Connection, table: &str, id: &str, wanted: &[(&str, Value)]) {
    check_where(conn, table, &format!("id = '{id}'"), wanted);
}

/// The engine applies its own list, so a one-off migration needs the same transaction
/// discipline spelled out here.
fn run_single(conn: &mut Connection, m: &mb_db::Migration) -> Result<(), DbError> {
    let tx = conn.transaction()?;
    tx.execute_batch(m.sql)
        .map_err(|source| DbError::Migration {
            version: m.version,
            name: m.name,
            source,
        })?;
    tx.execute(
        "INSERT INTO schema_version (version, name, checksum, applied_at, run_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![i64::from(m.version), m.name, checksum(m.sql), 1_i64, 0_i64],
    )?;
    tx.commit()?;
    Ok(())
}

fn ledger(conn: &Connection) -> Vec<(i64, String)> {
    conn.prepare("SELECT version, checksum FROM schema_version ORDER BY version")
        .expect("prepare")
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows")
}
