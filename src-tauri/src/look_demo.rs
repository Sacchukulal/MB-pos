//! A whole shop, so the look can be designed against a real screen.
//!
//! ```text
//! $env:MB_DEMO="C:\some\scratch\demo"
//! cargo test -p magic-bill --bin magic-bill demo_look -- --ignored --nocapture
//! $env:APPDATA="C:\some\scratch\demo"
//! cargo run -p magic-bill
//! ```

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines,
    reason = "a demo seeder: expect is the assertion, and the shop is a long list"
)]

use mb_core::businessday::BusinessDay;
use mb_core::{
    AnyOrder, Cart, DraftOrder, ItemId, ItemSnapshot, Money, OrderId, OrderType, Qty, StaffId,
    TableId, TaxRate, Timestamp,
};
use mb_db::repo::floor::{DiningTable, Section};
use mb_db::repo::menu::{Category, MenuItem};
use mb_db::{Db, DbConfig, Repos};

use crate::credit::{CustomerEdit, put_on_account_on, save_customer_on};
use crate::expenses::{ExpenseEdit, save_expense_on, save_movement_on};
use crate::inventory::{
    MaterialEdit, MovementEdit, PackEdit, record_movement_on, save_material_on,
};
use crate::state::{App, OUTLET};

/// The menu. Name, price in rupees, tax rate, and the category it sits in.
const MENU: &[(&str, &str, i64, u8, &str)] = &[
    ("itm_dosa_plain", "Plain Dosa", 60, 5, "cat_tiffin"),
    ("itm_dosa_masala", "Masala Dosa", 80, 5, "cat_tiffin"),
    ("itm_dosa_ghee", "Ghee Roast", 120, 5, "cat_tiffin"),
    ("itm_dosa_paneer", "Paneer Dosa", 140, 5, "cat_tiffin"),
    ("itm_idli", "Idli (2 pcs)", 50, 5, "cat_tiffin"),
    ("itm_idli_sambar", "Sambar Idli", 70, 5, "cat_tiffin"),
    ("itm_vada", "Medu Vada", 55, 5, "cat_tiffin"),
    ("itm_upma", "Rava Upma", 60, 5, "cat_tiffin"),
    ("itm_pongal", "Khara Pongal", 75, 5, "cat_tiffin"),
    ("itm_poori", "Poori Saagu", 90, 5, "cat_tiffin"),
    // Rice and curry.
    ("itm_meals", "Full Meals", 180, 5, "cat_rice"),
    ("itm_bisibele", "Bisi Bele Bath", 110, 5, "cat_rice"),
    ("itm_curd_rice", "Curd Rice", 80, 5, "cat_rice"),
    ("itm_lemon_rice", "Lemon Rice", 85, 5, "cat_rice"),
    ("itm_veg_pulao", "Veg Pulao", 160, 5, "cat_rice"),
    ("itm_jeera_rice", "Jeera Rice", 130, 5, "cat_rice"),
    ("itm_curry_pbm", "Paneer Butter Masala", 240, 5, "cat_rice"),
    ("itm_curry_kadai", "Kadai Vegetable", 210, 5, "cat_rice"),
    ("itm_curry_dal", "Dal Tadka", 160, 5, "cat_rice"),
    ("itm_curry_palak", "Palak Paneer", 230, 5, "cat_rice"),
    ("itm_paneer_tikka", "Paneer Tikka", 280, 5, "cat_tandoor"),
    (
        "itm_mushroom_tikka",
        "Mushroom Tikka",
        260,
        5,
        "cat_tandoor",
    ),
    ("itm_veg_seekh", "Veg Seekh Kebab", 240, 5, "cat_tandoor"),
    ("itm_gobi_65", "Gobi 65", 190, 5, "cat_tandoor"),
    ("itm_noodles", "Veg Hakka Noodles", 170, 5, "cat_chinese"),
    ("itm_fried_rice", "Veg Fried Rice", 165, 5, "cat_chinese"),
    ("itm_manchurian", "Gobi Manchurian", 180, 5, "cat_chinese"),
    ("itm_chilli_paneer", "Chilli Paneer", 220, 5, "cat_chinese"),
    ("itm_chapati", "Chapati", 25, 5, "cat_bread"),
    ("itm_naan", "Butter Naan", 55, 5, "cat_bread"),
    ("itm_naan_garlic", "Garlic Naan", 70, 5, "cat_bread"),
    ("itm_roti_tandoori", "Tandoori Roti", 35, 5, "cat_bread"),
    ("itm_kulcha", "Amritsari Kulcha", 95, 5, "cat_bread"),
    // Drinks — the packaged ones are 12% and 18%, which is the point: a bill with one rate on
    // it proves nothing about the tax block.
    ("itm_filter_coffee", "Filter Coffee", 40, 5, "cat_drinks"),
    ("itm_tea", "Tea", 30, 5, "cat_drinks"),
    ("itm_badam_milk", "Badam Milk", 60, 5, "cat_drinks"),
    ("itm_lassi", "Sweet Lassi", 90, 12, "cat_drinks"),
    ("itm_buttermilk", "Masala Buttermilk", 45, 12, "cat_drinks"),
    (
        "itm_soft_drink",
        "Soft Drink (bottle)",
        40,
        18,
        "cat_drinks",
    ),
    ("itm_water", "Mineral Water 1L", 20, 18, "cat_drinks"),
    ("itm_gulab", "Gulab Jamun (2 pcs)", 70, 5, "cat_sweet"),
    ("itm_rasmalai", "Rasmalai", 90, 5, "cat_sweet"),
    ("itm_ice_cream", "Ice Cream Cup", 60, 18, "cat_sweet"),
];

const CATEGORIES: &[(&str, &str, Option<&str>)] = &[
    ("cat_tiffin", "Tiffin", None),
    ("cat_rice", "Rice & Curry", None),
    ("cat_tandoor", "Tandoor", Some("Tandoor")),
    ("cat_chinese", "Chinese", Some("Chinese")),
    ("cat_bread", "Breads", Some("Tandoor")),
    ("cat_drinks", "Drinks", Some("Drinks")),
    ("cat_sweet", "Desserts", None),
];

/// The tables that are busy right now: table, minutes ago it was seated, what is on it, and how
/// many of the first line the kitchen has already been told.
type BusyTable = (
    &'static str,
    i64,
    &'static [(&'static str, i64)],
    Option<i64>,
);

