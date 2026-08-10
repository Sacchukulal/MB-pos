//! **Setting a shop up** — P22 part 6, and the design is not the one the
//! prompt's draft asked for.
//!
//! # D102 — the checklist READS the shop; it does not remember a position
//!
//! The obvious build is a wizard: seven screens, a stored step number, Next and
//! Back. It is the wrong shape here, for two reasons that both cost real money
//! if ignored.
//!
//! **First, every one of those screens already exists.** P17 has the shop's
//! details and the printers, P13 has the menu and its CSV import with a dry
//! run, P14 has the numbered run of tables, P11 has staff and PINs, and P05
//! has the backup. A wizard that re-implements them is seven more editors to
//! keep in step with the real ones, and the copy that drifts is always the one
//! a new shopkeeper sees first.
//!
//! **Second, a stored step number is a lie waiting to happen.** It says "you
//! did the menu" while somebody deletes every item; it says "you have not done
//! tax" after they set it from the Settings screen instead. So every step here
//! is **derived from what is actually in the shop**, which makes it
//! self-healing, makes skipping implicit, and makes resuming automatic. There
//! is no progress file to get out of step because there is no progress file.
//!
//! # And it is never in front of the till
//!
//! The prompt's draft said *"an owner must be able to bill within fifteen
//! minutes of installing"*. **PERFORMANCE S5 says three minutes** — "install →
//! first bill printable". Those are not the same requirement and one long form
//! cannot satisfy both.
//!
//! So: the counter bills the moment it opens. This is a list beside the till,
//! not a gate in front of it. Fifteen minutes is how long a full set-up takes;
//! three minutes is how long it takes to take money, and a form between a
//! shopkeeper and their first customer is how a product gets uninstalled.

use serde::Serialize;
use ts_rs::TS;

use crate::state::{App, OUTLET};
use crate::words::UiResult;

/// One thing worth doing before a shop is really set up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SetupStep {
    pub id: String,
    /// "Tell us about your shop".
    pub title: String,
    /// **Why it is worth doing**, in the shop's words — not what it is.
    /// "Your name and GSTIN go on every bill you print" beats "store details".
    pub why: String,
    pub done: bool,
    /// The screen that does it. There is no seventh editor.
    pub go_to: String,
    /// **True when a shop cannot really trade without it.** Nothing here stops
    /// the counter working; this only decides what is said loudest.
    pub matters_most: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SetupView {
    /// "Four things left" — or the sentence that says it is all done.
    pub headline: String,
    pub steps: Vec<SetupStep>,
    pub left: u32,
    /// True once nothing is outstanding, so the screen can stop showing it.
    pub finished: bool,
}

