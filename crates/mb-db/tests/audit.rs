//! **Can this shop's history be believed?**
//!
//! `mb-auth` proves the chain arithmetic works on rows it invented. These prove
//! it against the real database — the triggers, the sequence, the join, and the
//! part nobody can unit-test: what happens when somebody edits the file.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

mod common;

use common::Scratch;
use common::shop::{self, OUTLET};
use mb_auth::audit::{AuditEntry, BrokenWhy, action};
use mb_auth::{Permission, PermissionSet, RolePreset};
use mb_core::{BusinessDay, StaffId, Timestamp};
use mb_db::repo::AuditFilter;
use mb_db::{Db, Repos};

fn at(n: i64) -> Timestamp {
    Timestamp::from_millis(1_770_000_000_000 + n * 1_000)
}

fn day() -> BusinessDay {
    BusinessDay::from_days_since_epoch(20_600)
}

fn entry(n: i64, what: &'static str) -> AuditEntry {
    AuditEntry::new(
        at(n),
        day(),
        Some(StaffId::new("staff_1")),
        what,
        "bill",
    )
    .about(format!("bill_{n}"))
}

fn write_some(db: &Db, count: i64) {
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        for n in 1..=count {
            repos.audit().append(OUTLET, &entry(n, action::BILL_SETTLED))?;
        }
        Ok(())
    })
    .expect("write the history");
}

/// The sequence starts at one, has no gaps, and links.
#[test]
fn the_chain_is_built_as_it_is_written() {
    let scratch = Scratch::new("audit_chain");
    let db = scratch.open();
    shop::build(&db);
    write_some(&db, 5);

    let rows = db
        .transaction(|tx| {
            Repos::new(tx).audit().list(
                OUTLET,
                &AuditFilter {
                    limit: 50,
                    ..AuditFilter::default()
                },
            )
        })
        .expect("read");

    assert_eq!(rows.len(), 5);
    // Newest first, which is what a screen wants.
    assert_eq!(rows[0].seq, 5);
    assert_eq!(rows[4].seq, 1);
    assert_eq!(rows[4].prev_hash, None, "the first row links to nothing");
    assert_eq!(
        rows[0].prev_hash.as_deref(),
        Some(rows[1].hash.as_str()),
        "each row points at the one before it"
    );
    // The join, so the screen shows a name and not an id.
    assert_eq!(rows[0].staff_name.as_deref(), Some("Ravi"));

    let verified = db
        .transaction(|tx| Repos::new(tx).audit().verify(OUTLET))
        .expect("verify");
    assert_eq!(verified, Ok(()));
}

/// **T5, first half.** An UPDATE is refused by the database itself.
#[test]
fn the_history_cannot_be_changed() {
    let scratch = Scratch::new("audit_update");
    let db = scratch.open();
    shop::build(&db);
    write_some(&db, 3);

    let err = db
        .transaction(|tx| {
            tx.execute("UPDATE audit_log SET action = 'login.ok' WHERE seq = 2", [])
                .map_err(mb_db::DbError::from)
        })
        .expect_err("the history was rewritten");
    assert!(
        err.to_string().contains("the history cannot be changed"),
        "{err}"
    );
}

/// **T5, second half.** So is a DELETE. Two triggers, because they are two
/// different ways to rewrite history.
#[test]
fn the_history_cannot_be_deleted() {
    let scratch = Scratch::new("audit_delete");
    let db = scratch.open();
    shop::build(&db);
    write_some(&db, 3);

    let err = db
        .transaction(|tx| {
            tx.execute("DELETE FROM audit_log WHERE seq = 2", [])
                .map_err(mb_db::DbError::from)
        })
        .expect_err("the history was deleted");
    assert!(
        err.to_string().contains("the history cannot be deleted"),
        "{err}"
    );
}

