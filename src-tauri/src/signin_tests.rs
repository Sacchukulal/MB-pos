//! **The sign-in sequence, driven end to end against a real database.**
//!
//! # Why this file exists
//!
//! *"RUN IT AND LOOK AT IT"* has been part of the method since P08, and it has
//! earned its place: P08 shipped two visible bugs with every test green, P09
//! two, P10 **seven**. Every one was wiring rather than logic, and no unit test
//! was ever going to see them.
//!
//! P11's risk is the same shape and worse, because its bugs lock a shop out of
//! its own till. But its flows are *sequences* — set the first PIN, get locked
//! out, sign in, switch user, lock, come back — and a sequence that can only be
//! checked by clicking is a sequence that gets checked once.
//!
//! So the commands' bodies take `&App` (`ipc.rs`, "the command wrappers"), and
//! this file drives the real ones against a real SQLite file on disk. It is not
//! a substitute for looking at the window. It is a substitute for looking at
//! the window **twice**.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use mb_auth::{Permission, RolePreset};
use mb_db::{Db, DbConfig, Repos};

use crate::ipc::{
    StaffEdit, audit_trail_on, list_staff_on, lock_now_on, lock_state_on, login_on,
    recover_with_code_on, save_role_on, save_staff_member_on, set_staff_pin_on,
};
use crate::state::{App, OUTLET};

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    pub(crate) fn new(label: &str) -> Scratch {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("mb-signin-{label}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch directory");
        Scratch { dir }
    }

    /// Where a test may put a database. `menu_tests` opens its own shop here.
    pub(crate) fn dir(&self) -> &std::path::Path {
        &self.dir
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// An app with a real shop open — which is what `open_shop` does at start-up,
/// including seeding the roles and deciding whether to lock.
fn a_shop(scratch: &Scratch) -> App {
    let path = scratch.dir.join("shop.db");
    let db = Db::open(&DbConfig::new(path.clone())).expect("open");
    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    app.open_shop(db, path);
    app
}

/// Add somebody with a role, the way the Staff screen does.
fn hire(app: &App, id: &str, name: &str, role: RolePreset) {
    save_staff_member_on(
        app,
        StaffEdit {
            id: id.to_owned(),
            name: name.to_owned(),
            code: Some(name.chars().take(1).collect()),
            role_id: Some(role.id().to_owned()),
            status: "active".to_owned(),
        },
    )
    .expect("hired");
}

/// **The whole first day of a shop, in order.**
#[test]
fn a_shop_starts_open_locks_when_it_gets_a_pin_and_lets_the_right_person_in() {
    let scratch = Scratch::new("first_day");
    let app = a_shop(&scratch);

    // 1. Nobody has a PIN. The counter is NOT locked — requirement 3, a shop
    //    must be able to bill on its first day — and the banner is on.
    let state = lock_state_on(&app).expect("state");
    assert!(state.nobody_has_a_pin, "a new shop should not be locked");
    assert_eq!(state.signed_in_as.as_deref(), Some("Counter"));
    assert!(state.people.is_empty(), "nobody can sign in yet");
    assert!(!state.can_recover, "there is no recovery code yet either");

    // 2. The owner adds themselves and gives themselves a PIN.
    hire(&app, "staff_owner", "Sachin", RolePreset::Owner);
    let recovery = set_staff_pin_on(&app, "staff_owner".to_owned(), Some("246813".to_owned()))
        .expect("pin set");
    let recovery = recovery.expect("the first PIN issues a recovery code");
    assert_eq!(recovery.len(), 11, "ten characters and a dash: {recovery}");

    // 3. **Setting the first PIN locks the app then and there** — proving it
    //    works while that person is still standing at the counter.
    let state = lock_state_on(&app).expect("state");
    assert!(!state.nobody_has_a_pin);
    assert_eq!(state.signed_in_as, None, "the counter did not lock itself");
    assert_eq!(state.people.len(), 1, "the owner can now sign in");
    assert!(state.can_recover);

    // 4. The wrong PIN is refused, in Rust, in words.
    let refused = login_on(&app, "staff_owner".to_owned(), "111111".to_owned())
        .expect_err("the wrong PIN was accepted");
    assert_eq!(refused.code, "auth.wrong_pin");
    assert!(lock_state_on(&app).expect("state").signed_in_as.is_none());

    // 5. The right one is not.
    let state =
        login_on(&app, "staff_owner".to_owned(), "246813".to_owned()).expect("the owner signs in");
    assert_eq!(state.signed_in_as.as_deref(), Some("Sachin"));
    assert_eq!(state.role.as_deref(), Some("Owner"));
    assert!(state.permissions.contains(&"staff.manage".to_owned()));

    // 6. And the history says all of it. **Read back off the disk**, because
    //    the screen saying "signed in" is the thing being tested.
    let history = audit_trail_on(&app, None, None, None).expect("the history");
    assert!(history.tampered.is_none(), "{:?}", history.tampered);
    let what: Vec<&str> = history.entries.iter().map(|e| e.what.as_str()).collect();
    assert!(what.contains(&"Logged in"), "{what:?}");
    assert!(what.contains(&"Wrong PIN"), "{what:?}");
    assert!(what.contains(&"Set a PIN"), "{what:?}");
    assert!(what.contains(&"New recovery code printed"), "{what:?}");
    assert!(
        history.entries.iter().all(|e| {
            !e.after.as_deref().unwrap_or_default().contains("246813")
                && !e.before.as_deref().unwrap_or_default().contains("246813")
        }),
        "T2: the PIN reached the audit trail"
    );
}

/// **T12.** Five wrong PINs cost real time, and it survives a restart —
/// which an in-memory counter would not.
#[test]
fn the_lockout_is_real_and_outlives_the_process() {
    let scratch = Scratch::new("lockout");
    {
        let app = a_shop(&scratch);
        hire(&app, "staff_owner", "Sachin", RolePreset::Owner);
        set_staff_pin_on(&app, "staff_owner".to_owned(), Some("246813".to_owned()))
            .expect("pin set");

        for attempt in 1..=5 {
            let refused = login_on(&app, "staff_owner".to_owned(), "111111".to_owned())
                .expect_err("the wrong PIN was accepted");
            if attempt < 5 {
                assert_eq!(refused.code, "auth.wrong_pin", "attempt {attempt}");
            }
        }

        // The fifth failure has started the wait, and even the RIGHT PIN is
        // refused now.
        let refused = login_on(&app, "staff_owner".to_owned(), "246813".to_owned())
            .expect_err("the lockout let somebody straight in");
        assert_eq!(refused.code, "auth.locked_out");
        assert!(refused.message.contains("Try again in"), "{}", refused.message);
        // And it says nothing about how many attempts are left.
        assert!(
            !refused.message.to_lowercase().contains("attempt"),
            "{}",
            refused.message
        );
    }

    // A new process, the same shop.
    let app = a_shop(&scratch);
    let refused = login_on(&app, "staff_owner".to_owned(), "246813".to_owned())
        .expect_err("restarting cleared the lockout");
    assert_eq!(refused.code, "auth.locked_out");

    // The lock screen shows the wait against that person, so nobody stands
    // there typing into a pad that cannot succeed.
    let state = lock_state_on(&app).expect("state");
    assert!(state.people[0].locked_out.is_some());
}

/// **The way back in.** And the shape of it: it needs somebody who manages
/// staff, and it retires itself.
#[test]
fn the_recovery_code_sets_a_new_pin_and_then_stops_working() {
    let scratch = Scratch::new("recovery");
    let app = a_shop(&scratch);
    hire(&app, "staff_owner", "Sachin", RolePreset::Owner);
    hire(&app, "staff_waiter", "Priya", RolePreset::Waiter);
    let code = set_staff_pin_on(&app, "staff_owner".to_owned(), Some("246813".to_owned()))
        .expect("pin set")
        .expect("a code");

    // Not for a waiter. Otherwise the code is a way to hand somebody a PIN and
    // the run of the shop with it.
    let refused = recover_with_code_on(
        &app,
        code.clone(),
        "staff_waiter".to_owned(),
        "999999".to_owned(),
    )
    .expect_err("a waiter was given a PIN by the recovery code");
    assert_eq!(refused.code, "db.failed");

    // The wrong code is refused with something to do about it.
    let refused = recover_with_code_on(
        &app,
        "ABCDE-FGHJK".to_owned(),
        "staff_owner".to_owned(),
        "999999".to_owned(),
    )
    .expect_err("the wrong code worked");
    assert_eq!(refused.code, "auth.recovery_wrong");

    // The right one works, and hands back a NEW code.
    let fresh = recover_with_code_on(
        &app,
        code.clone(),
        "staff_owner".to_owned(),
        "999999".to_owned(),
    )
    .expect("recovered");
    assert_ne!(fresh, code, "the same code came back");

    // The new PIN works and the old one does not.
    assert!(login_on(&app, "staff_owner".to_owned(), "999999".to_owned()).is_ok());
    lock_now_on(&app).expect("locked");
    assert!(login_on(&app, "staff_owner".to_owned(), "246813".to_owned()).is_err());

    // And the old code is dead.
    lock_now_on(&app).expect("locked");
    assert!(
        recover_with_code_on(&app, code, "staff_owner".to_owned(), "555555".to_owned()).is_err(),
        "the used code still works"
    );
}

/// **T13, through the real command.** The rollback is the part worth checking:
/// a refusal that left the shop half-changed would be worse than the change.
#[test]
fn the_last_person_who_can_manage_staff_cannot_be_removed() {
    let scratch = Scratch::new("last_admin");
    let app = a_shop(&scratch);
    hire(&app, "staff_owner", "Sachin", RolePreset::Owner);
    hire(&app, "staff_waiter", "Priya", RolePreset::Waiter);

    // Suspending the only owner.
    let refused = save_staff_member_on(
        &app,
        StaffEdit {
            id: "staff_owner".to_owned(),
            name: "Sachin".to_owned(),
            code: Some("S".to_owned()),
            role_id: Some(RolePreset::Owner.id().to_owned()),
            status: "suspended".to_owned(),
        },
    )
    .expect_err("the last administrator was suspended");
    assert!(
        refused.detail.unwrap_or_default().contains("manage staff"),
        "the refusal should say why"
    );

    // The shop is untouched.
    let people = list_staff_on(&app).expect("staff");
    let owner = people
        .iter()
        .find(|p| p.id == "staff_owner")
        .expect("the owner");
    assert_eq!(owner.status, "active", "the rollback did not happen");

    // Taking the permission off the Owner role is the same refusal by another
    // route — which is why the check asks the database rather than predicting.
    let mut role = crate::ipc::list_roles_on(&app)
        .expect("roles")
        .into_iter()
        .find(|r| r.id == RolePreset::Owner.id())
        .expect("the Owner role");
    role.permissions.retain(|p| p != Permission::StaffManage.code());
    assert!(
        save_role_on(&app, role).is_err(),
        "the Owner role was stripped of staff.manage with nobody else holding it"
    );

    // Give somebody else the permission, and now it is allowed.
    let mut manager = crate::ipc::list_roles_on(&app)
        .expect("roles")
        .into_iter()
        .find(|r| r.id == RolePreset::Manager.id())
        .expect("the Manager role");
    manager
        .permissions
        .push(Permission::StaffManage.code().to_owned());
    save_role_on(&app, manager).expect("the manager may now manage staff");
    hire(&app, "staff_manager", "Anil", RolePreset::Manager);

    save_staff_member_on(
        &app,
        StaffEdit {
            id: "staff_owner".to_owned(),
            name: "Sachin".to_owned(),
            code: Some("S".to_owned()),
            role_id: Some(RolePreset::Owner.id().to_owned()),
            status: "left".to_owned(),
        },
    )
    .expect("with somebody else able to manage staff, the owner may leave");
}

/// **T3.** Deactivating somebody takes effect on the next action, not the next
/// shift — and their history stays (scope 9.15: nobody is ever deleted).
#[test]
fn somebody_who_has_left_cannot_sign_in_but_keeps_their_history() {
    let scratch = Scratch::new("left");
    let app = a_shop(&scratch);
    hire(&app, "staff_owner", "Sachin", RolePreset::Owner);
    hire(&app, "staff_cashier", "Rekha", RolePreset::Cashier);
    set_staff_pin_on(&app, "staff_owner".to_owned(), Some("246813".to_owned())).expect("pin");
    // **The first PIN locks the counter**, so the owner signs in with it before
    // setting anybody else's — which is the point of locking there and then,
    // and is the flow a real owner walks.
    login_on(&app, "staff_owner".to_owned(), "246813".to_owned()).expect("the owner proves it");
    set_staff_pin_on(&app, "staff_cashier".to_owned(), Some("135790".to_owned())).expect("pin");
    lock_now_on(&app).expect("locked");

    login_on(&app, "staff_cashier".to_owned(), "135790".to_owned()).expect("Rekha signs in");
    lock_now_on(&app).expect("locked");

    login_on(&app, "staff_owner".to_owned(), "246813".to_owned()).expect("the owner signs in");
    save_staff_member_on(
        &app,
        StaffEdit {
            id: "staff_cashier".to_owned(),
            name: "Rekha".to_owned(),
            code: Some("R".to_owned()),
            role_id: Some(RolePreset::Cashier.id().to_owned()),
            status: "left".to_owned(),
        },
    )
    .expect("Rekha leaves");
    lock_now_on(&app).expect("locked");

    let refused = login_on(&app, "staff_cashier".to_owned(), "135790".to_owned())
        .expect_err("somebody who has left signed in");
    assert_eq!(refused.code, "auth.not_active");

    // She is off the lock screen, still on the staff list, and still in the
    // history — all three at once.
    assert!(
        lock_state_on(&app)
            .expect("state")
            .people
            .iter()
            .all(|p| p.id != "staff_cashier")
    );
    login_on(&app, "staff_owner".to_owned(), "246813".to_owned()).expect("the owner signs in");
    assert!(
        list_staff_on(&app)
            .expect("staff")
            .iter()
            .any(|p| p.id == "staff_cashier"),
        "scope 9.15: a staff member is never deleted"
    );
    let history = audit_trail_on(&app, Some("staff_cashier".to_owned()), None, None)
        .expect("her history");
    assert!(!history.entries.is_empty(), "her history went with her");
}

/// **T7.** Locking loses nothing, and switching user keeps the order.
#[test]
fn locking_and_switching_user_do_not_touch_the_cart() {
    let scratch = Scratch::new("switch");
    let app = a_shop(&scratch);
    hire(&app, "staff_owner", "Sachin", RolePreset::Owner);
    hire(&app, "staff_cashier", "Rekha", RolePreset::Cashier);
    set_staff_pin_on(&app, "staff_owner".to_owned(), Some("246813".to_owned())).expect("pin");
    // **The first PIN locks the counter**, so the owner signs in with it before
    // setting anybody else's — which is the point of locking there and then,
    // and is the flow a real owner walks.
    login_on(&app, "staff_owner".to_owned(), "246813".to_owned()).expect("the owner proves it");
    set_staff_pin_on(&app, "staff_cashier".to_owned(), Some("135790".to_owned())).expect("pin");
    lock_now_on(&app).expect("locked");

    login_on(&app, "staff_cashier".to_owned(), "135790".to_owned()).expect("Rekha signs in");
    app.with_cart_mut(|state| {
        state.table = Some("tbl_7".to_owned());
        state.opened_by = Some(mb_core::StaffId::new("staff_cashier"));
        Ok(())
    })
    .expect("a table is open");

    lock_now_on(&app).expect("locked");
    login_on(&app, "staff_owner".to_owned(), "246813".to_owned()).expect("the owner takes over");

    app.with_cart(|state| {
        assert_eq!(state.table.as_deref(), Some("tbl_7"));
        assert_eq!(
            state.opened_by.as_ref().map(mb_core::StaffId::as_str),
            Some("staff_cashier"),
            "the order changed hands instead of changing owner"
        );
        Ok(())
    })
    .expect("the cart survived the shift change");
}

/// The four roles are seeded once, and a shop that has edited them keeps its
/// edits when it reopens.
#[test]
fn the_starting_roles_are_seeded_once_and_never_put_back() {
    let scratch = Scratch::new("roles");
    {
        let app = a_shop(&scratch);
        let roles = crate::ipc::list_roles_on(&app).expect("roles");
        assert_eq!(roles.len(), 4);

        let mut waiter = roles
            .into_iter()
            .find(|r| r.id == RolePreset::Waiter.id())
            .expect("the waiter");
        waiter.name = "Steward".to_owned();
        waiter.max_discount_percent = Some("2.5".to_owned());
        save_role_on(&app, waiter).expect("renamed");
    }

    let app = a_shop(&scratch);
    let roles = crate::ipc::list_roles_on(&app).expect("roles");
    assert_eq!(roles.len(), 4, "the presets came back and made a fifth");
    let steward = roles
        .iter()
        .find(|r| r.id == RolePreset::Waiter.id())
        .expect("the waiter");
    assert_eq!(steward.name, "Steward", "the shop's edit was overwritten");
    assert_eq!(steward.max_discount_percent.as_deref(), Some("2.5%"));
}

/// A PIN with no role is somebody who can sign in and do nothing, which looks
/// like a broken app rather than a locked one.
#[test]
fn a_pin_needs_a_role_behind_it() {
    let scratch = Scratch::new("pin_no_role");
    let app = a_shop(&scratch);
    save_staff_member_on(
        &app,
        StaffEdit {
            id: "staff_nobody".to_owned(),
            name: "Nobody".to_owned(),
            code: None,
            role_id: None,
            status: "active".to_owned(),
        },
    )
    .expect("hired with no role");

    let refused = set_staff_pin_on(&app, "staff_nobody".to_owned(), Some("246813".to_owned()))
        .expect_err("a PIN with no role was allowed");
    assert!(
        refused.detail.unwrap_or_default().contains("role"),
        "the refusal should say what to do first"
    );
}

/// The stand-in counter user has no permissions **in the database** — its
/// authority is the in-memory session, and only while no PIN exists.
///
/// Otherwise `active_administrators` would count somebody who can never sign
/// in, and the last-administrator rule would be satisfied by nobody.
#[test]
fn the_stand_in_user_holds_no_permissions_on_disk() {
    let scratch = Scratch::new("stand_in");
    let app = a_shop(&scratch);
    let admins = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| Repos::new(tx).people().active_administrators(OUTLET))
                .map_err(|e| crate::words::from_db(&e))
        })
        .expect("admins");
    assert!(
        admins.is_empty(),
        "the stand-in counts as an administrator on disk: {admins:?}"
    );
}

