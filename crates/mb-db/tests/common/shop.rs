//! A whole shop, built once, used by the backup and repository tests.
//!
//! Deliberately not minimal: T1's claim is that **nothing** is lost, and a
//! fixture that leaves out the awkward cases only proves the easy ones survive.
//! So this shop has a bill in every state, split payments including a khata
//! one, a capped discount, a non-GST line, a partial kitchen delta, a customer
//! who owes money, cash and non-cash expenses, and a locked day close.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::integer_division,
    reason = "tests: expect is the assertion, and the fixture splits a fake total"
)]

use mb_auth::{PermissionSet, RolePreset};

use super::Scratch;
use mb_core::{
    AnyOrder, BillInput, BusinessDay, Cart, CategoryId, Charge, ChargeKind, CustomerId, Discount,
    DiscountEntry, DraftOrder, ItemId, Money, OrderId, OrderType, Payment, PaymentMode,
    PlaceOfSupply, Qty, RoundingMode, Settlement, StaffId, TableId, TaxRate, TaxTreatment,
    Timestamp, compute_bill,
};
use mb_db::repo::floor::{DiningTable, Section};
use mb_db::repo::menu::{Category, MenuItem};
use mb_db::repo::money::{Customer, DayClose, Expense, KhataPayment};
use mb_db::repo::people::{StaffMember, StaffStatus};
use mb_db::repo::settings::{Printer, StoreProfile};
use mb_db::{Db, Repos, backup, settle};

pub const OUTLET: &str = "outlet_default";
pub const TERMINAL: &str = "terminal_default";

fn at(n: i64) -> Timestamp {
    Timestamp::from_millis(1_770_000_000_000 + n * 60_000)
}

fn day(n: i32) -> BusinessDay {
    BusinessDay::from_days_since_epoch(20_600 + n)
}

/// Everything a shop is. Returns the ids that the assertions need to look up.
pub fn build(db: &Db) -> BuiltShop {
    seed_masters(db);
    let orders = seed_orders(db);
    seed_money(db);
    BuiltShop { orders }
}

pub struct BuiltShop {
    pub orders: Vec<OrderId>,
}

