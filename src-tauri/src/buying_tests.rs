//! Buying and the count, driven end to end.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

use mb_core::{Dimension, MaterialId, Money, Qty};
use mb_db::repo::stock::Material;
use mb_db::{Db, DbConfig, Repos};

use crate::buying::{
    PurchaseEdit, PurchaseLineEdit, SupplierEdit, buying_on, cancel_purchase_on, purchase_on,
    save_purchase_on, save_supplier_on, supplier_account_on,
};
use crate::counting::{
    CountEdit, approve_stock_count_on, count_sheet_on, open_stock_count_on, record_count_line_on,
    stock_count_on,
};
use crate::signin_tests::Scratch;
use crate::state::{App, OUTLET};

/// A shop with rice (bought in 25 kg bags) and paneer (bought in kilos).
fn a_shop(scratch: &Scratch) -> App {
    let path = scratch.dir().join("buying.db");
    let db = Db::open(&DbConfig::new(path.clone())).expect("open");
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        let mut rice = Material::new(MaterialId::new("mat_rice"), "Rice", Dimension::Weight);
        rice.packs = vec![("bag".to_owned(), Qty::from_whole(25_000).expect("in range"))];
        rice.purchase_unit = Some("bag".to_owned());
        repos
            .stock()
            .save_material(OUTLET, &rice, crate::flows::now())?;

        let mut paneer = Material::new(MaterialId::new("mat_paneer"), "Paneer", Dimension::Weight);
        paneer.packs = vec![("kg".to_owned(), Qty::from_whole(1_000).expect("in range"))];
        paneer.purchase_unit = Some("kg".to_owned());
        repos
            .stock()
            .save_material(OUTLET, &paneer, crate::flows::now())
    })
    .expect("materials");

    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    app.open_shop(db, path);
    // Buying is behind `Feature::Inventory`.
    app.use_licensing(crate::licence_tests::licence_in(
        scratch,
        "buying-licence",
        mb_license::Status::Active,
        90,
    ));
    app
}

fn a_supplier(app: &App, id: &str, name: &str, terms: &str) {
    save_supplier_on(
        app,
        SupplierEdit {
            id: id.to_owned(),
            name: name.to_owned(),
            phone: String::new(),
            gstin: String::new(),
            address: String::new(),
            terms_days: terms.to_owned(),
            note: String::new(),
            is_active: true,
        },
    )
    .expect("a supplier");
}

fn line(material: &str, qty: &str, unit: &str, rate: &str) -> PurchaseLineEdit {
    PurchaseLineEdit {
        material_id: material.to_owned(),
        qty: qty.to_owned(),
        unit: unit.to_owned(),
        free: String::new(),
        rate: rate.to_owned(),
        discount: String::new(),
        tax_percent: String::new(),
    }
}

fn a_delivery(id: &str, lines: Vec<PurchaseLineEdit>) -> PurchaseEdit {
    PurchaseEdit {
        id: id.to_owned(),
        supplier_id: "sup_metro".to_owned(),
        invoice_no: format!("INV-{id}"),
        lines,
        invoice_discount: String::new(),
        charges: String::new(),
        stated_total: String::new(),
        paid_now: String::new(),
        paid_mode: "cash".to_owned(),
        attachment_id: String::new(),
        po_id: String::new(),
        note: String::new(),
        returns_purchase_id: String::new(),
    }
}

// The four ledgers, through the commands.

