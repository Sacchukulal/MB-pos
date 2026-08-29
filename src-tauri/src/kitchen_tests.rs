//! `mb-core`'s `kitchen_delivery` proves the state machine in isolation.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "tests: expect is the assertion"
)]

use mb_core::kitchen_delivery::ACK_SECONDS;
use mb_core::{CategoryId, ItemId, Money, TaxRate};
use mb_db::repo::menu::{Category, MenuItem};
use mb_db::{Db, DbConfig, Repos};

use crate::kitchen;
use crate::signin_tests::Scratch;
use crate::state::{App, OUTLET};

/// A shop with a tandoor and a wok, so "the right station" is a real question.
fn a_kitchen(scratch: &Scratch, name: &str) -> App {
    let path = scratch.dir().join(format!("{name}.db"));
    let db = Db::open(&DbConfig::new(path.clone())).expect("open");
    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    app.open_shop(db, path);
    seed(&app);
    // The shop's own configuration, book included — a default one has no slabs.
    let mut config = app.shop_config();
    config.billing.kitchen_screen = true;
    app.publish_shop_config(config);
    app
}

/// The menu, the room and the two stations.
fn seed(app: &App) {
    let at = crate::flows::now();

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = Repos::new(tx);
                // A real table, so the card says "Table 5" the way a cook reads it rather than
                // an id.
                repos.floor().save_section(
                    OUTLET,
                    &mb_db::repo::floor::Section {
                        id: "sec_hall".to_owned(),
                        name: "Hall".to_owned(),
                        sort_order: 0,
                        is_active: true,
                    },
                    at,
                )?;
                repos.floor().save_table(
                    OUTLET,
                    &mb_db::repo::floor::DiningTable {
                        id: mb_core::TableId::new("tbl_5"),
                        section_id: Some("sec_hall".to_owned()),
                        label: "5".to_owned(),
                        seats: 4,
                        pos: None,
                        sort_order: 1,
                        is_active: true,
                    },
                    at,
                )?;
                for (id, name, station) in [
                    ("cat_tandoor", "Tandoor", Some("Tandoor")),
                    ("cat_chinese", "Chinese", Some("Chinese")),
                    ("cat_drinks", "Drinks", None),
                ] {
                    repos.menu().save_category(
                        OUTLET,
                        &Category {
                            id: CategoryId::new(id),
                            name: name.to_owned(),
                            sort_order: 0,
                            is_active: true,
                            station: station.map(ToOwned::to_owned),
                            default_tax_class_id: None,
                        },
                        at,
                    )?;
                }
                for (id, name, category, prep, course) in [
                    (
                        "itm_naan",
                        "Butter Naan",
                        "cat_tandoor",
                        Some(6),
                        Some("Main"),
                    ),
                    (
                        "itm_tikka",
                        "Paneer Tikka",
                        "cat_tandoor",
                        Some(14),
                        Some("Starter"),
                    ),
                    (
                        "itm_noodles",
                        "Hakka Noodles",
                        "cat_chinese",
                        Some(9),
                        Some("Main"),
                    ),
                    ("itm_lassi", "Lassi", "cat_drinks", Some(2), None),
                ] {
                    repos.menu().save_item(
                        OUTLET,
                        &MenuItem {
                            id: ItemId::new(id),
                            category_id: Some(CategoryId::new(category)),
                            name: name.to_owned(),
                            unit_price: Money::from_paise(9_000),
                            tax_class_id: mb_core::seeded_placement(mb_core::TaxSpec::gst(TaxRate::from_percent(5).expect("5%"))).expect("a seeded slab").0,
                            price_basis: mb_core::seeded_placement(mb_core::TaxSpec::gst(TaxRate::from_percent(5).expect("5%"))).expect("a seeded slab").1,
                            hsn: None,
                            cost_price: None,
                            short_code: None,
                            prep_minutes: prep,
                            course: course.map(ToOwned::to_owned),
                            is_open_price: false,
                            is_available: true,
                            sort_order: 0,
                        },
                        at,
                    )?;
                }
                Ok(())
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("a menu");
}

