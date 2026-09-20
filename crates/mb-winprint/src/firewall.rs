//! Windows Firewall, read in this process — and repaired by one elevated command.
//!
//! The read goes through the firewall's own COM object (`HNetCfg.FwPolicy2`), late-bound
//! through `IDispatch`, so no child process is started and no interface layout beyond
//! `IDispatch` and `IEnumVARIANT` is carried here. It takes well under a second on the
//! reference machine; the PowerShell read it replaced took eleven seconds on a fast one.
//!
//! Every call runs on a thread of its own with a COM apartment of its own, so it never
//! collides with whatever apartment the caller's thread is in.

use std::time::Duration;

use crate::sys;
use crate::{Elevation, FirewallReport, FirewallRule, WinPrintError};

/// `NET_FW_RULE_DIRECTION`: in.
const DIRECTION_IN: i32 = 1;
/// `NET_FW_ACTION`: allow (block is 0).
const ACTION_ALLOW: i32 = 1;

/// Every inbound rule the firewall holds for `program`, enabled or not, and which network
/// profiles this PC is on right now.
pub fn rules_for(program: &str) -> Result<FirewallReport, WinPrintError> {
    let program = program.to_owned();
    on_own_thread("reading Windows Firewall", move || read(&program))
}

/// Run `file` with `parameters` as administrator (one UAC prompt), hidden, and wait up to
/// `wait` for it to finish.
pub fn run_elevated(
    file: &str,
    parameters: &str,
    wait: Duration,
) -> Result<Elevation, WinPrintError> {
    let file = file.to_owned();
    let parameters = parameters.to_owned();
    on_own_thread("running an elevated command", move || {
        launch("runas", &file, &parameters, wait)
    })
}

/// COM on a thread that is ours alone, initialised and released around the work.
fn on_own_thread<T: Send + 'static>(
    what: &'static str,
    work: impl FnOnce() -> Result<T, WinPrintError> + Send + 'static,
) -> Result<T, WinPrintError> {
    let spawned = std::thread::Builder::new()
        .name("mb-firewall".to_owned())
        .spawn(move || {
            let _apartment = Apartment::enter()?;
            work()
        })
        .map_err(|_| WinPrintError::Api { what, code: 0 })?;
    spawned
        .join()
        .unwrap_or(Err(WinPrintError::Api { what, code: 0 }))
}

/// One COM apartment on the current thread, released when dropped.
struct Apartment;

impl Apartment {
    fn enter() -> Result<Self, WinPrintError> {
        // SAFETY: takes a null reserved pointer and a flag; paired with `CoUninitialize` in
        // `Drop` on this same thread.
        let hr =
            unsafe { sys::CoInitializeEx(std::ptr::null_mut(), sys::COINIT_APARTMENTTHREADED) };
        // A fresh thread has no apartment, so a changed-mode answer cannot happen — but it is
        // still an answer that means COM works, so it is not an error.
        if hr < 0 && hr != sys::RPC_E_CHANGED_MODE {
            return Err(api("CoInitializeEx", hr));
        }
        Ok(Apartment)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: matches the `CoInitializeEx` in `enter`, on the same thread.
        unsafe { sys::CoUninitialize() };
    }
}

/// An `HRESULT`, as the Windows error number a support call can look up.
fn api(what: &'static str, hr: sys::Hresult) -> WinPrintError {
    WinPrintError::Api {
        what,
        code: u32::from_ne_bytes(hr.to_ne_bytes()),
    }
}

// The read.

fn read(program: &str) -> Result<FirewallReport, WinPrintError> {
    let policy = Dispatch::create("HNetCfg.FwPolicy2")?;
    let current_profiles = policy
        .get("CurrentProfileTypes")?
        .as_i32()
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0);
    let rules = policy.get("Rules")?.into_dispatch("Rules")?;
    let mut found = Vec::new();
    let mut each = rules.enumerate()?;
    while let Some(item) = each.next() {
        let rule = item.into_dispatch("a firewall rule")?;
        let Some(path) = rule.get("ApplicationName")?.as_string() else {
            continue;
        };
        if !path.eq_ignore_ascii_case(program) {
            continue;
        }
        if rule.get("Direction")?.as_i32() != Some(DIRECTION_IN) {
            continue;
        }
        found.push(FirewallRule {
            name: rule.get("Name")?.as_string().unwrap_or_default(),
            enabled: rule.get("Enabled")?.as_bool().unwrap_or(false),
            allows: rule.get("Action")?.as_i32() == Some(ACTION_ALLOW),
            profiles: rule
                .get("Profiles")?
                .as_i32()
                .map_or(0, |n| u32::from_ne_bytes(n.to_ne_bytes())),
        });
    }
    Ok(FirewallReport {
        current_profiles,
        rules: found,
    })
}

