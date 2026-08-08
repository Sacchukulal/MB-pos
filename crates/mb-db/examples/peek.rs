//! A read-only look at what a run actually wrote.
//!
//! Not a test and not shipped. It exists because "the bill is on disk" is a
//! claim about the disk, and reading it back is the only way to check it — the
//! screen that says "settled" is the thing under test.
//!
//!     cargo run -p mb-db --example peek -- <path-to.db>

#![allow(
    clippy::expect_used,
    reason = "a developer's one-shot reader: it takes a path on the command \
              line and its only sensible response to a bad one is to say so and \
              stop. Nothing ships it, and nothing calls it."
)]

fn main() {
    let path = std::env::args().nth(1).expect("usage: peek <db>");
    let db = mb_db::Db::open(&mb_db::DbConfig::new(std::path::Path::new(&path))).expect("open");
    db.transaction(|tx| {
        for sql in [
            "SELECT id, state, bill_number_formatted, token_formatted, order_type, table_id, created_by, terminal_id FROM orders",
            "SELECT order_id, seq, name, qty, unit_price FROM order_lines",
            "SELECT order_id, mode, amount FROM payments",
            "SELECT id, kind, printer_id, state, attempts, reason FROM print_jobs",
        ] {
            let mut stmt = tx.prepare(sql).expect("prepare");
            let cols = stmt.column_count();
            let rows: Vec<String> = stmt
                .query_map([], |row| {
                    Ok((0..cols)
                        .map(|i| match row.get_ref(i) {
                            Ok(rusqlite::types::ValueRef::Null) => "-".to_owned(),
                            Ok(rusqlite::types::ValueRef::Integer(n)) => n.to_string(),
                            Ok(rusqlite::types::ValueRef::Real(f)) => f.to_string(),
                            Ok(rusqlite::types::ValueRef::Text(t)) => String::from_utf8_lossy(t).into_owned(),
                            Ok(rusqlite::types::ValueRef::Blob(b)) => format!("{} bytes", b.len()),
                            Err(_) => "?".to_owned(),
                        })
                        .collect::<Vec<_>>()
                        .join(" | "))
                })
                .expect("query")
                .filter_map(Result::ok)
                .collect();
            println!("{sql}");
            if rows.is_empty() {
                println!("  (nothing)");
            } else {
                println!("  {}", rows.join("
  "));
            }
            println!();
        }
        Ok(())
    })
    .expect("read");
}