/// Seat an order on a table with real menu snapshots, then tell the kitchen.
fn order(app: &App, items: &[&str]) -> String {
    let id = seat(app, items);
    kitchen::send(app, &id, None).expect("the kitchen was told");
    id
}

/// Seat the order without telling the kitchen anything — for the course tests, which fire one
/// course at a time.
fn seat(app: &App, items: &[&str]) -> String {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let id = format!("ord_{n:04}");

    let mut cart = mb_core::Cart::new();
    for item_id in items {
        let item = app.find_menu_item(item_id).expect("on the menu");
        cart.add(
            crate::billing::snapshot_for(&item, &app.shop_config().tax).expect("a slab"),
            mb_core::Qty::from_whole(1).expect("in range"),
            None,
            Vec::new(),
        )
        .expect("added");
    }

    let day = crate::flows::today(crate::flows::now());
    let mut draft = mb_core::DraftOrder::new(
        mb_core::OrderId::new(id.clone()),
        day,
        crate::flows::now(),
        mb_core::Placement::on_table(mb_core::TableId::new("tbl_5")),
        mb_core::StaffId::new(crate::state::DEFAULT_STAFF),
    );
    draft.core.cart = cart;

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
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
                Repos::new(tx).orders().save(
                    OUTLET,
                    crate::terminals::TERMINAL,
                    &mb_core::AnyOrder::Open(mb_core::OpenOrder {
                        core: draft.core.clone(),
                        token,
                        bill_number,
                    }),
                )
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("seated");

    id
}

fn tickets(app: &App, station: &str) -> Vec<kitchen::KitchenTicket> {
    kitchen::look(app, station).tickets
}

// The right station, and nowhere else.

/// The tandoor screen shows tandoor food.
#[test]
fn a_ticket_reaches_its_own_station_and_no_other() {
    let scratch = Scratch::new("kds_station");
    let app = a_kitchen(&scratch, "station");
    order(&app, &["itm_naan", "itm_noodles"]);

    let tandoor = tickets(&app, "Tandoor");
    let chinese = tickets(&app, "Chinese");
    assert_eq!(tandoor.len(), 1, "the tandoor got one ticket");
    assert_eq!(chinese.len(), 1, "the wok got one ticket");

    // And each shows only its own food — a cook must not read past dishes that are not theirs
    // to find the one that is.
    let tandoor_names: Vec<&str> = tandoor[0].lines.iter().map(|l| l.name.as_str()).collect();
    assert!(tandoor_names.contains(&"Butter Naan"));

    // A category with no station falls to the shop's one screen.
    order(&app, &["itm_lassi"]);
    assert_eq!(tickets(&app, kitchen::DEFAULT_STATION).len(), 1);
}

// BOTH DIRECTIONS.

#[test]
fn a_ticket_no_screen_drew_goes_to_paper() {
    let scratch = Scratch::new("kds_paper");
    let app = a_kitchen(&scratch, "paper");
    let order_id = order(&app, &["itm_naan"]);
    assert!(!order_id.is_empty());

    // Nothing has drawn it, so it is still pending.
    let before = tickets(&app, "Tandoor");
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].tone, "new");

    let printed = kitchen::print_what_nobody_drew_at(&app, later(ACK_SECONDS + 1));
    assert_eq!(printed, 1, "the kitchen was left blind");

    let after = tickets(&app, "Tandoor");
    assert!(after[0].was_printed, "it did not go to paper");
    assert_eq!(after[0].tone, "printed");
}

#[test]
fn a_screen_that_comes_back_does_not_make_printed_work_new_again() {
    let scratch = Scratch::new("kds_return");
    let app = a_kitchen(&scratch, "return");
    order(&app, &["itm_naan"]);
    let id = tickets(&app, "Tandoor")[0].id.clone();

    assert_eq!(
        kitchen::print_what_nobody_drew_at(&app, later(ACK_SECONDS + 1)),
        1
    );

    // The screen comes back and acks.
    kitchen::shown_on(&app, id.clone()).expect("the screen asked");

    let after = tickets(&app, "Tandoor");
    assert!(
        after[0].was_printed,
        "a late ack turned printed work back into screen work — the kitchen \
         would cook it twice"
    );
    assert_eq!(after[0].tone, "printed");

    // And it is not printed a second time either.
    assert_eq!(
        kitchen::print_what_nobody_drew_at(&app, later(ACK_SECONDS * 10)),
        0
    );
}