fn seed_masters(db: &Db) {
    db.transaction(|tx| {
        let repos = Repos::new(tx);

        // Staff and a role, so permissions have somewhere to hang. The preset
        // rather than a hand-written list: a fixture that invents its own
        // permissions is a fixture that stops resembling a shop.
        repos
            .people()
            .save_role(OUTLET, &RolePreset::Cashier.shape(), at(0))?;
        repos.people().save_staff(
            OUTLET,
            &StaffMember {
                id: StaffId::new("staff_1"),
                name: "Ravi".to_owned(),
                code: Some("R1".to_owned()),
                role_id: Some("role_cashier".to_owned()),
                role_name: None,
                // A REAL Argon2 hash, not a placeholder string. P11 made
                // `StaffMember::pin()` refuse a column it cannot parse — a
                // truncated hash must be a locked door, never "no PIN set" —
                // so a fixture with a fake one would be testing a shop that
                // cannot exist.
                pin_hash: Some(
                    mb_auth::hash_pin(&mb_auth::Pin::parse("123456").expect("a valid PIN"))
                        .expect("hashes")
                        .as_str()
                        .to_owned(),
                ),
                status: StaffStatus::Active,
                permissions: PermissionSet::new(),
                max_discount_bp: None,
                max_discount: None,
            },
            at(0),
        )?;
        // Somebody who has left. Scope 9.15 — never deleted.
        repos.people().save_staff(
            OUTLET,
            &StaffMember {
                id: StaffId::new("staff_2"),
                name: "Priya".to_owned(),
                code: None,
                role_id: None,
                role_name: None,
                pin_hash: None,
                status: StaffStatus::Left,
                permissions: PermissionSet::new(),
                max_discount_bp: None,
                max_discount: None,
            },
            at(0),
        )?;

        repos.floor().save_section(
            OUTLET,
            &Section {
                id: "sec_hall".to_owned(),
                name: "Hall".to_owned(),
                sort_order: 0,
                is_active: true,
            },
            at(0),
        )?;
        for n in 1..=6 {
            repos.floor().save_table(
                OUTLET,
                &DiningTable {
                    id: TableId::new(format!("tbl_{n}")),
                    section_id: Some("sec_hall".to_owned()),
                    label: n.to_string(),
                    seats: 4,
                    pos: Some((n, 1)),
                    sort_order: n,
                    is_active: true,
                },
                at(0),
            )?;
        }

        repos.menu().save_category(
            OUTLET,
            &Category {
                id: CategoryId::new("cat_food"),
                name: "Food".to_owned(),
                sort_order: 0,
                is_active: true,
            },
            at(0),
        )?;
        repos.menu().save_category(
            OUTLET,
            &Category {
                id: CategoryId::new("cat_bar"),
                name: "Bar".to_owned(),
                sort_order: 1,
                is_active: true,
            },
            at(0),
        )?;

        for item in menu() {
            repos.menu().save_item(OUTLET, &item, at(0))?;
        }

        repos.settings().save_store_profile(
            OUTLET,
            &StoreProfile {
                name: "Anna Kuteera".to_owned(),
                address: "12 MG Road, Bengaluru".to_owned(),
                phone: Some("9880012345".to_owned()),
                gstin: Some("29ABCDE1234F1Z5".to_owned()),
                fssai: Some("11223344556677".to_owned()),
                state_code: Some("29".to_owned()),
                upi_id: Some("anna@upi".to_owned()),
                upi_merchant_name: Some("Anna Kuteera".to_owned()),
                is_composition: false,
            },
            at(0),
        )?;

        // Scope 7.11: the offset the owner nudged to get the print straight.
        repos.settings().save_printer(
            OUTLET,
            &Printer {
                id: "prn_counter".to_owned(),
                name: "Counter".to_owned(),
                kind: "spooler".to_owned(),
                address: Some("TVSE RP3160".to_owned()),
                paper_mm: 80,
                is_default: true,
                can_kick_drawer: true,
                offset_x_mm: -2,
                offset_y_mm: 1,
                // P07's three columns: what this printer may receive, which
                // sink draws for it, and v1's "Bold & Dark".
                role: "both".to_owned(),
                engine: "raster".to_owned(),
                is_bold_dark: false,
            },
            at(0),
        )?;

        repos
            .settings()
            .set(OUTLET, "day.starts_at_minutes", &300_i64, at(0), None)?;
        repos
            .settings()
            .set(OUTLET, "bill.show_hsn", &true, at(0), None)?;
        repos.settings().set(
            OUTLET,
            "bill.footer",
            &"Thank you, come again".to_owned(),
            at(0),
            None,
        )?;
        repos.settings().set(
            OUTLET,
            "day.opening_float",
            &Money::from_paise(200_000),
            at(0),
            None,
        )?;
        Ok(())
    })
    .expect("seed the masters");
}

