//! The rules the schema claims to obey, checked rather than believed:
//! T4, T8, T9, T10, T11, T12, T17, T19.
//!
//! Every one of these is a v1 finding turned into something a future session
//! cannot quietly undo.

// See clippy.toml (P01): the exemption reaches `#[test]` functions, but the
// closures passed to `Db::read` and the doc parser at the bottom are not
// `#[test]` functions themselves.
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect and panic are the assertion"
)]

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::Scratch;
use mb_db::schema;

/// T4. NOT ONE REAL COLUMN, anywhere in the database.
///
/// v1 declared nine, and every rupee the product ever touched went through one
/// of them: `items.price`, `expenses.amount`, `finalized_orders.subtotal`,
/// `.gst`, `.total`, `customers.credit_balance`, `customer_payments.amount`.
/// D2 exists because of that list.
#[test]
fn t4_there_is_not_one_real_column() {
    let db = Scratch::new("t4").open();
    db.read(|conn| {
        for table in schema::tables(conn)? {
            for column in schema::columns(conn, &table)? {
                assert_ne!(
                    column.decl_type, "REAL",
                    "{table}.{} is REAL — money is INTEGER paise (D2)",
                    column.name
                );
            }
        }
        Ok(())
    })
    .expect("walk the schema");
}

/// T9. Three types in the whole schema, and every table is STRICT.
///
/// STRICT is what makes this a rule rather than a habit: SQLite refuses to
/// create a STRICT table declaring `BOOLEAN`, `NUMERIC`, `DECIMAL` or
/// `VARCHAR`, and it enforces the declared type on every write. v1 declared 51
/// columns as BOOLEAN — a type SQLite does not have — and two of them defaulted
/// to NULL, so a "boolean" in that product had three values.
#[test]
fn t9_every_column_is_text_or_integer_and_every_table_is_strict() {
    let db = Scratch::new("t9").open();
    db.read(|conn| {
        let tables = schema::tables(conn)?;
        assert!(tables.len() > 30, "the schema should not have shrunk by accident");

        for table in tables {
            let sql = schema::create_sql(conn, &table)?;
            assert!(
                sql.to_uppercase().contains("STRICT"),
                "{table} is not STRICT — a non-STRICT table can hold a float in \
                 an INTEGER column and SQLite will not say a word"
            );

            for column in schema::columns(conn, &table)? {
                assert!(
                    matches!(column.decl_type.as_str(), "TEXT" | "INTEGER" | "BLOB"),
                    "{table}.{} is declared {} — this schema has three types",
                    column.name,
                    column.decl_type
                );
            }
        }
        Ok(())
    })
    .expect("walk the schema");
}

/// T10. Every boolean is INTEGER, NOT NULL, and constrained to 0 or 1 — and
/// then one of them is proved to bite.
#[test]
fn t10_booleans_are_zero_or_one_and_never_null() {
    let scratch = Scratch::new("t10");
    let db = scratch.open();

    let mut found = 0_usize;
    db.read(|conn| {
        for table in schema::tables(conn)? {
            let sql = schema::create_sql(conn, &table)?;
            for column in schema::columns(conn, &table)? {
                if !schema::is_boolean_name(&column.name) {
                    continue;
                }
                found += 1;
                assert_eq!(
                    column.decl_type, "INTEGER",
                    "{table}.{} is a boolean and must be INTEGER",
                    column.name
                );
                assert!(
                    column.not_null,
                    "{table}.{} is a boolean and must be NOT NULL — v1's \
                     subtotal_bold defaulted to NULL and had three values",
                    column.name
                );
                assert!(
                    sql.contains(&format!("CHECK ({} IN (0, 1))", column.name)),
                    "{table}.{} has no CHECK naming it",
                    column.name
                );
            }
        }
        Ok(())
    })
    .expect("walk the schema");
    assert!(found > 15, "expected the schema to have real booleans in it, found {found}");

    // And prove the constraint is not decorative.
    let wrote_two = db.transaction(|tx| {
        tx.execute(
            "UPDATE outlets SET is_active = 2 WHERE id = 'outlet_default'",
            [],
        )
        .map_err(Into::into)
    });
    assert!(wrote_two.is_err(), "a boolean column accepted 2");
}

