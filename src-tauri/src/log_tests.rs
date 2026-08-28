//! The log a shopkeeper emails us must not contain a secret.

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
fn exercise_everything_that_touches_a_secret(app: &App, scratch: &Scratch) {
    let at = crate::flows::now();
    let today = crate::flows::today(at);

    // A licence: the key, and the OTP that goes with it.
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

    // A refused activation, then a good one.
    let _ = crate::licensing::activate_on(app, "MB-4KQ7-9WTX-2100".to_owned());
    let _ = crate::licensing::activate_on(app, "MB-STUB-0001".to_owned());

    // An emergency code, right and wrong.
    let _ = crate::licensing::use_emergency_code_on(app, "K7M2Q-9XR4T-BW8HN-3PZ6D".to_owned());
    let code = mb_license::emergency::mint(&machine, today.days_since_epoch(), 72);
    let _ = crate::licensing::use_emergency_code_on(app, code.to_read_out());

    // A PIN, set and used.
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
    let _ = crate::ipc::set_staff_pin_on(app, "staff_owner".to_owned(), Some("4839".to_owned()));
    let _ = crate::ipc::login_on(app, "staff_owner".to_owned(), "4839".to_owned());
    // And a wrong one, which is the path that logs a failure.
    let _ = crate::ipc::login_on(app, "staff_owner".to_owned(), "1111".to_owned());

    // A customer, with a real-looking mobile number.
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

    // And the licence screen, which reads all of it back.
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

/// Drive it, then read what landed.
#[test]
fn no_secret_reaches_the_log() {
    let scratch = Scratch::new("log_secrets");
    let logs: PathBuf = scratch.dir().join("logs");
    crate::logging::redirect(&logs);

    let app = a_shop(&scratch, "secrets");
    exercise_everything_that_touches_a_secret(&app, &scratch);

    let written = everything_written();

    // The exercise really ran.
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

    // The control.
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
