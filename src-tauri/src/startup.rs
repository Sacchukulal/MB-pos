//! **The order of operations at start-up — the highest-risk hundred lines in
//! the product.**
//!
//! P05 already made these decisions and P08 must not re-make them. Getting the
//! order wrong does not produce a bug report; it produces a shop with no data.
//!
//! ```text
//!   1. logging up                          (main.rs, before this)
//!   2. read where the shop is              audit A5 — a file, never browser storage
//!   3. IS THERE A RESTORE REQUEST?         D27 — before anything opens
//!   4. is the database still there?        A5 — locate/ searches
//!   5. still nothing? open anyway          first run is a state, not an error
//!   6. Db::open (which migrates)
//!   7. build the print queue               (state.rs)
//!   8. show the window
//! ```
//!
//! # Step 3 is the one that matters
//!
//! **D27: a restore runs BEFORE the database is opened.** `mb_db::backup`'s
//! `restore` takes paths and not a `&Db` — proved by a `compile_fail` test —
//! and the target file is removed first so that Windows *refuses* a restore
//! aimed at an open database instead of corrupting it. P05's notes record that
//! the corruption really happened while those tests were being written.
//!
//! So the request is a plain file that `request_restore` writes, readable
//! without opening SQLite because the database may be the broken thing, and
//! this is the code that acts on it at the only moment it is safe to.

use std::path::{Path, PathBuf};

use mb_db::locate::FoundDatabase;
use mb_db::{Db, DbConfig};

use crate::words::{self, UiError};
use crate::{log_info, log_warn};

/// What start-up found, and therefore what the window opens into.
#[derive(Debug)]
pub enum Startup {
    /// A shop, open and migrated. The normal morning.
    Ready {
        db: Box<Db>,
        path: PathBuf,
        /// True when step 3 put a backup back. The shell says so once — an
        /// owner who has just restored needs to be told it worked.
        restored: bool,
    },
    /// No database and no candidate. The app opens to "create a new shop or
    /// restore a backup" — **never a blank screen, never an error dialog on
    /// top of nothing.**
    FirstRun,
    /// `locate` found databases the configuration did not mention.
    ///
    /// **We ask. We never adopt one silently.** Audit A5 is about an owner
    /// being shown a first-run wizard with their live shop three folders away;
    /// the opposite mistake — quietly opening a stale copy found on another
    /// drive — is worse, because it looks like it worked.
    FoundCandidates {
        candidates: Vec<FoundDatabase>,
        /// The path the configuration pointed at, when there was one. "Your
        /// D: drive is not plugged in" and "you have never set this up" are
        /// very different sentences.
        expected: Option<PathBuf>,
    },
    /// It is there and it will not open. The window still opens, and it says
    /// why, in words (audit F8).
    Failed { error: UiError },
}

/// Run the sequence. Every step is a log line, because this is the sequence
/// nobody can reproduce afterwards (audit E7).
#[must_use]
pub fn run(config_dir: &Path) -> Startup {
    log_info!("start-up: beginning, configuration in {}", config_dir.display());

    // ---- step 2: where does the configuration say the shop is? ------------
    // A recorded path that no longer exists comes back as `Some` on purpose —
    // P05's note: the caller needs to tell "we have never been set up" from
    // "the drive is not plugged in".
    let recorded = match mb_db::locate::read_config(config_dir) {
        Ok(found) => found,
        Err(e) => {
            log_warn!("start-up: the location file could not be read: {e}");
            None
        }
    };

    // ---- step 3: a restore request, BEFORE anything opens (D27) -----------
    let restored = match take_restore_request(config_dir, recorded.as_deref()) {
        Ok(done) => done,
        Err(error) => {
            log_warn!("start-up: the restore could not be completed: {error}");
            return Startup::Failed { error };
        }
    };

    // ---- step 4 --------------------------------------------------------
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

    // The drive letter changed, or the configuration was lost. A5: v1 showed a
    // first-run wizard in exactly this situation, with the shop three folders
    // away.
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
    Startup::FoundCandidates {
        candidates,
        expected: recorded,
    }
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

/// Adopt a database the owner has confirmed, and remember it.
pub fn adopt(config_dir: &Path, path: &Path) -> Result<Startup, UiError> {
    mb_db::locate::write_config(config_dir, path).map_err(|e| words::from_db(&e))?;
    log_info!("start-up: the owner chose {}", path.display());
    Ok(open(path, false))
}

/// Step 3, in full.
///
/// Returns whether a restore happened. The request file is cleared **after** a
/// successful restore and not before: a power cut half way through leaves the
/// request in place, so the next start tries again rather than opening a
/// half-restored file.
fn take_restore_request(config_dir: &Path, target: Option<&Path>) -> Result<bool, UiError> {
    let Some(request) = mb_db::backup::pending_restore(config_dir) else {
        return Ok(false);
    };

    let Some(target) = target else {
        // A restore with nowhere to go. Clearing it would be worse: the owner
        // asked for it, and it is the only record that they did.
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

    let report =
        mb_db::backup::restore(&request.from, target).map_err(|e| words::from_db(&e))?;

    if report.rolled_back {
        // P05's restore verifies what it put down and puts the old file back if
        // it is bad. That is a success of the machinery and a failure for the
        // owner, and they are not the same sentence.
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
        // Not fatal, but it must be loud: an uncleared request restores again
        // on the next start, over the top of a day's billing.
        log_warn!("start-up: THE RESTORE REQUEST COULD NOT BE CLEARED: {e}");
    }
    Ok(true)
}