/// T12. Ids are TEXT and there is no AUTOINCREMENT (D13).
///
/// Two terminals in one shop collide on an autoincrement id in the same second
/// and there is no way to repair it afterwards without renumbering history.
#[test]
fn t12_ids_are_text_and_nothing_autoincrements() {
    let db = Scratch::new("t12").open();
    db.read(|conn| {
        for table in schema::tables(conn)? {
            let sql = schema::create_sql(conn, &table)?.to_uppercase();
            assert!(
                !sql.contains("AUTOINCREMENT"),
                "{table} uses AUTOINCREMENT — ids are text (D13)"
            );

            for column in schema::columns(conn, &table)? {
                let looks_like_an_id = column.name == "id"
                    || column.name.ends_with("_id")
                    // The engine's own ledger is the one integer key in the
                    // database: a migration version is not a business id.
                    && table != "schema_version";
                if looks_like_an_id && table != "schema_version" {
                    assert_eq!(
                        column.decl_type, "TEXT",
                        "{table}.{} is an id and must be TEXT",
                        column.name
                    );
                }
            }
        }
        Ok(())
    })
    .expect("walk the schema");
}

/// T11. Every root table carries `outlet_id`, NOT NULL, with a foreign key to
/// `outlets`.
///
/// Scope 11.4. This is the dimension that cannot be retro-fitted: a table added
/// later is free, but back-filling an outlet across every row after a year of
/// trading means choosing a value nobody can verify.
#[test]
fn t11_every_root_table_carries_its_outlet() {
    let db = Scratch::new("t11").open();
    db.read(|conn| {
        for table in schema::ROOT_TABLES {
            let columns = schema::columns(conn, table)?;
            let outlet = columns
                .iter()
                .find(|c| c.name == "outlet_id")
                .unwrap_or_else(|| panic!("{table} has no outlet_id (scope 11.4)"));
            assert!(outlet.not_null, "{table}.outlet_id must be NOT NULL");
            assert_eq!(outlet.decl_type, "TEXT");

            let fks = schema::foreign_keys(conn, table)?;
            assert!(
                fks.iter()
                    .any(|fk| fk.from_column == "outlet_id" && fk.to_table == "outlets"),
                "{table}.outlet_id has no foreign key to outlets"
            );
        }
        Ok(())
    })
    .expect("walk the schema");
}

/// T19. Nothing in the money path cascades on delete — and deleting an order
/// that has lines fails.
///
/// A bill is never deleted. It is voided, which is a state, not an absence.
/// `ON DELETE CASCADE` from an order to its lines means one wrong DELETE erases
/// a bill and the evidence for it in the same statement.
#[test]
fn t19_nothing_in_the_money_path_cascades() {
    let scratch = Scratch::new("t19");
    let db = scratch.open();

    db.read(|conn| {
        for table in schema::tables(conn)? {
            for fk in schema::foreign_keys(conn, &table)? {
                if schema::MONEY_PATH_TABLES.contains(&fk.to_table.as_str()) {
                    assert_ne!(
                        fk.on_delete, "CASCADE",
                        "{table}.{} cascades into {} — a bill is voided, not deleted",
                        fk.from_column, fk.to_table
                    );
                }
            }
        }
        Ok(())
    })
    .expect("walk the schema");

    // And prove it, rather than only reading the declaration.
    db.transaction(|tx| {
        tx.execute_batch(common::STAFF_SQL)?;
        tx.execute_batch(common::FLOOR_SQL)?;
        tx.execute_batch(common::MENU_SQL)?;
        tx.execute_batch(
            "INSERT INTO orders (id, outlet_id, terminal_id, state, business_day, created_at,
                                 created_by, order_type, table_id, token_value, token_formatted,
                                 bill_number_value, bill_number_formatted)
             VALUES ('ord_1', 'outlet_default', 'terminal_default', 'open', 20669, 1,
                     'staff_1', 'dine_in', 'tbl_1', 1, '1', 1, '0001');
             INSERT INTO order_lines (id, order_id, seq, item_id, name, unit_price,
                                      tax_rate_bp, tax_treatment, qty)
             VALUES ('ln_1', 'ord_1', 0, 'itm_dosa', 'Masala Dosa', 12000, 500, 'exclusive', 2000);",
        )?;
        Ok(())
    })
    .expect("seed an order");

    let deleted = db.transaction(|tx| {
        tx.execute("DELETE FROM orders WHERE id = 'ord_1'", [])
            .map_err(Into::into)
    });
    assert!(
        deleted.is_err(),
        "an order with lines was deleted — the foreign key is not doing its job"
    );
}