/// A screen that IS working keeps the paper in the printer.
#[test]
fn a_screen_that_draws_it_stops_the_printer() {
    let scratch = Scratch::new("kds_drawn");
    let app = a_kitchen(&scratch, "drawn");
    order(&app, &["itm_naan"]);
    let id = tickets(&app, "Tandoor")[0].id.clone();

    kitchen::shown_on(&app, id.clone()).expect("the screen drew it");

    assert_eq!(
        kitchen::print_what_nobody_drew_at(&app, later(9_999)),
        0,
        "paper came out for a ticket that was on a screen"
    );
}

// Bump and recall, both levels, and they survive a reload.

/// Clearing a card, and bringing it back — because a cook bumping the wrong ticket is not an
/// edge case, it is Tuesday.
#[test]
fn a_card_can_be_cleared_and_brought_back() {
    let scratch = Scratch::new("kds_bump");
    let app = a_kitchen(&scratch, "bump");
    order(&app, &["itm_naan"]);
    let id = tickets(&app, "Tandoor")[0].id.clone();

    kitchen::shown_on(&app, id.clone()).expect("drawn");
    kitchen::bump_on(&app, id.clone()).expect("cleared");
    assert!(tickets(&app, "Tandoor").is_empty(), "a cleared card stayed");

    // The way back must be reachable.
    let bar = kitchen::look(&app, "Tandoor");
    let offered = bar.last_cleared.expect("no way to bring the card back");
    assert_eq!(offered.id, id);
    assert!(
        offered.what.contains('5'),
        "the button does not say which card: {}",
        offered.what
    );

    kitchen::recall_on(&app, id.clone()).expect("brought back");
    let back = tickets(&app, "Tandoor");
    assert_eq!(back.len(), 1, "it did not come back");
    assert_eq!(back[0].id, id);
    assert!(
        kitchen::look(&app, "Tandoor").last_cleared.is_none(),
        "it is still offered as cleared after coming back"
    );
}

#[test]
fn one_dish_can_be_ticked_off_and_unticked() {
    let scratch = Scratch::new("kds_line");
    let app = a_kitchen(&scratch, "line");
    order(&app, &["itm_naan", "itm_tikka"]);
    let ticket = tickets(&app, "Tandoor").remove(0);
    let key = ticket.lines[0].key.clone();

    kitchen::bump_line_on(&app, ticket.id.clone(), key.clone()).expect("ticked");
    let after = tickets(&app, "Tandoor").remove(0);
    assert!(after.lines.iter().any(|l| l.key == key && l.is_done));
    assert!(
        after.lines.iter().any(|l| l.key != key && !l.is_done),
        "ticking one dish ticked another"
    );

    // Pressing it again unticks it.
    kitchen::bump_line_on(&app, ticket.id.clone(), key.clone()).expect("unticked");
    assert!(tickets(&app, "Tandoor")[0].lines.iter().all(|l| !l.is_done));
}

/// Bump state is on disk, not in the screen.
#[test]
fn a_cooks_tap_survives_the_screen_reloading() {
    let scratch = Scratch::new("kds_reload");
    let app = a_kitchen(&scratch, "reload");
    order(&app, &["itm_naan"]);
    let id = tickets(&app, "Tandoor")[0].id.clone();
    kitchen::shown_on(&app, id.clone()).expect("drawn");
    kitchen::bump_on(&app, id).expect("cleared");

    // A completely fresh read, as a reloaded tablet does.
    assert!(kitchen::look(&app, "Tandoor").tickets.is_empty());
}

// A cancellation cannot be dismissed, only acknowledged.