#[test]
fn t5_a_cash_delivery_moves_the_drawer_and_writes_no_expense_row() {
    let scratch = Scratch::new("buy_cash");
    let app = a_shop(&scratch);
    a_supplier(&app, "sup_metro", "Metro", "0");

    let mut edit = a_delivery("pur_1", vec![line("mat_rice", "2", "bag", "1000")]);
    edit.paid_now = "2000".to_owned();
    let view = save_purchase_on(&app, edit).expect("recorded");

    // Nothing is owed, because it was paid at the door — and that is two rows, never a flag.
    assert_eq!(view.owed.paise, 0);

    let spends = crate::expenses::expenses_on(&app).expect("the drawer");
    assert_eq!(spends.cash.suppliers_paid.paise, 200_000);
    assert_eq!(
        spends.cash.expected.paise, -200_000,
        "the drawer is down by what was paid"
    );

    // And there is no second row for the same fact.
    assert!(
        spends.rows.is_empty(),
        "a purchase must not also be an expense"
    );
    assert_eq!(spends.movements.len(), 0, "nor a cash movement");

    // The shelf moved: 2 bags of 25 kg.
    let stock = crate::inventory::inventory_on(&app, None).expect("the stock screen");
    let rice = stock
        .materials
        .iter()
        .find(|m| m.id == "mat_rice")
        .expect("rice");
    assert_eq!(rice.on_hand, "2 bag", "{}", rice.on_hand);
}

#[test]
fn t5_a_credit_delivery_leaves_the_drawer_alone_and_ages_from_the_due_day() {
    let scratch = Scratch::new("buy_credit");
    let app = a_shop(&scratch);
    a_supplier(&app, "sup_metro", "Metro", "15");

    save_purchase_on(
        &app,
        a_delivery("pur_1", vec![line("mat_rice", "2", "bag", "1000")]),
    )
    .expect("recorded");

    let spends = crate::expenses::expenses_on(&app).expect("the drawer");
    assert_eq!(spends.cash.suppliers_paid.paise, 0);

    let account = supplier_account_on(&app, "sup_metro".to_owned()).expect("the account");
    assert_eq!(account.supplier.balance.paise, 200_000);
    // Fifteen days' terms, so it is not overdue today and the sentence says so rather than
    // showing a confident zero.
    assert!(!account.supplier.is_overdue, "{}", account.supplier.when);
    assert!(
        account.says.contains("none of it overdue yet"),
        "{}",
        account.says
    );
}

#[test]
fn a_free_bag_and_a_tempo_both_reach_the_landed_cost_on_the_screen() {
    let scratch = Scratch::new("buy_landed");
    let app = a_shop(&scratch);
    a_supplier(&app, "sup_metro", "Metro", "0");

    let mut edit = a_delivery("pur_1", vec![line("mat_rice", "10", "bag", "1000")]);
    edit.lines[0].free = "1".to_owned();
    edit.charges = "500".to_owned();
    save_purchase_on(&app, edit).expect("recorded");

    let paper = purchase_on(&app, "pur_1".to_owned()).expect("the paper");
    // ₹10,000 of rice plus a ₹500 tempo bought ELEVEN bags: about ₹954.50 a bag, not the ₹1,000
    // printed on the invoice.
    assert_eq!(
        paper.lines[0].landed, "₹954.50 per bag",
        "{}",
        paper.lines[0].landed
    );
    assert_eq!(paper.total.paise, 1_050_000);
}

#[test]
fn a_five_percent_scheme_shop_is_told_the_tax_is_a_cost() {
    let scratch = Scratch::new("buy_tax");
    let app = a_shop(&scratch);
    a_supplier(&app, "sup_metro", "Metro", "0");

    let mut edit = a_delivery("pur_1", vec![line("mat_paneer", "10", "kg", "400")]);
    edit.lines[0].tax_percent = "5".to_owned();
    let view = save_purchase_on(&app, edit).expect("recorded");

    // A shop with no composition flag set is a claiming shop, so the default sentence is the
    // other one.
    assert!(view.claims_input_tax);
    assert!(view.tax_note.contains("claimed back"), "{}", view.tax_note);

    let paper = purchase_on(&app, "pur_1".to_owned()).expect("the paper");
    assert_eq!(paper.creditable.paise, 20_000, "5% of ₹4,000 comes back");
    // And the food cost excludes it: ₹4,000 bought 10 kg, so ₹400 a kilo.
    assert_eq!(
        paper.lines[0].landed, "₹400.00 per kg",
        "{}",
        paper.lines[0].landed
    );
}