const BUSY: &[BusyTable] = &[
    (
        "tbl_H2",
        6,
        &[("itm_dosa_masala", 2), ("itm_filter_coffee", 2)],
        Some(2),
    ),
    (
        "tbl_H5",
        11,
        &[("itm_meals", 4), ("itm_buttermilk", 4)],
        Some(4),
    ),
    (
        "tbl_H7",
        14,
        &[("itm_idli_sambar", 2), ("itm_vada", 1), ("itm_tea", 3)],
        Some(2),
    ),
    (
        "tbl_H11",
        18,
        &[
            ("itm_paneer_tikka", 1),
            ("itm_naan_garlic", 4),
            ("itm_curry_dal", 1),
        ],
        None,
    ),
    (
        "tbl_H14",
        22,
        &[
            ("itm_noodles", 2),
            ("itm_manchurian", 1),
            ("itm_soft_drink", 3),
        ],
        Some(2),
    ),
    (
        "tbl_H18",
        9,
        &[("itm_dosa_ghee", 1), ("itm_badam_milk", 1)],
        Some(1),
    ),
    (
        "tbl_T1",
        27,
        &[("itm_curry_pbm", 1), ("itm_naan", 6), ("itm_jeera_rice", 2)],
        Some(1),
    ),
    (
        "tbl_T4",
        33,
        &[
            ("itm_veg_pulao", 2),
            ("itm_curry_palak", 1),
            ("itm_lassi", 2),
        ],
        Some(2),
    ),
    (
        "tbl_T8",
        16,
        &[
            ("itm_gobi_65", 1),
            ("itm_chilli_paneer", 1),
            ("itm_water", 2),
        ],
        Some(1),
    ),
    (
        "tbl_A2",
        41,
        &[
            ("itm_curry_kadai", 1),
            ("itm_kulcha", 3),
            ("itm_curd_rice", 2),
        ],
        Some(1),
    ),
    (
        "tbl_A5",
        12,
        &[("itm_fried_rice", 2), ("itm_manchurian", 1)],
        Some(2),
    ),
    (
        "tbl_A7",
        52,
        &[("itm_meals", 2), ("itm_gulab", 2), ("itm_filter_coffee", 2)],
        Some(2),
    ),
    (
        "tbl_F1",
        63,
        &[("itm_bisibele", 3), ("itm_rasmalai", 3), ("itm_tea", 3)],
        Some(3),
    ),
    (
        "tbl_F4",
        8,
        &[("itm_poori", 2), ("itm_upma", 1), ("itm_tea", 2)],
        None,
    ),
];

/// The bills already settled today, so reports and the day close have a day behind them.
const DELIVERY_CUSTOMERS: &[(&str, &str, &str, &str)] = &[
    (
        "cus_meera",
        "Meera",
        "98400 11223",
        "14/3 Kamaraj Street, second gate, blue door",
    ),
    (
        "cus_arun",
        "Arun",
        "98450 66112",
        "Flat 3B, Srinivasa Apartments, behind the temple",
    ),
    (
        "cus_farida",
        "Farida",
        "94440 88221",
        "22 Mount Road, above the medical shop",
    ),
];

/// Five deliveries at five different points of an evening — and the two that matter are the
/// last two: one still on the road with the money to collect, and one that never arrived.
const DELIVERIES: &[(&str, i64, &str, &str, &str)] = &[
    // Item, quantity, customer, state, why it failed.
    ("itm_meals", 2, "cus_meera", "delivered", ""),
    ("itm_curry_pbm", 1, "cus_arun", "delivered", ""),
    ("itm_noodles", 2, "cus_farida", "assigned", ""),
    ("itm_dosa_masala", 3, "cus_meera", "out", ""),
    (
        "itm_meals",
        1,
        "cus_arun",
        "failed",
        "Nobody was home, phone switched off",
    ),
];

const SETTLED: &[(&str, i64, &str)] = &[
    ("itm_meals", 2, "cash"),
    ("itm_dosa_masala", 3, "upi"),
    ("itm_idli_sambar", 2, "cash"),
    ("itm_curry_pbm", 1, "card"),
    ("itm_filter_coffee", 4, "cash"),
    ("itm_noodles", 2, "upi"),
    ("itm_meals", 4, "card"),
    ("itm_dosa_ghee", 2, "cash"),
    ("itm_paneer_tikka", 1, "upi"),
    ("itm_pongal", 2, "cash"),
    ("itm_veg_pulao", 1, "upi"),
    ("itm_lassi", 3, "cash"),
    ("itm_chilli_paneer", 1, "card"),
    ("itm_curd_rice", 2, "cash"),
    ("itm_gulab", 4, "upi"),
    ("itm_meals", 3, "cash"),
    ("itm_soft_drink", 6, "cash"),
    ("itm_fried_rice", 2, "upi"),
    ("itm_dosa_plain", 5, "cash"),
    ("itm_curry_dal", 2, "card"),
    ("itm_naan_garlic", 8, "upi"),
    ("itm_tea", 6, "cash"),
    ("itm_upma", 2, "cash"),
    ("itm_manchurian", 1, "upi"),
    ("itm_rasmalai", 2, "card"),
    ("itm_meals", 2, "cash"),
    ("itm_bisibele", 3, "upi"),
    ("itm_vada", 3, "cash"),
    ("itm_kulcha", 2, "card"),
    ("itm_water", 4, "cash"),
];

