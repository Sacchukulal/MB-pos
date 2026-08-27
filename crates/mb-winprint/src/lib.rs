//! The Windows printing edge — the only crate in Magic Bill that may say `unsafe`.

#![deny(missing_debug_implementations)]

use std::fmt;
use std::io::Write;

#[cfg(windows)]
pub mod serial;
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

    /// Anything the API refused, with the Windows error number, because that number is what a
    /// support call can actually look up.
    #[error("{what} failed (Windows error {code})")]
    Api { what: &'static str, code: u32 },
}

/// One printer, as Windows sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrinterInfo {
    pub name: String,
    /// True when Windows has this marked as the machine's default.
    pub is_default: bool,
    /// True when it is a connection to a printer shared by another PC.
    pub is_network: bool,
}

/// Every printer Windows knows about, local and connected.
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

/// Send bytes to a printer through the spooler, as RAW.
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

/// A serial port a caller can also READ from.
#[cfg(windows)]
pub fn open_serial_duplex(port: &str, baud: u32) -> Result<serial::SerialPort, WinPrintError> {
    serial::open(port, baud)
}

/// True when this build can actually reach the two OS transports.
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