#[test]
fn a_cancellation_stays_until_somebody_says_they_saw_it() {
    let scratch = Scratch::new("kds_cancel");
    let app = a_kitchen(&scratch, "cancel");
    let order_id = order(&app, &["itm_naan"]);
    let id = tickets(&app, "Tandoor")[0].id.clone();
    kitchen::shown_on(&app, id.clone()).expect("drawn");

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                Repos::new(tx)
                    .kitchen()
                    .cancel_order(&order_id, crate::flows::now())
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("cancelled");

    let shown = tickets(&app, "Tandoor");
    assert_eq!(
        shown.len(),
        1,
        "a cancelled ticket vanished — a cook is still cooking it"
    );
    assert!(shown[0].is_cancelled);
    assert_eq!(shown[0].tone, "cancelled");
    assert!(shown[0].says.contains("CANCELLED"));

    // It beats late, which is the only other thing that shouts.
    let hours_later = kitchen::look_at(&app, "Tandoor", later(9_999));
    assert_eq!(hours_later.tickets[0].tone, "cancelled");

    // Seen, so it leaves the screen: nobody is cooking it now.
    kitchen::acknowledge_on(&app, id).expect("got it");
    assert!(
        tickets(&app, "Tandoor").is_empty(),
        "it stayed on the screen after being acknowledged"
    );
}

/// Firing the mains does not re-show the starters.
#[test]
fn firing_the_next_course_does_not_re_show_the_first() {
    let scratch = Scratch::new("kds_course");
    let app = a_kitchen(&scratch, "course");
    // Starters first — the tikka goes, the naan waits.
    let order_id = seat(&app, &["itm_tikka", "itm_naan"]);
    kitchen::send(&app, &order_id, Some("Starter")).expect("starters away");

    let starters = tickets(&app, "Tandoor");
    assert_eq!(starters.len(), 1, "the starters did not go");
    let started: Vec<&str> = starters[0].lines.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(
        started,
        vec!["Paneer Tikka"],
        "the mains went with the starters"
    );

    // The waiter clears the plates and fires the mains.
    let waiting = kitchen::look(&app, "Tandoor").waiting_courses;
    assert!(
        waiting.iter().any(|w| w.course == "Main"),
        "the mains were not offered to fire"
    );
    kitchen::fire_on(&app, order_id.clone(), "Main".to_owned()).expect("fired");

    let mains: Vec<kitchen::KitchenTicket> = kitchen::look(&app, "Tandoor")
        .tickets
        .into_iter()
        .filter(|t| t.course == "Main")
        .collect();
    assert_eq!(
        mains.len(),
        1,
        "the mains were not fired as their own ticket"
    );
    let names: Vec<&str> = mains[0].lines.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["Butter Naan"],
        "the starters came back with the mains"
    );
}

/// A firing that named no course covered the whole order, so nothing on it may be fired again.
#[test]
fn a_course_that_already_went_is_never_offered_again() {
    let scratch = Scratch::new("kds_refire");
    let app = a_kitchen(&scratch, "refire");
    let order_id = order(&app, &["itm_tikka", "itm_naan"]);

    assert!(
        kitchen::look(&app, "Tandoor").waiting_courses.is_empty(),
        "food that has already gone to the kitchen was offered to fire again"
    );

    // And if a second terminal presses it anyway, the counter refuses.
    let refused = kitchen::fire_on(&app, order_id, "Main".to_owned());
    assert!(refused.is_err(), "the kitchen was told twice");
}

/// The same rule the other way round: firing the starters must not hide the mains, which have
/// not gone anywhere.
#[test]
fn firing_one_course_leaves_the_others_on_offer() {
    let scratch = Scratch::new("kds_partial");
    let app = a_kitchen(&scratch, "partial");
    let order_id = seat(&app, &["itm_tikka", "itm_naan"]);
    kitchen::send(&app, &order_id, Some("Starter")).expect("starters away");

    let waiting = kitchen::look(&app, "Tandoor").waiting_courses;
    let courses: Vec<&str> = waiting.iter().map(|w| w.course.as_str()).collect();
    assert_eq!(courses, vec!["Main"], "the wrong courses are on offer");

    // And the button says which table AND which bill.
    assert!(
        waiting[0].place.contains("Table 5") && waiting[0].place.contains('#'),
        "the fire button does not say which order: {}",
        waiting[0].place
    );
}