#[test]
#[ignore = "D55: run by hand to look at the screens, not part of the suite"]
fn demo_look() {
    let Some(root) = std::env::var_os("MB_DEMO").map(std::path::PathBuf::from) else {
        panic!("set MB_DEMO to the folder that should become the demo's APPDATA");
    };
    let home = root.join("MagicBill");
    std::fs::create_dir_all(&home).expect("the demo folder");
    let db_path = home.join("magicbill.db");

    // Loudly, not `let _ =` — the same trap demo_stock documents: seeding on top of the last
    // one doubles every figure, and the usual cause is the app still being open.
    for suffix in ["", "-wal", "-shm"] {
        let path = std::path::PathBuf::from(format!("{}{suffix}", db_path.display()));
        if path.exists() && std::fs::remove_file(&path).is_err() {
            panic!(
                "{} is still open — close the app before seeding",
                path.display()
            );
        }
    }

    let db = Db::open(&DbConfig::new(db_path.clone())).expect("open");
    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    app.open_shop(db, db_path.clone());

    // A trial, so the screens behind a feature gate (stock, buying, the kitchen display) open
    // rather than showing a licence wall.
    let machine = mb_license::MachineId::of(&home);
    let mut licensing = mb_license::Licensing::new(
        home.clone(),
        machine.clone(),
        std::sync::Arc::new(mb_license::cloud::Stub::active(
            &machine,
            crate::flows::today(crate::flows::now()),
            crate::flows::now(),
        )) as std::sync::Arc<dyn mb_license::Cloud>,
        env!("CARGO_PKG_VERSION"),
    );
    licensing
        .start_trial(
            "+91 90000 00000",
            crate::flows::now(),
            std::time::Duration::from_secs(5),
        )
        .expect("the stub starts a trial");
    app.use_licensing(licensing);

    seed_identity(&app);
    seed_menu(&app);
    seed_room(&app);
    let settled = seed_settled_bills(&app);
    seed_open_orders(&app);
    seed_credit(&app);
    seed_expenses(&app);
    seed_shelf(&app);
    seed_people(&app);
    seed_delivery(&app);
    seed_unconfirmed(&app);

    std::fs::write(
        mb_db::locate::config_path(&home),
        db_path.display().to_string(),
    )
    .expect("the location file");

    println!("demo ready: {}", db_path.display());
    println!(
        "  {} menu items in {} categories",
        MENU.len(),
        CATEGORIES.len()
    );
    println!("  46 tables, {} of them busy", BUSY.len());
    println!("  {settled} bills settled today");
    println!("  6 credit customers, 8 expenses");
    println!("  {} materials on the shelf", MATERIALS.len());
    println!("  {} people, plus one who left", PEOPLE.len());
    println!(
        "  {} deliveries, 2 riders, 1 short handback",
        DELIVERIES.len()
    );
    println!("  1 UPI payment nobody has confirmed");
    println!();
    println!(
        "now: $env:APPDATA=\"{}\"; cargo run -p magic-bill",
        root.display()
    );
}

/// The shop's own name on its own bill.
fn seed_identity(app: &App) {
    let old = app.shop_config();
    let mut new = old.clone();
    new.store.name = "Anna Kuteera".to_owned();
    new.store.address = "12, 4th Block, Jayanagar, Bengaluru 560011".to_owned();
    new.store.phone = "9880012345".to_owned();
    new.store.gstin = "29ABCDE1234F1ZW".to_owned();
    new.store.fssai = "11223344556677".to_owned();
    new.store.state_code = "29".to_owned();
    new.store.upi_id = "annakuteera@okaxis".to_owned();
    new.store.upi_merchant_name = "Anna Kuteera".to_owned();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                crate::settings::save_changes(
                    &Repos::new(tx),
                    OUTLET,
                    &old,
                    &new,
                    crate::flows::now(),
                    None,
                )
                .map(|_| ())
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("the shop's own name");

    app.reload_shop_config();
}

fn seed_menu(app: &App) {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = Repos::new(tx);
                for (n, (id, name, station)) in CATEGORIES.iter().enumerate() {
                    repos.menu().save_category(
                        OUTLET,
                        &Category {
                            id: mb_core::CategoryId::new(*id),
                            name: (*name).to_owned(),
                            sort_order: i64::try_from(n).expect("seven categories"),
                            is_active: true,
                            station: station.map(str::to_owned),
                        },
                        crate::flows::now(),
                    )?;
                }
                for (n, (id, name, rupees, rate, category)) in MENU.iter().enumerate() {
                    repos.menu().save_item(
                        OUTLET,
                        &MenuItem {
                            id: ItemId::new(*id),
                            category_id: Some(mb_core::CategoryId::new(*category)),
                            name: (*name).to_owned(),
                            unit_price: Money::from_paise(rupees * 100),
                            tax: mb_core::TaxSpec::gst_inclusive(rate_of(*rate)),
                            tax_class_id: None,
                            hsn: None,
                            // A cost on the food, so the food-cost and profit screens are not a
                            // column of dashes.
                            cost_price: Some(Money::from_paise(rupees * 34)),
                            short_code: None,
                            prep_minutes: None,
                            course: None,
                            is_open_price: false,
                            is_available: true,
                            sort_order: i64::try_from(n).expect("forty items"),
                        },
                        crate::flows::now(),
                    )?;
                }
                Ok(())
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("a menu");
}

fn rate_of(percent: u8) -> TaxRate {
    match percent {
        12 => TaxRate::from_percent(12).expect("12%"),
        18 => TaxRate::from_percent(18).expect("18%"),
        _ => TaxRate::from_percent(5).expect("5%"),
    }
}

/// Four sections and forty-six tables.
fn seed_room(app: &App) {
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = Repos::new(tx);
                for (n, (id, name)) in [
                    ("sec_hall", "Main Hall"),
                    ("sec_terrace", "Terrace"),
                    ("sec_ac", "A/C Room"),
                    ("sec_family", "Family"),
                ]
                .iter()
                .enumerate()
                {
                    repos.floor().save_section(
                        OUTLET,
                        &Section {
                            id: (*id).to_owned(),
                            name: (*name).to_owned(),
                            sort_order: i64::try_from(n).expect("four sections"),
                            is_active: true,
                        },
                        crate::flows::now(),
                    )?;
                }
                Ok(())
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("four sections");

    // Written directly rather than through `add_tables_on`, for one reason: that helper derives
    // the id from a slug of the label, and this seeder has to be able to NAME the table it
    // seats an order on.
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = Repos::new(tx);
                let mut sort = 0_i64;
                for (section, prefix, count, seats) in [
                    ("sec_hall", "H", 20_u32, 4_i64),
                    ("sec_terrace", "T", 12, 4),
                    ("sec_ac", "A", 8, 6),
                    ("sec_family", "F", 6, 8),
                ] {
                    for n in 1..=count {
                        sort += 1;
                        repos.floor().save_table(
                            OUTLET,
                            &DiningTable {
                                id: TableId::new(format!("tbl_{prefix}{n}")),
                                section_id: Some(section.to_owned()),
                                label: format!("{prefix}{n}"),
                                seats,
                                pos: None,
                                sort_order: sort,
                                is_active: true,
                            },
                            crate::flows::now(),
                        )?;
                    }
                }
                Ok(())
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("forty-six tables");
}