/// **C3 — the cashier's name on the bill, on the real document.**
///
/// > *"The bill always says 'Cashier: Admin'. Even though a staff list exists."*
///
/// Until P11 this path queued P07's test slip. It now builds P06's real bill,
/// which is the first time that template has rendered a real settled order in
/// the app — and it does it on a shop that has filled nothing in, because that
/// is every shop on its first day.
#[test]
fn the_bill_that_prints_carries_the_real_cashier_and_survives_an_empty_shop() {
    use mb_core::{
        BillInput, Cart, ItemSnapshot, Money, OrderType, Payment, PaymentMode, PlaceOfSupply,
        Qty, RoundingMode, Settlement, TaxRate, TaxTreatment, compute_bill,
    };

    let scratch = Scratch::new("real_bill");
    let app = a_shop(&scratch);
    hire(&app, "staff_cashier", "Rekha", RolePreset::Cashier);

    // A line points at a menu row, so there has to be one. Found by running
    // this: "FOREIGN KEY constraint failed" from `order_lines.item_id`, which
    // is P04's schema doing its job.
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                Repos::new(tx).menu().save_item(
                    OUTLET,
                    &mb_db::repo::menu::MenuItem {
                        id: mb_core::ItemId::new("itm_tea"),
                        category_id: None,
                        name: "Masala Tea".to_owned(),
                        unit_price: mb_core::Money::from_paise(2_500),
                        tax_rate: mb_core::TaxRate::GST_5,
                        tax_treatment: mb_core::TaxTreatment::Exclusive,
                        tax_class_id: None,
                        hsn: None,
                        cost_price: None,
                        short_code: None,
                        prep_minutes: None,
                        is_open_price: false,
                        is_available: true,
                        sort_order: 0,
                    },
                    crate::flows::now(),
                )
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("a menu item");

    let mut cart = Cart::new();
    cart.add(
        ItemSnapshot {
            item_id: mb_core::ItemId::new("itm_tea"),
            name: "Masala Tea".to_owned(),
            unit_price: Money::from_paise(2_500),
            tax_rate: TaxRate::GST_5,
            tax_treatment: TaxTreatment::Exclusive,
            hsn: None,
            category_id: None,
        },
        Qty::from_whole(2).expect("two"),
        None,
        vec![],
    )
    .expect("added");

    let bill = compute_bill(
        BillInput::new(&cart)
            .with_order_type(OrderType::Parcel)
            .with_place_of_supply(PlaceOfSupply::Intra)
            .with_rounding(RoundingMode::NearestRupee),
    )
    .expect("a bill");

    let mut settlement = Settlement::new();
    settlement
        .add(Payment::new(PaymentMode::Cash, bill.grand_total).expect("a payment"))
        .expect("paid");

    let at = crate::flows::now();
    let mut draft = mb_core::DraftOrder::new(
        mb_core::OrderId::new("ord_tea"),
        crate::flows::today(at),
        at,
        OrderType::Parcel,
        mb_core::StaffId::new("staff_cashier"),
    );
    draft.core.cart = cart;

    let settled = app
        .with_shop(|shop| {
            let till = mb_db::Till::new(OUTLET, crate::billing::TERMINAL);
            let open = mb_db::open_draft(&shop.db, till, draft).expect("opened");
            Ok(mb_db::settle(
                &shop.db,
                till,
                open,
                bill.clone(),
                settlement,
                at,
                mb_core::StaffId::new("staff_cashier"),
            )
            .expect("settled"))
        })
        .expect("the shop");

    // The whole point: this must not fail on a shop with no store profile, no
    // logo and no GSTIN — which is every shop on day one.
    crate::flows::queue_bill_print(&app, "0001", &settled, &bill, "Rekha")
        .expect("the real bill could not be built");

    let jobs = app.print_queue_snapshot();
    assert!(
        jobs.iter().any(|j| j.reason.as_deref() == Some("bill 0001")),
        "the bill did not reach the queue: {jobs:?}"
    );
}