/// T8. Every named index exists.
///
/// A report's speed is not a property of the report. If somebody removes an
/// index because "nothing seemed to use it", this is the test that says which
/// report just became a full scan.
#[test]
fn t8_every_named_index_exists() {
    const REQUIRED: &[&str] = &[
        "idx_audit_log_at",
        "idx_audit_log_staff",
        "idx_bill_charges_order",
        "idx_bill_lines_order",
        "idx_customer_payments_customer",
        "idx_expenses_day",
        "idx_items_category",
        "idx_items_short_code",
        "idx_order_events_order",
        "idx_order_lines_item",
        "idx_order_lines_order",
        "idx_orders_bill_number",
        "idx_orders_created_by",
        "idx_orders_customer",
        "idx_orders_day",
        "idx_orders_state",
        "idx_orders_table",
        "idx_orders_token",
        "idx_payments_customer",
        // P15: one phone number is one customer (scope 5.4), and the ledger
        // read for one customer.
        "idx_customers_phone_key",
        "idx_credit_adjustments_customer",
        "idx_payments_day_mode",
        "idx_payments_order",
        "idx_reprints_day",
        "idx_reservations_day",
        "idx_sync_outbox_pending",
        // P19: the authentication path reads the live device register on EVERY
        // request, because revocation has to bite on the next request and not
        // on the next login (D77's sibling). Partial, so it is the size of the
        // phones in use rather than the size of every phone ever paired.
        "idx_lan_devices_live",
        // P24: the kitchen screen asks "what is outstanding at my station"
        // several times a minute, and it must not scan a year of tickets to
        // answer. Partial on `state <> 'bumped'`, so it is the size of the
        // kitchen's current work rather than of every ticket ever sent.
        "idx_kitchen_live",
        // And the kitchen-speed report (scope 3.7) reads finished tickets by
        // day. Partial on a bumped_at that exists, because an unfinished
        // ticket has no time to report.
        "idx_kitchen_done",
        // P25. One recipe per owner, three partial unique indexes rather than
        // one constraint, because SQLite treats NULLs as distinct and two of a
        // recipe's three owner columns are always NULL.
        "idx_recipes_item",
        "idx_recipes_modifier",
        "idx_recipes_material",
        // "What uses this material?" — asked before deactivating one.
        "idx_recipe_lines_material",
        // One material's history. Without this, opening a material is a full
        // scan of the largest table in the module.
        "idx_stock_movements_material",
        "idx_stock_movements_day",
        // **The void path.** D113 finds the rows a bill wrote and negates them,
        // and must not scan a year of movements to do it.
        "idx_stock_movements_order",
        // P26. A supplier's ledger, oldest first — read every time somebody
        // asks "what do I owe him", and it must not scan a year of every
        // supplier's invoices to answer for one.
        "idx_purchases_supplier",
        // Every buying report, on the STORED business day (D5).
        "idx_purchases_day",
        // "What did I pay for onions, and when did it go up" — the price-trend
        // report, which is the finding an owner acts on fastest.
        "idx_purchase_lines_material",
        "idx_supplier_payments_supplier",
        "idx_supplier_adjustments_supplier",
        "idx_stock_counts_day",
        // D132. Finding the photograph of one invoice, and listing what a
        // backup has to carry. Partial, because most attachments will belong to
        // something and a row with no subject is a stray this must not index.
        "idx_attachments_subject",
    ];

    let db = Scratch::new("t8").open();
    db.read(|conn| {
        let present: BTreeSet<String> = schema::indexes(conn)?.into_iter().collect();
        for name in REQUIRED {
            assert!(present.contains(*name), "index {name} is missing");
        }
        // And the other direction: an index nobody named should not exist, so
        // that a stray one gets discussed rather than absorbed. Each index
        // costs write time on the billing path and bytes against budget M5.
        let required: BTreeSet<&str> = REQUIRED.iter().copied().collect();
        for name in &present {
            assert!(
                required.contains(name.as_str()),
                "index {name} exists but is not in this test's list — add it \
                 there with a reason, or remove it"
            );
        }
        Ok(())
    })
    .expect("read the indexes");
}

