#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

mod common;

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use mb_core::Timestamp;
use mb_db::{DbError, Repos};

/// The screens are told what changed by the database itself, not by whoever wrote.
#[test]
fn a_commit_names_the_tables_it_touched_and_a_rollback_names_none() {
    let scratch = common::Scratch::new("watch");
    let db = scratch.open();
    let seen: Arc<Mutex<Vec<BTreeSet<String>>>> = Arc::default();
    let noting = Arc::clone(&seen);
    db.watch(Arc::new(move |tables| {
        noting.lock().expect("lock").push(tables.clone());
    }));

    let at = Timestamp::from_millis(1_700_000_000_000);
    db.transaction(|tx| {
        Repos::new(tx)
            .settings()
            .set(common::OUTLET, "watch.test", &true, at, None)
    })
    .expect("written");
    let told = seen.lock().expect("lock").clone();
    assert_eq!(told.len(), 1, "one commit, one word");
    assert!(told[0].contains("settings"), "{:?}", told[0]);

    // A write that is rolled back changed nothing, so nothing is said.
    let refused: Result<(), DbError> = db.transaction(|tx| {
        Repos::new(tx)
            .settings()
            .set(common::OUTLET, "watch.rolled", &true, at, None)?;
        Err(DbError::invariant("changed my mind"))
    });
    assert!(refused.is_err());
    assert_eq!(
        seen.lock().expect("lock").len(),
        1,
        "a rollback was announced"
    );

    // And a read says nothing either.
    db.read_transaction(|tx| {
        Repos::new(tx)
            .settings()
            .get::<bool>(common::OUTLET, "watch.test")
    })
    .expect("read");
    assert_eq!(seen.lock().expect("lock").len(), 1);
}