// ---------------------------------------------------------------------------
// P12 — the four ways a shop takes something back, driven end to end.
// ---------------------------------------------------------------------------

use crate::corrections::{
    cancel_order_on, day_totals_on, list_bills_on, refund_on, reprint_bill_on, void_bill_on,
    void_line_on,
};

/// A shop with a menu and an owner, ready to trade.
fn a_trading_shop(scratch: &Scratch) -> App {
    let app = a_shop(scratch);
    hire(&app, "staff_owner", "Sachin", RolePreset::Owner);
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                Repos::new(tx).menu().save_item(
                    OUTLET,
                    &mb_db::repo::menu::MenuItem {
                        id: mb_core::ItemId::new("itm_tea"),
                        category_id: None,
                        name: "Masala Tea".to_owned(),
                        unit_price: mb_core::Money::from_paise(2_500),
                        tax_rate: mb_core::TaxRate::GST_5,
                        tax_treatment: mb_core::TaxTreatment::Exclusive,
                        tax_class_id: None,
                        hsn: None,
                        cost_price: None,
                        short_code: None,
                        prep_minutes: None,
                        is_open_price: false,
                        is_available: true,
                        sort_order: 0,
                    },
                    crate::flows::now(),
                )
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("a menu item");
    app
}

/// Put `qty` teas in the cart, the way the billing screen does.
fn order_teas(app: &App, qty: u32) {
    let item = app.find_menu_item("itm_tea").expect("on the menu");
    app.with_cart_mut(|state| {
        *state = crate::billing::CartState {
            order_type: mb_core::OrderType::Parcel,
            ..crate::billing::CartState::default()
        };
        state
            .cart
            .add(
                crate::billing::snapshot_for(&item),
                mb_core::Qty::from_whole(i64::from(qty)).expect("in range"),
                None,
                vec![],
            )
            .expect("added");
        Ok(())
    })
    .expect("a cart");
}

