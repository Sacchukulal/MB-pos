//! **T5 — the log a shopkeeper emails us must not contain a secret.**
//!
//! `redact.rs` holds the scanner and proves it can see; this drives the counter
//! through everything that touches a secret and then reads back what actually
//! landed on disk.
//!
//! That distinction is the whole point. A unit test of the scanner shows the
//! patterns work. **This shows the app does not write them** — and those are
//! different claims, the second of which is the one audit E7 and E10 are about.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "tests: expect is the assertion"
)]

use std::path::PathBuf;
use std::sync::Arc;

use mb_license::cloud::Stub;
use mb_license::{Cloud, Licensing, MachineId};

use crate::redact;
use crate::signin_tests::Scratch;
use crate::state::App;

fn a_shop(scratch: &Scratch, name: &str) -> App {
    let path = scratch.dir().join(format!("{name}.db"));
    let db = mb_db::Db::open(&mb_db::DbConfig::new(path.clone())).expect("open");
    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    app.open_shop(db, path);
    app
}

/// Everything in the product that has a secret anywhere near it.
///
/// **If a future session adds a flow that handles a secret, it belongs in
/// here.** The scanner cannot find what was never written.
fn exercise_everything_that_touches_a_secret(app: &App, scratch: &Scratch) {
    let at = crate::flows::now();
    let today = crate::flows::today(at);

    // --- a licence: the key, and the OTP that goes with it -------------------
    let dir = scratch.dir().join("licence");
    let _ = std::fs::create_dir_all(&dir);
    let machine = MachineId::for_tests("4c4c4544-0043-4a10-8033-b8c04f4d3132");
    let stub = Arc::new(Stub::active(&machine, today, at));
    let licensing = Licensing::new(
        dir,
        machine.clone(),
        Arc::clone(&stub) as Arc<dyn Cloud>,
        "test",
    );
    app.use_licensing(licensing);

    // A refused activation, then a good one. The refused path is the one that
    // historically writes `{key}` into a warning.
    let _ = crate::licensing::activate_on(
        app,
        "MB-4KQ7-9WTX-2100".to_owned(),
        "000000".to_owned(),
    );
    let _ = crate::licensing::activate_on(
        app,
        "MB-STUB-0001".to_owned(),
        "123456".to_owned(),
    );

    // --- an emergency code, right and wrong ---------------------------------
    let _ = crate::licensing::use_emergency_code_on(app, "K7M2Q-9XR4T-BW8HN-3PZ6D".to_owned());
    let code = mb_license::emergency::mint(&machine, today.days_since_epoch(), 72);
    let _ = crate::licensing::use_emergency_code_on(app, code.to_read_out());

    // --- a PIN, set and used ------------------------------------------------
    let _ = crate::ipc::save_staff_member_on(
        app,
        crate::ipc::StaffEdit {
            id: "staff_owner".to_owned(),
            name: "Sachin".to_owned(),
            code: None,
            role_id: Some("role_owner".to_owned()),
            status: "active".to_owned(),
        },
    );
    let _ = crate::ipc::set_staff_pin_on(
        app,
        "staff_owner".to_owned(),
        Some("483920".to_owned()),
    );
    let _ = crate::ipc::login_on(app, "staff_owner".to_owned(), "483920".to_owned());
    // And a wrong one, which is the path that logs a failure.
    let _ = crate::ipc::login_on(app, "staff_owner".to_owned(), "111111".to_owned());

    // --- a customer, with a real-looking mobile number ----------------------
    let _ = crate::credit::save_customer_on(
        app,
        crate::credit::CustomerEdit {
            id: String::new(),
            name: "Ravi Kumar".to_owned(),
            phone: "9845012345".to_owned(),
            gstin: String::new(),
            address: String::new(),
            credit_limit: "5000".to_owned(),
            is_active: true,
        },
    );

    // --- and the licence screen, which reads all of it back ----------------
    let _ = crate::licensing::view_on(app);
    let _ = crate::licensing::deactivate_on(app);
}

fn everything_written() -> String {
    crate::logging::files()
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

/// **T5.** Drive it, then read what landed.
///
/// **One test, not two**, because `logging::redirect` moves a process-wide
/// logger: a second test doing the same thing in parallel would have the two of
/// them writing to each other's folder, and the failure would look like a leak.
/// The second half is the control — a deliberate leak, through the same reading
/// path, so that a green first half means something.
#[test]
fn no_secret_reaches_the_log() {
    let scratch = Scratch::new("log_secrets");
    let logs: PathBuf = scratch.dir().join("logs");
    crate::logging::redirect(&logs);

    let app = a_shop(&scratch, "secrets");
    exercise_everything_that_touches_a_secret(&app, &scratch);

    let written = everything_written();

    // **The exercise really ran.** `!is_empty()` is not enough: one stray line
    // from another thread would satisfy it, and then a green result would mean
    // "we scanned nothing and found nothing".
    assert!(
        written.contains("signed in"),
        "the sign-in path did not log, so the exercise did not reach it. What \
         was written:\n{written}"
    );
    assert!(
        written.lines().count() >= 3,
        "only {} line(s) were logged by the whole exercise:\n{written}",
        written.lines().count()
    );

    let found = redact::scan(&written);
    assert!(
        found.is_empty(),
        "a secret reached the log, which is the file audit E7 asks a shopkeeper \
         to email us:\n{}\n\nThe offending lines are in {}",
        redact::describe(&found),
        logs.display()
    );

    // --- the control -------------------------------------------------------
    //
    // The check above is only worth anything if it WOULD fail. Same scanner,
    // same reading path, one line that must never be written for real.
    let leaky = scratch.dir().join("leaky");
    crate::logging::redirect(&leaky);
    crate::log_warn!(
        "could not activate MB-4KQ7-9WTX-2100 for +91 9845012345 — this line is \
         written by a test on purpose"
    );
    let caught = redact::scan(&everything_written());
    assert!(
        caught.len() >= 2,
        "the scanner did not see a deliberately leaked key and phone number in \
         a real log file, so the assertion above is checking nothing: {}",
        redact::describe(&caught)
    );
}