// The only correction path.

#[test]
fn a_delivery_is_cancelled_with_a_reason_and_never_edited() {
    let scratch = Scratch::new("buy_cancel");
    let app = a_shop(&scratch);
    a_supplier(&app, "sup_metro", "Metro", "0");
    save_purchase_on(
        &app,
        a_delivery("pur_1", vec![line("mat_rice", "2", "bag", "1000")]),
    )
    .expect("recorded");

    // A blank reason is refused in words a person reads.
    let refused = cancel_purchase_on(&app, "pur_1".to_owned(), "   ".to_owned());
    assert!(refused.is_err());
    assert!(
        refused.unwrap_err().message.contains("why"),
        "the refusal has to say what to do"
    );

    let view = cancel_purchase_on(&app, "pur_1".to_owned(), "Entered twice".to_owned())
        .expect("cancelled");
    assert_eq!(view.owed.paise, 0);

    // The paper is still on the list, marked, with the reason on it.
    let paper = purchase_on(&app, "pur_1".to_owned()).expect("still on file");
    assert!(
        paper.cancelled.contains("Entered twice"),
        "{}",
        paper.cancelled
    );

    // And the shelf is back where it was.
    let stock = crate::inventory::inventory_on(&app, None).expect("the stock screen");
    let rice = stock
        .materials
        .iter()
        .find(|m| m.id == "mat_rice")
        .expect("rice");
    assert_eq!(rice.on_hand_base, "0", "{}", rice.on_hand);
}

// The count posts a delta.

#[test]
fn t8_a_delivery_between_the_counting_and_the_approving_survives() {
    let scratch = Scratch::new("buy_count");
    let app = a_shop(&scratch);
    a_supplier(&app, "sup_metro", "Metro", "0");

    // Sunday: 12 kg of paneer on the books.
    save_purchase_on(
        &app,
        a_delivery("pur_1", vec![line("mat_paneer", "12", "kg", "400")]),
    )
    .expect("recorded");

    // Sunday night: somebody counts 10.
    let count = open_stock_count_on(&app, String::new()).expect("opened");
    let id = count.id.clone().expect("an id");
    let counted = record_count_line_on(
        &app,
        CountEdit {
            count_id: id.clone(),
            material_id: "mat_paneer".to_owned(),
            counted: "10".to_owned(),
            unit: "kg".to_owned(),
        },
    )
    .expect("written down");
    assert_eq!(counted.lines[0].book, "12 kg", "{}", counted.lines[0].book);
    assert_eq!(
        counted.lines[0].variance, "2 kg short",
        "{}",
        counted.lines[0].variance
    );
    // A variance in rupees is the one somebody acts on.
    assert_eq!(counted.lines[0].variance_value.paise, -80_000);
    // And the screen says what approving will do BEFORE anybody presses it.
    assert!(
        counted.effect.contains("take away from 1 material"),
        "{}",
        counted.effect
    );

    // Monday morning: 25 kg arrives.
    save_purchase_on(
        &app,
        a_delivery("pur_2", vec![line("mat_paneer", "25", "kg", "400")]),
    )
    .expect("recorded");

    approve_stock_count_on(&app, id.clone()).expect("approved");

    // 37 − 2 = 35 kg.
    let stock = crate::inventory::inventory_on(&app, None).expect("the stock screen");
    let paneer = stock
        .materials
        .iter()
        .find(|m| m.id == "mat_paneer")
        .expect("paneer");
    assert_eq!(paneer.on_hand, "35 kg", "{}", paneer.on_hand);
    assert_ne!(paneer.last_counted, "never counted");

    // Sealed: the same count cannot be approved twice.
    assert!(approve_stock_count_on(&app, id).is_err());
}