/// **T6 — and this is the test the whole chain exists for.**
///
/// The triggers stop this program. They stop nobody with a SQLite browser, who
/// drops them first. So: drop them, exactly as that person would, rewrite a
/// row, and check that the shop can still tell.
#[test]
fn a_hand_edited_row_is_detected_even_after_the_triggers_are_dropped() {
    let scratch = Scratch::new("audit_tamper");
    let db = scratch.open();
    shop::build(&db);
    write_some(&db, 6);

    db.transaction(|tx| {
        tx.execute("DROP TRIGGER audit_log_is_append_only_update", [])?;
        tx.execute("DROP TRIGGER audit_log_is_append_only_delete", [])?;
        // "It was not me who voided that bill."
        tx.execute(
            "UPDATE audit_log SET staff_id = 'staff_2' WHERE seq = 3",
            [],
        )?;
        Ok(())
    })
    .expect("tamper");

    let broken = db
        .transaction(|tx| Repos::new(tx).audit().verify(OUTLET))
        .expect("verify")
        .expect_err("the edit was not noticed");
    assert_eq!(broken.seq, 3);
    assert_eq!(broken.why, BrokenWhy::Edited);
    assert_eq!(
        broken.to_string(),
        "history entry 3 was changed after it was written"
    );
}

/// The other way somebody covers their tracks: delete the row entirely.
#[test]
fn a_removed_row_leaves_a_gap_that_shows() {
    let scratch = Scratch::new("audit_gap");
    let db = scratch.open();
    shop::build(&db);
    write_some(&db, 6);

    db.transaction(|tx| {
        tx.execute("DROP TRIGGER audit_log_is_append_only_delete", [])?;
        tx.execute("DELETE FROM audit_log WHERE seq = 4", [])?;
        Ok(())
    })
    .expect("tamper");

    let broken = db
        .transaction(|tx| Repos::new(tx).audit().verify(OUTLET))
        .expect("verify")
        .expect_err("the deletion was not noticed");
    assert_eq!(broken.seq, 5);
    assert_eq!(broken.why, BrokenWhy::Gap);
}

/// Before AND after — draft T4, and audit C4's real ask. An audit that says
/// "somebody changed a price" and not "from ₹120 to ₹90" cannot settle an
/// argument.
#[test]
fn before_and_after_survive_the_round_trip() {
    let scratch = Scratch::new("audit_values");
    let db = scratch.open();
    shop::build(&db);

    db.transaction(|tx| {
        Repos::new(tx).audit().append(
            OUTLET,
            &AuditEntry::new(
                at(1),
                day(),
                Some(StaffId::new("staff_1")),
                action::PRICE_CHANGED,
                "menu_item",
            )
            .about("itm_dosa")
            .changed(
                serde_json::json!({ "unit_price": 12_000 }),
                serde_json::json!({ "unit_price": 9_000 }),
            ),
        )?;
        Ok(())
    })
    .expect("write");

    let rows = db
        .transaction(|tx| {
            Repos::new(tx).audit().list(
                OUTLET,
                &AuditFilter {
                    action: Some(action::PRICE_CHANGED.to_owned()),
                    limit: 10,
                    ..AuditFilter::default()
                },
            )
        })
        .expect("read");

    assert_eq!(rows.len(), 1);
    let before: serde_json::Value =
        serde_json::from_str(rows[0].before_json.as_deref().expect("before")).expect("json");
    let after: serde_json::Value =
        serde_json::from_str(rows[0].after_json.as_deref().expect("after")).expect("json");
    assert_eq!(before["unit_price"], 12_000);
    assert_eq!(after["unit_price"], 9_000);
}

