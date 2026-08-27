#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "tests: expect is the assertion"
)]

use std::sync::Arc;

use mb_license::cloud::Stub;
use mb_license::{Cloud, Licensing, MachineId, Standing, Status};

use crate::signin_tests::Scratch;
use crate::state::App;

fn a_shop(scratch: &Scratch, name: &str) -> App {
    let path = scratch.dir().join(format!("{name}.db"));
    let db = mb_db::Db::open(&mb_db::DbConfig::new(path.clone())).expect("open");
    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    app.open_shop(db, path);
    app
}

fn machine() -> MachineId {
    MachineId::for_tests("4c4c4544-0043-4a10-8033-b8c04f4d3132")
}

/// A licence in a given state, through the real activate path.
fn licence(scratch: &Scratch, label: &str, status: Status, renews_in_days: i32) -> Licensing {
    let dir = scratch.dir().join(label);
    let _ = std::fs::create_dir_all(&dir);
    let at = crate::flows::now();
    let today = crate::flows::today(at);
    let stub = Arc::new(Stub::active(
        &machine(),
        mb_core::BusinessDay::from_days_since_epoch(today.days_since_epoch() + renews_in_days),
        at,
    ));
    let mut licensing = Licensing::new(dir, machine(), Arc::clone(&stub) as Arc<dyn Cloud>, "test");
    licensing
        .activate(
            "MB-STUB-0001",
            "123456",
            at,
            std::time::Duration::from_secs(2),
        )
        .expect("activates");
    if status != Status::Active {
        stub.set_status(status);
        licensing
            .refresh(at, std::time::Duration::from_secs(2))
            .expect("refreshes");
    }
    licensing
}

fn row<'a>(view: &'a crate::health::HealthView, id: &str) -> &'a crate::health::HealthRow {
    view.rows
        .iter()
        .find(|r| r.id == id)
        .unwrap_or_else(|| panic!("there is no {id} row: {:?}", view.rows))
}

// Break each thing in turn.

/// A healthy counter says so, and says nothing else.
#[test]
fn a_counter_with_nothing_wrong_says_nothing_needs_you() {
    let scratch = Scratch::new("health_ok");
    let app = a_shop(&scratch, "ok");
    app.use_licensing(licence(&scratch, "ok", Status::Active, 30));
    // A release build is the healthy case; this test binary is not one, so the version row is
    // asserted separately below.
    let view = crate::health::look(&app);

    assert!(row(&view, "licence").is_ok(), "{:?}", row(&view, "licence"));
    assert!(row(&view, "printers").is_ok());
    assert!(row(&view, "network").is_ok());
    assert!(row(&view, "disk").is_ok());
    assert!(row(&view, "log").is_ok());
}

#[test]
fn breaking_the_licence_shows_in_health() {
    let scratch = Scratch::new("health_licence");
    let app = a_shop(&scratch, "licence");

    app.use_licensing(licence(&scratch, "good", Status::Active, 30));
    assert!(
        crate::health::look(&app)
            .rows
            .iter()
            .any(|r| r.id == "licence" && r.is_ok())
    );

    app.use_licensing(licence(&scratch, "suspended", Status::Suspended, 365));
    let view = crate::health::look(&app);
    let licence_row = row(&view, "licence");
    assert!(!licence_row.is_ok());
    assert_eq!(licence_row.go_to.as_deref(), Some("account"));
    let today = crate::flows::today(crate::flows::now());
    assert_eq!(
        Some(licence_row.says.clone()),
        crate::words::licence_banner(&app.entitlement(), today)
    );
    // And it still says what works, because every licensing sentence does.
    assert!(licence_row.says.to_lowercase().contains("bill"));
}

/// The version row, and ANDROID-G4.
#[test]
fn a_development_build_shows_in_health() {
    let scratch = Scratch::new("health_version");
    let app = a_shop(&scratch, "version");
    let view = crate::health::look(&app);
    let version = row(&view, "update");
    assert!(
        !version.is_ok(),
        "a dev build reported itself as up to date"
    );
    assert!(
        version.says.contains("development build"),
        "{}",
        version.says
    );
}

#[test]
fn a_dismissed_update_still_shows_in_health() {
    let scratch = Scratch::new("health_update");
    let app = a_shop(&scratch, "update");
    app.set_updates(crate::updates::UpdateState {
        running: "1.4.4".to_owned(),
        available: Some("1.5.0".to_owned()),
        dismissed_on: Some(crate::flows::today(crate::flows::now()).to_string()),
        is_dev_build: false,
        ..crate::updates::UpdateState::default()
    });
    let view = crate::health::look(&app);
    let update = row(&view, "update");
    assert!(
        !update.is_ok(),
        "a dismissed update vanished — that is audit I1"
    );
    assert!(update.says.contains("1.5.0"));
}