/// The bills already taken today, through the real billing path — so the reports show figures
/// the product produced rather than figures a fixture typed, and the day close has a drawer to
/// count.
fn seed_settled_bills(app: &App) -> usize {
    let mut done = 0;
    for (item, qty, mode) in SETTLED {
        app.with_cart_mut(|state| {
            state.set_order_type(OrderType::Parcel);
            Ok(())
        })
        .expect("parcel");
        crate::ipc::cart_add_on(app, (*item).to_owned(), Some(qty.to_string()), None)
            .expect("added");
        let total = app
            .with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total))
            .expect("bill");
        // Every fourth cash bill carries a tip, so the day close's tips line and the tips
        // report are not judged empty.
        let tip = if done % 4 == 3 && *mode == "cash" {
            Money::from_paise(2_000)
        } else {
            Money::ZERO
        };
        app.with_cart_mut(|state| {
            state.settlement.set_tip(tip).map_err(|e| {
                crate::words::UiError::new("bill.tip", "That tip could not be taken.")
                    .with_detail(e.to_string())
            })?;
            let payment = mb_core::Payment::new(mode_of(mode), total.add(tip).unwrap_or(total))
                .expect("a payment");
            state.settlement.add(payment).map_err(|e| {
                crate::words::UiError::new("bill.pay", "That payment could not be taken.")
                    .with_detail(e.to_string())
            })
        })
        .expect("paid");
        crate::flows::complete_bill_on(app, None).expect("settled");
        done += 1;
    }
    done
}

fn mode_of(tag: &str) -> mb_core::PaymentMode {
    match tag {
        "card" => mb_core::PaymentMode::Card,
        "upi" => mb_core::PaymentMode::Upi,
        _ => mb_core::PaymentMode::Cash,
    }
}

/// Fourteen tables occupied at different ages, plus two parcels and a self-service order so the
/// grid's "No table" group is not empty either.
fn seed_open_orders(app: &App) {
    let now = crate::flows::now();
    let day = crate::flows::today(now);

    for (n, (table, minutes_ago, items, told)) in BUSY.iter().enumerate() {
        let at = Timestamp::from_millis(now.millis() - minutes_ago * 60_000);
        let mut cart = Cart::new();
        for (item, qty) in items.iter() {
            let Some(price) = price_of(item) else {
                continue;
            };
            cart.add(
                ItemSnapshot::new(ItemId::new(*item), name_of(item), price, rate_for(item)),
                Qty::from_whole(*qty).expect("qty"),
                None,
                Vec::new(),
            )
            .expect("added");
        }
        if cart.lines().is_empty() {
            continue;
        }

        let mut draft = DraftOrder::new(
            OrderId::new(format!("ord_demo_{n}")),
            day,
            at,
            mb_core::Placement::on_table(TableId::new(*table)),
            StaffId::new(crate::state::DEFAULT_STAFF),
        );
        draft.core.cart = cart;

        if let Some(told) = told {
            let identity = draft.core.cart.lines()[0].identity();
            draft
                .core
                .kitchen
                .mark_printed(&[(identity, Qty::from_whole(*told).expect("qty"))])
                .expect("told");
        }

        save_open(app, draft);
    }

    // Two parcels and a self-service order, waiting.
    for (n, (kind, minutes_ago, items)) in [
        (
            OrderType::Parcel,
            4_i64,
            &[("itm_dosa_masala", 2_i64), ("itm_vada", 2)][..],
        ),
        (OrderType::Parcel, 9, &[("itm_meals", 3)][..]),
        (
            OrderType::SelfService,
            2,
            &[("itm_filter_coffee", 2), ("itm_gulab", 2)][..],
        ),
    ]
    .iter()
    .enumerate()
    {
        let at = Timestamp::from_millis(now.millis() - minutes_ago * 60_000);
        let mut cart = Cart::new();
        for (item, qty) in items.iter() {
            let Some(price) = price_of(item) else {
                continue;
            };
            cart.add(
                ItemSnapshot::new(ItemId::new(*item), name_of(item), price, rate_for(item)),
                Qty::from_whole(*qty).expect("qty"),
                None,
                Vec::new(),
            )
            .expect("added");
        }
        let mut draft = DraftOrder::new(
            OrderId::new(format!("ord_demo_nt_{n}")),
            day,
            at,
            mb_core::Placement::new(*kind, None, None).expect("no table"),
            StaffId::new(crate::state::DEFAULT_STAFF),
        );
        draft.core.cart = cart;
        save_open(app, draft);
    }
}

fn save_open(app: &App, draft: DraftOrder) {
    let day = draft.core.business_day;
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = Repos::new(tx);
                let token = mb_db::numbering::claim(
                    tx,
                    OUTLET,
                    crate::terminals::TERMINAL,
                    mb_db::numbering::CounterKind::Token,
                    day,
                )?;
                let bill_number = mb_db::numbering::claim(
                    tx,
                    OUTLET,
                    crate::terminals::TERMINAL,
                    mb_db::numbering::CounterKind::Bill,
                    day,
                )?;
                repos.orders().save(
                    OUTLET,
                    crate::terminals::TERMINAL,
                    &AnyOrder::Open(mb_core::OpenOrder {
                        core: draft.core.clone(),
                        token,
                        bill_number,
                    }),
                )
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("seated");
}

fn price_of(id: &str) -> Option<Money> {
    MENU.iter()
        .find(|(i, ..)| *i == id)
        .map(|(_, _, r, ..)| Money::from_paise(r * 100))
}

fn name_of(id: &str) -> &'static str {
    MENU.iter()
        .find(|(i, ..)| *i == id)
        .map_or("Item", |(_, n, ..)| *n)
}

fn rate_for(id: &str) -> TaxRate {
    MENU.iter()
        .find(|(i, ..)| *i == id)
        .map_or(TaxRate::from_percent(5).expect("5%"), |(.., r, _)| {
            rate_of(*r)
        })
}