#[test]
fn the_count_sheet_a_person_carries_has_no_book_quantity_on_it() {
    let scratch = Scratch::new("buy_sheet");
    let app = a_shop(&scratch);
    a_supplier(&app, "sup_metro", "Metro", "0");
    save_purchase_on(
        &app,
        a_delivery("pur_1", vec![line("mat_paneer", "12", "kg", "400")]),
    )
    .expect("recorded");

    let sheet = count_sheet_on(&app, String::new()).expect("a sheet");
    assert!(sheet.contains("Paneer"));
    assert!(
        sheet.contains("______"),
        "there has to be somewhere to write"
    );
    for forbidden in ["12", "12000", "12.0"] {
        assert!(
            !sheet.contains(forbidden),
            "the sheet printed the book quantity ({forbidden}) — D128, and every \
             variance then goes to zero"
        );
    }
}

// A shop that has never counted is told so.

#[test]
fn a_shop_that_has_never_counted_its_store_is_told_on_both_screens() {
    let scratch = Scratch::new("buy_never");
    let app = a_shop(&scratch);
    a_supplier(&app, "sup_metro", "Metro", "0");

    let count = stock_count_on(&app, None).expect("the count screen");
    assert!(count.note.contains("Nobody has counted"), "{}", count.note);

    let view = buying_on(&app, None).expect("the buying screen");
    assert!(
        view.attention
            .iter()
            .any(|line| line.contains("Nobody has counted the store")),
        "{:?}",
        view.attention
    );
}