fn menu() -> Vec<MenuItem> {
    vec![
        MenuItem {
            id: ItemId::new("itm_dosa"),
            category_id: Some(CategoryId::new("cat_food")),
            name: "Masala Dosa".to_owned(),
            unit_price: Money::from_paise(12_000),
            tax_class_id: Some(mb_core::TaxClassId::new("tax_food_5")),
            tax_rate: TaxRate::GST_5,
            tax_treatment: TaxTreatment::Exclusive,
            hsn: Some("2106".to_owned()),
            cost_price: Some(Money::from_paise(4_000)),
            short_code: Some("MD".to_owned()),
            prep_minutes: Some(8),
            is_open_price: false,
            is_available: true,
            sort_order: 0,
        },
        MenuItem {
            id: ItemId::new("itm_water"),
            category_id: Some(CategoryId::new("cat_food")),
            name: "Water".to_owned(),
            unit_price: Money::from_paise(2_000),
            // **No class, deliberately.** This is an inclusive-priced one-off,
            // and the seeded "Packaged goods 18%" is exclusive — an item
            // pointing at a class must carry that class's treatment (D56), so
            // giving it one here would be a fixture describing a shop that
            // cannot exist. Found by a CSV round trip, which resolves the
            // class and quite correctly overwrote the disagreement.
            tax_class_id: None,
            tax_rate: TaxRate::GST_18,
            tax_treatment: TaxTreatment::Inclusive,
            hsn: Some("2201".to_owned()),
            cost_price: None,
            short_code: None,
            prep_minutes: None,
            is_open_price: false,
            is_available: true,
            sort_order: 1,
        },
        MenuItem {
            id: ItemId::new("itm_beer"),
            category_id: Some(CategoryId::new("cat_bar")),
            name: "Beer".to_owned(),
            unit_price: Money::from_paise(22_000),
            tax_class_id: Some(mb_core::TaxClassId::new("tax_liquor")),
            tax_rate: TaxRate::ZERO,
            tax_treatment: TaxTreatment::NonGst,
            hsn: None,
            cost_price: Some(Money::from_paise(14_000)),
            short_code: Some("BR".to_owned()),
            prep_minutes: None,
            is_open_price: false,
            is_available: true,
            sort_order: 2,
        },
        MenuItem {
            id: ItemId::new("itm_sweets"),
            category_id: Some(CategoryId::new("cat_food")),
            name: "Mysore Pak (by weight)".to_owned(),
            unit_price: Money::from_paise(60_000),
            tax_class_id: Some(mb_core::TaxClassId::new("tax_food_5")),
            tax_rate: TaxRate::GST_5,
            tax_treatment: TaxTreatment::Exclusive,
            hsn: Some("1704".to_owned()),
            cost_price: None,
            short_code: None,
            prep_minutes: None,
            is_open_price: true,
            is_available: false,
            sort_order: 3,
        },
    ]
}

/// Forty orders across three business days, in every state.
fn seed_orders(db: &Db) -> Vec<OrderId> {
    let mut ids = Vec::new();

    db.transaction(|tx| {
        Repos::new(tx).money().save_customer(
            OUTLET,
            &Customer {
                id: CustomerId::new("cus_1"),
                name: "Suresh".to_owned(),
                phone: Some("9845098450".to_owned()),
                gstin: None,
                address: None,
                credit_limit: Some(Money::from_paise(500_000)),
                is_active: true,
            },
            at(0),
        )
    })
    .expect("seed the customer");

    for n in 0..40_i64 {
        let d = day(i32::try_from(n % 3).expect("small"));
        let id = OrderId::new(format!("ord_{n:03}"));
        let draft = DraftOrder::new(
            id.clone(),
            d,
            at(n + 1),
            if n % 4 == 0 {
                OrderType::Parcel
            } else {
                OrderType::DineIn
            },
            StaffId::new("staff_1"),
        );
        let draft = if n % 4 == 0 {
            draft
        } else {
            draft.on_table(TableId::new(format!("tbl_{}", (n % 6) + 1)))
        };

        // Every fifth order is left as a draft: an order still being typed is a
        // real state and a backup that loses it loses somebody's work.
        if n % 5 == 4 {
            let mut draft = draft;
            draft.core.cart = cart_for(n);
            db.transaction(|tx| {
                Repos::new(tx)
                    .orders()
                    .save(OUTLET, TERMINAL, &AnyOrder::Draft(draft.clone()))
            })
            .expect("save the draft");
            ids.push(id);
            continue;
        }

        let mut draft = draft;
        draft.core.cart = cart_for(n);
        // The kitchen has been told about SOME of it — a partial delta, which
        // is crown jewel 2 and the thing a naive backup flattens.
        let pending = draft
            .core
            .kitchen
            .pending(&draft.core.cart)
            .expect("pending");
        if let Some(first) = pending.first() {
            draft
                .core
                .kitchen
                .mark_printed(std::slice::from_ref(first))
                .expect("mark printed");
        }

        let open = settle::open_draft(db, mb_db::Till::new(OUTLET, TERMINAL), draft).expect("open");

        match n % 5 {
            // Left open on the floor.
            0 => {}
            // Settled.
            1 | 3 => {
                let bill = bill_for(&open.core.cart);
                let settlement = settlement_for(&bill, n);
                let settled = settle::settle(
                    db,
                    mb_db::Till::new(OUTLET, TERMINAL),
                    open,
                    bill,
                    settlement,
                    at(n + 100),
                    StaffId::new("staff_1"),
                )
                .expect("settle");

                // Every tenth settled bill is then voided — with its bill
                // number and its amounts kept (P03).
                if n % 10 == 3 {
                    let voided = settled
                        .void("wrong table", StaffId::new("staff_1"), at(n + 200))
                        .expect("void");
                    db.transaction(|tx| {
                        Repos::new(tx).orders().save(
                            OUTLET,
                            TERMINAL,
                            &AnyOrder::Voided(voided.clone()),
                        )
                    })
                    .expect("save the void");
                }
            }
            // Cancelled: the customer walked out (audit B6).
            _ => {
                let cancelled = open
                    .cancel("customer left", StaffId::new("staff_1"), at(n + 150))
                    .expect("cancel");
                db.transaction(|tx| {
                    Repos::new(tx).orders().save(
                        OUTLET,
                        TERMINAL,
                        &AnyOrder::Cancelled(cancelled.clone()),
                    )
                })
                .expect("save the cancel");
            }
        }
        ids.push(id);
    }
    ids
}