/// Pay it and settle it.
fn settle_the_cart(app: &App) -> String {
    app.with_cart_mut(|state| {
        let total = state.bill()?.grand_total;
        state
            .settlement
            .add(mb_core::Payment::new(mb_core::PaymentMode::Cash, total).expect("a payment"))
            .expect("paid");
        Ok(())
    })
    .expect("paid");
    crate::flows::complete_bill_on(app).expect("settled")
}

/// **T1, T2, T3 and T9 — one walk through a shop's afternoon.**
#[test]
fn a_bill_can_be_voided_and_the_days_figures_still_tie() {
    let scratch = Scratch::new("void_day");
    let app = a_trading_shop(&scratch);

    order_teas(&app, 2);
    let first = settle_the_cart(&app);
    order_teas(&app, 4);
    let second = settle_the_cart(&app);
    assert_ne!(first, second, "two bills took the same number");

    let bills = list_bills_on(&app).expect("the day's bills");
    assert_eq!(bills.len(), 2);
    // Newest first — the bill somebody wants is the one that just printed.
    assert_eq!(bills[0].number, second);

    let before = day_totals_on(&app).expect("totals");
    assert_eq!(before.bills, 2);
    assert_eq!(before.voids.paise, 0);
    assert_eq!(before.net.paise, before.gross.paise);

    // T1: a void with no reason is refused, by Rust, on a direct call.
    let target = bills[0].order_id.clone();
    let refused = void_bill_on(&app, target.clone(), "   ".to_owned(), None, None)
        .expect_err("a void with no reason was allowed");
    assert_eq!(refused.code, "void.refused");

    let after_void = void_bill_on(&app, target.clone(), "Billed twice".to_owned(), None, None)
        .expect("voided");

    // T2: it keeps its number and STAYS IN THE LIST.
    let voided = after_void
        .iter()
        .find(|b| b.order_id == target)
        .expect("the voided bill vanished from the day's list");
    assert_eq!(voided.number, second);
    assert_eq!(voided.state, "voided");
    assert_eq!(voided.void_reason.as_deref(), Some("Billed twice"));

    // T3: gross - voids = net.
    let after = day_totals_on(&app).expect("totals");
    println!(
        "\n  gross {}   voids {}   net {}   ({} bills, {} voided)",
        after.gross.text, after.voids.text, after.net.text, after.bills, after.voided_bills
    );
    assert_eq!(after.bills, 2, "a voided bill left the count");
    assert_eq!(after.voided_bills, 1);
    assert_eq!(after.gross.paise, before.gross.paise, "a void edited the past");
    assert_eq!(
        after.net.paise,
        after.gross.paise - after.voids.paise,
        "gross - voids != net"
    );

    // T9: in the history, with before and after.
    let history = crate::ipc::audit_trail_on(&app, None, None, None).expect("history");
    let entry = history
        .entries
        .iter()
        .find(|e| e.what == "Voided a bill")
        .expect("the void is not in the history");
    assert!(entry.before.as_deref().unwrap_or_default().contains("settled"));
    assert!(entry.after.as_deref().unwrap_or_default().contains("Billed twice"));

    // And a bill cannot be voided twice.
    let again = void_bill_on(&app, target, "Again".to_owned(), None, None)
        .expect_err("the same bill was voided twice");
    assert_eq!(again.code, "void.not_settled");
}