/// The card says the table's LABEL, never its id.
#[test]
fn a_card_names_the_table_the_way_a_cook_says_it() {
    let scratch = Scratch::new("kds_label");
    let app = a_kitchen(&scratch, "label");
    order(&app, &["itm_naan"]);

    let place = &tickets(&app, "Tandoor")[0].place;
    assert_eq!(place, "Table 5", "the raw id reached the kitchen wall");
}

/// A shop that does not use courses never sees one.
#[test]
fn a_menu_with_no_courses_fires_everything_at_once() {
    let scratch = Scratch::new("kds_nocourse");
    let app = a_kitchen(&scratch, "nocourse");
    order(&app, &["itm_lassi"]);

    let view = kitchen::look(&app, kitchen::DEFAULT_STATION);
    assert_eq!(view.tickets.len(), 1);
    assert_eq!(view.tickets[0].course, "", "a course appeared from nowhere");
    assert!(
        view.waiting_courses.is_empty(),
        "a shop with no courses was offered something to fire"
    );
}

/// The dish's own prep time decides late, and the ticket's target is its slowest dish — the
/// order is ready when the last thing is.
#[test]
fn the_target_is_the_slowest_dish_on_the_ticket() {
    let scratch = Scratch::new("kds_target");
    let app = a_kitchen(&scratch, "target");
    // Naan is 6 minutes, tikka is 14. The ticket's target must be 14.
    order(&app, &["itm_naan", "itm_tikka"]);
    let ticket = tickets(&app, "Tandoor").remove(0);
    assert_eq!(ticket.expected, "14 min");
}

/// Build a shop the real app can be opened on, so this screen can be looked at rather than only
/// asserted about.
///
/// ```text
/// $env:MB_DEMO="C:\some\scratch\demo"
/// cargo test -p magic-bill --bin magic-bill demo_kitchen -- --ignored --nocapture
/// $env:APPDATA="C:\some\scratch\demo"   # the app's whole world, isolated
/// cargo run -p magic-bill
/// ```
#[test]
#[ignore = "D55: run by hand to look at the screen, not part of the suite"]
fn demo_kitchen() {
    let Some(root) = std::env::var_os("MB_DEMO").map(std::path::PathBuf::from) else {
        panic!("set MB_DEMO to the folder that should become the demo's APPDATA");
    };
    let home = root.join("MagicBill");
    std::fs::create_dir_all(&home).expect("the demo folder");
    let db_path = home.join("magicbill.db");
    let _ = std::fs::remove_file(&db_path);

    let db = Db::open(&DbConfig::new(db_path.clone())).expect("open");
    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    app.open_shop(db, db_path.clone());
    seed(&app);

    // Several tables of food, each at a different point in its life, so every state on the
    // screen is on screen at once.
    let hot = order(&app, &["itm_naan", "itm_tikka"]);
    let started = order(&app, &["itm_noodles"]);
    let doomed = order(&app, &["itm_naan"]);
    let _quiet = order(&app, &["itm_lassi"]);

    // A table part way through its meal: the starters have gone, the mains are waiting for the
    // waiter to fire them.
    let courses = seat(&app, &["itm_tikka", "itm_naan"]);
    kitchen::send(&app, &courses, Some("Starter")).expect("starters away");

    for id in kitchen::look(&app, "Tandoor")
        .tickets
        .iter()
        .map(|t| t.id.clone())
    {
        kitchen::shown_on(&app, id).expect("drawn");
    }
    kitchen::shown_on(&app, kitchen::look(&app, "Chinese").tickets[0].id.clone()).expect("drawn");

    // One order cancelled, so the loudest state is visible too.
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                Repos::new(tx)
                    .kitchen()
                    .cancel_order(&doomed, crate::flows::now())
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("cancelled");

    // Tell the app where its data is, the way a real install does.
    std::fs::write(
        mb_db::locate::config_path(&home),
        db_path.display().to_string(),
    )
    .expect("the location file");

    println!("demo ready: {}", db_path.display());
    println!("hot={hot} started={started} cancelled={doomed}");
    println!(
        "now: $env:APPDATA=\"{}\"; cargo run -p magic-bill",
        root.display()
    );
}

