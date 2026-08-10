//! **`licence.json`, and the orchestration that reads it.**
//!
//! # D85 — the licence lives beside the config, never in the shop's database
//!
//! Exactly D79's reasoning, and this time it is the whole feature. P05 copies
//! the shop's database to a pen drive **on purpose**, and D27 restores it onto
//! other machines. A licence inside that database is a licence that rides a
//! backup onto a second PC and activates it — one paid shop, four tills, no
//! detection.
//!
//! v1 did not have that hole, and it did not have it by accident: it kept the
//! key in the browser's local storage, which is audit **A5**, a data-loss
//! finding. It was accidentally right for a terrible reason. This is
//! deliberately right for the stated one.
//!
//! So:
//!
//! | what | where | why |
//! |---|---|---|
//! | the signed snapshot, the machine id, the clock watch, used codes, the transfer history, a queued release | `%APPDATA%\MagicBill\licence.json` | it is this **machine's**, and a backup must not carry it |
//! | who activated, deactivated, transferred, or used an emergency code | the audit trail, in the database | it is the **shop's** history, and it belongs in the hash chain (D43) |
//!
//! **This session adds no table.** The audit trail already is the table a
//! second one would have been, it is already append-only by trigger and
//! hash-chained, and a second history of the same events is a second answer.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mb_core::{BusinessDay, Timestamp};
use serde::{Deserialize, Serialize};

use crate::clock::{ClockSays, Watch};
use crate::cloud::{Ask, Cloud};
use crate::deadline;
use crate::emergency;
use crate::entitlement::{Entitlement, decide};
use crate::error::LicenceError;
use crate::machine::MachineId;
use crate::snapshot::{self, SignedSnapshot};
use crate::status::Standing;

const FILE: &str = "licence.json";

/// **One transfer per 30 days.** POS-A4's own suggested number. It is a
/// cooldown rather than a hard limit because the thing being prevented is a
/// licence being walked around a chain of shops, and the thing being permitted
/// is a PC dying — which happens to a shop about once every three years, and to
/// a fleet-hopper about once a night.
pub const TRANSFER_COOLDOWN_DAYS: i32 = 30;

/// A deactivate that could not reach the server.
///
/// **BACKEND-C5 in one struct.** The local side is done — this counter has
/// stopped using the licence — and the server still holds the binding. The
/// screen says so in those words, and this row is retried on every refresh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRelease {
    pub key: String,
    pub machine: String,
    pub since: Timestamp,
}

/// Everything on disk beside the config.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LicenceFile {
    pub snapshot: Option<SignedSnapshot>,
    pub watch: Watch,
    /// Fingerprints, never codes — see [`emergency::Code::fingerprint`].
    pub used_codes: Vec<String>,
    /// While this is in the future, the shop is unlocked whatever else says.
    pub emergency_until: Option<Timestamp>,
    pub pending_release: Option<PendingRelease>,
    pub last_transfer_on: Option<BusinessDay>,
    /// POS-C10's local half.
    pub emergency_tries: u32,
    pub tries_locked_until: Option<Timestamp>,
}

impl LicenceFile {
    #[must_use]
    pub fn path(dir: &Path) -> PathBuf {
        dir.join(FILE)
    }

    /// Read it, or start empty.
    ///
    /// **A corrupt licence file is never fatal**, for the same reason a corrupt
    /// `app-config.json` is not: it costs the shop its plan features until the
    /// next successful check, and it must not cost the shop its counter. The
    /// broken file is kept beside the new one, because it is evidence.
    #[must_use]
    pub fn load(dir: &Path) -> LicenceFile {
        let path = LicenceFile::path(dir);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return LicenceFile::default();
        };
        match serde_json::from_str(&text) {
            Ok(file) => file,
            Err(_) => {
                let _ = std::fs::copy(&path, path.with_extension("broken.json"));
                LicenceFile::default()
            }
        }
    }

    /// Write it, atomically — through a temporary file and a rename, so a power
    /// cut mid-save cannot leave half a JSON object where the licence was.
    ///
    /// # Errors
    ///
    /// [`LicenceError::File`] when the folder will not take it.
    pub fn save(&self, dir: &Path) -> Result<(), LicenceError> {
        let path = LicenceFile::path(dir);
        std::fs::create_dir_all(dir).map_err(|e| LicenceError::File {
            doing: "written",
            why: e.to_string(),
        })?;
        let text = serde_json::to_string_pretty(self).map_err(|e| LicenceError::File {
            doing: "written",
            why: e.to_string(),
        })?;
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, text).map_err(|e| LicenceError::File {
            doing: "written",
            why: e.to_string(),
        })?;
        std::fs::rename(&temporary, &path).map_err(|e| LicenceError::File {
            doing: "written",
            why: e.to_string(),
        })
    }
}