/// **T10.** The manager PIN is enforced in Rust, four ways.
#[test]
fn a_big_void_needs_a_second_person() {
    let scratch = Scratch::new("void_approval");
    let app = a_trading_shop(&scratch);
    hire(&app, "staff_waiter", "Priya", RolePreset::Waiter);

    // Every void in this shop needs approval.
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                Repos::new(tx).settings().set(
                    OUTLET,
                    "bill.void.approval_above_paise",
                    &mb_core::Money::from_paise(1),
                    crate::flows::now(),
                    None,
                )
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("a threshold");

    order_teas(&app, 2);
    settle_the_cart(&app);
    let bills = list_bills_on(&app).expect("bills");
    let target = bills[0].order_id.clone();

    // (a) no approver at all.
    let refused = void_bill_on(&app, target.clone(), "Wrong price".to_owned(), None, None)
        .expect_err("a big void went through unapproved");
    assert_eq!(refused.code, "void.needs_approval");
    assert!(refused.message.contains("manager"), "{}", refused.message);

    // (b) an approver who may not void. The owner gets a PIN first, because
    // the FIRST PIN in a shop locks the counter (P11 item 9) and everything
    // after it would be refused as `auth.locked`.
    set_staff_pin_on(&app, "staff_owner".to_owned(), Some("246813".to_owned()))
        .expect("a PIN for the owner");
    login_on(&app, "staff_owner".to_owned(), "246813".to_owned()).expect("signed back in");
    set_staff_pin_on(&app, "staff_waiter".to_owned(), Some("111111".to_owned()))
        .expect("a PIN for Priya");
    let refused = void_bill_on(
        &app,
        target.clone(),
        "Wrong price".to_owned(),
        Some("staff_waiter".to_owned()),
        Some("111111".to_owned()),
    )
    .expect_err("a waiter waved a void through");
    assert_eq!(refused.code, "void.approver_denied");

    // (c) the right person, the wrong PIN.
    let refused = void_bill_on(
        &app,
        target.clone(),
        "Wrong price".to_owned(),
        Some("staff_owner".to_owned()),
        Some("999999".to_owned()),
    )
    .expect_err("a wrong PIN approved a void");
    assert_eq!(refused.code, "void.approver_wrong_pin");

    // (d) and correctly.
    void_bill_on(
        &app,
        target,
        "Wrong price".to_owned(),
        Some("staff_owner".to_owned()),
        Some("246813".to_owned()),
    )
    .expect("an approved void was still refused");
}