/// A ticket that has gone past its own target shouts — and it shouts on the dish's own time,
/// not on one number for the whole menu.
#[test]
fn a_ticket_goes_amber_and_then_red_on_its_own_time() {
    let scratch = Scratch::new("kds_late");
    let app = a_kitchen(&scratch, "late");
    // Naan alone: six minutes.
    order(&app, &["itm_naan"]);

    let fresh = kitchen::look(&app, "Tandoor").tickets.remove(0);
    assert_eq!(
        fresh.tone, "new",
        "a ticket one second old was already late"
    );

    let five = kitchen::look_at(&app, "Tandoor", later(5 * 60))
        .tickets
        .remove(0);
    assert_ne!(five.tone, "late", "it went red before its own target");

    let seven = kitchen::look_at(&app, "Tandoor", later(7 * 60))
        .tickets
        .remove(0);
    assert_eq!(
        seven.tone, "late",
        "six-minute naan was not late after seven"
    );
    assert!(
        seven.waiting.contains("min"),
        "the card does not say how long it has been waiting: {}",
        seven.waiting
    );
}

// The tandoor's food goes to the tandoor's printer.

/// Put a printer on this shop and hand back its id.
fn a_station(app: &App, name: &str) -> String {
    let view = crate::settings::printers::save_printer_on(
        app,
        crate::settings::printers::PrinterEdit {
            id: String::new(),
            name: name.to_owned(),
            kind: "spooler".to_owned(),
            address: format!("Windows {name}"),
            paper_mm: 80,
            is_default: false,
            role: "both".to_owned(),
            engine: "raster".to_owned(),
            is_bold_dark: false,
            can_kick_drawer: false,
        },
    )
    .expect("the printer saves");
    view.printers
        .iter()
        .find(|p| p.name == name)
        .expect("it is in the list")
        .id
        .clone()
}

/// Two stations, two rolls, and the right food on each.
#[test]
fn each_category_goes_to_its_own_printer() {
    let scratch = Scratch::new("kitchen_routing");
    let app = a_kitchen(&scratch, "routing");

    // The counter, which everything falls back to, and the tandoor.
    a_station(&app, "Counter");
    let tandoor = a_station(&app, "Tandoor printer");
    crate::settings::printers::route_category_on(&app, "cat_tandoor".to_owned(), tandoor)
        .expect("the route saves");

    // A naan (tandoor) and some noodles (chinese, unrouted) on one bill.
    app.with_cart_mut(|state| {
        for id in ["itm_naan", "itm_noodles"] {
            let item = app.find_menu_item(id).expect("on the menu");
            state
                .cart
                .add(
                    crate::billing::snapshot_for(&item, &app.shop_config().tax).expect("a slab"),
                    mb_core::Qty::from_whole(1).expect("in range"),
                    None,
                    Vec::new(),
                )
                .expect("added");
        }
        state.place_on(mb_core::TableId::new("tbl_5"), "5".to_owned(), None);
        Ok(())
    })
    .expect("a cart");

    crate::flows::print_kitchen_ticket_on(&app).expect("the kitchen was told");

    let tickets: Vec<_> = app
        .print_queue_snapshot()
        .into_iter()
        .filter(|j| j.what == "Kitchen ticket")
        .collect();

    assert_eq!(
        tickets.len(),
        2,
        "one ticket for two stations — the routing did nothing: {tickets:?}"
    );
    assert!(
        tickets.iter().any(|j| j.printer == "Tandoor printer"),
        "the naan never reached the tandoor: {tickets:?}"
    );
    assert!(
        tickets.iter().any(|j| j.printer == "Counter"),
        "the noodles did not fall back to the counter: {tickets:?}"
    );
}