/// Six credit customers, one of them over the limit — because "over the limit" is a state with
/// its own words and its own colour, and it cannot be looked at if nobody is over.
fn seed_credit(app: &App) {
    for (id, name, phone, limit) in [
        ("cus_ramesh", "Ramesh Kumar", "9845012345", "5000"),
        ("cus_lakshmi", "Lakshmi Stores", "9886054321", "10000"),
        (
            "cus_infosys",
            "Infotech Park Canteen",
            "9900011122",
            "25000",
        ),
        ("cus_suresh", "Suresh (Auto stand)", "9741023456", "2000"),
        ("cus_meena", "Meena Aunty", "9448099887", ""),
        ("cus_arvind", "Arvind Textiles", "9535066778", "8000"),
    ] {
        save_customer_on(
            app,
            CustomerEdit {
                id: id.to_owned(),
                name: name.to_owned(),
                phone: phone.to_owned(),
                gstin: String::new(),
                address: String::new(),
                credit_limit: limit.to_owned(),
                is_active: true,
            },
        )
        .expect("a customer");
    }

    // Balances, put there the way a shop puts them there: a bill on account.
    for (customer, item, qty, override_limit) in [
        ("cus_ramesh", "itm_meals", 4_i64, false),
        ("cus_lakshmi", "itm_curry_pbm", 3, false),
        ("cus_infosys", "itm_meals", 20, false),
        ("cus_suresh", "itm_meals", 2, false),
        // Over the limit on purpose, and approved — so the screen has the state it otherwise
        // never shows.
        ("cus_suresh", "itm_paneer_tikka", 6, true),
        ("cus_arvind", "itm_veg_pulao", 4, false),
    ] {
        app.with_cart_mut(|state| {
            state.set_order_type(OrderType::Parcel);
            Ok(())
        })
        .expect("parcel");
        crate::ipc::cart_add_on(app, item.to_owned(), Some(qty.to_string()), None).expect("added");
        if put_on_account_on(app, customer.to_owned(), override_limit).is_err() {
            // A refusal here is the product working.
            app.with_cart_mut(|state| {
                *state = Default::default();
                Ok(())
            })
            .expect("cleared");
            continue;
        }
        crate::flows::complete_bill_on(app, None).expect("on account");
    }
}

fn seed_expenses(app: &App) {
    // An opening float, so the drawer figure is not the day's takings alone.
    save_movement_on(
        app,
        "float".to_owned(),
        "3000".to_owned(),
        "Opening float for the evening".to_owned(),
    )
    .expect("the opening float");

    for (id, what, amount, mode, who, gst) in [
        (
            "exp_veg",
            "Vegetables — morning market",
            "2450",
            "cash",
            "Vegetable market",
            "",
        ),
        ("exp_milk", "Milk and curd", "1180", "cash", "Milk van", ""),
        (
            "exp_gas",
            "Gas cylinder refill",
            "1850",
            "upi",
            "Bharat Gas",
            "5",
        ),
        (
            "exp_rent",
            "Shop rent — August",
            "45000",
            "bank",
            "Landlord",
            "",
        ),
        (
            "exp_power",
            "Electricity bill",
            "8640",
            "bank",
            "BESCOM",
            "",
        ),
        (
            "exp_wages",
            "Casual staff — evening",
            "1200",
            "cash",
            "Ravi",
            "",
        ),
        (
            "exp_packing",
            "Parcel boxes and covers",
            "2200",
            "upi",
            "Sri Packaging",
            "18",
        ),
        (
            "exp_repair",
            "Mixer repair",
            "650",
            "cash",
            "Kumar Electricals",
            "",
        ),
    ] {
        save_expense_on(
            app,
            ExpenseEdit {
                id: id.to_owned(),
                category_id: None,
                description: what.to_owned(),
                amount: amount.to_owned(),
                mode: mode.to_owned(),
                paid_to: who.to_owned(),
                reference: String::new(),
                gst_percent: gst.to_owned(),
                note: String::new(),
            },
        )
        .expect("an expense");
    }
}