/// **T11.** Money only goes back against a voided bill, and never more than
/// came in.
#[test]
fn money_goes_back_only_after_a_void() {
    let scratch = Scratch::new("refund_flow");
    let app = a_trading_shop(&scratch);
    order_teas(&app, 2);
    settle_the_cart(&app);

    let bills = list_bills_on(&app).expect("bills");
    let target = bills[0].order_id.clone();
    let total = bills[0].total.paise;

    let refused = refund_on(
        &app,
        target.clone(),
        total,
        "cash".to_owned(),
        "Billed twice".to_owned(),
    )
    .expect_err("money went back on a live bill");
    assert!(
        refused.detail.unwrap_or_default().contains("voided"),
        "the refusal should say why"
    );

    void_bill_on(&app, target.clone(), "Billed twice".to_owned(), None, None).expect("voided");

    let refused = refund_on(
        &app,
        target.clone(),
        total + 100,
        "cash".to_owned(),
        "Billed twice".to_owned(),
    )
    .expect_err("more went back than came in");
    assert!(refused.detail.unwrap_or_default().contains("left to give back"));

    let after = refund_on(
        &app,
        target.clone(),
        total,
        "cash".to_owned(),
        "Billed twice".to_owned(),
    )
    .expect("the refund was refused");

    let row = after.iter().find(|b| b.order_id == target).expect("the bill");
    assert_eq!(row.refunded.as_ref().map(|m| m.paise), Some(total));

    let totals = day_totals_on(&app).expect("totals");
    assert_eq!(totals.refunded.paise, total);
    // The void is the whole bill whether or not money went back.
    assert_eq!(totals.voids.paise, total);
}