/// And a shop with no routes still gets exactly one ticket.
#[test]
fn an_unrouted_shop_still_prints_one_ticket() {
    let scratch = Scratch::new("kitchen_one_ticket");
    let app = a_kitchen(&scratch, "one_ticket");
    a_station(&app, "Counter");

    app.with_cart_mut(|state| {
        // Three items across three different categories.
        for id in ["itm_naan", "itm_noodles", "itm_lassi"] {
            let item = app.find_menu_item(id).expect("on the menu");
            state
                .cart
                .add(
                    crate::billing::snapshot_for(&item, &app.shop_config().tax).expect("a slab"),
                    mb_core::Qty::from_whole(1).expect("in range"),
                    None,
                    Vec::new(),
                )
                .expect("added");
        }
        state.place_on(mb_core::TableId::new("tbl_5"), "5".to_owned(), None);
        Ok(())
    })
    .expect("a cart");

    crate::flows::print_kitchen_ticket_on(&app).expect("the kitchen was told");

    let tickets: Vec<_> = app
        .print_queue_snapshot()
        .into_iter()
        .filter(|j| j.what == "Kitchen ticket")
        .collect();
    assert_eq!(
        tickets.len(),
        1,
        "three categories became three rolls of paper: {tickets:?}"
    );
    assert_eq!(tickets[0].printer, "Counter");
}

/// Ageing is done by moving the clock, not by rewriting the row.
fn later(seconds: i64) -> mb_core::Timestamp {
    mb_core::Timestamp::from_millis(crate::flows::now().millis().saturating_add(seconds * 1_000))
}

// FIX PLAN 1.

/// A printer with a stated ROLE, so "the bill printer" and "the kitchen printer" can be two
/// different machines and the wrong one can be caught.
fn a_printer(app: &App, name: &str, role: &str, is_default: bool) -> String {
    let view = crate::settings::printers::save_printer_on(
        app,
        crate::settings::printers::PrinterEdit {
            id: String::new(),
            name: name.to_owned(),
            kind: "spooler".to_owned(),
            address: format!("Windows {name}"),
            paper_mm: 80,
            is_default,
            role: role.to_owned(),
            engine: "raster".to_owned(),
            is_bold_dark: false,
            can_kick_drawer: false,
        },
    )
    .expect("the printer saves");
    view.printers
        .iter()
        .find(|p| p.name == name)
        .expect("it is in the list")
        .id
        .clone()
}

/// Put a naan on the bill and seat it at table 5.
fn a_naan_on_table_5(app: &App) {
    app.with_cart_mut(|state| {
        let item = app.find_menu_item("itm_naan").expect("on the menu");
        state
            .cart
            .add(
                crate::billing::snapshot_for(&item, &app.shop_config().tax).expect("a slab"),
                mb_core::Qty::from_whole(1).expect("in range"),
                None,
                Vec::new(),
            )
            .expect("added");
        state.place_on(mb_core::TableId::new("tbl_5"), "5".to_owned(), None);
        Ok(())
    })
    .expect("a cart");
}

fn kitchen_paper(app: &App) -> Vec<crate::state::PrintJobView> {
    app.print_queue_snapshot()
        .into_iter()
        .filter(|j| j.what == "Kitchen ticket")
        .collect()
}

#[test]
fn one_press_makes_one_ticket_when_one_printer_does_both_jobs() {
    let scratch = Scratch::new("fix1_one_press_both");
    let app = a_kitchen(&scratch, "one_press_both");
    a_printer(&app, "Counter", "both", true);
    a_naan_on_table_5(&app);

    crate::flows::print_kitchen_ticket_on(&app).expect("the kitchen was told");
    assert_eq!(
        kitchen_paper(&app).len(),
        1,
        "the press itself printed twice"
    );

    // Nobody draws it, and the fallback comes round.
    let printed = kitchen::print_what_nobody_drew_at(&app, later(ACK_SECONDS + 1));

    let paper = kitchen_paper(&app);
    assert_eq!(
        paper.len(),
        1,
        "the fallback printed the order a second time: {paper:?}"
    );

    // And it says nothing went to paper, because nothing did.
    assert_eq!(
        printed, 0,
        "the fallback reported paper for a ticket the counter had already printed"
    );
}