/// **The counter's licensing, all of it.**
///
/// Owns the file, the machine id and the cloud. Everything `src-tauri` does
/// goes through here, and every cloud call inside it is wrapped in
/// [`deadline::within`] — `no_cloud_call_is_unwrapped` reads this file and
/// proves it.
pub struct Licensing {
    dir: PathBuf,
    machine: MachineId,
    cloud: Arc<dyn Cloud>,
    version: String,
    file: LicenceFile,
}

/// By hand, because `Arc<dyn Cloud>` is not `Debug` and never will be — the
/// same treatment `magic-bill`'s `Network` gets. **And the snapshot is not in
/// here**: a licence key in a log line is a licence key in a support email.
impl std::fmt::Debug for Licensing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Licensing")
            .field("machine", &self.machine.short())
            .field("activated", &self.file.snapshot.is_some())
            .field("version", &self.version)
            .finish()
    }
}

impl Licensing {
    #[must_use]
    pub fn new(dir: PathBuf, machine: MachineId, cloud: Arc<dyn Cloud>, version: &str) -> Licensing {
        let file = LicenceFile::load(&dir);
        Licensing {
            dir,
            machine,
            cloud,
            version: version.to_owned(),
            file,
        }
    }

    #[must_use]
    pub const fn machine(&self) -> &MachineId {
        &self.machine
    }

    #[must_use]
    pub const fn file(&self) -> &LicenceFile {
        &self.file
    }

    fn ask(&self, key: &str) -> Ask {
        Ask {
            key: key.to_owned(),
            machine: self.machine.clone(),
            counter_version: self.version.clone(),
        }
    }

    /// The key the stored snapshot carries, if there is one.
    #[must_use]
    pub fn key(&self) -> Option<String> {
        self.stored().map(|s| s.licence.key)
    }

    fn stored(&self) -> Option<snapshot::Snapshot> {
        let signed = self.file.snapshot.as_ref()?;
        snapshot::verify(signed, &snapshot::trusted_keys()).ok()
    }

    /// **What the shop is entitled to, right now.**
    ///
    /// Reads the cache and decides. **Touches no network**, takes no lock the
    /// billing path holds, and cannot fail — every bad state resolves to an
    /// entitlement that still bills. This is what budget L1 measures.
    #[must_use]
    pub fn entitlement(&self, now: Timestamp, today: BusinessDay) -> Entitlement {
        // The emergency unlock beats everything, including a snapshot that has
        // gone stale and including a machine that is not the bound one — it
        // exists precisely for the case where the PC has changed and there is
        // no internet to say so.
        if let Some(until) = self.file.emergency_until
            && now.millis() < until.millis()
        {
            return match self.stored() {
                Some(snap) => Entitlement::from_licence(
                    &snap.licence,
                    Standing::Emergency { until },
                    self.file.watch.last_online,
                    until,
                ),
                None => {
                    let mut open = Entitlement::unactivated(now);
                    open.standing = Standing::Emergency { until };
                    open.good_until = until;
                    open
                }
            };
        }

        let Some(snap) = self.stored() else {
            // No licence, or one this build cannot verify (T13). A first run
            // and a tampered file land in the same place, and that place bills.
            return Entitlement::unactivated(now);
        };

        let good_until = snap.good_until(&self.file.watch);

        // Is this still the machine the licence is for? BACKEND-C4's half that
        // the counter can enforce on its own, with no network.
        if let Some(bound) = &snap.licence.bound_to
            && bound != &self.machine
        {
            return Entitlement::from_licence(
                &snap.licence,
                Standing::BoundElsewhere,
                self.file.watch.last_online,
                good_until,
            );
        }

        // Past both of D89's expiries: we have not been able to ask for too
        // long. **Not "expired"** — we do not know that, and saying it would be
        // the same class of lie as v1's Suspend button.
        if !snap.is_usable(now, &self.file.watch) {
            return Entitlement::from_licence(
                &snap.licence,
                Standing::NeedsChecking,
                self.file.watch.last_online,
                good_until,
            );
        }

        let standing = decide(&snap.licence, snap.global_grace_days, today);
        Entitlement::from_licence(&snap.licence, standing, self.file.watch.last_online, good_until)
    }