/// **The lockout count comes from the log, and survives a restart.**
///
/// The restart is the point: an in-memory counter is the first thing anybody
/// trying PINs would discover.
#[test]
fn failed_logins_are_counted_from_the_history_and_reset_on_success() {
    let scratch = Scratch::new("audit_lockout");
    {
        let db = scratch.open();
        shop::build(&db);
        db.transaction(|tx| {
            let repos = Repos::new(tx);
            for n in 1..=3 {
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at(n),
                        day(),
                        Some(StaffId::new("staff_1")),
                        action::LOGIN_FAILED,
                        "staff",
                    ),
                )?;
            }
            Ok(())
        })
        .expect("write");
    }

    // A new process, a new connection, the same shop.
    let db = scratch.open();
    let failures = db
        .transaction(|tx| {
            Repos::new(tx).audit().failed_logins_since_success(OUTLET, "staff_1")
        })
        .expect("count");
    assert_eq!(failures, 3, "the lockout did not survive a restart");

    db.transaction(|tx| {
        Repos::new(tx).audit().append(
            OUTLET,
            &AuditEntry::new(
                at(9),
                day(),
                Some(StaffId::new("staff_1")),
                action::LOGIN_OK,
                "staff",
            ),
        )?;
        Ok(())
    })
    .expect("write");

    let failures = db
        .transaction(|tx| {
            Repos::new(tx).audit().failed_logins_since_success(OUTLET, "staff_1")
        })
        .expect("count");
    assert_eq!(failures, 0, "getting in did not clear the count");
}

/// One person's mistakes are their own. A global count would be a waiter's
/// route to locking the owner out on a Saturday night.
#[test]
fn the_lockout_is_per_person() {
    let scratch = Scratch::new("audit_lockout_each");
    let db = scratch.open();
    shop::build(&db);

    db.transaction(|tx| {
        let repos = Repos::new(tx);
        for n in 1..=6 {
            repos.audit().append(
                OUTLET,
                &AuditEntry::new(
                    at(n),
                    day(),
                    Some(StaffId::new("staff_2")),
                    action::LOGIN_FAILED,
                    "staff",
                ),
            )?;
        }
        Ok(())
    })
    .expect("write");

    let (theirs, mine) = db
        .transaction(|tx| {
            let repos = Repos::new(tx);
            Ok((
                repos.audit().failed_logins_since_success(OUTLET, "staff_2")?,
                repos.audit().failed_logins_since_success(OUTLET, "staff_1")?,
            ))
        })
        .expect("count");
    assert_eq!(theirs, 6);
    assert_eq!(mine, 0, "one person's mistakes locked another one out");
}

/// **T10.** The enum and the seeded table are the same set, both directions.
///
/// A variant with no row is a permission that can never be granted; a row with
/// no variant is a permission that nothing ever checks — which looks like
/// security and is not. Neither is visible by reading one file.
#[test]
fn the_permission_enum_and_the_seeded_table_agree() {
    let scratch = Scratch::new("audit_perms");
    let db = scratch.open();

    let seeded = db
        .transaction(|tx| Repos::new(tx).people().permission_codes())
        .expect("permissions");

    let in_the_table: std::collections::BTreeSet<String> =
        seeded.iter().map(|(code, _)| code.clone()).collect();
    let in_the_enum: std::collections::BTreeSet<String> = Permission::ALL
        .iter()
        .map(|p| p.code().to_owned())
        .collect();

    let missing_rows: Vec<&String> = in_the_enum.difference(&in_the_table).collect();
    assert!(
        missing_rows.is_empty(),
        "these permissions are in the enum with no row, so they can never be granted: {missing_rows:?}"
    );
    let unchecked: Vec<&String> = in_the_table.difference(&in_the_enum).collect();
    assert!(
        unchecked.is_empty(),
        "these permissions are rows that nothing checks: {unchecked:?}"
    );

    // And every seeded row has a description a roles screen can show.
    for (code, description) in &seeded {
        assert!(!description.is_empty(), "{code} has no description");
    }
}