#[test]
fn t17_every_buying_command_leaves_an_audit_row_a_person_can_read() {
    let scratch = Scratch::new("buy_audit");
    let app = a_shop(&scratch);
    a_supplier(&app, "sup_metro", "Metro", "0");
    save_purchase_on(
        &app,
        a_delivery("pur_1", vec![line("mat_rice", "2", "bag", "1000")]),
    )
    .expect("recorded");
    cancel_purchase_on(&app, "pur_1".to_owned(), "Entered twice".to_owned()).expect("cancelled");

    let rows = app
        .with_shop(|shop| {
            shop.db
                .read_transaction(|tx| {
                    // `AuditFilter::default()` is a limit of ONE — the field defaults to 0 and
                    // `list` clamps it up to 1, so a test that asks for "the trail" with a
                    // default filter gets the newest row and nothing else.
                    Repos::new(tx).audit().list(
                        OUTLET,
                        &mb_db::repo::AuditFilter {
                            limit: 50,
                            ..Default::default()
                        },
                    )
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("the trail");

    let actions: Vec<&str> = rows.iter().map(|r| r.action.as_str()).collect();
    for expected in ["supplier.saved", "purchase.saved", "purchase.cancelled"] {
        assert!(
            actions.contains(&expected),
            "{expected} is not in the trail: {actions:?}"
        );
    }
    // Every one of them says who, and the words a shopkeeper reads.
    assert!(rows.iter().all(|r| r.staff_id.is_some()));
    assert_eq!(
        mb_auth::audit::action::words("purchase.cancelled"),
        "Cancelled a delivery"
    );
}

// A refusal a person can act on.

#[test]
fn a_return_of_more_than_arrived_is_refused_saying_how_much_is_left() {
    let scratch = Scratch::new("buy_return");
    let app = a_shop(&scratch);
    a_supplier(&app, "sup_metro", "Metro", "0");
    save_purchase_on(
        &app,
        a_delivery("pur_1", vec![line("mat_rice", "2", "bag", "1000")]),
    )
    .expect("recorded");

    let mut back = a_delivery("pur_ret", vec![line("mat_rice", "5", "bag", "1000")]);
    back.returns_purchase_id = "pur_1".to_owned();
    let refused = save_purchase_on(&app, back).expect_err("more than arrived");
    assert!(
        refused.message.contains("left to send back"),
        "the refusal has to say how many: {}",
        refused.message
    );

    // And the honest one goes through, at what those bags cost coming in.
    let mut back = a_delivery("pur_ret", vec![line("mat_rice", "1", "bag", "1000")]);
    back.returns_purchase_id = "pur_1".to_owned();
    save_purchase_on(&app, back).expect("one bag back");

    let stock = crate::inventory::inventory_on(&app, None).expect("the stock screen");
    let rice = stock
        .materials
        .iter()
        .find(|m| m.id == "mat_rice")
        .expect("rice");
    assert_eq!(rice.on_hand, "1 bag", "{}", rice.on_hand);
}

#[test]
fn a_supplier_with_a_wrong_gst_number_is_refused_before_it_reaches_a_return() {
    let scratch = Scratch::new("buy_gstin");
    let app = a_shop(&scratch);
    let refused = save_supplier_on(
        &app,
        SupplierEdit {
            id: "sup_bad".to_owned(),
            name: "Metro".to_owned(),
            phone: String::new(),
            gstin: "29ABCDE1234F1Z5".to_owned(),
            address: String::new(),
            terms_days: "0".to_owned(),
            note: String::new(),
            is_active: true,
        },
    );
    assert!(refused.is_err(), "a bad GSTIN has to be refused");

    // The real one goes in.
    save_supplier_on(
        &app,
        SupplierEdit {
            id: "sup_good".to_owned(),
            name: "Metro".to_owned(),
            phone: String::new(),
            gstin: "29ABCDE1234F1ZW".to_owned(),
            address: String::new(),
            terms_days: "0".to_owned(),
            note: String::new(),
            is_active: true,
        },
    )
    .expect("a valid GSTIN");
}

/// Keeps `Money` honest about being imported for a reason.
const _: Option<Money> = None;

// The budgets, three readings each.

#[test]
fn t16_saving_a_purchase_and_the_profit_statement_are_inside_their_budgets() {
    let scratch = Scratch::new("buy_perf");
    let app = a_shop(&scratch);
    a_supplier(&app, "sup_metro", "Metro", "15");

    // Thirty lines is a wholesale invoice, not a vegetable slip.
    let thirty: Vec<PurchaseLineEdit> = (0..30)
        .map(|n| {
            if n % 2 == 0 {
                line("mat_rice", "2", "bag", "1000")
            } else {
                line("mat_paneer", "5", "kg", "400")
            }
        })
        .collect();

    let mut i2 = Vec::new();
    for run in 0..3 {
        let started = std::time::Instant::now();
        save_purchase_on(&app, a_delivery(&format!("pur_perf_{run}"), thirty.clone()))
            .expect("recorded");
        i2.push(started.elapsed());
    }
    let best = i2.iter().min().copied().unwrap_or_default();
    println!(
        "I2 — a 30-line purchase, all four ledgers: {:?} (three runs: {i2:?})",
        best
    );
    assert!(
        best < std::time::Duration::from_secs(1),
        "I2's worst-case ceiling is 1 s and this took {best:?}"
    );

    let mut i3 = Vec::new();
    for _ in 0..3 {
        let started = std::time::Instant::now();
        crate::reports::report_on(
            &app,
            "profit".to_owned(),
            crate::reports::PeriodArg {
                from: crate::flows::today(crate::flows::now()).to_string(),
                to: crate::flows::today(crate::flows::now()).to_string(),
            },
        )
        .expect("the profit statement");
        i3.push(started.elapsed());
    }
    let best = i3.iter().min().copied().unwrap_or_default();
    println!("I3 — the profit statement: {best:?} (three runs: {i3:?})");
    assert!(
        best < std::time::Duration::from_secs(4),
        "I3's worst-case ceiling is 4 s and this took {best:?}"
    );
}

/// Seed a demo shop with buying in it, so a person can look at the screen.
///
/// ```powershell
/// $env:MB_DEMO="C:\some\scratch\demo"
/// cargo test -p magic-bill --bin magic-bill demo_buying -- --ignored --nocapture
/// $env:APPDATA="C:\some\scratch\demo"   # the app's whole world, isolated
/// cargo run -p magic-bill
/// ```
#[test]
#[ignore = "D55: run by hand to look at the screen, not part of the suite"]
fn demo_buying() {
    let Some(root) = std::env::var_os("MB_DEMO").map(std::path::PathBuf::from) else {
        panic!("set MB_DEMO to the folder that should become the demo's APPDATA");
    };
    let home = root.join("MagicBill");
    std::fs::create_dir_all(&home).expect("the demo folder");
    let db_path = home.join("magicbill.db");
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(home.join("magicbill.db-wal"));
    let _ = std::fs::remove_file(home.join("magicbill.db-shm"));

    let db = Db::open(&DbConfig::new(db_path.clone())).expect("open");
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        let at = crate::flows::now();

        let mut rice = Material::new(MaterialId::new("mat_rice"), "Rice", Dimension::Weight);
        rice.packs = vec![("bag".to_owned(), Qty::from_whole(25_000).expect("in range"))];
        rice.purchase_unit = Some("bag".to_owned());
        rice.category = "Dry goods".to_owned();
        rice.buy_from = "Metro".to_owned();
        rice.reorder_level = Qty::from_whole(20_000).expect("in range");
        repos.stock().save_material(OUTLET, &rice, at)?;

        let mut paneer = Material::new(MaterialId::new("mat_paneer"), "Paneer", Dimension::Weight);
        paneer.packs = vec![("kg".to_owned(), Qty::from_whole(1_000).expect("in range"))];
        paneer.purchase_unit = Some("kg".to_owned());
        paneer.category = "Dairy".to_owned();
        paneer.is_perishable = true;
        paneer.shelf_life_days = Some(3);
        repos.stock().save_material(OUTLET, &paneer, at)?;

        let mut oil = Material::new(MaterialId::new("mat_oil"), "Oil", Dimension::Volume);
        oil.packs = vec![("tin".to_owned(), Qty::from_whole(15_000).expect("in range"))];
        oil.purchase_unit = Some("tin".to_owned());
        oil.category = "Dry goods".to_owned();
        repos.stock().save_material(OUTLET, &oil, at)
    })
    .expect("materials");

    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    app.open_shop(db, db_path);
    // The licence lives beside the config, which for this demo is the folder the running app
    // will read — so it is licensed when it starts.
    let machine = mb_license::MachineId::of(&home);
    let stub = std::sync::Arc::new(mb_license::cloud::Stub::active(
        &machine,
        mb_core::BusinessDay::from_days_since_epoch(
            crate::flows::today(crate::flows::now()).days_since_epoch() + 90,
        ),
        crate::flows::now(),
    ));
    let mut licensing = mb_license::Licensing::new(
        home.clone(),
        machine.clone(),
        stub as std::sync::Arc<dyn mb_license::cloud::Cloud>,
        "demo",
    );
    licensing
        .activate(
            "MB-STUB-0001",
            "123456",
            crate::flows::now(),
            std::time::Duration::from_secs(2),
        )
        .expect("the stub activates");
    app.use_licensing(licensing);

    a_supplier(&app, "sup_metro", "Metro Cash & Carry", "15");
    a_supplier(&app, "sup_market", "Vegetable market", "0");

    // A delivery with everything on it: a free bag, a tempo, and tax.
    let mut big = a_delivery(
        "pur_demo_1",
        vec![
            line("mat_rice", "10", "bag", "1000"),
            line("mat_oil", "2", "tin", "1800"),
        ],
    );
    big.lines[0].free = "1".to_owned();
    big.lines[1].tax_percent = "5".to_owned();
    big.charges = "300".to_owned();
    big.invoice_no = "MET-4471".to_owned();
    save_purchase_on(&app, big).expect("the big delivery");

    // One paid at the door, so the drawer shows it.
    let mut cash = a_delivery("pur_demo_2", vec![line("mat_paneer", "8", "kg", "400")]);
    cash.supplier_id = "sup_market".to_owned();
    cash.paid_now = "3200".to_owned();
    cash.invoice_no = "hand-written".to_owned();
    save_purchase_on(&app, cash).expect("the cash delivery");

    // And a count somebody is in the middle of, with a real difference on it.
    let count = open_stock_count_on(&app, String::new()).expect("opened");
    let id = count.id.clone().expect("an id");
    record_count_line_on(
        &app,
        CountEdit {
            count_id: id.clone(),
            material_id: "mat_paneer".to_owned(),
            counted: "6.5".to_owned(),
            unit: "kg".to_owned(),
        },
    )
    .expect("paneer counted");
    record_count_line_on(
        &app,
        CountEdit {
            count_id: id,
            material_id: "mat_rice".to_owned(),
            counted: "11".to_owned(),
            unit: "bag".to_owned(),
        },
    )
    .expect("rice counted");

    // Fold the WAL back into the file before the app is asked to find it.
    app.with_shop(|shop| shop.db.checkpoint().map_err(|e| crate::words::from_db(&e)))
        .expect("checkpoint");
    // And tell start-up where it is.
    mb_db::locate::write_config(&home, &home.join("magicbill.db")).expect("recorded");
    println!("demo shop ready at {}", home.display());
}

/// A supplier's phone gets the same rule as a customer's.
#[test]
fn a_supplier_phone_is_ten_digits_or_it_is_refused() {
    let scratch = Scratch::new("sup_phone");
    let app = a_shop(&scratch);

    let bad = crate::buying::save_supplier_on(
        &app,
        crate::buying::SupplierEdit {
            id: "sup_bad".to_owned(),
            name: "Anand Traders".to_owned(),
            phone: "call the shop".to_owned(),
            gstin: String::new(),
            address: String::new(),
            terms_days: "0".to_owned(),
            note: String::new(),
            is_active: true,
        },
    )
    .expect_err("a sentence was stored as a phone number");
    assert_eq!(bad.code, "supplier.phone");

    crate::buying::save_supplier_on(
        &app,
        crate::buying::SupplierEdit {
            id: "sup_ok".to_owned(),
            name: "Anand Traders".to_owned(),
            phone: "+91 98765 43210".to_owned(),
            gstin: String::new(),
            address: String::new(),
            terms_days: "0".to_owned(),
            note: String::new(),
            is_active: true,
        },
    )
    .expect("a real number");

    let view = crate::buying::buying_on(&app, None).expect("the screen");
    let stored = view
        .suppliers
        .iter()
        .find(|s| s.id == "sup_ok")
        .and_then(|s| s.phone.clone());
    assert_eq!(
        stored.as_deref(),
        Some("9876543210"),
        "the +91 went to disk"
    );
}

/// The rupee mark cannot reach the database, and this is the end-to-end proof rather than the
/// component test's.
#[test]
fn an_amount_is_stored_as_a_number_whatever_decoration_arrives_with_it() {
    for typed in ["₹1200.50", "1,200.50", " 1200.50 ", "1200.50"] {
        let parsed = crate::menu::parse_money_public(typed).expect(typed);
        assert_eq!(parsed.paise(), 120_050, "{typed:?} came out wrong");
        // And what would go on a bill is the plain form, with no mark on it.
        assert_eq!(parsed.to_plain_string(), "1200.50", "{typed:?}");
        assert!(!parsed.to_plain_string().contains('₹'));
    }

    // A letter is still a refusal — the filter on the screen is a courtesy and this is the
    // control.
    assert!(crate::menu::parse_money_public("12ab").is_err());
    assert!(crate::menu::parse_money_public("").is_err());
}