fn cart_for(n: i64) -> Cart {
    let items = menu();
    let mut cart = Cart::new();
    cart.add(
        items[0].snapshot(),
        Qty::from_whole(1 + n % 3).expect("qty"),
        if n % 3 == 0 {
            Some("extra crispy".to_owned())
        } else {
            None
        },
        vec![],
    )
    .expect("add");
    cart.add(items[1].snapshot(), Qty::from_whole(2).expect("qty"), None, vec![])
        .expect("add");
    if n % 2 == 0 {
        // A non-GST line, so a bar can bill (scope 2.3).
        cart.add(items[2].snapshot(), Qty::from_whole(1).expect("qty"), None, vec![])
            .expect("add");
    }
    if n % 7 == 0 {
        // A discount big enough to be capped, so `was_capped` has something to
        // say (D15).
        cart.set_line_discount(
            1,
            Some(
                DiscountEntry::new(Discount::amount(Money::from_paise(999_999)).expect("valid"))
                    .with_reason("goodwill")
                    .authorised_by(StaffId::new("staff_1")),
            ),
        )
        .expect("discount");
    }
    cart
}

fn bill_for(cart: &Cart) -> mb_core::Bill {
    let charges = [
        Charge::percent(ChargeKind::Service, "Service Charge", 500, TaxRate::GST_5),
        Charge::flat(
            ChargeKind::Other("Donation".to_owned()),
            "Donation",
            Money::from_paise(500),
            TaxRate::ZERO,
        )
        .with_treatment(TaxTreatment::Exempt),
    ];
    compute_bill(
        BillInput::new(cart)
            .with_bill_discount(DiscountEntry::new(
                Discount::percent_bp(500).expect("valid"),
            ))
            .with_charges(&charges)
            .with_place_of_supply(PlaceOfSupply::Intra)
            .with_order_type(OrderType::DineIn)
            .with_rounding(RoundingMode::NearestRupee),
    )
    .expect("compute")
}

fn settlement_for(bill: &mb_core::Bill, n: i64) -> Settlement {
    let mut settlement = Settlement::with_tip(Money::from_paise(if n % 3 == 0 { 2_000 } else { 0 }))
        .expect("tip");
    // `amount_due` is the bill PLUS the tip — the tip is not taken out of what
    // was owed. Paying only the grand total leaves the bill unsettled, which
    // mb-core refuses, and rightly.
    let total = settlement
        .amount_due(bill.grand_total)
        .expect("amount due")
        .paise();

    if n % 6 == 1 {
        // Split three ways, including khata — the case audit B12 is about.
        let a = Money::from_paise(total / 2);
        let b = Money::from_paise(total / 4);
        let c = Money::from_paise(total - a.paise() - b.paise());
        settlement
            .add(Payment::new(PaymentMode::Card, a).expect("payment").with_reference("APPR-771"))
            .expect("add");
        settlement
            .add(Payment::new(PaymentMode::Other("Sodexo".to_owned()), b).expect("payment"))
            .expect("add");
        settlement
            .add(
                Payment::new(PaymentMode::Credit(CustomerId::new("cus_1")), c)
                    .expect("payment")
                    .settling_khata(),
            )
            .expect("add");
    } else {
        settlement
            .add(Payment::new(PaymentMode::Cash, Money::from_paise(total)).expect("payment"))
            .expect("add");
    }
    settlement
}