/// The presets round-trip: what a shop is given on day one is what it reads
/// back, limits included.
#[test]
fn the_role_presets_round_trip_with_their_discount_limits() {
    let scratch = Scratch::new("audit_roles");
    let db = scratch.open();
    shop::build(&db);

    db.transaction(|tx| {
        let repos = Repos::new(tx);
        for preset in RolePreset::ALL {
            repos.people().save_role(OUTLET, &preset.shape(), at(0))?;
        }
        Ok(())
    })
    .expect("seed roles");

    let roles = db
        .transaction(|tx| Repos::new(tx).people().list_roles(OUTLET))
        .expect("roles");

    for preset in RolePreset::ALL {
        let stored = roles
            .iter()
            .find(|r| r.id == preset.id())
            .unwrap_or_else(|| panic!("{} was not saved", preset.name()));
        assert_eq!(*stored, preset.shape(), "{} changed in the round trip", preset.name());
    }

    // The waiter's zero is a real zero and not a missing limit — those mean
    // opposite things (`None` is no limit at all).
    let waiter = roles.iter().find(|r| r.id == "role_waiter").expect("waiter");
    assert_eq!(waiter.max_discount_bp, Some(0));
    assert_eq!(
        roles.iter().find(|r| r.id == "role_owner").expect("owner").max_discount_bp,
        None
    );
}

/// Who can still administer the shop — the query the "last administrator" rule
/// is built on.
#[test]
fn the_administrators_are_the_active_people_who_can_manage_staff() {
    let scratch = Scratch::new("audit_admins");
    let db = scratch.open();
    shop::build(&db);

    db.transaction(|tx| {
        let repos = Repos::new(tx);
        repos.people().save_role(OUTLET, &RolePreset::Owner.shape(), at(0))?;
        repos.people().save_staff(
            OUTLET,
            &mb_db::repo::people::StaffMember {
                id: StaffId::new("staff_owner"),
                name: "Sachin".to_owned(),
                code: Some("1".to_owned()),
                role_id: Some(RolePreset::Owner.id().to_owned()),
                role_name: None,
                pin_hash: None,
                status: mb_db::repo::people::StaffStatus::Active,
                permissions: PermissionSet::new(),
                max_discount_bp: None,
                max_discount: None,
            },
            at(0),
        )?;
        Ok(())
    })
    .expect("an owner");

    let admins = db
        .transaction(|tx| Repos::new(tx).people().active_administrators(OUTLET))
        .expect("admins");
    assert_eq!(admins.len(), 1);
    assert_eq!(admins[0], StaffId::new("staff_owner"));

    // Suspending them leaves the shop with nobody — which is what the caller
    // refuses. Here we only assert the query notices.
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        let mut owner = repos
            .people()
            .find_staff(OUTLET, "staff_owner")?
            .expect("the owner");
        owner.status = mb_db::repo::people::StaffStatus::Suspended;
        repos.people().save_staff(OUTLET, &owner, at(1))
    })
    .expect("suspend");

    let admins = db
        .transaction(|tx| Repos::new(tx).people().active_administrators(OUTLET))
        .expect("admins");
    assert!(admins.is_empty());
}

