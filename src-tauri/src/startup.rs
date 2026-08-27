//! The order of operations at start-up — the highest-risk hundred lines in the product.
//!
//! ```text
//!   1. logging up                          (main.rs, before this)
//!   2. read where the shop is              a file, never browser storage
//!   3. IS THERE A RESTORE REQUEST?         before anything opens
//!   4. is the database still there?        locate/ searches
//!   5. still nothing? open anyway          first run is a state, not an error
//!   6. Db::open (which migrates)
//!   7. build the print queue               (state.rs)
//!   8. show the window
//! ```

use std::path::{Path, PathBuf};

use mb_db::locate::FoundDatabase;
use mb_db::{Db, DbConfig};

use crate::words::{self, UiError};
use crate::{log_info, log_warn};

/// What start-up found, and therefore what the window opens into.
#[derive(Debug)]
pub enum Startup {
    /// A shop, open and migrated.
    Ready {
        db: Box<Db>,
        path: PathBuf,
        /// True when step 3 put a backup back.
        restored: bool,
    },
    /// No database and no candidate.
    FirstRun,
    /// `locate` found databases the configuration did not mention.
    FoundCandidates { candidates: Vec<FoundDatabase> },
    /// It is there and it will not open.
    Failed { error: UiError },
}

/// Run the sequence. Every step is a log line, because this is the sequence nobody can
/// reproduce afterwards.
#[must_use]
pub fn run(config_dir: &Path) -> Startup {
    log_info!(
        "start-up: beginning, configuration in {}",
        config_dir.display()
    );

    // Where does the configuration say the shop is?
    let recorded = match mb_db::locate::read_config(config_dir) {
        Ok(found) => found,
        Err(e) => {
            log_warn!("start-up: the location file could not be read: {e}");
            None
        }
    };

    // A restore request, BEFORE anything opens.
    let restored = match take_restore_request(config_dir, recorded.as_deref()) {
        Ok(done) => done,
        Err(error) => {
            log_warn!("start-up: the restore could not be completed: {error}");
            return Startup::Failed { error };
        }
    };

    // Step 4.
    if let Some(path) = recorded.as_ref() {
        if path.exists() {
            log_info!("start-up: opening the shop at {}", path.display());
            return open(path, restored);
        }
        log_warn!(
            "start-up: the recorded data file is not there any more: {}",
            path.display()
        );
    } else {
        log_info!("start-up: no data file has been recorded yet");
    }

    // The drive letter changed, or the configuration was lost.
    let extra: Vec<PathBuf> = recorded.iter().cloned().collect();
    let candidates = mb_db::locate::search_usual_places(&extra);
    if candidates.is_empty() {
        log_info!("start-up: nothing found — opening to first run");
        return Startup::FirstRun;
    }
    log_info!(
        "start-up: found {} possible data file(s) — asking rather than adopting",
        candidates.len()
    );
    Startup::FoundCandidates { candidates }
}

/// Open and migrate. Step 6.
#[must_use]
pub fn open(path: &Path, restored: bool) -> Startup {
    match Db::open(&DbConfig::new(path)) {
        Ok(db) => {
            log_info!("start-up: the shop is open and the schema is up to date");
            Startup::Ready {
                db: Box::new(db),
                path: path.to_path_buf(),
                restored,
            }
        }
        Err(e) => {
            log_warn!("start-up: the shop would not open: {e}");
            Startup::Failed {
                error: words::from_db(&e),
            }
        }
    }
}

pub fn adopt(config_dir: &Path, path: &Path) -> Result<Startup, UiError> {
    mb_db::locate::write_config(config_dir, path).map_err(|e| words::from_db(&e))?;
    log_info!("start-up: the owner chose {}", path.display());
    Ok(open(path, false))
}

/// Step 3, in full.
fn take_restore_request(config_dir: &Path, target: Option<&Path>) -> Result<bool, UiError> {
    let Some(request) = mb_db::backup::pending_restore(config_dir) else {
        return Ok(false);
    };

    let Some(target) = target else {
        // A restore with nowhere to go.
        log_warn!(
            "start-up: a restore from {} was requested, but no data file location \
             is recorded, so there is nothing to restore ONTO",
            request.from.display()
        );
        return Err(UiError::new(
            "restore.no_target",
            "A backup was set to be restored, but Magic Bill does not know where \
             this shop's data file should go. Choose or create the shop's data \
             file first, then restore again.",
        ));
    };

    log_info!(
        "start-up: restoring {} onto {} — BEFORE the database is opened (D27)",
        request.from.display(),
        target.display()
    );

    let report = mb_db::backup::restore(&request.from, target).map_err(|e| words::from_db(&e))?;

    if report.rolled_back {
        let why = report.failure.clone().unwrap_or_default();
        log_warn!("start-up: the restore was rolled back: {why}");
        let _ = mb_db::backup::clear_pending_restore(config_dir);
        return Err(UiError::new(
            "restore.rolled_back",
            "That backup could not be restored, so your existing data has been put \
             back exactly as it was. Nothing has been lost. Try a different backup.",
        )
        .with_detail(why));
    }

    log_info!(
        "start-up: restore finished — schema {} migrated to {}, safety copy at {}",
        report.restored_schema_version,
        report.migrated_to,
        report
            .safety_copy
            .as_ref()
            .map_or_else(|| "(none)".to_owned(), |p| p.display().to_string())
    );

    if let Err(e) = mb_db::backup::clear_pending_restore(config_dir) {
        // Not fatal, but it must be loud: an uncleared request restores again on the next
        // start, over the top of a day's billing.
        log_warn!("start-up: THE RESTORE REQUEST COULD NOT BE CLEARED: {e}");
    }
    Ok(true)
}