/// The shelf: what the kitchen buys, in the packs a shop actually buys it in.
type Material = (
    &'static str,
    &'static str,
    &'static str,
    Option<(&'static str, &'static str, &'static str)>,
    &'static str,
    &'static str,
    &'static str,
);

const MATERIALS: &[Material] = &[
    (
        "mat_rice",
        "Sona Masoori Rice",
        "weight",
        Some(("bag", "25", "kg")),
        "Metro",
        "2",
        "1450",
    ),
    (
        "mat_atta",
        "Wheat Atta",
        "weight",
        Some(("bag", "10", "kg")),
        "Metro",
        "2",
        "420",
    ),
    (
        "mat_paneer",
        "Paneer",
        "weight",
        None,
        "The milk van",
        "3",
        "340",
    ),
    (
        "mat_milk",
        "Milk",
        "volume",
        Some(("crate", "12", "l")),
        "The milk van",
        "1",
        "660",
    ),
    (
        "mat_curd",
        "Curd",
        "weight",
        None,
        "The milk van",
        "5",
        "70",
    ),
    (
        "mat_oil",
        "Sunflower Oil",
        "volume",
        Some(("tin", "15", "l")),
        "Metro",
        "1",
        "2250",
    ),
    ("mat_ghee", "Ghee", "weight", None, "Metro", "2", "620"),
    (
        "mat_onion",
        "Onion",
        "weight",
        Some(("sack", "50", "kg")),
        "Vegetable market",
        "1",
        "1400",
    ),
    (
        "mat_tomato",
        "Tomato",
        "weight",
        None,
        "Vegetable market",
        "10",
        "38",
    ),
    (
        "mat_potato",
        "Potato",
        "weight",
        Some(("sack", "50", "kg")),
        "Vegetable market",
        "1",
        "1150",
    ),
    (
        "mat_gobi",
        "Cauliflower",
        "count",
        None,
        "Vegetable market",
        "10",
        "35",
    ),
    (
        "mat_capsicum",
        "Capsicum",
        "weight",
        None,
        "Vegetable market",
        "3",
        "80",
    ),
    (
        "mat_maida",
        "Maida",
        "weight",
        Some(("bag", "25", "kg")),
        "Metro",
        "1",
        "1180",
    ),
    (
        "mat_sugar",
        "Sugar",
        "weight",
        Some(("bag", "25", "kg")),
        "Metro",
        "1",
        "1080",
    ),
    (
        "mat_coffee",
        "Coffee Powder",
        "weight",
        None,
        "Coffee Works",
        "2",
        "540",
    ),
    (
        "mat_tea",
        "Tea Powder",
        "weight",
        None,
        "Coffee Works",
        "2",
        "480",
    ),
    (
        "mat_dal",
        "Toor Dal",
        "weight",
        Some(("bag", "30", "kg")),
        "Metro",
        "1",
        "4200",
    ),
    (
        "mat_gaspacket",
        "LPG (commercial)",
        "count",
        None,
        "Bharat Gas",
        "1",
        "1850",
    ),
];

/// What is actually on the shelf right now, in the material's own unit — with two deliberately
/// low so the "what to buy" list is not empty, and one that has gone below zero, which is a
/// real thing that happens and has its own sentence on the screen.
const ON_HAND: &[(&str, &str, &str)] = &[
    ("mat_rice", "48", "kg"),
    ("mat_atta", "16", "kg"),
    ("mat_paneer", "2.4", "kg"),
    ("mat_milk", "22", "l"),
    ("mat_curd", "8", "kg"),
    ("mat_oil", "19", "l"),
    ("mat_ghee", "3.5", "kg"),
    ("mat_onion", "34", "kg"),
    ("mat_tomato", "6", "kg"),
    ("mat_potato", "41", "kg"),
    ("mat_gobi", "14", "piece"),
    ("mat_capsicum", "1.8", "kg"),
    ("mat_maida", "12", "kg"),
    ("mat_sugar", "19", "kg"),
    ("mat_coffee", "3.2", "kg"),
    ("mat_tea", "1.4", "kg"),
    ("mat_dal", "22", "kg"),
    ("mat_gaspacket", "2", "piece"),
];

/// Eighteen materials, because the stock screen is a TABLE and a table with three rows says
/// nothing about a table.
fn seed_shelf(app: &App) {
    for (id, name, dimension, pack, buy_from, reorder, _cost) in MATERIALS {
        save_material_on(
            app,
            MaterialEdit {
                id: (*id).to_owned(),
                name: (*name).to_owned(),
                dimension: (*dimension).to_owned(),
                category: "Kitchen".to_owned(),
                buy_from: (*buy_from).to_owned(),
                reorder_level: (*reorder).to_owned(),
                reorder_qty: (*reorder).to_owned(),
                reorder_unit: pack
                    .map_or_else(|| base_unit(dimension).to_owned(), |(n, _, _)| n.to_owned()),
                is_perishable: matches!(*id, "mat_paneer" | "mat_curd" | "mat_milk"),
                shelf_life_days: match *id {
                    "mat_paneer" => Some(3),
                    "mat_curd" | "mat_milk" => Some(2),
                    _ => None,
                },
                is_active: true,
                packs: pack
                    .map(|(n, size, unit)| {
                        vec![PackEdit {
                            name: (*n).to_owned(),
                            size: (*size).to_owned(),
                            unit: (*unit).to_owned(),
                        }]
                    })
                    .unwrap_or_default(),
                purchase_unit: pack.map_or(String::new(), |(n, _, _)| (*n).to_owned()),
                recipe_unit: String::new(),
            },
        )
        .expect("a material");
    }

    for (id, qty, unit) in ON_HAND {
        let cost = MATERIALS
            .iter()
            .find(|(m, ..)| m == id)
            .map(|(.., c)| (*c).to_owned());
        record_movement_on(
            app,
            MovementEdit {
                material_id: (*id).to_owned(),
                kind: "purchase".to_owned(),
                qty: (*qty).to_owned(),
                unit: (*unit).to_owned(),
                reason_id: None,
                note: None,
                cost,
            },
        )
        .expect("stock in");
    }
}

fn base_unit(dimension: &str) -> &'static str {
    match dimension {
        "volume" => "l",
        "count" => "piece",
        _ => "kg",
    }
}

/// Point this shop's printer at the TVSE and say so.
///
/// ```text
/// $env:MB_DEMO="C:\some\scratch\demo"
/// $env:MB_PRINTER="TVSE RP3200 Lite"
/// cargo test -p magic-bill --bin magic-bill demo_printer -- --ignored --nocapture
/// ```
#[test]
#[ignore = "points a demo shop at a real printer; run by hand"]
fn demo_printer() {
    let Some(root) = std::env::var_os("MB_DEMO").map(std::path::PathBuf::from) else {
        panic!("set MB_DEMO to the demo's APPDATA folder");
    };
    let Some(windows_name) =
        std::env::var_os("MB_PRINTER").map(|s| s.to_string_lossy().into_owned())
    else {
        panic!("set MB_PRINTER to the Windows printer name, e.g. \"TVSE RP3200 Lite\"");
    };

    let home = root.join("MagicBill");
    let db_path = home.join("magicbill.db");
    assert!(
        db_path.exists(),
        "seed the shop first: {}",
        db_path.display()
    );

    let db = Db::open(&DbConfig::new(db_path.clone())).expect("open");
    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    app.open_shop(db, db_path);

    let before = crate::settings::printers::printers_on(&app).expect("the printers");
    let existing = before
        .printers
        .first()
        .map(|p| p.id.clone())
        .unwrap_or_default();

    let view = crate::settings::printers::save_printer_on(
        &app,
        crate::settings::printers::PrinterEdit {
            id: existing,
            name: "Counter".to_owned(),
            kind: "spooler".to_owned(),
            address: windows_name.clone(),
            paper_mm: 80,
            is_default: true,
            role: "bill".to_owned(),
            // `raster`, not "picture" — the schema names the two engines `raster` and `text`,
            // and the SCREEN calls them Picture and Printer font.
            engine: "raster".to_owned(),
            is_bold_dark: false,
            can_kick_drawer: true,
        },
    )
    .expect("the printer");

    for printer in &view.printers {
        println!(
            "  {} — {} — {}",
            printer.name, printer.connection, printer.role
        );
    }
    println!();
    println!("Now open the app against this folder and press Settings → Printers →");
    println!("Test print. The checklist is in docs/OWNER_TESTS.md.");
}

/// The people, and their employment.
const PEOPLE: &[(&str, &str, &str, &str, &str, i64)] = &[
    // Id, name, designation, department, basis, amount in rupees.
    (
        "staff_meena",
        "Meena",
        "Manager",
        "Counter",
        "monthly",
        28_000,
    ),
    (
        "staff_ravi",
        "Ravi",
        "Head cook",
        "Kitchen",
        "monthly",
        24_000,
    ),
    (
        "staff_shashi",
        "Shashi",
        "Cook",
        "Kitchen",
        "monthly",
        18_000,
    ),
    (
        "staff_iqbal",
        "Iqbal",
        "Tandoor",
        "Kitchen",
        "monthly",
        20_000,
    ),
    (
        "staff_deepa",
        "Deepa",
        "Cashier",
        "Counter",
        "monthly",
        16_000,
    ),
    ("staff_kumar", "Kumar", "Waiter", "Service", "daily", 650),
    ("staff_arun", "Arun", "Waiter", "Service", "daily", 650),
    ("staff_sita", "Sita", "Helper", "Kitchen", "daily", 500),
    ("staff_gopal", "Gopal", "Helper", "Kitchen", "hourly", 90),
];