/// T17. docs/SCHEMA.md and the database agree, in both directions.
///
/// Audit E10: v1 carried dead columns — `bill_font_size`, `logo_opacity`, an
/// unused `pin`, KOT font settings nothing read — and the fix the auditor asked
/// for was "start clean; do not carry any of it forward". Starting clean is
/// easy. STAYING clean is this test: a column cannot be added without writing
/// down what reads it, and cannot be left behind after its feature is deleted.
///
/// A future session will curse this test. That is why the reason is written
/// here at length.
#[test]
fn t17_the_document_and_the_database_agree() {
    let doc = include_str!("../../../../docs/SCHEMA.md");
    let documented = parse_schema_doc(doc);

    let db = Scratch::new("t17").open();
    db.read(|conn| {
        let mut live: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
        for table in schema::tables(conn)? {
            let columns = schema::columns(conn, &table)?
                .into_iter()
                .map(|c| (c.name, c.decl_type))
                .collect();
            live.insert(table, columns);
        }

        let live_tables: BTreeSet<&String> = live.keys().collect();
        let doc_tables: BTreeSet<&String> = documented.keys().collect();

        let undocumented: Vec<_> = live_tables.difference(&doc_tables).collect();
        assert!(
            undocumented.is_empty(),
            "these tables are in the database but not in docs/SCHEMA.md: {undocumented:?}"
        );
        let phantom: Vec<_> = doc_tables.difference(&live_tables).collect();
        assert!(
            phantom.is_empty(),
            "these tables are in docs/SCHEMA.md but not in the database: {phantom:?}"
        );

        for (table, live_columns) in &live {
            let doc_columns = &documented[table];
            let missing: Vec<_> = live_columns.difference(doc_columns).collect();
            assert!(
                missing.is_empty(),
                "{table}: in the database but not documented: {missing:?}"
            );
            let extra: Vec<_> = doc_columns.difference(live_columns).collect();
            assert!(
                extra.is_empty(),
                "{table}: documented but not in the database: {extra:?}"
            );
        }
        Ok(())
    })
    .expect("compare the document with the database");
}

/// Prints the column half of `docs/SCHEMA.md` from the live schema.
///
/// Not a test — a generator, so that keeping the document in step with T17 is
/// mechanical rather than an evening of typing:
///
/// ```text
/// cargo test -p mb-db --test schema_rules -- --ignored --nocapture dump
/// ```
///
/// The prose against each table stays hand-written. A generated description of
/// why a column exists would say nothing, and saying nothing is how E10's dead
/// columns survived two years of review.
#[test]
#[ignore = "generator, not a test"]
fn dump_schema_markdown() {
    let db = Scratch::new("dump").open();
    db.read(|conn| {
        for table in schema::tables(conn)? {
            println!("\n### {table}\n");
            println!("| column | type | null | notes |");
            println!("|---|---|---|---|");
            for c in schema::columns(conn, &table)? {
                let null = if c.not_null { "no" } else { "yes" };
                println!("| {} | {} | {} | |", c.name, c.decl_type, null);
            }
        }
        Ok(())
    })
    .expect("dump");
}