/// One reference to an automation object, released when dropped.
struct Dispatch(sys::ComObject);

impl Dispatch {
    /// `CoCreateInstance` by ProgID, asking for `IDispatch`.
    fn create(progid: &str) -> Result<Self, WinPrintError> {
        let wide = sys::wide(progid).ok_or(WinPrintError::Api {
            what: "CLSIDFromProgID",
            code: 0,
        })?;
        let mut clsid = sys::IID_NULL;
        // SAFETY: `wide` is NUL-terminated and outlives the call; `clsid` is ours to fill.
        let hr = unsafe { sys::CLSIDFromProgID(wide.as_ptr(), &raw mut clsid) };
        if hr < 0 {
            return Err(api("CLSIDFromProgID", hr));
        }
        let mut object: sys::ComObject = std::ptr::null_mut();
        // SAFETY: every pointer is to a value this function owns; the interface asked for is
        // the one the vtable below is read as.
        let hr = unsafe {
            sys::CoCreateInstance(
                &raw const clsid,
                std::ptr::null_mut(),
                sys::CLSCTX_INPROC_SERVER,
                &sys::IID_IDISPATCH,
                &raw mut object,
            )
        };
        if hr < 0 || object.is_null() {
            return Err(api("CoCreateInstance", hr));
        }
        Ok(Dispatch(object))
    }

    fn vtable(&self) -> &sys::IDispatchVtbl {
        // SAFETY: `self.0` is a live `IDispatch`, whose first word is its vtable.
        unsafe { &**self.0.cast::<*const sys::IDispatchVtbl>() }
    }

    /// Read one property by name.
    fn get(&self, name: &str) -> Result<Value, WinPrintError> {
        let wide = sys::wide(name).ok_or(WinPrintError::Api {
            what: "GetIDsOfNames",
            code: 0,
        })?;
        let names = [wide.as_ptr()];
        let mut id = 0_i32;
        // SAFETY: one name, NUL-terminated, alive for the call; `id` is ours to fill.
        let hr = unsafe {
            (self.vtable().get_ids_of_names)(
                self.0,
                &sys::IID_NULL,
                names.as_ptr(),
                1,
                sys::LOCALE_USER_DEFAULT,
                &raw mut id,
            )
        };
        if hr < 0 {
            return Err(api("GetIDsOfNames", hr));
        }
        self.invoke(id, sys::DISPATCH_PROPERTYGET, "Invoke")
    }

    /// The collection's enumerator.
    fn enumerate(&self) -> Result<Enumerator, WinPrintError> {
        let value = self.invoke(
            sys::DISPID_NEWENUM,
            sys::DISPATCH_METHOD | sys::DISPATCH_PROPERTYGET,
            "_NewEnum",
        )?;
        let object = value.query(&sys::IID_IENUMVARIANT, "IEnumVARIANT")?;
        Ok(Enumerator(object))
    }