/// **T7 and T8.** A reprint is counted and marked, and a voided bill reprints
/// as voided rather than as a duplicate.
#[test]
fn a_reprint_says_which_copy_it_is() {
    let scratch = Scratch::new("reprint_flow");
    let app = a_trading_shop(&scratch);
    order_teas(&app, 2);
    settle_the_cart(&app);

    let bills = list_bills_on(&app).expect("bills");
    let target = bills[0].order_id.clone();

    let said = reprint_bill_on(&app, target.clone(), "Customer asked for a copy".to_owned())
        .expect("reprinted");
    assert!(said.contains("Copy 2"), "{said}");
    let said = reprint_bill_on(&app, target.clone(), "Printer jammed".to_owned())
        .expect("reprinted");
    assert!(said.contains("Copy 3"), "{said}");

    let bills = list_bills_on(&app).expect("bills");
    let row = bills.iter().find(|b| b.order_id == target).expect("the bill");
    assert_eq!(row.reprints, 2, "the copies were not counted");

    let jobs = app.print_queue_snapshot();
    assert!(
        jobs.iter().any(|j| j.reason.as_deref() == Some("copy 2")),
        "the copy did not reach the printer: {jobs:?}"
    );

    // And a voided bill reprints as VOIDED — the more important fact about
    // that piece of paper.
    void_bill_on(&app, target.clone(), "Billed twice".to_owned(), None, None).expect("voided");
    reprint_bill_on(&app, target, "Customer asked for a copy".to_owned())
        .expect("a voided bill could not be reprinted");
    let jobs = app.print_queue_snapshot();
    assert!(
        jobs.iter().any(|j| j.reason.as_deref() == Some("voided copy")),
        "the voided copy printed as an ordinary duplicate: {jobs:?}"
    );
}