/// **T8 — the two columns that finally mean something.**
///
/// `orders.created_by` is whoever put the first line on the bill;
/// `orders.settled_by` is whoever was signed in when the money was taken. They
/// have been on the table since P04 and nothing had ever written two different
/// values into them. A shift change is exactly when they differ, and this is
/// read back off the disk rather than off a screen.
#[test]
fn a_bill_opened_by_one_person_and_settled_by_another_records_both() {
    use mb_core::{
        BillInput, Cart, ItemSnapshot, Money, OrderType, Payment, PaymentMode, PlaceOfSupply,
        Qty, RoundingMode, Settlement, TaxRate, TaxTreatment, compute_bill,
    };

    let scratch = Scratch::new("audit_two_people");
    let db = scratch.open();
    shop::build(&db);

    let mut cart = Cart::new();
    cart.add(
        ItemSnapshot {
            item_id: mb_core::ItemId::new("itm_dosa"),
            name: "Masala Dosa".to_owned(),
            unit_price: Money::from_paise(12_000),
            tax_rate: TaxRate::GST_5,
            tax_treatment: TaxTreatment::Exclusive,
            hsn: None,
            category_id: None,
        },
        Qty::from_whole(1).expect("one"),
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

    let opener = StaffId::new("staff_1");
    let settler = StaffId::new("staff_2");

    let draft = mb_core::DraftOrder::new(
        mb_core::OrderId::new("ord_shift_change"),
        day(),
        at(1),
        OrderType::Parcel,
        opener.clone(),
    );
    let mut draft = draft;
    draft.core.cart = cart;

    let till = mb_db::Till::new(OUTLET, "terminal_default");
    let open = mb_db::open_draft(&db, till, draft).expect("opened");
    mb_db::settle(&db, till, open, bill, settlement, at(2), settler.clone())
        .expect("settled");

    let (created_by, settled_by): (String, String) = db
        .transaction(|tx| {
            Ok(tx.query_row(
                "SELECT created_by, settled_by FROM orders WHERE id = 'ord_shift_change'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?)
        })
        .expect("read the row back");

    assert_eq!(created_by, opener.as_str());
    assert_eq!(settled_by, settler.as_str());
    assert_ne!(created_by, settled_by, "a shift change was flattened into one person");
}

/// **T13.** Removing the last person who can manage staff is refused, and the
/// refusal rolls the write back rather than leaving the shop half-changed.
///
/// This is the shape `ipc::save_staff_member` uses: write, ask the database the
/// real question — "who can administer this shop NOW?" — and abort if the
/// answer is nobody. Asking after the write rather than predicting the answer
/// is what makes it right for a role change as well as a suspension.
#[test]
fn the_last_administrator_cannot_be_removed() {
    let scratch = Scratch::new("audit_last_admin");
    let db = scratch.open();
    shop::build(&db);

    db.transaction(|tx| {
        let repos = Repos::new(tx);
        repos.people().save_role(OUTLET, &RolePreset::Owner.shape(), at(0))?;
        repos.people().save_staff(
            OUTLET,
            &mb_db::repo::people::StaffMember {
                id: StaffId::new("staff_owner"),
                name: "Sachin".to_owned(),
                code: None,
                role_id: Some(RolePreset::Owner.id().to_owned()),
                role_name: None,
                pin_hash: None,
                status: mb_db::repo::people::StaffStatus::Active,
                permissions: PermissionSet::new(),
                max_discount_bp: None,
                max_discount: None,
            },
            at(0),
        )
    })
    .expect("an owner");

    // Three ways in, one refusal: suspend them, mark them as left, or take the
    // role away.
    for attempt in ["suspend", "left", "role"] {
        let result = db.transaction(|tx| {
            let repos = Repos::new(tx);
            let mut owner = repos
                .people()
                .find_staff(OUTLET, "staff_owner")?
                .expect("the owner");
            match attempt {
                "suspend" => owner.status = mb_db::repo::people::StaffStatus::Suspended,
                "left" => owner.status = mb_db::repo::people::StaffStatus::Left,
                _ => owner.role_id = Some(RolePreset::Waiter.id().to_owned()),
            }
            if attempt == "role" {
                repos.people().save_role(OUTLET, &RolePreset::Waiter.shape(), at(1))?;
            }
            repos.people().save_staff(OUTLET, &owner, at(1))?;
            if repos.people().active_administrators(OUTLET)?.is_empty() {
                return Err(mb_db::DbError::invariant(
                    "that would leave nobody able to manage staff",
                ));
            }
            Ok(())
        });

        assert!(result.is_err(), "{attempt} was allowed");

        // And the shop is untouched: the transaction rolled back.
        let admins = db
            .transaction(|tx| Repos::new(tx).people().active_administrators(OUTLET))
            .expect("admins");
        assert_eq!(admins.len(), 1, "after {attempt}, the rollback did not happen");
    }
}
