//! An update is pending until the new counter has actually opened.
//!
//! The copied worker survives replacement of magic-bill.exe. Its journal is separate from
//! Current, so a failed install never turns the pending download into a rollback candidate.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::{Current, Previous, UpdateFailure, Version};

const WORKER: &str = "updater-worker.exe";
const READY_TIMEOUT: Duration = Duration::from_secs(15);
const EXIT_TIMEOUT: Duration = Duration::from_secs(60);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Candidate {
    version: Version,
    installer: PathBuf,
    sha256: String,
    supports_startup_ack: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Pending {
    id: String,
    version: Version,
    installer: PathBuf,
    sha256: String,
    executable: PathBuf,
    parent_pid: u32,
    previous: Option<Candidate>,
    expects_ack: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Stage {
    Preparing,
    Ready,
    Installing,
    AwaitingStartup,
    Succeeded,
    LegacyStarted,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Outcome {
    stage: Stage,
    helper_pid: Option<u32>,
    message: Option<String>,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

fn updates(dir: &Path) -> PathBuf {
    dir.join("updates")
}

fn pending_path(dir: &Path) -> PathBuf {
    updates(dir).join("pending.json")
}

fn attempt_dir(dir: &Path, pending: &Pending) -> PathBuf {
    updates(dir).join("attempts").join(&pending.id)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<T> {
    serde_json::from_slice(&fs::read(path)?).map_err(io::Error::other)
}

pub(super) fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("Missing journal folder"))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = File::create(&temporary)?;
    file.write_all(&serde_json::to_vec_pretty(value).map_err(io::Error::other)?)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)
}

fn outcome(dir: &Path, pending: &Pending) -> io::Result<Outcome> {
    read_json(&attempt_dir(dir, pending).join("result.json"))
}

fn set_outcome(dir: &Path, pending: &Pending, result: &Outcome) -> io::Result<()> {
    write_json(&attempt_dir(dir, pending).join("result.json"), result)
}

pub(super) fn failure(dir: &Path) -> Option<UpdateFailure> {
    read_json(&updates(dir).join("failure.json")).ok()
}

pub(super) fn downloaded_for(dir: &Path, version: Version, installer: &Path) -> bool {
    if let Ok(pending) = read_json::<Pending>(&pending_path(dir))
        && pending.version == version
        && pending.installer == installer
    {
        return verify_installer(installer, &pending.sha256).is_ok();
    }
    let checked = || -> Option<()> {
        let json = fs::read_to_string(updates(dir).join("manifest.json")).ok()?;
        let signature = fs::read_to_string(updates(dir).join("manifest.json.sig")).ok()?;
        let manifest = super::check(&json, signature.trim()).ok()?;
        if manifest.version != version {
            return None;
        }
        verify_installer(installer, &manifest.sha256).ok()
    };
    checked().is_some()
}

pub(super) fn record_failure(dir: &Path, version: Version, message: &str) -> io::Result<()> {
    write_json(
        &updates(dir).join("failure.json"),
        &UpdateFailure {
            version: version.to_string(),
            message: message.to_owned(),
        },
    )
}

pub(super) fn clear_failure(dir: &Path) -> io::Result<()> {
    match fs::remove_file(updates(dir).join("failure.json")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn fail(dir: &Path, pending: &Pending, message: &str) -> io::Result<()> {
    let helper_pid = outcome(dir, pending)
        .ok()
        .and_then(|result| result.helper_pid);
    set_outcome(
        dir,
        pending,
        &Outcome {
            stage: Stage::Failed,
            helper_pid,
            message: Some(message.to_owned()),
        },
    )?;
    if read_json::<Pending>(&pending_path(dir)).is_ok_and(|active| active.id == pending.id) {
        record_failure(dir, pending.version, message)?;
    }
    Ok(())
}

pub(super) fn fingerprint(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest
        .finish()
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub(super) fn verify_installer(path: &Path, sha256: &str) -> io::Result<()> {
    if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid("The update fingerprint is invalid."));
    }
    if !path.is_file() {
        return Err(invalid(
            "The downloaded installer is missing. Download the update again from Updates.",
        ));
    }
    if !fingerprint(path)?.eq_ignore_ascii_case(sha256) {
        return Err(invalid(
            "The downloaded installer changed after verification. Download the update again from Updates.",
        ));
    }
    Ok(())
}

fn live(pid: u32) -> io::Result<bool> {
    match mb_winprint::open_process(pid)? {
        Some(process) => process.wait(Duration::ZERO).map(|ended| !ended),
        None => Ok(false),
    }
}

fn validate(dir: &Path, pending: &Pending) -> io::Result<()> {
    if pending.id.is_empty()
        || !pending
            .id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'-')
        || pending.parent_pid == 0
    {
        return Err(invalid("The update journal is invalid."));
    }
    let name = format!("MagicBill-{}.exe", pending.version);
    let allowed = [
        updates(dir).join("incoming").join(&name),
        Previous::folder(dir).join(&name),
    ];
    if !allowed.contains(&pending.installer)
        || !pending.installer.is_absolute()
        || !pending.executable.is_absolute()
        || pending
            .executable
            .file_name()
            .and_then(|name| name.to_str())
            .is_none_or(|name| !name.eq_ignore_ascii_case("magic-bill.exe"))
        || pending
            .executable
            .parent()
            .is_none_or(|path| !path.is_dir())
    {
        return Err(invalid(
            "The update journal points outside the expected installer or application location.",
        ));
    }
    Ok(())
}

pub(super) fn ensure_idle(dir: &Path) -> io::Result<()> {
    let old: Pending = match read_json(&pending_path(dir)) {
        Ok(pending) => pending,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => {
            return Err(invalid(
                "The previous update record could not be read. Contact support before trying another update.",
            ));
        }
    };
    validate(dir, &old)?;
    if let Ok(result) = outcome(dir, &old) {
        if matches!(
            result.stage,
            Stage::Succeeded | Stage::Failed | Stage::LegacyStarted
        ) {
            return Ok(());
        }
        if result
            .helper_pid
            .is_some_and(|pid| live(pid).unwrap_or(true))
        {
            return Err(invalid(
                "An installer is still running. Wait for it to finish before trying another update.",
            ));
        }
    }
    fail(
        dir,
        &old,
        "The previous update stopped before completion. Your downloaded installer was kept. You can retry from Updates.",
    )
}

fn candidate(dir: &Path, running: Version, installer: &Path) -> Option<Candidate> {
    let current = Current::load(dir)?;
    if current.version != running || current.installer == installer {
        return None;
    }
    Some(Candidate {
        version: current.version,
        sha256: fingerprint(&current.installer).ok()?,
        installer: current.installer,
        supports_startup_ack: current.supports_startup_ack,
    })
}

fn hidden(command: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt as _;
    command.creation_flags(0x0800_0000).stdin(Stdio::null())
}

/// Prepare durable state and wait until the worker has opened the parent's process handle.
pub(super) fn handoff(
    dir: &Path,
    version: Version,
    installer: &Path,
    sha256: &str,
    expects_ack: bool,
) -> io::Result<()> {
    ensure_idle(dir)?;
    cleanup_settled_attempts(dir);
    verify_installer(installer, sha256)?;
    let executable = std::env::current_exe()?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let pending = Pending {
        id: format!("{}-{stamp}", std::process::id()),
        version,
        installer: installer.to_path_buf(),
        sha256: sha256.to_owned(),
        executable: executable.clone(),
        parent_pid: std::process::id(),
        previous: candidate(dir, Version::running(), installer),
        expects_ack,
    };
    validate(dir, &pending)?;
    let folder = attempt_dir(dir, &pending);
    fs::create_dir_all(&folder)?;
    // Different name and directory: NSIS must neither overwrite nor kill this worker.
    let worker = folder.join(WORKER);
    fs::copy(&executable, &worker)?;
    write_json(&folder.join("job.json"), &pending)?;
    set_outcome(
        dir,
        &pending,
        &Outcome {
            stage: Stage::Preparing,
            helper_pid: None,
            message: None,
        },
    )?;
    write_json(&pending_path(dir), &pending)?;
    clear_failure(dir)?;
    let started = hidden(
        Command::new(&worker)
            .arg("--apply-update")
            .current_dir(&folder),
    )
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn();
    let mut child = match started {
        Ok(child) => child,
        Err(error) => {
            let message =
                format!("Windows could not start the updater: {error}. Magic Bill will stay open.");
            fail(dir, &pending, &message)?;
            return Err(invalid(message));
        }
    };
    let ready = wait_for_ready(dir, &pending, &mut child)
        .and_then(|()| write_json(&folder.join("proceed.json"), &pending.id));
    if let Err(error) = ready {
        // This worker belongs to this attempt. It must not wake up and install later.
        // The marker is required even if Windows refuses to terminate the worker.
        write_json(&folder.join("cancelled.json"), &pending.id)?;
        if child.kill().is_ok() {
            let _ = child.wait();
        }
        let message = format!(
            "The updater could not prepare the installation: {error}. Magic Bill will stay open."
        );
        fail(dir, &pending, &message)?;
        return Err(invalid(message));
    }
    Ok(())
}

fn wait_for_ready(dir: &Path, pending: &Pending, child: &mut Child) -> io::Result<()> {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        if let Ok(result) = outcome(dir, pending) {
            if result.stage == Stage::Ready && result.helper_pid == Some(child.id()) {
                return Ok(());
            }
            if result.stage == Stage::Failed {
                return Err(invalid(
                    result
                        .message
                        .unwrap_or_else(|| "The updater stopped.".to_owned()),
                ));
            }
        }
        if child.try_wait()?.is_some() {
            return Err(invalid("The updater closed before it was ready."));
        }
        if Instant::now() >= deadline {
            return Err(invalid("Windows did not start the updater in time."));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn installer_command(installer: &Path, directory: &Path) -> io::Result<Command> {
    use std::os::windows::process::CommandExt as _;
    if !directory.is_absolute() || directory.as_os_str().is_empty() {
        return Err(invalid("The application's installation folder is invalid."));
    }
    let mut command = Command::new(installer);
    command.args(["/S", "/UPDATE"]);
    // NSIS requires /D last, without quotes even when the directory contains spaces.
    // This is a direct process launch; no shell interprets its characters.
    let mut destination = std::ffi::OsString::from("/D=");
    destination.push(directory);
    command.raw_arg(destination);
    hidden(&mut command);
    Ok(command)
}

fn installer_result(code: Option<i32>) -> io::Result<()> {
    match code {
        Some(0) => Ok(()),
        Some(3010) => Err(invalid(
            "Windows needs to restart to finish the update. Restart this computer, then check the version in Updates.",
        )),
        Some(code) => Err(invalid(format!(
            "Windows could not finish installing the update (installer code {code}). Your downloaded installer was kept. Retry from Updates or contact support."
        ))),
        None => Err(invalid(
            "The installer stopped without reporting a result. Your downloaded installer was kept. Retry from Updates or contact support.",
        )),
    }
}

fn start_counter(pending: &Pending) -> io::Result<Child> {
    let directory = pending
        .executable
        .parent()
        .ok_or_else(|| invalid("Missing application folder"))?;
    hidden(Command::new(&pending.executable).current_dir(directory))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

fn apply(dir: &Path, pending: &Pending) -> io::Result<()> {
    let current: Pending = read_json(&pending_path(dir))?;
    if current != *pending || pending.parent_pid == std::process::id() {
        return Err(invalid("This update attempt is no longer active."));
    }
    validate(dir, pending)?;
    // Open before publishing readiness: the handle identifies this exact parent process.
    let parent = mb_winprint::open_process(pending.parent_pid)?;
    let result = |stage| Outcome {
        stage,
        helper_pid: Some(std::process::id()),
        message: None,
    };
    set_outcome(dir, pending, &result(Stage::Ready))?;
    if let Some(parent) = parent
        && !parent.wait(EXIT_TIMEOUT)?
    {
        return Err(invalid(
            "Magic Bill did not finish closing, so the update was not installed. Close it normally and retry from Updates.",
        ));
    }
    authorized_after_exit(dir, pending)?;
    set_outcome(dir, pending, &result(Stage::Installing))?;
    let installed = (|| {
        verify_installer(&pending.installer, &pending.sha256)?;
        authorized_after_exit(dir, pending)?;
        let log = File::create(attempt_dir(dir, pending).join("installer.log"))?;
        let directory = pending
            .executable
            .parent()
            .ok_or_else(|| invalid("Missing application folder"))?;
        let status = installer_command(&pending.installer, directory)?
            .current_dir(attempt_dir(dir, pending))
            .stdout(log.try_clone()?)
            .stderr(log)
            .status()?;
        installer_result(status.code())
    })();
    if let Err(error) = installed {
        fail(dir, pending, &error.to_string())?;
        // Reopen the existing counter so the persisted failure is visible. Never claim success.
        if pending.executable.is_file() {
            let _ = start_counter(pending);
        }
        return Ok(());
    }
    set_outcome(dir, pending, &result(Stage::AwaitingStartup))?;
    let mut counter = start_counter(pending)?;
    if !pending.expects_ack {
        // Old counters predate this journal. A successful installer and launch are all we
        // can report; do not label a working rollback failed or promote Current as healthy.
        set_outcome(dir, pending, &Outcome {
            stage: Stage::LegacyStarted,
            helper_pid: Some(std::process::id()),
            message: Some("The previous version was installed and launched; this older version cannot confirm startup.".to_owned()),
        })?;
        return Ok(());
    }
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        if outcome(dir, pending)?.stage == Stage::Succeeded {
            return Ok(());
        }
        if counter.try_wait()?.is_some() || Instant::now() >= deadline {
            // Leave AwaitingStartup intact: a later healthy start of this exact version can
            // still acknowledge it. Installer success alone does not change Current.
            record_failure(
                dir,
                pending.version,
                "The installer finished, but Magic Bill has not confirmed opening the new version. Open Magic Bill and check Updates. If it still shows the old version, contact support.",
            )?;
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn authorized_after_exit(dir: &Path, pending: &Pending) -> io::Result<()> {
    let active: Pending = read_json(&pending_path(dir))?;
    let folder = attempt_dir(dir, pending);
    let proceed: String = read_json(&folder.join("proceed.json"))?;
    if active != *pending || proceed != pending.id || folder.join("cancelled.json").exists() {
        return Err(invalid(
            "This update attempt was cancelled before installation.",
        ));
    }
    Ok(())
}

/// The parent should have exited by now. Cancelling while it is still alive is safe:
/// the worker cannot have passed its process wait, and checks this marker afterwards.
pub(super) fn recover_handoff(dir: &Path) -> io::Result<Option<UpdateFailure>> {
    let pending: Pending = read_json(&pending_path(dir))?;
    if pending.parent_pid != std::process::id() {
        return Ok(None);
    }
    let result = outcome(dir, &pending)?;
    if !matches!(
        result.stage,
        Stage::Preparing | Stage::Ready | Stage::Failed
    ) {
        return Ok(None);
    }
    write_json(
        &attempt_dir(dir, &pending).join("cancelled.json"),
        &pending.id,
    )?;
    let message = result.message.unwrap_or_else(|| "Magic Bill did not finish closing, so the update was cancelled. You can continue working and retry from Updates.".to_owned());
    fail(dir, &pending, &message)?;
    Ok(failure(dir))
}

/// Called before logging, database access, or the single-instance plugin in main().
pub(super) fn run_helper_if_requested() -> bool {
    if std::env::args_os()
        .nth(1)
        .is_none_or(|arg| arg != "--apply-update")
    {
        return false;
    }
    let run = || -> io::Result<()> {
        let executable = std::env::current_exe()?;
        if executable.file_name().is_none_or(|name| name != WORKER) {
            return Err(invalid(
                "The update worker must run from its own attempt folder.",
            ));
        }
        let folder = executable
            .parent()
            .ok_or_else(|| invalid("Missing worker folder"))?;
        let pending: Pending = read_json(&folder.join("job.json"))?;
        let dir = folder
            .ancestors()
            .nth(3)
            .ok_or_else(|| invalid("Missing update folder"))?;
        if attempt_dir(dir, &pending) != folder {
            return Err(invalid("The update worker is outside its attempt folder."));
        }
        if let Err(error) = apply(dir, &pending) {
            fail(dir, &pending, &error.to_string())?;
        }
        Ok(())
    };
    // A helper never falls through to billing, including when its journal is unreadable.
    let _ = run();
    true
}

/// Commit only after a healthy window from the target version and installation path exists.
pub(super) fn acknowledge_start(dir: &Path, running: Version, executable: &Path) -> io::Result<()> {
    let pending: Pending = match read_json(&pending_path(dir)) {
        Ok(pending) => pending,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    validate(dir, &pending)?;
    let result = outcome(dir, &pending)?;
    if result.stage != Stage::AwaitingStartup {
        if !matches!(
            result.stage,
            Stage::Succeeded | Stage::Failed | Stage::LegacyStarted
        ) && !result
            .helper_pid
            .is_some_and(|pid| live(pid).unwrap_or(true))
        {
            fail(
                dir,
                &pending,
                "The previous update stopped before completion. Your downloaded installer was kept. You can retry from Updates.",
            )?;
        }
        return Ok(());
    }
    if running != pending.version || executable != pending.executable {
        record_failure(
            dir,
            pending.version,
            "The update did not open the expected version of Magic Bill. This copy is still running from its original location. Retry from Updates or contact support.",
        )?;
        return Ok(());
    }
    if let Some(previous) = &pending.previous {
        let keep = || -> io::Result<()> {
            verify_installer(&previous.installer, &previous.sha256)?;
            let kept = Previous::folder(dir).join(format!("MagicBill-{}.exe", previous.version));
            fs::create_dir_all(Previous::folder(dir))?;
            if previous.installer != kept {
                fs::copy(&previous.installer, &kept)?;
            }
            Previous {
                version: previous.version,
                installer: kept,
                sha256: Some(previous.sha256.clone()),
                supports_startup_ack: previous.supports_startup_ack,
            }
            .save(dir)
        };
        if let Err(error) = keep() {
            crate::log_warn!(
                "the new version opened, but its rollback installer could not be kept: {error}"
            );
        }
    }
    Current {
        version: running,
        installer: pending.installer.clone(),
        sha256: Some(pending.sha256.clone()),
        supports_startup_ack: true,
    }
    .save(dir)?;
    // Completion is written last; a crash before this is safely reconciled again.
    set_outcome(
        dir,
        &pending,
        &Outcome {
            stage: Stage::Succeeded,
            helper_pid: result.helper_pid,
            message: None,
        },
    )?;
    clear_failure(dir)?;
    cleanup_settled_attempts(dir);
    Ok(())
}

fn cleanup_settled_attempts(dir: &Path) {
    let active = read_json::<Pending>(&pending_path(dir))
        .ok()
        .map(|pending| pending.id);
    let attempts = updates(dir).join("attempts");
    let Ok(entries) = fs::read_dir(&attempts) else {
        return;
    };
    let mut settled = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if active.as_ref() == Some(&name)
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'-')
            || !entry
                .file_type()
                .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
        {
            continue;
        }
        let path = entry.path();
        let Ok(result) = read_json::<Outcome>(&path.join("result.json")) else {
            continue;
        };
        if !matches!(
            result.stage,
            Stage::Succeeded | Stage::Failed | Stage::LegacyStarted
        ) || result
            .helper_pid
            .is_some_and(|pid| live(pid).unwrap_or(true))
        {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(UNIX_EPOCH);
        settled.push((modified, path));
    }
    // Retain the active attempt and the most recent settled journal for support.
    settled.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    for (_, path) in settled.into_iter().skip(1) {
        // Only direct, non-link child directories examined above can reach this delete.
        if path.parent() == Some(attempts.as_path()) {
            let _ = fs::remove_dir_all(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(label: &str) -> (crate::signin_tests::Scratch, Pending) {
        let scratch = crate::signin_tests::Scratch::new(label);
        let dir = scratch.dir();
        let installer = updates(dir).join("incoming").join("MagicBill-1.8.5.exe");
        fs::create_dir_all(installer.parent().expect("folder")).expect("create");
        fs::write(&installer, b"verified installer").expect("installer");
        let executable = dir.join("Magic Bill").join("magic-bill.exe");
        fs::create_dir_all(executable.parent().expect("folder")).expect("app folder");
        fs::write(&executable, b"counter").expect("counter");
        let pending = Pending {
            id: "100-200".to_owned(),
            version: Version::new(1, 8, 5),
            sha256: fingerprint(&installer).expect("hash"),
            installer,
            executable,
            parent_pid: std::process::id(),
            previous: None,
            expects_ack: true,
        };
        write_json(&pending_path(dir), &pending).expect("pending");
        (scratch, pending)
    }

    fn stage(dir: &Path, pending: &Pending, stage: Stage) {
        set_outcome(
            dir,
            pending,
            &Outcome {
                stage,
                helper_pid: None,
                message: None,
            },
        )
        .expect("stage");
    }

    #[test]
    fn failed_retry_keeps_pending_installer_and_real_current() {
        let (scratch, pending) = fixture("update_failed_retry");
        let dir = scratch.dir();
        let old = updates(dir).join("incoming").join("MagicBill-1.8.4.exe");
        fs::write(&old, b"old installer").expect("old");
        Current {
            version: Version::new(1, 8, 4),
            installer: old.clone(),
            sha256: None,
            supports_startup_ack: false,
        }
        .save(dir)
        .expect("current");
        fail(dir, &pending, "Windows refused the installer").expect("failure");
        ensure_idle(dir).expect("retry allowed");
        assert_eq!(Current::load(dir).expect("current").installer, old);
        verify_installer(&pending.installer, &pending.sha256).expect("pending retained");
        assert_eq!(
            failure(dir).expect("reason").message,
            "Windows refused the installer"
        );
    }

    #[test]
    fn only_healthy_target_from_expected_path_commits_current() {
        let (scratch, pending) = fixture("update_ack");
        let dir = scratch.dir();
        stage(dir, &pending, Stage::Installing);
        acknowledge_start(dir, pending.version, &pending.executable).expect("not ready");
        assert!(Current::load(dir).is_none());
        stage(dir, &pending, Stage::AwaitingStartup);
        acknowledge_start(dir, Version::new(1, 8, 4), &pending.executable).expect("old app");
        assert!(Current::load(dir).is_none());
        acknowledge_start(dir, pending.version, &dir.join("other.exe")).expect("wrong copy");
        assert!(Current::load(dir).is_none());
        acknowledge_start(dir, pending.version, &pending.executable).expect("healthy target");
        assert_eq!(
            Current::load(dir).expect("current").version,
            pending.version
        );
        assert_eq!(
            outcome(dir, &pending).expect("outcome").stage,
            Stage::Succeeded
        );
        assert!(failure(dir).is_none());
    }

    #[test]
    fn confirmation_copies_real_rollback_without_moving_downloads() {
        let (scratch, mut pending) = fixture("update_commit_rollback");
        let dir = scratch.dir();
        let old = updates(dir).join("incoming").join("MagicBill-1.8.4.exe");
        fs::write(&old, b"older installer").expect("old");
        Current {
            version: Version::new(1, 8, 4),
            installer: old.clone(),
            sha256: None,
            supports_startup_ack: false,
        }
        .save(dir)
        .expect("current");
        pending.previous = candidate(dir, Version::new(1, 8, 4), &pending.installer);
        write_json(&pending_path(dir), &pending).expect("pending");
        stage(dir, &pending, Stage::AwaitingStartup);
        acknowledge_start(dir, pending.version, &pending.executable).expect("confirm");
        assert_eq!(
            Previous::load(dir).expect("rollback").version,
            Version::new(1, 8, 4)
        );
        assert!(old.is_file());
        verify_installer(&pending.installer, &pending.sha256).expect("new file remains");
    }

    #[test]
    fn legacy_current_pointing_at_pending_is_not_a_rollback_candidate() {
        let (scratch, pending) = fixture("update_legacy_pending");
        Current {
            version: pending.version,
            installer: pending.installer.clone(),
            sha256: None,
            supports_startup_ack: false,
        }
        .save(scratch.dir())
        .expect("legacy current");
        assert!(candidate(scratch.dir(), Version::new(1, 8, 4), &pending.installer).is_none());
        verify_installer(&pending.installer, &pending.sha256).expect("not moved");
    }

    #[test]
    fn changed_cache_is_never_reused_but_a_healthy_target_still_confirms() {
        let (scratch, pending) = fixture("update_changed");
        stage(scratch.dir(), &pending, Stage::AwaitingStartup);
        fs::write(&pending.installer, b"changed").expect("change");
        assert!(verify_installer(&pending.installer, &pending.sha256).is_err());
        acknowledge_start(scratch.dir(), pending.version, &pending.executable)
            .expect("running target is healthy");
        assert_eq!(
            outcome(scratch.dir(), &pending).expect("outcome").stage,
            Stage::Succeeded
        );
        fs::remove_file(&pending.installer).expect("remove");
        assert!(verify_installer(&pending.installer, &pending.sha256).is_err());
        assert!(Current::load(scratch.dir()).is_none());
    }

    #[test]
    fn installer_failure_and_reboot_are_not_success() {
        assert!(installer_result(Some(0)).is_ok());
        assert!(installer_result(Some(1)).is_err());
        assert!(
            installer_result(Some(3010))
                .expect_err("restart required")
                .to_string()
                .contains("restart")
        );
        assert!(installer_result(None).is_err());
    }

    #[test]
    fn journal_cannot_escape_attempt_or_installer_folders() {
        let (scratch, mut pending) = fixture("update_paths");
        validate(scratch.dir(), &pending).expect("valid");
        pending.id = "..".to_owned();
        assert!(validate(scratch.dir(), &pending).is_err());
        pending.id = "100-200".to_owned();
        pending.installer = scratch.dir().join("other.exe");
        assert!(validate(scratch.dir(), &pending).is_err());
    }

    #[test]
    fn missing_rollback_cache_does_not_reject_a_healthy_target() {
        let (scratch, mut pending) = fixture("update_old_cache_gone");
        let dir = scratch.dir();
        let retained = Previous::folder(dir).join("MagicBill-1.8.2.exe");
        fs::create_dir_all(Previous::folder(dir)).expect("previous folder");
        fs::write(&retained, b"valid earlier rollback").expect("rollback");
        Previous {
            version: Version::new(1, 8, 2),
            installer: retained.clone(),
            sha256: Some(fingerprint(&retained).expect("hash")),
            supports_startup_ack: false,
        }
        .save(dir)
        .expect("previous");
        pending.previous = Some(Candidate {
            version: Version::new(1, 8, 4),
            installer: dir.join("missing.exe"),
            sha256: "00".repeat(32),
            supports_startup_ack: false,
        });
        write_json(&pending_path(dir), &pending).expect("pending");
        stage(dir, &pending, Stage::AwaitingStartup);
        acknowledge_start(dir, pending.version, &pending.executable).expect("healthy target");
        assert_eq!(
            Current::load(dir).expect("current").version,
            pending.version
        );
        assert_eq!(
            Previous::load(dir).expect("retained rollback").installer,
            retained
        );
    }

    #[test]
    fn an_obsolete_worker_cannot_overwrite_the_new_attempts_failure() {
        let (scratch, old) = fixture("update_obsolete_worker");
        let mut new = old.clone();
        new.id = "100-201".to_owned();
        write_json(&pending_path(scratch.dir()), &new).expect("new pending");
        fail(scratch.dir(), &old, "old attempt stopped").expect("old journal");
        assert!(failure(scratch.dir()).is_none());
        assert_eq!(
            outcome(scratch.dir(), &old).expect("old result").stage,
            Stage::Failed
        );
    }
}

#[cfg(test)]
#[path = "transaction_tests.rs"]
mod transaction_tests;