/// **T6.** A cancel frees the table; a void does not touch one.
///
/// This is also the test that proves an OPEN order reaches the disk at all —
/// before P12 it never did, and `cancel_order` was a command that could not
/// find anything to cancel.
#[test]
fn a_cancel_frees_the_table_and_a_void_does_not() {
    let scratch = Scratch::new("free_table");
    let app = a_trading_shop(&scratch);

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = Repos::new(tx);
                repos.floor().save_section(
                    OUTLET,
                    &mb_db::repo::floor::Section {
                        id: "sec_hall".to_owned(),
                        name: "Hall".to_owned(),
                        sort_order: 0,
                        is_active: true,
                    },
                    crate::flows::now(),
                )?;
                repos.floor().save_table(
                    OUTLET,
                    &mb_db::repo::floor::DiningTable {
                        id: mb_core::TableId::new("tbl_7"),
                        section_id: Some("sec_hall".to_owned()),
                        label: "7".to_owned(),
                        seats: 4,
                        pos: None,
                        sort_order: 7,
                        is_active: true,
                    },
                    crate::flows::now(),
                )
            })
            .map_err(|e| crate::words::from_db(&e))
    })
    .expect("a table");

    let item = app.find_menu_item("itm_tea").expect("on the menu");
    app.with_cart_mut(|state| {
        *state = crate::billing::CartState {
            order_type: mb_core::OrderType::DineIn,
            table: Some("tbl_7".to_owned()),
            table_label: Some("7".to_owned()),
            ..crate::billing::CartState::default()
        };
        state
            .cart
            .add(
                crate::billing::snapshot_for(&item),
                mb_core::Qty::from_whole(2).expect("in range"),
                None,
                vec![],
            )
            .expect("added");
        Ok(())
    })
    .expect("a dine-in cart");

    // Telling the kitchen is what puts the order on the floor.
    crate::flows::print_kitchen_ticket_on(&app).expect("the kitchen was told");
    let order_id = app
        .with_cart(|state| Ok(state.order_id.clone()))
        .expect("cart")
        .expect("the order was never parked");

    let busy = crate::ipc::open_orders_on(&app).expect("floor");
    assert!(
        busy.iter()
            .any(|t| t.id == "tbl_7" && t.order_id.is_some()),
        "the table is not busy after the kitchen was told: {busy:?}"
    );

    cancel_order_on(&app, order_id, "Customer left".to_owned()).expect("cancelled");

    let free = crate::ipc::open_orders_on(&app).expect("floor");
    let table = free.iter().find(|t| t.id == "tbl_7").expect("table 7");
    assert!(
        table.order_id.is_none(),
        "the table is still busy after the order was cancelled — audit B6"
    );

    // And the kitchen was told to stop.
    let jobs = app.print_queue_snapshot();
    assert!(
        jobs.iter().any(|j| j.reason.as_deref() == Some("cancellation")),
        "the kitchen was never told to stop: {jobs:?}"
    );
}

/// **T4 and T5.** Voiding a told item prints a slip for exactly that item, and
/// the line is never re-sent.
#[test]
fn voiding_one_item_tells_the_kitchen_once_and_never_re_sends_it() {
    let scratch = Scratch::new("void_line");
    let app = a_trading_shop(&scratch);

    order_teas(&app, 3);
    crate::flows::print_kitchen_ticket_on(&app).expect("told");
    let before = app.print_queue_snapshot().len();

    let view = void_line_on(&app, 0, "Ordered by mistake".to_owned()).expect("voided");
    assert!(view.lines.is_empty(), "the line is still on the bill");

    // T4: exactly one slip, and it is a cancellation.
    let jobs = app.print_queue_snapshot();
    assert_eq!(jobs.len(), before + 1, "expected exactly one cancellation slip");
    assert!(jobs.iter().any(|j| j.reason.as_deref() == Some("cancellation")));

    // T5: nothing comes back. There is nothing pending and nothing over-told,
    // so a second ticket has nothing to say.
    let nothing = crate::flows::print_kitchen_ticket_on(&app)
        .expect_err("a voided line was sent to the kitchen again");
    assert_eq!(nothing.code, "kitchen.nothing");
}

/// Shared with `perf_tests`, which needs the same scratch shop.
pub(crate) fn scratch(label: &str) -> Scratch {
    Scratch::new(label)
}

pub(crate) fn shop_for_perf(scratch: &Scratch) -> App {
    a_shop(scratch)
}