/// **Look at the shop and work out what is still worth doing.**
///
/// Never fails: a set-up list that cannot draw on a shop that will not open is
/// a list that is missing exactly when it is needed.
#[must_use]
pub fn look(app: &App) -> SetupView {
    let config = app.shop_config();

    let has_shop_name = !config.store.name.trim().is_empty();
    let has_gstin = !config.store.gstin.trim().is_empty();

    let counts = app
        .with_shop(|shop| {
            shop.db
                .transaction(|tx| {
                    let one = |sql: &str| -> Result<i64, mb_db::DbError> {
                        let mut statement = tx.prepare(sql)?;
                        Ok(statement.query_row([], |row| row.get(0))?)
                    };
                    Ok(Counts {
                        items: one("SELECT COUNT(*) FROM menu_items WHERE is_active = 1")?,
                        tables: one("SELECT COUNT(*) FROM dining_tables WHERE is_active = 1")?,
                        printers: one("SELECT COUNT(*) FROM printers")?,
                        with_pin: one("SELECT COUNT(*) FROM staff WHERE pin_hash IS NOT NULL")?,
                        backups: one("SELECT COUNT(*) FROM backups")?,
                    })
                })
                .map_err(|e| crate::words::from_db(&e))
        })
        .unwrap_or_default();

    let steps = vec![
        SetupStep {
            id: "shop".to_owned(),
            title: "Tell us about your shop".to_owned(),
            why: "Your name, address and GSTIN go on every bill you print — a \
                  bill without them is not one a customer can claim."
                .to_owned(),
            done: has_shop_name && has_gstin,
            go_to: "settings".to_owned(),
            matters_most: true,
        },
        SetupStep {
            id: "menu".to_owned(),
            title: "Put your menu in".to_owned(),
            why: "Type the items, or import a spreadsheet — the import shows \
                  you what it will do before it does it."
                .to_owned(),
            done: counts.items > 0,
            go_to: "menu".to_owned(),
            matters_most: true,
        },
        SetupStep {
            id: "printer".to_owned(),
            title: "Set up your printer and print a test bill".to_owned(),
            why: "The test looks like a real bill, so you can see the size, the \
                  spacing and your own name on it before a customer does."
                .to_owned(),
            done: counts.printers > 0,
            go_to: "settings".to_owned(),
            matters_most: true,
        },
        SetupStep {
            id: "tables".to_owned(),
            title: "Add your tables".to_owned(),
            why: "Only if you have table service. You can add them in a \
                  numbered run rather than one at a time."
                .to_owned(),
            done: counts.tables > 0,
            go_to: "floor".to_owned(),
            matters_most: false,
        },
        SetupStep {
            id: "people".to_owned(),
            title: "Add your staff and set a PIN".to_owned(),
            why: "Until somebody has a PIN, anybody who walks behind the \
                  counter can open your reports and change your prices. \
                  Setting one is what switches the lock on."
                .to_owned(),
            done: counts.with_pin > 0,
            go_to: "staff".to_owned(),
            matters_most: true,
        },
        SetupStep {
            id: "backup".to_owned(),
            title: "Choose where backups go, and take one".to_owned(),
            why: "A copy on the same disk is not a backup. Pick a pen drive or \
                  another folder, and take one now so you know it works."
                .to_owned(),
            done: counts.backups > 0,
            go_to: "settings".to_owned(),
            matters_most: true,
        },
    ];

    let left = u32::try_from(steps.iter().filter(|s| !s.done).count()).unwrap_or(0);
    let finished = left == 0;
    let headline = if finished {
        "Your shop is set up. Everything below is done.".to_owned()
    } else {
        format!(
            "{} to do — and you can take money in the meantime.",
            crate::words::count(i64::from(left), "thing", "things")
        )
    };

    SetupView {
        headline,
        steps,
        left,
        finished,
    }
}

#[derive(Debug, Default)]
struct Counts {
    items: i64,
    tables: i64,
    printers: i64,
    with_pin: i64,
    backups: i64,
}

/// The health panel's row, so the list is nagged about from the other end too.
#[must_use]
pub fn health_row(view: &SetupView) -> crate::health::HealthRow {
    if view.finished {
        return crate::health::HealthRow::ok(
            "setup",
            "Set-up",
            "Your shop is fully set up.",
        );
    }
    let urgent: Vec<&str> = view
        .steps
        .iter()
        .filter(|s| !s.done && s.matters_most)
        .map(|s| s.title.as_str())
        .collect();
    if urgent.is_empty() {
        return crate::health::HealthRow::ok(
            "setup",
            "Set-up",
            "Everything that matters is set up.",
        );
    }
    crate::health::HealthRow::warn(
        "setup",
        "Set-up",
        format!(
            "{} still to do before this shop is really set up: {}. Open Billing \
             and the list is on the right.",
            crate::words::count(
                i64::try_from(urgent.len()).unwrap_or(0),
                "thing",
                "things"
            ),
            urgent.join("; ").to_lowercase(),
        ),
    )
    .go("billing")
}

pub fn look_on(app: &App) -> UiResult<SetupView> {
    // **Deliberately not guarded beyond the counter existing.** On a first run
    // the stand-in is who is standing there, and a set-up list that refuses to
    // draw until somebody has a PIN is a list nobody can use to create the PIN.
    let _ = OUTLET;
    Ok(look(app))
}