    fn invoke(&self, id: i32, flags: u16, what: &'static str) -> Result<Value, WinPrintError> {
        let mut params = sys::DispParams {
            args: std::ptr::null_mut(),
            named_ids: std::ptr::null_mut(),
            n_args: 0,
            n_named: 0,
        };
        let mut result = Value(sys::Variant::empty());
        // SAFETY: no arguments; `result` is an initialised VARIANT this function owns and
        // will clear; the exception and argument-error outputs are allowed to be null.
        let hr = unsafe {
            (self.vtable().invoke)(
                self.0,
                id,
                &sys::IID_NULL,
                sys::LOCALE_USER_DEFAULT,
                flags,
                &raw mut params,
                &raw mut result.0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if hr < 0 {
            return Err(api(what, hr));
        }
        Ok(result)
    }
}

impl Drop for Dispatch {
    fn drop(&mut self) {
        // SAFETY: one reference was taken when this was made, and this gives it back.
        unsafe { (self.vtable().base.release)(self.0) };
    }
}

/// One `IEnumVARIANT`, released when dropped.
struct Enumerator(sys::ComObject);

impl Enumerator {
    fn vtable(&self) -> &sys::IEnumVariantVtbl {
        // SAFETY: `self.0` is a live `IEnumVARIANT`, whose first word is its vtable.
        unsafe { &**self.0.cast::<*const sys::IEnumVariantVtbl>() }
    }

    /// The next item, or nothing at the end.
    fn next(&mut self) -> Option<Value> {
        let mut item = Value(sys::Variant::empty());
        let mut fetched = 0_u32;
        // SAFETY: asks for one item into a VARIANT this function owns.
        let hr = unsafe { (self.vtable().next)(self.0, 1, &raw mut item.0, &raw mut fetched) };
        (hr == sys::S_OK && fetched == 1).then_some(item)
    }
}

impl Drop for Enumerator {
    fn drop(&mut self) {
        // SAFETY: one reference was taken by `QueryInterface`, and this gives it back.
        unsafe { (self.vtable().base.release)(self.0) };
    }
}

/// A `VARIANT` this process owns, cleared when dropped.
struct Value(sys::Variant);

impl Value {
    /// The first pointer-sized word of the union.
    fn word(&self) -> usize {
        let mut bytes = [0_u8; size_of::<usize>()];
        bytes.copy_from_slice(&self.0.data[..size_of::<usize>()]);
        usize::from_ne_bytes(bytes)
    }

    fn as_i32(&self) -> Option<i32> {
        match self.0.vt {
            sys::VT_I4 => {
                let mut bytes = [0_u8; 4];
                bytes.copy_from_slice(&self.0.data[..4]);
                Some(i32::from_ne_bytes(bytes))
            }
            sys::VT_I2 => {
                let mut bytes = [0_u8; 2];
                bytes.copy_from_slice(&self.0.data[..2]);
                Some(i32::from(i16::from_ne_bytes(bytes)))
            }
            _ => None,
        }
    }

    /// `VARIANT_BOOL`: any non-zero is true (`VARIANT_TRUE` is -1).
    fn as_bool(&self) -> Option<bool> {
        (self.0.vt == sys::VT_BOOL).then(|| self.0.data[0] != 0 || self.0.data[1] != 0)
    }

    /// A `BSTR`, or nothing for null, empty and every other kind.
    fn as_string(&self) -> Option<String> {
        if self.0.vt != sys::VT_BSTR {
            return None;
        }
        let bstr = self.word() as *const u16;
        if bstr.is_null() {
            return None;
        }
        // SAFETY: a BSTR carries its length in front of the characters, and `SysStringLen`
        // reads exactly that.
        let len = unsafe { sys::SysStringLen(bstr) } as usize;
        // SAFETY: `len` characters are what the BSTR holds.
        let chars = unsafe { std::slice::from_raw_parts(bstr, len) };
        let text = String::from_utf16_lossy(chars);
        (!text.is_empty()).then_some(text)
    }

    /// The object inside, as `IDispatch`.
    fn into_dispatch(self, what: &'static str) -> Result<Dispatch, WinPrintError> {
        Ok(Dispatch(self.query(&sys::IID_IDISPATCH, what)?))
    }

    /// `QueryInterface` on the object inside (`VT_DISPATCH` or `VT_UNKNOWN`), which takes a
    /// reference of its own; this VARIANT is cleared as usual.
    fn query(&self, iid: &sys::Guid, what: &'static str) -> Result<sys::ComObject, WinPrintError> {
        if self.0.vt != sys::VT_DISPATCH && self.0.vt != sys::VT_UNKNOWN {
            return Err(WinPrintError::Api { what, code: 0 });
        }
        let object = self.word() as sys::ComObject;
        if object.is_null() {
            return Err(WinPrintError::Api { what, code: 0 });
        }
        // SAFETY: `object` is a live COM object (the VARIANT holds a reference to it), whose
        // first word is a vtable that begins with `IUnknown`.
        let unknown = unsafe { &**object.cast::<*const sys::IUnknownVtbl>() };
        let mut out: sys::ComObject = std::ptr::null_mut();
        // SAFETY: `out` is ours to fill.
        let hr = unsafe { (unknown.query_interface)(object, iid, &raw mut out) };
        if hr < 0 || out.is_null() {
            return Err(api(what, hr));
        }
        Ok(out)
    }
}

impl Drop for Value {
    fn drop(&mut self) {
        // SAFETY: the VARIANT was initialised empty and only ever filled by COM.
        unsafe { sys::VariantClear(&raw mut self.0) };
    }
}

// The repair.

/// `ShellExecuteEx` with `verb`, hidden, waiting up to `wait` for the process to end.
fn launch(
    verb: &str,
    file: &str,
    parameters: &str,
    wait: Duration,
) -> Result<Elevation, WinPrintError> {
    let bad = |what| WinPrintError::Api { what, code: 0 };
    let verb = sys::wide(verb).ok_or(bad("ShellExecuteExW"))?;
    let file = sys::wide(file).ok_or(bad("ShellExecuteExW"))?;
    let parameters = sys::wide(parameters).ok_or(bad("ShellExecuteExW"))?;
    let mut info = sys::ShellExecuteInfoW {
        size: u32::try_from(size_of::<sys::ShellExecuteInfoW>()).unwrap_or(0),
        mask: sys::SEE_MASK_NOCLOSEPROCESS | sys::SEE_MASK_NOASYNC | sys::SEE_MASK_FLAG_NO_UI,
        hwnd: std::ptr::null_mut(),
        verb: verb.as_ptr(),
        file: file.as_ptr(),
        parameters: parameters.as_ptr(),
        directory: std::ptr::null(),
        show: sys::SW_HIDE,
        inst_app: std::ptr::null_mut(),
        id_list: std::ptr::null_mut(),
        class: std::ptr::null(),
        hkey_class: std::ptr::null_mut(),
        hot_key: 0,
        icon: std::ptr::null_mut(),
        process: std::ptr::null_mut(),
    };
    // SAFETY: every string is NUL-terminated and outlives the call, and `info` is sized as
    // declared.
    let ok = unsafe { sys::ShellExecuteExW(&raw mut info) };
    if ok == sys::FALSE {
        // SAFETY: takes nothing.
        let code = unsafe { sys::GetLastError() };
        return if code == sys::ERROR_CANCELLED {
            Ok(Elevation::Refused)
        } else {
            Err(WinPrintError::Api {
                what: "ShellExecuteExW",
                code,
            })
        };
    }
    let process = info.process;
    if process.is_null() {
        // Nothing to wait on: the shell ran it without a process of its own.
        return Ok(Elevation::Finished { exit_code: 0 });
    }
    let millis = u32::try_from(wait.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: `process` is the handle the shell just gave us, closed below.
    let waited = unsafe { sys::WaitForSingleObject(process, millis) };
    let outcome = if waited == sys::WAIT_OBJECT_0 {
        let mut exit_code = 0_u32;
        // SAFETY: the process has ended; `exit_code` is ours to fill.
        let ok = unsafe { sys::GetExitCodeProcess(process, &raw mut exit_code) };
        if ok == sys::FALSE {
            // SAFETY: takes nothing.
            let code = unsafe { sys::GetLastError() };
            Err(WinPrintError::Api {
                what: "GetExitCodeProcess",
                code,
            })
        } else {
            Ok(Elevation::Finished { exit_code })
        }
    } else if waited == sys::WAIT_TIMEOUT {
        Ok(Elevation::StillRunning)
    } else {
        Err(WinPrintError::Api {
            what: "WaitForSingleObject",
            code: waited,
        })
    };
    // SAFETY: closes the one handle taken above, once.
    unsafe { sys::CloseHandle(process) };
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The COM read end to end, on whatever this test runs as: the point is that it answers,
    /// and answers quickly, not what it finds.
    #[test]
    fn reads_the_firewall_in_process() {
        let exe = std::env::current_exe().expect("this test's own path");
        let started = std::time::Instant::now();
        let report = rules_for(&exe.display().to_string()).expect("the firewall answers");
        assert!(report.current_profiles != 0, "Windows always has a profile on");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the whole point: {:?}",
            started.elapsed()
        );
    }

    /// The launch-wait-exit-code plumbing, through the same function the repair uses, with
    /// the one verb that does not put a UAC prompt on the screen.
    #[test]
    fn waits_for_the_process_and_reads_its_exit_code() {
        let exit = on_own_thread("test", || {
            launch("open", "cmd.exe", "/C exit 7", Duration::from_secs(30))
        });
        assert!(
            matches!(exit, Ok(Elevation::Finished { exit_code: 7 })),
            "{exit:?}"
        );
    }
}