/// Nine people, their salaries, a fortnight of attendance, a roster, leave entitlements and two
/// advances.
fn seed_people(app: &App) {
    let at = crate::flows::now();
    let today = crate::flows::today(at);

    for (id, name, designation, department, basis, amount) in PEOPLE {
        crate::ipc::save_staff_member_on(
            app,
            crate::ipc::StaffEdit {
                id: (*id).to_owned(),
                name: (*name).to_owned(),
                code: Some(name.chars().take(2).collect()),
                role_id: Some(
                    match *department {
                        "Counter" => "role_manager",
                        _ => "role_waiter",
                    }
                    .to_owned(),
                ),
                status: "active".to_owned(),
            },
        )
        .expect("hired");

        crate::employment::save_employee_on(
            app,
            crate::employment::EmployeeEdit {
                id: (*id).to_owned(),
                designation: (*designation).to_owned(),
                department: (*department).to_owned(),
                address: String::new(),
                emergency_name: String::new(),
                emergency_phone: String::new(),
                id_proof: String::new(),
                employment_type: if *basis == "monthly" {
                    "full_time"
                } else {
                    "part_time"
                }
                .to_owned(),
                left_on: String::new(),
            },
        )
        .expect("employment record");

        crate::employment::save_salary_on(
            app,
            crate::employment::SalaryEdit {
                staff_id: (*id).to_owned(),
                // Effective from a month ago, so a run over the last fortnight finds a
                // structure rather than skipping everybody.
                effective_from: ymd(BusinessDay::from_days_since_epoch(
                    today.days_since_epoch() - 40,
                )),
                basis: (*basis).to_owned(),
                amount: amount.to_string(),
                components: Vec::new(),
            },
        )
        .expect("salary");

        // The yearly entitlement, granted.
        crate::employment::adjust_leave_on(
            app,
            (*id).to_owned(),
            "lv_casual".to_owned(),
            24,
            "Yearly entitlement".to_owned(),
            true,
        )
        .expect("leave granted");
    }

    // Somebody who left. The record stays.
    crate::ipc::save_staff_member_on(
        app,
        crate::ipc::StaffEdit {
            id: "staff_prakash".to_owned(),
            name: "Prakash".to_owned(),
            code: Some("Pr".to_owned()),
            role_id: Some("role_waiter".to_owned()),
            status: "active".to_owned(),
        },
    )
    .expect("hired");
    crate::employment::save_employee_on(
        app,
        crate::employment::EmployeeEdit {
            id: "staff_prakash".to_owned(),
            designation: "Waiter".to_owned(),
            department: "Service".to_owned(),
            address: String::new(),
            emergency_name: String::new(),
            emergency_phone: String::new(),
            id_proof: String::new(),
            employment_type: "part_time".to_owned(),
            left_on: ymd(BusinessDay::from_days_since_epoch(
                today.days_since_epoch() - 6,
            )),
        },
    )
    .expect("left");

    // A fortnight of attendance: everybody in, everybody out, with two people late on two of
    // the days so the verdict column has something in it.
    for back in 1..=14_i32 {
        let day = BusinessDay::from_days_since_epoch(today.days_since_epoch() - back);
        for (n, (id, ..)) in PEOPLE.iter().enumerate() {
            // A rest day each, staggered, so the roster has days off in it.
            if (i32::try_from(n).unwrap_or(0) + back) % 7 == 0 {
                // A rostered day OFF — which is a different fact from having no roster row at
                // all, and the screen must not call it an absence.
                put_roster(app, id, day, None);
                continue;
            }
            // The times match the pattern they are rostered against — shp_morning is 07:00 to
            // 15:00. The first version rostered the morning shift and clocked everybody in at
            // 09:00, so the screen said "Late by 2h" against every name in the shop, which is
            // correct and useless.
            let late = back % 5 == 0 && n % 3 == 0;
            let start = if late { 7 * 60 + 40 } else { 7 * 60 };
            let end = 15 * 60;
            // The roster first, then what happened.
            put_roster(app, id, day, Some("shp_morning"));
            put_shift(app, id, day, start, end);
        }
    }

    // Two advances, one of them in instalments — so the payroll screen has a recovery on it and
    // the drawer has a payout in it.
    crate::employment::give_advance_on(
        app,
        "staff_kumar".to_owned(),
        "3000".to_owned(),
        1,
        "Festival".to_owned(),
    )
    .expect("advance");
    crate::employment::give_advance_on(
        app,
        "staff_shashi".to_owned(),
        "6000".to_owned(),
        3,
        "School fees".to_owned(),
    )
    .expect("advance");

    // One approved leave and one still waiting, so both states are on screen.
    let asked = crate::employment::request_leave_on(
        app,
        "staff_deepa".to_owned(),
        "lv_casual".to_owned(),
        ymd(BusinessDay::from_days_since_epoch(
            today.days_since_epoch() - 3,
        )),
        ymd(BusinessDay::from_days_since_epoch(
            today.days_since_epoch() - 3,
        )),
        2,
        "Family function".to_owned(),
    )
    .expect("asked");
    if let Some(request) = asked.requests.first() {
        crate::employment::decide_leave_on(app, request.id.clone(), true, String::new())
            .expect("approved");
    }
    crate::employment::request_leave_on(
        app,
        "staff_iqbal".to_owned(),
        "lv_casual".to_owned(),
        ymd(day_ahead(today, 3)),
        ymd(day_ahead(today, 4)),
        4,
        "Going home".to_owned(),
    )
    .expect("asked");
}

