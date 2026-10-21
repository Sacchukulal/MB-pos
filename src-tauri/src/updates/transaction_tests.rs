//! Exercise the updater with real, hidden child processes in a disposable directory.

use super::*;
use std::os::windows::process::CommandExt as _;

const ENTRY: &str = "updates::transaction::subprocess_tests::worker_entry";
const EARLY_EXIT: &str = "updates::transaction::subprocess_tests::worker_exits_before_ready";

struct RunningChild(Child);

impl Drop for RunningChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for_child(child: &mut Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().expect("child status") {
            return status;
        }
        assert!(Instant::now() < deadline, "test child did not finish");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.is_file() {
        assert!(
            Instant::now() < deadline,
            "fixture did not create {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn test_child(entry: &str, dir: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["--exact", entry, "--ignored", "--nocapture"])
        .env("MB_UPDATE_TEST_DIRECTORY", dir)
        // A fixture must never touch the real counter's profile, even if it fails.
        .env("APPDATA", dir.join("isolated-profile"))
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000);
    command
}

/// This fixture does not install anything. It records the real Windows command line,
/// returns the requested installer result, or stands in for a waiting/restarted counter.
fn compile_fixture(dir: &Path) -> PathBuf {
    let source = dir.join("fake_update_process.rs");
    let executable = dir.join("fake-update-process.exe");
    fs::write(&source, r#"
use std::{env, fs, io::{self, Read}, path::Path, process};

#[link(name = "kernel32")]
unsafe extern "system" { fn GetCommandLineW() -> *const u16; }

fn command_line() -> String {
    // The Windows process command line is a valid, null-terminated UTF-16 string.
    unsafe {
        let text = GetCommandLineW();
        let mut length = 0;
        while *text.add(length) != 0 { length += 1; }
        String::from_utf16_lossy(std::slice::from_raw_parts(text, length))
    }
}

fn main() {
    if env::args().nth(1).as_deref() == Some("--hold-parent") {
        let _ = io::stdin().read_exact(&mut [0_u8]);
        return;
    }
    let executable = env::current_exe().expect("fixture executable");
    if executable.file_name().and_then(|name| name.to_str()) == Some("magic-bill.exe") {
        fs::write(env::var_os("MB_UPDATE_TEST_COUNTER").expect("counter marker"), b"opened").expect("marker");
        return;
    }
    let capture = env::var_os("MB_UPDATE_TEST_CAPTURE").expect("command capture");
    fs::write(Path::new(&capture), command_line()).expect("command capture");
    let code: i32 = env::var("MB_UPDATE_TEST_EXIT").expect("exit code").parse().expect("integer");
    process::exit(code);
}
"#).expect("fixture source");
    let output = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc.exe".into()))
        .args(["--edition=2024", "--crate-name", "magicbill_update_fixture"])
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .stdin(Stdio::null())
        .creation_flags(0x0800_0000)
        .output()
        .expect("compile isolated fake process");
    assert!(
        output.status.success(),
        "fixture compile failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    executable
}

fn prepare(dir: &Path, fixture: &Path, parent_pid: u32) -> Pending {
    let installer = updates(dir).join("incoming").join("MagicBill-1.8.5.exe");
    fs::create_dir_all(installer.parent().expect("incoming folder")).expect("incoming");
    fs::copy(fixture, &installer).expect("fake installer");
    // Spaces, shell metacharacters, percent text and Unicode must remain literal.
    let executable = dir
        .join("Magic Bill & %UPDATE_TEST% café")
        .join("magic-bill.exe");
    fs::create_dir_all(executable.parent().expect("application folder")).expect("app directory");
    fs::copy(fixture, &executable).expect("fake counter");
    let pending = Pending {
        id: format!("{}-100", std::process::id()),
        version: Version::new(1, 8, 5),
        sha256: fingerprint(&installer).expect("installer hash"),
        installer,
        executable,
        parent_pid,
        previous: None,
        // The fake counter cannot acknowledge healthy Tauri startup.
        expects_ack: false,
    };
    write_json(&pending_path(dir), &pending).expect("pending");
    set_outcome(
        dir,
        &pending,
        &Outcome {
            stage: Stage::Preparing,
            helper_pid: None,
            message: None,
        },
    )
    .expect("preparing");
    pending
}

#[test]
fn real_worker_waits_cancels_and_reports_installer_results() {
    let scratch = crate::signin_tests::Scratch::new("update_real_processes");
    let fixture = compile_fixture(scratch.dir());
    for (label, code, cancel) in [
        ("success", 0, false),
        ("failure", 23, false),
        ("cancel", 0, true),
    ] {
        let dir = scratch.dir().join(label);
        fs::create_dir_all(&dir).expect("scenario directory");
        let mut parent = RunningChild(
            Command::new(&fixture)
                .arg("--hold-parent")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(0x0800_0000)
                .spawn()
                .expect("fake parent"),
        );
        let pending = prepare(&dir, &fixture, parent.0.id());
        let capture = dir.join("installer-command.txt");
        let reopened = dir.join("counter-opened.txt");
        let mut worker = RunningChild(
            test_child(ENTRY, &dir)
                .env("MB_UPDATE_TEST_CAPTURE", &capture)
                .env("MB_UPDATE_TEST_COUNTER", &reopened)
                .env("MB_UPDATE_TEST_EXIT", code.to_string())
                .spawn()
                .expect("worker"),
        );

        wait_for_ready(&dir, &pending, &mut worker.0).expect("worker ready");
        write_json(
            &attempt_dir(&dir, &pending).join("proceed.json"),
            &pending.id,
        )
        .expect("authorize");
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            !capture.exists(),
            "installer ran while the original process was still alive"
        );
        assert!(parent.0.try_wait().expect("parent status").is_none());
        if cancel {
            write_json(
                &attempt_dir(&dir, &pending).join("cancelled.json"),
                &pending.id,
            )
            .expect("cancel");
        }
        parent
            .0
            .stdin
            .take()
            .expect("parent input")
            .write_all(b"x")
            .expect("close parent");
        assert!(wait_for_child(&mut parent.0).success());
        let status = wait_for_child(&mut worker.0);

        if cancel {
            assert!(!status.success());
            assert!(
                !capture.exists(),
                "a cancelled worker launched the installer after parent exit"
            );
            assert!(!reopened.exists());
            assert!(
                failure(&dir)
                    .expect("cancel reason")
                    .message
                    .contains("cancelled")
            );
        } else {
            assert!(status.success());
            wait_for_file(&reopened);
            let raw = fs::read_to_string(&capture).expect("real Windows command line");
            let directory = pending.executable.parent().expect("destination");
            assert!(
                raw.ends_with(&format!(" /S /UPDATE /D={}", directory.display())),
                "{raw}"
            );
            if code == 0 {
                assert_eq!(
                    outcome(&dir, &pending).expect("outcome").stage,
                    Stage::LegacyStarted
                );
                assert!(failure(&dir).is_none());
            } else {
                assert_eq!(
                    outcome(&dir, &pending).expect("outcome").stage,
                    Stage::Failed
                );
                assert!(
                    failure(&dir)
                        .expect("failure reason")
                        .message
                        .contains("installer code 23")
                );
            }
        }
        assert!(
            Current::load(&dir).is_none(),
            "launch alone cannot confirm healthy startup"
        );
        verify_installer(&pending.installer, &pending.sha256).expect("download remains intact");
    }
}

#[test]
fn exited_or_wrong_worker_is_not_accepted_as_ready() {
    let scratch = crate::signin_tests::Scratch::new("update_worker_early_exit");
    let pending = Pending {
        id: "100-200".to_owned(),
        version: Version::new(1, 8, 5),
        installer: scratch.dir().join("unused.exe"),
        sha256: "00".repeat(32),
        executable: scratch.dir().join("magic-bill.exe"),
        parent_pid: std::process::id(),
        previous: None,
        expects_ack: true,
    };
    set_outcome(
        scratch.dir(),
        &pending,
        &Outcome {
            stage: Stage::Ready,
            helper_pid: Some(u32::MAX),
            message: None,
        },
    )
    .expect("unrelated ready message");
    let mut child = RunningChild(
        test_child(EARLY_EXIT, scratch.dir())
            .spawn()
            .expect("early worker"),
    );
    let error = wait_for_ready(scratch.dir(), &pending, &mut child.0)
        .expect_err("worker did not become ready");
    assert!(error.to_string().contains("closed before it was ready"));
    assert_eq!(wait_for_child(&mut child.0).code(), Some(7));
}

#[test]
#[ignore = "isolated subprocess fixture, launched by transaction tests"]
fn worker_entry() {
    let dir =
        PathBuf::from(std::env::var_os("MB_UPDATE_TEST_DIRECTORY").expect("fixture directory"));
    let pending: Pending = read_json(&pending_path(&dir)).expect("fixture pending");
    if let Err(error) = apply(&dir, &pending) {
        fail(&dir, &pending, &error.to_string()).expect("fixture failure journal");
        std::process::exit(1);
    }
}

#[test]
#[ignore = "isolated subprocess fixture, launched by transaction tests"]
fn worker_exits_before_ready() {
    assert!(std::env::var_os("MB_UPDATE_TEST_DIRECTORY").is_some());
    std::process::exit(7);
}