/// And a kitchen ticket is never lost in a shop with two machines.
#[test]
fn a_pending_ticket_reaches_the_kitchen_printer_and_is_not_lost() {
    let scratch = Scratch::new("fix1_not_lost");
    let app = a_kitchen(&scratch, "not_lost");
    a_printer(&app, "Bill printer", "bill", true);
    a_printer(&app, "Kitchen printer", "kitchen", false);
    a_naan_on_table_5(&app);

    // Parked, and the screen told — but no paper has been printed for it.
    crate::flows::park_open_order(&app).expect("parked");
    let order_id = app
        .with_cart(|state| Ok(state.order_id().map(str::to_owned)))
        .expect("a cart")
        .expect("parked");
    kitchen::send(&app, &order_id, None).expect("the screen was told");
    assert!(
        kitchen_paper(&app).is_empty(),
        "nothing should be on paper yet"
    );

    let _ = kitchen::print_what_nobody_drew_at(&app, later(ACK_SECONDS + 1));

    let paper = kitchen_paper(&app);
    assert_eq!(
        paper.len(),
        1,
        "the pending ticket never reached paper: {paper:?}"
    );
    assert_eq!(
        paper[0].printer, "Kitchen printer",
        "a kitchen ticket went to the wrong machine: {:?}",
        paper[0]
    );
    assert!(
        paper[0].last_error.is_none(),
        "the queue refused the ticket: {:?}",
        paper[0]
    );

    // And it is remembered, so the next round does not print it again.
    let _ = kitchen::print_what_nobody_drew_at(&app, later(ACK_SECONDS * 4));
    assert_eq!(
        kitchen_paper(&app).len(),
        1,
        "the fallback printed the same pending line twice"
    );
}

/// A cancellation slip is a kitchen ticket, so it goes to the kitchen.
#[test]
fn a_cancellation_slip_goes_to_the_kitchen_printer() {
    let scratch = Scratch::new("fix1_cancel");
    let app = a_kitchen(&scratch, "cancel");
    a_printer(&app, "Bill printer", "bill", true);
    a_printer(&app, "Kitchen printer", "kitchen", false);
    a_naan_on_table_5(&app);

    crate::flows::print_kitchen_ticket_on(&app).expect("the kitchen was told");
    let order_id = app
        .with_cart(|state| Ok(state.order_id().map(str::to_owned)))
        .expect("a cart")
        .expect("parked");
    let before = kitchen_paper(&app).len();

    crate::corrections::cancel_order_on(&app, order_id, "Customer left".to_owned())
        .expect("cancelled");

    let paper = kitchen_paper(&app);
    assert_eq!(
        paper.len(),
        before + 1,
        "no cancellation slip was printed: {paper:?}"
    );
    let slip = paper.last().expect("a slip");
    assert_eq!(
        slip.printer, "Kitchen printer",
        "the stop-cooking slip went to the counter: {slip:?}"
    );
    assert!(
        slip.last_error.is_none(),
        "the queue refused the cancellation slip: {slip:?}"
    );
}

/// A dine-in order with no table is refused BEFORE the paper.
#[test]
fn a_dine_in_order_with_no_table_prints_nothing() {
    let scratch = Scratch::new("fix1_no_table");
    let app = a_kitchen(&scratch, "no_table");
    a_printer(&app, "Counter", "both", true);

    app.with_cart_mut(|state| {
        let item = app.find_menu_item("itm_naan").expect("on the menu");
        state
            .cart
            .add(
                crate::billing::snapshot_for(&item, &app.shop_config().tax).expect("a slab"),
                mb_core::Qty::from_whole(1).expect("in range"),
                None,
                Vec::new(),
            )
            .expect("added");
        state.set_order_type(mb_core::OrderType::DineIn);
        Ok(())
    })
    .expect("a cart with no table");

    let outcome = crate::flows::print_kitchen_ticket_on(&app);

    assert!(
        outcome.is_err(),
        "a dine-in order with no table was sent to the kitchen: {outcome:?}"
    );
    assert!(
        kitchen_paper(&app).is_empty(),
        "paper went out before the table was asked for: {:?}",
        kitchen_paper(&app)
    );
}