/// The headline counts the faults, and it counts each of them once.
#[test]
fn the_headline_counts_what_is_wrong() {
    let scratch = Scratch::new("health_headline");
    let app = a_shop(&scratch, "headline");
    app.use_licensing(licence(&scratch, "revoked", Status::Revoked, 365));

    let view = crate::health::look(&app);
    let broken = view.rows.iter().filter(|r| !r.is_ok()).count();
    assert!(broken >= 2, "the licence and the dev build are both faults");
    assert!(
        view.headline.starts_with(&broken.to_string()),
        "the headline does not match the rows: {} vs {broken}",
        view.headline
    );
    assert!(view.headline.ends_with("looking at."), "{}", view.headline);
    assert_eq!(view.tone, "danger", "a revoked licence is not a warning");
}

/// Every row, whatever its state, ends in a full stop and names itself.
#[test]
fn every_row_says_something() {
    let scratch = Scratch::new("health_rows");
    let app = a_shop(&scratch, "rows");
    for row in &crate::health::look(&app).rows {
        assert!(!row.name.is_empty(), "{row:?}");
        assert!(!row.says.is_empty(), "{row:?}");
        assert!(row.says.ends_with('.'), "{}", row.says);
        assert!(
            ["ok", "warn", "danger"].contains(&row.tone.as_str()),
            "{row:?}"
        );
    }
}

// The bundle.

/// What the screen promised is what the zip holds, and the whole zip passes the secret scanner.
#[test]
fn the_bundle_holds_what_the_manifest_promised_and_no_secrets() {
    let scratch = Scratch::new("bundle");
    let logs = scratch.dir().join("logs");
    crate::logging::redirect(&logs);
    let app = a_shop(&scratch, "bundle");
    app.use_licensing(licence(&scratch, "bundle", Status::Active, 30));

    // Something to put in it.
    crate::log_info!("a line so the bundle has a log to carry");

    let plan = crate::diagnostics::plan_on(&app).expect("planned");
    assert!(!plan.items.is_empty());
    assert!(plan.excludes.contains("licence key"));
    assert!(!plan.folder.is_empty());

    let written = crate::diagnostics::write_on(&app).expect("written");
    let bytes = std::fs::read(&written).expect("the zip is on disk");
    assert!(bytes.len() > 100, "the zip is empty");

    // Read it back with a reader that is not ours.
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("a real zip");
    let names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_owned()))
        .collect();

    // Everything the screen listed is in it.
    for item in &plan.items {
        let promised = item.name.split_whitespace().next().unwrap_or(&item.name);
        let stem = promised.trim_end_matches('\\');
        assert!(
            names
                .iter()
                .any(|n| n == stem || n.starts_with(&format!("{stem}/"))),
            "the screen promised {promised} and the zip has {names:?}"
        );
    }

    // And nothing in it is a secret.
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).expect("an entry");
        let name = file.name().to_owned();
        let mut text = String::new();
        use std::io::Read as _;
        if file.read_to_string(&mut text).is_err() {
            continue;
        }
        let found = crate::redact::scan(&text);
        assert!(
            found.is_empty(),
            "{name} in the bundle contains something that looks like a secret: {}",
            crate::redact::describe(&found)
        );
    }

    // The licence KEY specifically, because that is the one an owner would email us without
    // thinking about it.
    let mut about = String::new();
    {
        use std::io::Read as _;
        zip.by_name("about.txt")
            .expect("about.txt")
            .read_to_string(&mut about)
            .expect("readable");
    }
    assert!(
        !about.contains("MB-STUB-0001"),
        "the licence key is in the bundle:\n{about}"
    );
    assert!(
        about.contains("Free trial"),
        "the plan is missing:\n{about}"
    );

    let _ = std::fs::remove_file(&written);
}

/// The bundle draws on a counter with no shop — which is the state somebody is in when the
/// shop's data is the thing that will not open.
#[test]
fn a_bundle_can_be_made_when_the_shop_will_not_open() {
    let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
    let plan = crate::diagnostics::plan_on(&app).expect("planned");
    assert!(plan.items.iter().any(|i| i.name == "database.txt"));
    let view = crate::health::look(&app);
    assert_eq!(
        view.rows
            .iter()
            .find(|r| r.id == "licence")
            .map(|r| r.tone.clone()),
        Some("warn".to_owned())
    );
    assert!(!matches!(app.entitlement().standing, Standing::Fine));
}
