//! Hold the exact process the updater must wait for, without polling a reusable PID.

use std::io;
use std::time::Duration;

use crate::sys;

/// A wait-only process handle. It cannot terminate or change the process.
#[derive(Debug)]
pub struct ProcessWait(sys::Handle);

/// Open a process for waiting. `None` means it has already exited; access errors are errors.
/// https://learn.microsoft.com/windows/win32/api/processthreadsapi/nf-processthreadsapi-openprocess
pub fn open_process(pid: u32) -> io::Result<Option<ProcessWait>> {
    if pid == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "A process ID is required",
        ));
    }
    // SAFETY: no pointers are passed; the returned owned handle is closed once by Drop.
    let handle = unsafe { sys::OpenProcess(sys::PROCESS_SYNCHRONIZE, sys::FALSE, pid) };
    if handle.is_null() {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == i32::try_from(sys::ERROR_INVALID_PARAMETER).ok() {
            return Ok(None);
        }
        return Err(error);
    }
    Ok(Some(ProcessWait(handle)))
}

impl ProcessWait {
    /// True only after this process exits. A timeout never grants permission to install.
    pub fn wait(&self, timeout: Duration) -> io::Result<bool> {
        // MAX is INFINITE in this API; even a very large caller timeout must stay finite.
        let millis = u32::try_from(timeout.as_millis())
            .unwrap_or(u32::MAX - 1)
            .min(u32::MAX - 1);
        // SAFETY: self owns a live SYNCHRONIZE handle for the duration of this call.
        match unsafe { sys::WaitForSingleObject(self.0, millis) } {
            sys::WAIT_OBJECT_0 => Ok(true),
            sys::WAIT_TIMEOUT => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

impl Drop for ProcessWait {
    fn drop(&mut self) {
        // SAFETY: this handle was opened above and is closed exactly once.
        unsafe { sys::CloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as _, Write as _};
    use std::os::windows::process::CommandExt as _;
    use std::process::{Command, Stdio};

    #[test]
    fn a_running_process_does_not_signal_completion() -> io::Result<()> {
        let process = open_process(std::process::id())?
            .ok_or_else(|| io::Error::other("the test process must exist"))?;
        assert!(!process.wait(Duration::ZERO)?);
        assert!(open_process(0).is_err());
        assert!(open_process(u32::MAX)?.is_none());
        Ok(())
    }

    #[test]
    fn waits_for_the_actual_child_to_exit() -> io::Result<()> {
        let mut child = Command::new(std::env::current_exe()?)
            .args(["--exact", "process::tests::child_wait_target", "--ignored"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()?;
        let process = open_process(child.id())?
            .ok_or_else(|| io::Error::other("the child must wait for input"))?;
        assert!(!process.wait(Duration::ZERO)?);
        if let Some(mut input) = child.stdin.take() {
            input.write_all(b"x")?;
        }
        let ended = process.wait(Duration::from_secs(10))?;
        if !ended {
            let _ = child.kill();
        }
        let result = child.wait()?;
        assert!(ended, "the handle must signal after the child exits");
        assert!(result.success());
        Ok(())
    }

    #[test]
    #[ignore = "child fixture, started by waits_for_the_actual_child_to_exit"]
    fn child_wait_target() -> io::Result<()> {
        std::io::stdin().read_exact(&mut [0_u8])
    }
}
