// See clippy.toml: the exemption reaches `#[test]` functions, but the closures passed to
// `Db::read` and the doc parser at the bottom are not `#[test]` functions themselves.
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect and panic are the assertion"
)]

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::Scratch;
use mb_db::schema;

/// NOT ONE REAL COLUMN, anywhere in the database.
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

/// Three types in the whole schema, and every table is STRICT.
#[test]
fn t9_every_column_is_text_or_integer_and_every_table_is_strict() {
    let db = Scratch::new("t9").open();
    db.read(|conn| {
        let tables = schema::tables(conn)?;
        assert!(
            tables.len() > 30,
            "the schema should not have shrunk by accident"
        );

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

/// Every boolean is INTEGER, NOT NULL, and constrained to 0 or 1 — and then one of them is
/// proved to bite.
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
    assert!(
        found > 15,
        "expected the schema to have real booleans in it, found {found}"
    );

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

/// Ids are TEXT and there is no AUTOINCREMENT.
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
                    // The engine's own ledger is the one integer key in the database: a
                    // migration version is not a business id.
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

/// Every root table carries `outlet_id`, NOT NULL, with a foreign key to `outlets`.
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

/// Nothing in the money path cascades on delete — and deleting an order that has lines fails.
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
                                      tax_rate_bp, tax_kind, tax_basis, qty)
             VALUES ('ln_1', 'ord_1', 0, 'itm_dosa', 'Masala Dosa', 12000, 500, 'gst', 'exclusive', 2000);",
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

/// Every named index exists.
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
        // One phone number is one customer, and the ledger read for one customer.
        "idx_customers_phone_key",
        "idx_credit_adjustments_customer",
        "idx_payments_day_mode",
        "idx_payments_order",
        "idx_reprints_day",
        "idx_reservations_day",
        "idx_sync_outbox_pending",
        // The authentication path reads the live device register on EVERY request, because
        // revocation has to bite on the next request and not on the next login.
        "idx_lan_devices_live",
        // One phone, one seat: the pairing looks the install up before it adds a row (0010).
        "idx_lan_devices_install",
        // The kitchen screen asks "what is outstanding at my station" several times a minute,
        // and it must not scan a year of tickets to answer.
        "idx_kitchen_live",
        // And the kitchen-speed report reads finished tickets by day.
        "idx_kitchen_done",
        // One recipe per owner, three partial unique indexes rather than one constraint,
        // because SQLite treats NULLs as distinct and two of a recipe's three owner columns are
        // always NULL.
        "idx_recipes_item",
        "idx_recipes_modifier",
        "idx_recipes_material",
        // "What uses this material?" — asked before deactivating one.
        "idx_recipe_lines_material",
        // One material's history. Without this, opening a material is a full scan of the
        // largest table in the module.
        "idx_stock_movements_material",
        "idx_stock_movements_day",
        "idx_stock_movements_order",
        // A supplier's ledger, oldest first — read every time somebody asks "what do I owe
        // him", and it must not scan a year of every supplier's invoices to answer for one.
        "idx_purchases_supplier",
        // Every buying report, on the STORED business day.
        "idx_purchases_day",
        // "What did I pay for onions, and when did it go up".
        "idx_purchase_lines_material",
        "idx_supplier_payments_supplier",
        "idx_supplier_adjustments_supplier",
        "idx_stock_counts_day",
        // Finding the photograph of one invoice, and listing what a backup has to carry.
        "idx_attachments_subject",
        "idx_counters_prefix",
        "idx_day_closes_drawer",
        "idx_day_closes_shop",
        // Every one of these answers a question a screen asks by name.
        "idx_roster_person_day",
        "idx_roster_day",
        "idx_attendance_person_day",
        "idx_attendance_day",
        // "Who is still clocked in?" — asked at every day close and by the handover report.
        "idx_attendance_open",
        "idx_leave_requests_person",
        // The approval queue, partial for the same reason.
        "idx_leave_requests_pending",
        // The leave calendar: who is away in this window.
        "idx_leave_requests_window",
        // The balance is SUM(half_days) over these three columns, and it is read every time a
        // request is made.
        "idx_leave_ledger_person",
        // A request may write exactly one `taken` row, ever.
        "idx_leave_ledger_request",
        // One structure per person per effective date — a raise on a date that already has one
        // is an edit, not a second raise.
        "idx_salary_structures_person_from",
        "idx_salary_components_structure",
        "idx_salary_advances_person",
        "idx_advance_recoveries_advance",
        // One recovery per advance per run.
        "idx_advance_recoveries_run",
        "idx_payroll_runs_period",
        // One line per person per run, and a person's payslip history.
        "idx_payroll_lines_run_person",
        "idx_payroll_lines_person",
        "idx_rider_handbacks_rider",
        // And the drawer's own question: what came back today, at all.
        "idx_rider_handbacks_day",
        "idx_payment_attempts_day",
        // "What happened on that bill?", asked at the counter mid-argument.
        "idx_payment_attempts_order",
        // The bell: "any notice from Magic Bill I have not seen?" — partial, so a shop with a
        // year of read notices still answers from a handful of rows.
        "idx_cloud_notices_unseen",
    ];

    let db = Scratch::new("t8").open();
    db.read(|conn| {
        let present: BTreeSet<String> = schema::indexes(conn)?.into_iter().collect();
        for name in REQUIRED {
            assert!(present.contains(*name), "index {name} is missing");
        }
        // And the other direction: an index nobody named should not exist, so that a stray one
        // gets discussed rather than absorbed.
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

///
/// ```text
/// cargo test -p mb-db --test schema_rules -- --ignored --nocapture dump
/// ```
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
        // Any other heading ends the table's section, so the file can carry prose and summary
        // tables (the index list, the reservations) without them being read as columns of
        // whatever came last.
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

#[test]
fn the_product_says_credit_and_never_khata() {
    use std::path::Path;

    let mut offenders = Vec::new();
    let roots = [
        "../../crates",
        "../../src-tauri/src",
        "../../ui/src",
        "../../ui/tests",
    ];

    fn walk(dir: &Path, offenders: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
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
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (n, line) in text.lines().enumerate() {
                let lower = line.to_lowercase();
                if !lower.contains("khata") {
                    continue;
                }
                // One exemption, and it is narrow: a line may say the old word while explaining
                // that it IS the old word.
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