#[tauri::command]
pub fn setup_list(app: tauri::State<'_, App>) -> UiResult<SetupView> {
    look_on(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_view(done: &[&str]) -> SetupView {
        let mut view = SetupView {
            headline: String::new(),
            steps: vec![
                SetupStep {
                    id: "shop".to_owned(),
                    title: "Tell us about your shop".to_owned(),
                    why: "w".to_owned(),
                    done: done.contains(&"shop"),
                    go_to: "settings".to_owned(),
                    matters_most: true,
                },
                SetupStep {
                    id: "tables".to_owned(),
                    title: "Add your tables".to_owned(),
                    why: "w".to_owned(),
                    done: done.contains(&"tables"),
                    go_to: "floor".to_owned(),
                    matters_most: false,
                },
            ],
            left: 0,
            finished: false,
        };
        view.left = u32::try_from(view.steps.iter().filter(|s| !s.done).count()).unwrap_or(0);
        view.finished = view.left == 0;
        view
    }

    /// **The list nags about what matters and lets the rest be.**
    ///
    /// A shop with no table service must not be told forever that it has not
    /// added tables — that is the nag people learn to ignore, and then they
    /// ignore the one about backups too.
    #[test]
    fn only_what_matters_reaches_the_health_panel() {
        let no_tables = a_view(&["shop"]);
        assert!(!no_tables.finished, "there is still a step outstanding");
        assert!(
            health_row(&no_tables).is_ok(),
            "a takeaway with no tables was nagged: {}",
            health_row(&no_tables).says
        );

        let no_shop = a_view(&["tables"]);
        let row = health_row(&no_shop);
        assert!(!row.is_ok());
        assert!(row.says.contains("tell us about your shop"), "{}", row.says);
        assert_eq!(row.go_to.as_deref(), Some("billing"));
    }

    #[test]
    fn a_finished_setup_says_so_and_stops() {
        let all = a_view(&["shop", "tables"]);
        assert!(all.finished);
        assert_eq!(all.left, 0);
        assert!(health_row(&all).is_ok());
    }

    /// **The headline says the thing that matters most**: you can trade now.
    #[test]
    fn the_headline_says_billing_works_anyway() {
        let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
        let view = look(&app);
        assert!(!view.finished, "a counter with no shop has everything to do");
        assert!(
            view.headline.contains("take money in the meantime"),
            "{}",
            view.headline
        );
        // And `count` put the number in exactly once — D78, again.
        assert!(view.headline.starts_with(&view.left.to_string()));
    }

    /// **D102.** Nothing is stored, so nothing can be out of step. The list is
    /// a function of the shop, and calling it twice gives the same answer.
    #[test]
    fn the_list_is_read_and_never_remembered() {
        let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
        assert_eq!(look(&app), look(&app));
        // And there is no file behind it, which is the property that makes it
        // self-healing.
        // The shipped half, and **code lines only** — the module note explains
        // at length why there is no progress file, so scanning the comments
        // finds the word "progress" and fails on its own explanation. The same
        // trap `gate.rs`'s prose scan fell into.
        let source = include_str!("setup.rs");
        let shipped = source.split("\n#[cfg(test)]").next().unwrap_or(source);
        for line in shipped.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            let code = line.split("//").next().unwrap_or("");
            for writing in ["fs::write", "fs::create_dir", "progress"] {
                assert!(
                    !code.contains(writing),
                    "`{writing}` suggests the list has started remembering \
                     something, and a remembered position is a position that \
                     goes out of step (D102): {code}"
                );
            }
        }
    }

    #[test]
    fn every_step_says_why_rather_than_what() {
        let app = App::new(crate::config::AppConfig::default()).expect("the font loads");
        for step in &look(&app).steps {
            assert!(!step.why.is_empty(), "{}", step.id);
            assert!(step.why.ends_with('.'), "{}", step.why);
            // A reason is longer than a label. This is coarse and it is the
            // check that would have caught "Store details".
            assert!(
                step.why.split_whitespace().count() > 8,
                "{} explains nothing: {}",
                step.id,
                step.why
            );
            assert!(!step.go_to.is_empty());
        }
    }
}