/// Reads `docs/SCHEMA.md`: a `### table_name` heading, then a markdown table
/// whose first cell is the column name and second cell the declared type.
///
/// Deliberately about twenty lines. If checking the document ever needs a
/// markdown parser, the document has become too clever to be a reference.
fn parse_schema_doc(doc: &str) -> BTreeMap<String, BTreeSet<(String, String)>> {
    let mut out: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
    let mut current: Option<String> = None;

    for line in doc.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix("### ") {
            let name = name.trim().trim_matches('`').to_owned();
            current = Some(name.clone());
            out.entry(name).or_default();
            continue;
        }
        // Any other heading ends the table's section, so the file can carry
        // prose and summary tables (the index list, the reservations) without
        // them being read as columns of whatever came last.
        if line.starts_with("## ") || line.starts_with("# ") {
            current = None;
            continue;
        }
        let Some(table) = current.as_ref() else {
            continue;
        };
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() < 2 {
            continue;
        }
        let name = cells[0].trim_matches('`');
        let decl = cells[1].trim_matches('`');
        // Skip the header row and the |---|---| separator.
        if name.is_empty() || name == "column" || name.starts_with("---") {
            continue;
        }
        out.entry(table.clone())
            .or_default()
            .insert((name.to_owned(), decl.to_uppercase()));
    }
    out
}

/// **The owner renamed it, and the old word does not come back.**
///
/// 2026-08-08: *"its not khata, rename it as credit"*. "Khata" is what a Kirana
/// shop calls this and it is not what the product says.
///
/// This is D40's rule — *the rules that erode are enforced by scripts, not by
/// agreement* — because a rename is exactly the kind of change that half
/// happens: one screen keeps the old word, or the next session writes it back
/// from memory, and then the product says both.
///
/// The **audits** keep saying khata on purpose. They quote v1 and the owner's
/// own words from before the decision, and editing a quotation to match a later
/// decision is falsifying the record. They are not in this crate.
#[test]
fn the_product_says_credit_and_never_khata() {
    use std::path::Path;

    let mut offenders = Vec::new();
    let roots = ["../../crates", "../../src-tauri/src", "../../ui/src", "../../ui/tests"];

    fn walk(dir: &Path, offenders: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // `target` and `node_modules` are build output, not the product.
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name != "target" && name != "node_modules" {
                    walk(&path, offenders);
                }
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !matches!(ext, "rs" | "ts" | "tsx" | "sql" | "css") {
                continue;
            }
            // The guard itself has to say the word to look for it.
            if path.ends_with("schema_rules.rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            for (n, line) in text.lines().enumerate() {
                let lower = line.to_lowercase();
                if !lower.contains("khata") {
                    continue;
                }
                // One exemption, and it is narrow: a line may say the old word
                // while explaining that it IS the old word. That covers this
                // test, and the comments quoting audit A3 and B12, which are
                // findings about v1 and mean nothing renamed.
                let explains = lower.contains("renamed")
                    || lower.contains("v1 ")
                    || lower.contains("audit a3")
                    || lower.contains("audit b12")
                    || lower.contains("kirana");
                if !explains {
                    offenders.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                }
            }
        }
    }

    for root in roots {
        walk(Path::new(root), &mut offenders);
    }

    assert!(
        offenders.is_empty(),
        "the product still says khata in {} place(s):\n{}",
        offenders.len(),
        offenders.join("\n"),
    );
}