fn seed_money(db: &Db) {
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        tx.execute(
            "INSERT INTO expense_categories (id, outlet_id, name) VALUES ('exc_gas', ?1, 'Gas')",
            [OUTLET],
        )?;
        for n in 0..6_i64 {
            repos.money().save_expense(
                OUTLET,
                &Expense {
                    id: format!("exp_{n}"),
                    category_id: Some("exc_gas".to_owned()),
                    description: format!("Gas cylinder {n}"),
                    amount: Money::from_paise(180_000 + n),
                    is_cash: n % 2 == 0,
                    paid_at: at(n),
                    paid_by: Some(StaffId::new("staff_1")),
                    business_day: day(i32::try_from(n % 3).expect("small")),
                },
            )?;
        }
        repos.money().record_khata_payment(
            OUTLET,
            &KhataPayment {
                id: "cpay_1".to_owned(),
                customer_id: CustomerId::new("cus_1"),
                amount: Money::from_paise(30_000),
                mode: "cash".to_owned(),
                reference: None,
                received_at: at(500),
                received_by: Some(StaffId::new("staff_1")),
                business_day: day(1),
            },
        )?;

        let opening = Money::from_paise(200_000);
        let expected = repos.money().expected_cash(OUTLET, day(0), opening)?;
        repos.money().save_day_close(
            OUTLET,
            &DayClose {
                id: "close_0".to_owned(),
                business_day: day(0),
                opening_float: opening,
                expected_cash: expected,
                counted_cash: Money::from_paise(expected.paise() - 5_000),
                variance: Money::from_paise(-5_000),
                is_locked: true,
                closed_at: at(600),
                closed_by: Some(StaffId::new("staff_1")),
            },
        )?;
        Ok(())
    })
    .expect("seed the money");
}

/// Every row of every table, as text, so two databases can be compared without
/// naming four hundred columns.
pub fn snapshot(db: &Db) -> Vec<(String, Vec<String>)> {
    db.read(|conn| {
        let mut out = Vec::new();
        for table in mb_db::schema::tables(conn)? {
            if table == "schema_version" {
                // The ledger legitimately differs: a restored older backup is
                // migrated forward and gains a row with its own timestamp.
                continue;
            }
            let columns = mb_db::schema::columns(conn, &table)?;
            let names: Vec<String> = columns.iter().map(|c| c.name.clone()).collect();
            let sql = format!(
                "SELECT {} FROM {table} ORDER BY {}",
                names.join(", "),
                names.join(", ")
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut rows = stmt.query([])?;
            let mut lines = Vec::new();
            while let Some(row) = rows.next()? {
                let mut cells = Vec::with_capacity(names.len());
                for i in 0..names.len() {
                    cells.push(match row.get_ref(i)? {
                        rusqlite::types::ValueRef::Null => "<null>".to_owned(),
                        rusqlite::types::ValueRef::Integer(v) => v.to_string(),
                        rusqlite::types::ValueRef::Real(v) => v.to_string(),
                        rusqlite::types::ValueRef::Text(v) => {
                            String::from_utf8_lossy(v).into_owned()
                        }
                        rusqlite::types::ValueRef::Blob(_) => "<blob>".to_owned(),
                    });
                }
                lines.push(cells.join("\u{1f}"));
            }
            out.push((table, lines));
        }
        Ok(out)
    })
    .expect("snapshot the database")
}

/// The backup folder used by the backup tests.
#[must_use]
pub fn backup_dir(scratch: &Scratch) -> std::path::PathBuf {
    scratch.db_path().with_file_name("backups")
}

/// Take a backup and verify it, which is what the scheduler will do.
pub fn take_and_verify(db: &Db, dir: &std::path::Path, name: &str) -> backup::Backup {
    let taken = backup::take(db, &dir.join(name), "test").expect("take a backup");
    let report = backup::verify(&taken.path).expect("verify");
    assert!(report.is_ok(), "{}", report.summary());
    taken
}
