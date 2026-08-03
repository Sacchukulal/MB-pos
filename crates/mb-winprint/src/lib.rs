//! The Windows printing edge — **the only crate in Magic Bill that may say
//! `unsafe`** (decision D34).
//!
//! Two things P07 must do cannot be done in safe Rust, and both are at the
//! operating-system boundary:
//!
//! * enumerating and writing to Windows printers (`winspool.drv`);
//! * opening and configuring a serial port.
//!
//! The workspace sets `unsafe_code = "forbid"`, and `forbid` cannot be lifted
//! by a crate that would rather not obey it. That is the right rule and it
//! should stay. So the FFI lives here, in about three hundred lines, behind a
//! surface with no raw pointer, no handle and no `unsafe` in it — and mb-print,
//! mb-db and mb-core all keep `forbid`.
//!
//! # Why not PowerShell
//!
//! Audit **D2**: *"printer names are fetched by running a PowerShell command
//! each time."* Spawning a shell to answer "which printers exist" costs a
//! process, a runtime and roughly a second, on a 4 GB machine that is also
//! running the billing screen — and the settings screen asks more than once.
//! [`list_printers`] is one call into the spooler.
//!
//! # Why not `windows-sys`
//!
//! It would be a large compile for nine functions, and every call into it is
//! `unsafe` at the call site regardless — so it would buy declarations, not
//! safety. The declarations are below, each beside the documentation page it
//! came from.
//!
//! # Not Windows
//!
//! Every function still exists and returns [`WinPrintError::Unsupported`], so
//! the crates above compile and test on any platform. We ship Windows; the
//! tests should not need it.

#![deny(missing_debug_implementations)]

use std::fmt;
use std::io::Write;

#[cfg(windows)]
mod serial;
#[cfg(windows)]
mod spooler;
#[cfg(windows)]
mod sys;

/// What the operating system refused to do, in words a shopkeeper can act on.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WinPrintError {
    #[error("this build is not for Windows, so {0} is not available here")]
    Unsupported(&'static str),

    #[error("Windows does not know a printer called \"{name}\"")]
    NoSuchPrinter { name: String },

    #[error("the printer name \"{name}\" cannot be used — it contains a NUL")]
    BadName { name: String },

    /// Anything the API refused, with the Windows error number, because that
    /// number is what a support call can actually look up.
    #[error("{what} failed (Windows error {code})")]
    Api { what: &'static str, code: u32 },
}

/// One printer, as Windows sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrinterInfo {
    pub name: String,
    /// True when Windows has this marked as the machine's default.
    pub is_default: bool,
    /// True when it is a connection to a printer shared by another PC. Worth
    /// showing: a network share behaves differently from a local port when the
    /// other PC is off.
    pub is_network: bool,
}

/// Every printer Windows knows about, local and connected.
///
/// **The caller caches this.** It is a spooler round-trip, not a free lookup,
/// and audit D2's complaint was the "each time" rather than the call itself.
pub fn list_printers() -> Result<Vec<PrinterInfo>, WinPrintError> {
    #[cfg(windows)]
    {
        spooler::list()
    }
    #[cfg(not(windows))]
    {
        Err(WinPrintError::Unsupported("listing Windows printers"))
    }
}

/// The machine's default printer, if it has one.
pub fn default_printer() -> Result<Option<String>, WinPrintError> {
    #[cfg(windows)]
    {
        spooler::default_name()
    }
    #[cfg(not(windows))]
    {
        Err(WinPrintError::Unsupported("the default Windows printer"))
    }
}

/// Send bytes to a printer through the spooler, as **RAW**.
///
/// RAW means "these bytes are already the printer's own language" — the driver
/// passes them through instead of rendering a page. That is the only datatype
/// an ESC/POS stream can survive.
///
/// `document` is the job name a shop sees in the Windows print queue window, so
/// it should say something ("Magic Bill — kitchen ticket", not "Untitled").
///
/// **This call cannot be cancelled once it has started.** There is no safe
/// cancellation point in `WritePrinter`, which is why the queue above gives
/// each printer its own worker thread rather than promising a timeout it cannot
/// keep (P07 item 7.3).
pub fn write_raw(printer: &str, document: &str, bytes: &[u8]) -> Result<(), WinPrintError> {
    #[cfg(windows)]
    {
        spooler::write_raw(printer, document, bytes)
    }
    #[cfg(not(windows))]
    {
        let _ = (printer, document, bytes);
        Err(WinPrintError::Unsupported("the Windows print spooler"))
    }
}

/// Open a COM port for writing, configured 8-N-1 at `baud`.
///
/// Returns something that writes bytes and closes its handle when dropped —
/// including on an unwind, which is why the handle never appears in this
/// crate's public surface.
pub fn open_serial(port: &str, baud: u32) -> Result<Box<dyn Write + Send>, WinPrintError> {
    #[cfg(windows)]
    {
        serial::open(port, baud).map(|s| Box::new(s) as Box<dyn Write + Send>)
    }
    #[cfg(not(windows))]
    {
        let _ = (port, baud);
        Err(WinPrintError::Unsupported("serial ports"))
    }
}

/// True when this build can actually reach the two OS transports.
///
/// The queue asks before it offers a shop a spooler printer, so that a
/// non-Windows developer sees "not available on this platform" instead of a
/// printer that silently never prints.
#[must_use]
pub const fn available() -> bool {
    cfg!(windows)
}

impl fmt::Display for PrinterInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)?;
        if self.is_default {
            write!(f, " (default)")?;
        }
        Ok(())
    }
}