    /// Has the clock gone backwards? D90.
    #[must_use]
    pub fn clock_says(&self, now: Timestamp) -> ClockSays {
        self.file.watch.check(now)
    }

    /// Note that time has passed. Called on the same timer as the refresh, so
    /// the high-water mark keeps up with a counter that is never online.
    ///
    /// # Errors
    ///
    /// [`LicenceError::File`].
    pub fn tick(&mut self, now: Timestamp) -> Result<(), LicenceError> {
        let before = self.file.watch;
        self.file.watch.saw(now);
        if self.file.watch != before {
            self.file.save(&self.dir)?;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Everything below here talks to the cloud, and every one of them goes
    // through `deadline::within`. D92.
    // -----------------------------------------------------------------------

    /// The routine check, plus the retry of any queued release.
    ///
    /// # Errors
    ///
    /// [`LicenceError`] — and **the caller carries on with the cached
    /// entitlement whatever this returns.**
    pub fn refresh(&mut self, now: Timestamp, limit: std::time::Duration) -> Result<(), LicenceError> {
        self.retry_pending_release(limit);

        let Some(key) = self.key() else {
            return Ok(());
        };
        let ask = self.ask(&key);
        let cloud = Arc::clone(&self.cloud);
        let answer = deadline::within(limit, move || cloud.refresh(&ask))??;
        self.accept(answer, now)
    }

    /// First activation. BACKEND-C6: a key and a proof, never a key alone.
    ///
    /// # Errors
    ///
    /// [`LicenceError`].
    pub fn activate(
        &mut self,
        key: &str,
        proof: &str,
        now: Timestamp,
        limit: std::time::Duration,
    ) -> Result<(), LicenceError> {
        let ask = self.ask(key);
        let proof = proof.to_owned();
        let cloud = Arc::clone(&self.cloud);
        let answer = deadline::within(limit, move || cloud.activate(&ask, &proof))??;
        self.accept(answer, now)
    }

    /// Requirement 4.
    ///
    /// # Errors
    ///
    /// [`LicenceError`].
    pub fn start_trial(
        &mut self,
        contact: &str,
        now: Timestamp,
        limit: std::time::Duration,
    ) -> Result<(), LicenceError> {
        let ask = self.ask("");
        let contact = contact.to_owned();
        let cloud = Arc::clone(&self.cloud);
        let answer = deadline::within(limit, move || cloud.start_trial(&ask, &contact))??;
        self.accept(answer, now)
    }

    /// **Deactivate — and BACKEND-C5 is the whole of what this returns.**
    ///
    /// The local side always happens: this counter stops using the licence. The
    /// server side is attempted, and when it cannot be reached the release is
    /// QUEUED and `Ok(false)` comes back so the screen can say *"the licence is
    /// still held"* rather than "done".
    ///
    /// # Errors
    ///
    /// [`LicenceError::File`] only. A cloud that refuses is not an error here —
    /// it is the `false`.
    pub fn deactivate(
        &mut self,
        now: Timestamp,
        limit: std::time::Duration,
    ) -> Result<bool, LicenceError> {
        let key = self.key().unwrap_or_default();
        let ask = self.ask(&key);
        let cloud = Arc::clone(&self.cloud);
        let released = matches!(
            deadline::within(limit, move || cloud.release(&ask)),
            Ok(Ok(()))
        );

        self.file.snapshot = None;
        self.file.emergency_until = None;
        self.file.pending_release = if released {
            None
        } else {
            Some(PendingRelease {
                key,
                machine: self.machine.value().to_owned(),
                since: now,
            })
        };
        self.file.save(&self.dir)?;
        Ok(released)
    }

    /// Move a licence onto this machine. POS-A4.
    ///
    /// # Errors
    ///
    /// [`LicenceError::TooSoon`] inside the cooldown, or whatever the cloud
    /// said.
    pub fn transfer(
        &mut self,
        key: &str,
        proof: &str,
        now: Timestamp,
        today: BusinessDay,
        limit: std::time::Duration,
    ) -> Result<(), LicenceError> {
        if let Some(last) = self.file.last_transfer_on {
            let since = last.days_until(today);
            if since < TRANSFER_COOLDOWN_DAYS {
                let left = TRANSFER_COOLDOWN_DAYS.saturating_sub(since);
                return Err(LicenceError::TooSoon {
                    days_left: u16::try_from(left).unwrap_or(u16::MAX),
                });
            }
        }
        let ask = self.ask(key);
        let proof = proof.to_owned();
        let cloud = Arc::clone(&self.cloud);
        let answer = deadline::within(limit, move || cloud.transfer(&ask, &proof))??;
        self.accept(answer, now)?;
        self.file.last_transfer_on = Some(today);
        self.file.save(&self.dir)
    }

    /// The code support read out. POS-A4's offline half.
    ///
    /// # Errors
    ///
    /// [`LicenceError::Emergency`] — wrong, used, expired, or too many tries.
    pub fn use_emergency_code(
        &mut self,
        typed: &str,
        now: Timestamp,
    ) -> Result<Timestamp, LicenceError> {
        if let Some(until) = self.file.tries_locked_until
            && now.millis() < until.millis()
        {
            let wait = std::time::Duration::from_millis(
                u64::try_from(until.millis().saturating_sub(now.millis())).unwrap_or(0),
            );
            return Err(emergency::EmergencyError::TooManyTries { wait }.into());
        }

        match emergency::redeem(typed, &self.machine, now, &self.file.used_codes) {
            Ok((code, until)) => {
                self.file.used_codes.push(code.fingerprint());
                self.file.emergency_until = Some(until);
                self.file.emergency_tries = 0;
                self.file.tries_locked_until = None;
                self.file.save(&self.dir)?;
                Ok(until)
            }
            Err(problem) => {
                self.file.emergency_tries = self.file.emergency_tries.saturating_add(1);
                if let Some(wait) = emergency::wait_after(self.file.emergency_tries) {
                    let millis = i64::try_from(wait.as_millis()).unwrap_or(0);
                    self.file.tries_locked_until =
                        Some(Timestamp::from_millis(now.millis().saturating_add(millis)));
                }
                let _ = self.file.save(&self.dir);
                Err(problem.into())
            }
        }
    }

    /// Store what the cloud said, and move the mark.
    fn accept(&mut self, signed: SignedSnapshot, now: Timestamp) -> Result<(), LicenceError> {
        // **Verified before it is stored.** A snapshot that will not verify is
        // not written, so a broken server cannot replace a good cached answer
        // with a useless one.
        snapshot::verify(&signed, &snapshot::trusted_keys())?;
        self.file.snapshot = Some(signed);
        self.file.watch.reached_the_cloud(now);
        self.file.save(&self.dir)
    }

    /// Retry a queued deactivate. Best effort, and never an error the caller
    /// has to handle — the sentence on screen already says it is outstanding.
    fn retry_pending_release(&mut self, limit: std::time::Duration) {
        let Some(pending) = self.file.pending_release.clone() else {
            return;
        };
        let ask = self.ask(&pending.key);
        let cloud = Arc::clone(&self.cloud);
        if let Ok(Ok(())) = deadline::within(limit, move || cloud.release(&ask)) {
            self.file.pending_release = None;
            let _ = self.file.save(&self.dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::{Behaviour, Stub};
    use crate::gate::Feature;
    use crate::status::Status;
    use std::time::Duration;

    const DAY: i64 = 86_400_000;
    const TODAY_DAYS: i32 = 20_000;

    fn now_on(day: i32) -> Timestamp {
        Timestamp::from_millis(i64::from(day) * DAY + 10 * 3_600_000)
    }
    fn day(d: i32) -> BusinessDay {
        BusinessDay::from_days_since_epoch(d)
    }

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mb-licence-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    fn a_machine() -> MachineId {
        MachineId::for_tests("4c4c4544-0043-4a10-8033-b8c04f4d3132")
    }

    fn setup(label: &str) -> (PathBuf, Arc<Stub>, Licensing) {
        let dir = scratch(label);
        let stub = Arc::new(Stub::active(
            &a_machine(),
            day(TODAY_DAYS + 30),
            now_on(TODAY_DAYS),
        ));
        let licensing = Licensing::new(
            dir.clone(),
            a_machine(),
            Arc::clone(&stub) as Arc<dyn Cloud>,
            "0.1.0",
        );
        (dir, stub, licensing)
    }

    fn quick() -> Duration {
        Duration::from_millis(500)
    }

    #[test]
    fn a_first_run_is_unactivated_and_still_has_limits() {
        let (dir, _stub, licensing) = setup("first-run");
        let entitlement = licensing.entitlement(now_on(TODAY_DAYS), day(TODAY_DAYS));
        assert_eq!(entitlement.standing, Standing::NeverActivated);
        assert!(entitlement.may(Feature::Reports).is_err());
        assert_eq!(entitlement.last_checked, Timestamp::EPOCH, "it has spoken to nothing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **"Last checked" means the last time we reached the cloud**, not the
    /// last time we made a decision from the cache. The screen's label is the
    /// one an owner reads when the internet is down, and it has to be true.
    #[test]
    fn last_checked_is_the_last_time_the_cloud_answered() {
        let (dir, stub, mut licensing) = setup("last-checked");
        licensing
            .activate("MB-STUB-0001", "123456", now_on(TODAY_DAYS), quick())
            .expect("activates");
        assert_eq!(
            licensing
                .entitlement(now_on(TODAY_DAYS), day(TODAY_DAYS))
                .last_checked,
            now_on(TODAY_DAYS)
        );

        // Five days offline. Decisions keep being made; none of them is a check.
        stub.behave(Behaviour::Unreachable);
        licensing.tick(now_on(TODAY_DAYS + 5)).expect("ticks");
        assert_eq!(
            licensing
                .entitlement(now_on(TODAY_DAYS + 5), day(TODAY_DAYS + 5))
                .last_checked,
            now_on(TODAY_DAYS),
            "an offline decision was reported as a check"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn activating_stores_a_verified_snapshot_and_entitles_the_shop() {
        let (dir, _stub, mut licensing) = setup("activate");
        licensing
            .activate("MB-STUB-0001", "123456", now_on(TODAY_DAYS), quick())
            .expect("activates");
        let entitlement = licensing.entitlement(now_on(TODAY_DAYS), day(TODAY_DAYS));
        assert_eq!(entitlement.standing, Standing::Fine);
        assert!(entitlement.may(Feature::Reports).is_ok());
        // And it survives a restart, which is the whole point of the file.
        let again = Licensing::new(
            dir.clone(),
            a_machine(),
            Arc::new(Stub::active(&a_machine(), day(TODAY_DAYS + 30), now_on(TODAY_DAYS))),
            "0.1.0",
        );
        assert_eq!(
            again.entitlement(now_on(TODAY_DAYS), day(TODAY_DAYS)).standing,
            Standing::Fine
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **BACKEND-C6, through the whole stack.**
    #[test]
    fn a_key_without_the_owners_code_does_not_activate() {
        let (dir, _stub, mut licensing) = setup("no-proof");
        assert!(
            licensing
                .activate("MB-STUB-0001", "000000", now_on(TODAY_DAYS), quick())
                .is_err()
        );
        assert_eq!(
            licensing
                .entitlement(now_on(TODAY_DAYS), day(TODAY_DAYS))
                .standing,
            Standing::NeverActivated
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **T4. Deactivate releases the binding.**
    #[test]
    fn deactivate_releases_the_binding_server_side() {
        let (dir, stub, mut licensing) = setup("deactivate");
        licensing
            .activate("MB-STUB-0001", "123456", now_on(TODAY_DAYS), quick())
            .expect("activates");

        let released = licensing
            .deactivate(now_on(TODAY_DAYS), quick())
            .expect("deactivates");
        assert!(released, "the server was not told");
        assert_eq!(stub.released(), vec![a_machine().value().to_owned()]);
        assert!(licensing.file().pending_release.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **T4's other half, and it is BACKEND-C5's exact sentence.** Offline, the
    /// release is queued and the caller is told the licence is STILL HELD.
    #[test]
    fn an_offline_deactivate_queues_and_says_the_licence_is_still_held() {
        let (dir, stub, mut licensing) = setup("deactivate-offline");
        licensing
            .activate("MB-STUB-0001", "123456", now_on(TODAY_DAYS), quick())
            .expect("activates");

        stub.behave(Behaviour::Unreachable);
        let released = licensing
            .deactivate(now_on(TODAY_DAYS), quick())
            .expect("deactivates locally");
        assert!(!released, "it claimed to have released while offline");
        assert!(licensing.file().pending_release.is_some());
        assert!(stub.released().is_empty());

        // And the queued release goes out on the next refresh.
        stub.behave(Behaviour::Normal);
        let _ = licensing.refresh(now_on(TODAY_DAYS + 1), quick());
        assert!(licensing.file().pending_release.is_none());
        assert_eq!(stub.released(), vec![a_machine().value().to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **T5. The cooldown, and the days left are in the error.**
    #[test]
    fn a_transfer_respects_the_cooldown() {
        let (dir, _stub, mut licensing) = setup("cooldown");
        licensing
            .transfer(
                "MB-STUB-0001",
                "123456",
                now_on(TODAY_DAYS),
                day(TODAY_DAYS),
                quick(),
            )
            .expect("first transfer");

        match licensing.transfer(
            "MB-STUB-0001",
            "123456",
            now_on(TODAY_DAYS + 5),
            day(TODAY_DAYS + 5),
            quick(),
        ) {
            Err(LicenceError::TooSoon { days_left }) => assert_eq!(days_left, 25),
            other => panic!("{other:?}"),
        }

        // Past the cooldown it is allowed again.
        assert!(
            licensing
                .transfer(
                    "MB-STUB-0001",
                    "123456",
                    now_on(TODAY_DAYS + 31),
                    day(TODAY_DAYS + 31),
                    quick(),
                )
                .is_ok()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **T9, through the real call path.** A cloud that never answers is a
    /// refusal, not a hang.
    #[test]
    fn a_cloud_that_never_answers_does_not_hang_the_counter() {
        let (dir, stub, mut licensing) = setup("never-answers");
        licensing
            .activate("MB-STUB-0001", "123456", now_on(TODAY_DAYS), quick())
            .expect("activates");
        stub.behave(Behaviour::NeverAnswers);

        let started = std::time::Instant::now();
        let outcome = licensing.refresh(now_on(TODAY_DAYS + 1), Duration::from_millis(150));
        assert!(matches!(outcome, Err(LicenceError::Timedout)));
        assert!(started.elapsed() < Duration::from_secs(5));

        // **And the shop is still entitled**, from the cache. A slow server is
        // not a reason to stop a restaurant working.
        assert_eq!(
            licensing
                .entitlement(now_on(TODAY_DAYS + 1), day(TODAY_DAYS + 1))
                .standing,
            Standing::Fine
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **T13.** A tampered file is refused, and the counter falls back to
    /// unactivated — which still bills.
    #[test]
    fn a_tampered_licence_file_falls_back_rather_than_failing() {
        let (dir, _stub, mut licensing) = setup("tampered");
        licensing
            .activate("MB-STUB-0001", "123456", now_on(TODAY_DAYS), quick())
            .expect("activates");
        drop(licensing);

        // Somebody edits the plan name in Notepad.
        let path = LicenceFile::path(&dir);
        let text = std::fs::read_to_string(&path).expect("reads");
        std::fs::write(&path, text.replace("Free trial", "Unlimited")).expect("writes");

        let tampered = Licensing::new(
            dir.clone(),
            a_machine(),
            Arc::new(Stub::active(&a_machine(), day(TODAY_DAYS + 30), now_on(TODAY_DAYS))),
            "0.1.0",
        );
        assert_eq!(
            tampered.entitlement(now_on(TODAY_DAYS), day(TODAY_DAYS)).standing,
            Standing::NeverActivated
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file that is not JSON at all costs the plan and not the counter.
    #[test]
    fn a_corrupt_licence_file_is_not_fatal_and_is_kept() {
        let dir = scratch("corrupt");
        std::fs::write(LicenceFile::path(&dir), "{ this is not json").expect("writes");
        let file = LicenceFile::load(&dir);
        assert_eq!(file, LicenceFile::default());
        assert!(LicenceFile::path(&dir).with_extension("broken.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **T12, and it is the piracy path.** A licence taken from machine A does
    /// not activate machine B.
    #[test]
    fn a_licence_from_another_machine_does_not_entitle_this_one() {
        let (dir, _stub, mut licensing) = setup("other-machine");
        licensing
            .activate("MB-STUB-0001", "123456", now_on(TODAY_DAYS), quick())
            .expect("activates");
        drop(licensing);

        // The same folder, carried to a different PC.
        let other_pc = Licensing::new(
            dir.clone(),
            MachineId::for_tests("9f8e7d6c-5b4a-3928-1716-0504030201ff"),
            Arc::new(Stub::active(&a_machine(), day(TODAY_DAYS + 30), now_on(TODAY_DAYS))),
            "0.1.0",
        );
        let entitlement = other_pc.entitlement(now_on(TODAY_DAYS), day(TODAY_DAYS));
        assert_eq!(entitlement.standing, Standing::BoundElsewhere);
        assert!(!entitlement.operating());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **T8, end to end.** Offline past the allowance, the answer is "we have
    /// not been able to check" — and winding the clock back does not fix it.
    #[test]
    fn an_offline_snapshot_runs_out_and_a_stopped_clock_does_not_extend_it() {
        let (dir, stub, mut licensing) = setup("offline-expiry");
        stub.set_max_offline_days(3);
        licensing
            .activate("MB-STUB-0001", "123456", now_on(TODAY_DAYS), quick())
            .expect("activates");
        stub.behave(Behaviour::Unreachable);

        // Two days offline: still fine.
        licensing.tick(now_on(TODAY_DAYS + 2)).expect("ticks");
        assert_eq!(
            licensing
                .entitlement(now_on(TODAY_DAYS + 2), day(TODAY_DAYS + 2))
                .standing,
            Standing::Fine
        );

        // Five days: past both the wall-clock life and the allowance.
        licensing.tick(now_on(TODAY_DAYS + 5)).expect("ticks");
        assert_eq!(
            licensing
                .entitlement(now_on(TODAY_DAYS + 5), day(TODAY_DAYS + 5))
                .standing,
            Standing::NeedsChecking
        );

        // Wind the clock back to activation day.
        assert_eq!(
            licensing
                .entitlement(now_on(TODAY_DAYS), day(TODAY_DAYS))
                .standing,
            Standing::NeedsChecking,
            "winding the clock back bought more offline time"
        );
        assert!(licensing.clock_says(now_on(TODAY_DAYS)).needs_an_online_check());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **T6, through the file.** Single use survives a restart, because the
    /// fingerprint is on disk.
    #[test]
    fn an_emergency_code_is_single_use_across_a_restart() {
        let (dir, _stub, mut licensing) = setup("emergency");
        let code = emergency::mint(&a_machine(), TODAY_DAYS, 72);

        let until = licensing
            .use_emergency_code(&code.to_read_out(), now_on(TODAY_DAYS))
            .expect("accepted");
        assert!(until.millis() > now_on(TODAY_DAYS).millis());

        // Unlocked, with no licence at all.
        let entitlement = licensing.entitlement(now_on(TODAY_DAYS), day(TODAY_DAYS));
        assert!(entitlement.operating());
        assert!(matches!(entitlement.standing, Standing::Emergency { .. }));

        drop(licensing);
        let mut again = Licensing::new(
            dir.clone(),
            a_machine(),
            Arc::new(Stub::active(&a_machine(), day(TODAY_DAYS + 30), now_on(TODAY_DAYS))),
            "0.1.0",
        );
        assert!(matches!(
            again.use_emergency_code(&code.to_read_out(), now_on(TODAY_DAYS)),
            Err(LicenceError::Emergency(emergency::EmergencyError::AlreadyUsed))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// And the unlock runs out on its own.
    #[test]
    fn an_emergency_unlock_expires() {
        let (dir, _stub, mut licensing) = setup("emergency-expiry");
        let code = emergency::mint(&a_machine(), TODAY_DAYS, 72);
        licensing
            .use_emergency_code(&code.to_read_out(), now_on(TODAY_DAYS))
            .expect("accepted");
        // Inside the 72 hours: unlocked.
        assert!(matches!(
            licensing
                .entitlement(now_on(TODAY_DAYS + 2), day(TODAY_DAYS + 2))
                .standing,
            Standing::Emergency { .. }
        ));
        // Past them: back to whatever the licence really says, which on a
        // counter that never activated is "not activated".
        let after = licensing.entitlement(now_on(TODAY_DAYS + 4), day(TODAY_DAYS + 4));
        assert!(!matches!(after.standing, Standing::Emergency { .. }));
        assert_eq!(after.standing, Standing::NeverActivated);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **POS-C10's local half, through the file.**
    #[test]
    fn wrong_emergency_codes_are_rate_limited() {
        let (dir, _stub, mut licensing) = setup("emergency-limit");
        for _ in 0..emergency::MAX_TRIES {
            assert!(
                licensing
                    .use_emergency_code("K7M2Q-9XR4T-BW8HN-3PZ6D", now_on(TODAY_DAYS))
                    .is_err()
            );
        }
        // The next one is refused for a different reason, and the reason
        // carries the wait.
        match licensing.use_emergency_code("K7M2Q-9XR4T-BW8HN-3PZ6D", now_on(TODAY_DAYS)) {
            Err(LicenceError::Emergency(emergency::EmergencyError::TooManyTries { wait })) => {
                assert!(wait.as_secs() > 0);
            }
            other => panic!("{other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **BACKEND-C1, all the way through the real path.** Suspend the licence
    /// in the cloud, refresh, and the counter stops being entitled today.
    #[test]
    fn suspending_a_licence_in_the_cloud_reaches_the_counter() {
        let (dir, stub, mut licensing) = setup("suspend");
        licensing
            .activate("MB-STUB-0001", "123456", now_on(TODAY_DAYS), quick())
            .expect("activates");
        assert!(licensing.entitlement(now_on(TODAY_DAYS), day(TODAY_DAYS)).operating());

        stub.set_status(Status::Suspended);
        licensing
            .refresh(now_on(TODAY_DAYS), quick())
            .expect("refreshes");

        let entitlement = licensing.entitlement(now_on(TODAY_DAYS), day(TODAY_DAYS));
        assert_eq!(entitlement.standing, Standing::Suspended);
        assert!(!entitlement.operating());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **BACKEND-C3, through the real path.** The shop-wide grace travels
    /// inside the snapshot, so the counter cannot disagree with the cloud.
    #[test]
    fn the_shop_wide_grace_period_reaches_the_counter() {
        let (dir, stub, mut licensing) = setup("grace");
        let mut licence = stub.licence();
        licence.renews_on = day(TODAY_DAYS - 20);
        licence.status = Status::Active;
        stub.set_licence(licence);
        stub.set_global_grace(Some(30));
        licensing
            .activate("MB-STUB-0001", "123456", now_on(TODAY_DAYS), quick())
            .expect("activates");

        assert_eq!(
            licensing
                .entitlement(now_on(TODAY_DAYS), day(TODAY_DAYS))
                .standing,
            Standing::InGrace { days_left: 11 }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **D92's coverage check.** Every cloud call in this file goes through
    /// `deadline::within`. A future session that adds a sixth call and forgets
    /// gets a red build, not a frozen counter three months later.
    #[test]
    fn no_cloud_call_is_unwrapped() {
        let source = crate::shipped_part_of(include_str!("state.rs"));
        let mut unwrapped = Vec::new();
        for (number, line) in source.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            for call in ["cloud.refresh(", "cloud.activate(", "cloud.release(", "cloud.transfer(", "cloud.start_trial("] {
                if code.contains(call) && !code.contains("deadline::within") {
                    unwrapped.push(format!("line {}: {}", number + 1, code.trim()));
                }
            }
        }
        assert!(
            unwrapped.is_empty(),
            "these cloud calls have no deadline, which is D92 and v1's \
             deadlocked socket: {unwrapped:?}"
        );
    }
}