fn seed_delivery(app: &App) {
    use mb_db::repo::delivery::DeliveryState;

    // The riders. Both are already on the staff list — a rider is a member of staff with a
    // flag, not a second people table.
    for id in ["staff_kumar", "staff_ravi"] {
        crate::delivery::set_rider_on(app, id.to_owned(), true).expect("a rider");
    }

    // Somebody to deliver to.
    let now = crate::flows::now();
    let day = crate::flows::today(now);
    for (id, name, phone, address) in DELIVERY_CUSTOMERS {
        app.with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    mb_db::Repos::new(tx).money().save_customer(
                        OUTLET,
                        &mb_db::repo::money::Customer {
                            id: mb_core::CustomerId::new(*id),
                            name: (*name).to_owned(),
                            phone: Some((*phone).to_owned()),
                            gstin: None,
                            address: Some((*address).to_owned()),
                            credit_limit: None,
                            is_active: true,
                        },
                        now,
                    )
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("a delivery customer");
    }

    // Five orders, billed and settled in cash — which is what a shop that takes the money at
    // the counter and sends the food out does, and the case the drawer gets wrong.
    for (n, (item, qty, customer, state, failure)) in DELIVERIES.iter().enumerate() {
        // A parked order stays in the cart.
        crate::ipc::cart_clear_on(app, false).expect("a fresh cart");
        app.with_cart_mut(|state| {
            state.set_order_type(OrderType::Delivery);
            Ok(())
        })
        .expect("delivery");
        crate::ipc::cart_add_on(app, (*item).to_owned(), Some(qty.to_string()), None)
            .expect("added");
        let total = app
            .with_cart(|s| Ok(s.bill(&app.shop_config())?.grand_total))
            .expect("bill");
        // The last two are not settled: one is still on the road with the money to collect, and
        // one never arrived at all.
        let settle = !matches!(*state, "out" | "failed");
        if settle {
            app.with_cart_mut(|s| {
                let payment =
                    mb_core::Payment::new(mb_core::PaymentMode::Cash, total).expect("a payment");
                s.settlement.add(payment).map_err(|e| {
                    crate::words::UiError::new("bill.pay", "That payment could not be taken.")
                        .with_detail(e.to_string())
                })
            })
            .expect("paid");
            crate::flows::complete_bill_on(app, None).expect("settled");
        } else {
            crate::flows::park_open_order(app).expect("held");
        }

        // The one that has not been moved yet.
        let order_id = crate::delivery::board_on(app, None)
            .expect("the board")
            .deliveries
            .iter()
            .find(|d| d.state == "pending")
            .expect("the new delivery is on the board")
            .order_id
            .clone();

        let rider = if n % 2 == 0 {
            "staff_kumar"
        } else {
            "staff_ravi"
        };
        // Walk the state machine, because it refuses a jump — which is the point of it, and a
        // seeder that could skip a step would be seeding a state the product cannot reach.
        let steps: &[&str] = match *state {
            "assigned" => &["assigned"],
            "out" => &["assigned", "out"],
            "delivered" => &["assigned", "out", "delivered"],
            "failed" => &["assigned", "out", "failed"],
            _ => &[],
        };
        for step in steps {
            crate::delivery::save_delivery_on(
                app,
                crate::delivery::DeliveryEdit {
                    order_id: order_id.clone(),
                    address: String::new(),
                    customer_id: (*customer).to_owned(),
                    rider_id: rider.to_owned(),
                    state: (*step).to_owned(),
                    failure: if *step == "failed" {
                        (*failure).to_owned()
                    } else {
                        String::new()
                    },
                },
            )
            .expect("the delivery moved along");
        }
        let _ = DeliveryState::Delivered;
        let _ = day;
    }

    // One handback, and it is SHORT.
    crate::delivery::record_handback_on(
        app,
        "staff_kumar".to_owned(),
        "200".to_owned(),
        "first round".to_owned(),
    )
    .expect("a handback");
}

/// One UPI payment nobody has confirmed, so the day close's list is not empty on a screen that
/// exists to show it.
fn seed_unconfirmed(app: &App) {
    // The last delivery was PARKED, so it is still in the cart — and adding to it here would
    // have turned a failed delivery into a settled parcel.
    crate::ipc::cart_clear_on(app, false).expect("a fresh cart");
    app.with_cart_mut(|state| {
        state.set_order_type(OrderType::Parcel);
        Ok(())
    })
    .expect("parcel");
    crate::ipc::cart_add_on(app, "itm_curry_pbm".to_owned(), Some("2".to_owned()), None)
        .expect("added");
    let total = app
        .with_cart(|s| Ok(s.bill(&app.shop_config())?.grand_total))
        .expect("bill");
    crate::ipc::cart_add_payment_on(
        app,
        "UPI".to_owned(),
        total.paise(),
        Some("UPI/4477112233".to_owned()),
    )
    .expect("taken");
    crate::flows::complete_bill_on(app, None).expect("settled");
}

fn day_ahead(day: BusinessDay, n: i32) -> BusinessDay {
    BusinessDay::from_days_since_epoch(day.days_since_epoch() + n)
}

fn ymd(day: BusinessDay) -> String {
    let (y, m, d) = day.to_ymd();
    format!("{y:04}-{m:02}-{d:02}")
}

/// One finished shift, written straight to the repo.
fn put_shift(app: &App, staff_id: &str, day: BusinessDay, start_minute: i64, end_minute: i64) {
    let stamp = |minute: i64| {
        mb_core::Timestamp::from_local_parts(
            day.days_since_epoch(),
            u32::try_from(minute * 60).unwrap_or(0),
            mb_core::UtcOffset::INDIA,
        )
        .expect("a time on that day")
    };

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                Repos::new(tx).employment().save_attendance(
                    OUTLET,
                    &mb_db::repo::employment::Attendance {
                        id: format!("att_{staff_id}_{}", day.days_since_epoch()),
                        staff_id: staff_id.to_owned(),
                        day,
                        terminal_id: None,
                        shift_no: 0,
                        pattern_id: None,
                        started_at: stamp(start_minute),
                        ended_at: Some(stamp(end_minute)),
                        corrected_at: None,
                        corrected_by: None,
                        correction_reason: None,
                        note: None,
                    },
                )
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("a shift");
}

/// One roster day. `None` is a rostered day OFF.
fn put_roster(app: &App, staff_id: &str, day: BusinessDay, pattern: Option<&str>) {
    let at = crate::flows::now();
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                Repos::new(tx).employment().save_roster_day(
                    OUTLET,
                    &mb_db::repo::employment::RosterDay {
                        id: format!("ros_{staff_id}_{}", day.days_since_epoch()),
                        staff_id: staff_id.to_owned(),
                        day,
                        pattern_id: pattern.map(str::to_owned),
                        note: if pattern.is_none() {
                            Some("Weekly off".to_owned())
                        } else {
                            None
                        },
                    },
                    at,
                    None,
                )
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("a roster day");
}
