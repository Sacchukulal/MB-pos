//! Set a database up as one till of a two-till shop, so P27 can be RUN (D55).
//!
//! `newshop` makes an empty one; this gives it a menu to sell, a table to sit
//! at, and its own identity and series prefix. Two of these, against two
//! `APPDATA` folders, is a two-counter shop on one machine.
//!
//!     cargo run -p mb-db --example twotills -- <path-to.db> <id> <name> <prefix>
//!
//! Not a test and not shipped. It exists for the same reason `peek` does: a
//! session that cannot see the product ships bugs only a person notices.

#![allow(
    clippy::expect_used,
    clippy::print_stdout,
    reason = "a developer's one-shot setter-upper: it takes its arguments on \
              the command line and its only sensible response to a bad one is \
              to say so and stop. Nothing ships it, and nothing calls it."
)]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [path, id, name, prefix] = <[String; 4]>::try_from(args)
        .expect("usage: twotills <db> <terminal-id> <name> <prefix>");

    let db = mb_db::Db::open(&mb_db::DbConfig::new(std::path::PathBuf::from(&path)))
        .expect("it opens and migrates");
    let at = mb_core::Timestamp::from_millis(
        i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("a clock after 1970")
                .as_millis(),
        )
        .expect("a clock before the year 292 million"),
    );

    db.transaction(|tx| {
        let repos = mb_db::Repos::new(tx);

        // Something to sell. Two items, so a bill has more than one line and
        // the screen has something to look at.
        for (item, label, paise) in [
            ("itm_dosa", "Masala Dosa", 12_000_i64),
            ("itm_coffee", "Filter Coffee", 3_000),
        ] {
            repos.menu().save_item(
                "outlet_default",
                &mb_db::repo::menu::MenuItem {
                    id: mb_core::ItemId::new(item),
                    category_id: None,
                    name: label.to_owned(),
                    unit_price: mb_core::Money::from_paise(paise),
                    tax: mb_core::TaxSpec::gst(mb_core::TaxRate::from_percent(5).expect("5%")),
                    tax_class_id: None,
                    hsn: None,
                    cost_price: None,
                    short_code: None,
                    prep_minutes: None,
                    course: None,
                    is_open_price: false,
                    is_available: true,
                    sort_order: 0,
                },
                at,
            )?;
        }

        // A floor, so D137's "table service needs the main till" has a table to
        // be about.
        for (table, label) in [("tbl_5", "5"), ("tbl_6", "6")] {
            repos.floor().save_table(
                "outlet_default",
                &mb_db::repo::floor::DiningTable {
                    id: mb_core::TableId::new(table),
                    section_id: None,
                    label: label.to_owned(),
                    seats: 4,
                    pos: None,
                    sort_order: 0,
                    is_active: true,
                },
                at,
            )?;
        }

        // **The tills, and their series** (D135).
        //
        // BOTH go into both books, because that is what a joined shop looks
        // like: the main till has to know who is forwarding to it (a forwarded
        // bill points at a terminal, and the day close per drawer needs the
        // row), and a secondary has to be able to name the main one on its
        // screen. In the running product the main till learns about a
        // secondary from its first forwarded batch, and a secondary learns
        // about the main one when it joins — this just gets there in one step.
        for (this, label, series) in [
            ("terminal_default", "Counter 1", "A/"),
            ("term_second", "Counter 2", "B/"),
        ] {
            let mut row = mb_db::repo::terminals::Terminal::new(this, label, at);
            row.series_prefix = series.to_owned();
            repos
                .terminals()
                .save("outlet_default", &row, at)
                .expect("its own series");
        }
        // One main till, and it is a person's decision (D139) — here, this
        // script's.
        repos
            .terminals()
            .make_master("outlet_default", "terminal_default", at)?;
        // Anything this run said about ITS till, applied on top.
        let mut mine = mb_db::repo::terminals::Terminal::new(&id, &name, at);
        mine.series_prefix = prefix.clone();
        repos.terminals().save("outlet_default", &mine, at).expect("named");
        Ok(())
    })
    .expect("the shop is set up");

    println!("{path}: {name} ({id}), bills print as {prefix}0001");
}
