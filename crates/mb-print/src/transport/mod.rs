//! Getting bytes to a printer — **audit D2, which is the largest hardware gap
//! in the product.**
//!
//! > *"Windows printers only, and only through the print spooler… There is no
//! > direct network/IP printer support, no USB/serial, no Bluetooth, no
//! > cash-drawer kick."*
//!
//! Four ways out, behind one trait:
//!
//! | | scope | |
//! |---|---|---|
//! | [`spooler`] | 7.1 | the Windows spooler, RAW — what almost every shop runs |
//! | [`network`] | 7.2 | raw TCP to port 9100 — a kitchen printer with no PC |
//! | [`serial`] | 7.3 | a COM port |
//! | [`file`] | — | a file: testing, and "print to file" |
//!
//! Plus [`NullTransport`], which is not a placeholder: a shop whose printer has
//! not arrived must still be able to bill (requirement 3 of the ten), and a
//! queue with nowhere to send would have to grow a special case for it.
//!
//! # Every test in this crate uses the file transport
//!
//! That is deliberate, and it is the answer to "how much of P07 can be proved
//! without hardware". The queue, the retries, the parking, the parallelism, the
//! drawer rules, the routing and the bytes are all provable on any machine. The
//! two OS transports are the only part that needs the owner's TVSE.

pub mod file;
pub mod network;
pub mod serial;
pub mod spooler;

use std::fmt;
use std::sync::Mutex;
use std::time::Duration;

use crate::printer::Target;

/// What went wrong on the way to the paper.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    /// This build cannot reach that kind of printer at all — a spooler printer
    /// on a machine that is not Windows.
    #[error("{what} is not available on this platform")]
    Unavailable { what: String },

    /// Could not get to the printer. **Transient**: the kitchen printer is
    /// switched off, and it will be switched on again.
    #[error("could not reach {target}: {reason}")]
    Connect { target: String, reason: String },

    /// Got there and the write failed. Also transient — out of paper, cover
    /// open, cable knocked.
    #[error("{target} accepted the connection but not the print: {reason}")]
    Write { target: String, reason: String },

    /// Something no amount of retrying will fix: a printer name that does not
    /// exist, a path that cannot be written. **Permanent**, so the queue parks
    /// it at once rather than trying five times to be sure.
    #[error("{target} cannot be printed to: {reason}")]
    Refused { target: String, reason: String },
}

impl TransportError {
    /// Whether retrying could ever help.
    ///
    /// Retrying a permanent failure five times with backoff is fifteen seconds
    /// of pretending, and it delays the moment the cashier finds out.
    #[must_use]
    pub const fn is_permanent(&self) -> bool {
        matches!(
            self,
            TransportError::Refused { .. } | TransportError::Unavailable { .. }
        )
    }
}

/// Somewhere bytes can go.
///
/// `send` returns when the bytes have been **handed over**, not when paper has
/// moved: a one-way ESC/POS link cannot tell us the second thing, and a trait
/// that implied otherwise would be a promise the hardware does not make.
pub trait Transport: Send + fmt::Debug {
    /// `document` is the job name a shop sees in the Windows print queue, so it
    /// should say something.
    fn send(&mut self, bytes: &[u8], document: &str) -> Result<(), TransportError>;

    /// Where this goes, in words, for the status stream.
    fn describe(&self) -> String;
}

/// Open a transport for a target.
///
/// Opened per job and closed after it, deliberately — see [`network`] for why a
/// held-open socket to an idle kitchen printer is a trap this product has
/// already fallen into once, on the cloud side.
pub fn open(target: &Target, timeout: Duration) -> Result<Box<dyn Transport>, TransportError> {
    match target {
        Target::File { path } => Ok(Box::new(file::FileTransport::new(path.clone()))),
        Target::Network { host, port } => Ok(Box::new(network::NetworkTransport::new(
            host.clone(),
            *port,
            timeout,
        ))),
        Target::Serial { port, baud } => {
            Ok(Box::new(serial::SerialTransport::new(port.clone(), *baud)))
        }
        Target::Spooler { name } => Ok(Box::new(spooler::SpoolerTransport::new(name.clone()))),
        Target::None => Ok(Box::new(NullTransport)),
    }
}

/// How the queue gets a transport.
///
/// A trait rather than a call to [`open`] so that a test can hand the queue a
/// printer that never answers — which is the only way to prove audit D3, and
/// proving D3 is what this session is for. P08 gets the same seam if a terminal
/// ever spools to another terminal (scope 11.1).
pub trait TransportFactory: Send + Sync + fmt::Debug {
    fn open(&self, target: &Target, timeout: Duration)
    -> Result<Box<dyn Transport>, TransportError>;
}

/// The real four.
#[derive(Debug, Default, Clone, Copy)]
pub struct RealTransports;

impl TransportFactory for RealTransports {
    fn open(
        &self,
        target: &Target,
        timeout: Duration,
    ) -> Result<Box<dyn Transport>, TransportError> {
        open(target, timeout)
    }
}

/// Accepts everything, prints nothing.
#[derive(Debug, Clone, Copy)]
pub struct NullTransport;

impl Transport for NullTransport {
    fn send(&mut self, _bytes: &[u8], _document: &str) -> Result<(), TransportError> {
        Ok(())
    }

    fn describe(&self) -> String {
        "no printer".to_owned()
    }
}

// ---------------------------------------------------------------------------
// Discovery, and the cache that is audit D2's other half.
// ---------------------------------------------------------------------------

/// A printer the operating system knows about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    pub name: String,
    pub is_default: bool,
    pub is_network: bool,
}

/// The list of Windows printers, **cached**.
///
/// > Audit D2: *"printer names are fetched by running a PowerShell command
/// > **each time**."*
///
/// The fix is two things and both matter: the list comes from the spooler API
/// rather than from a shell, and it is asked for once rather than on every
/// repaint of the settings screen. [`Discovery::refresh`] is the explicit
/// reload, because a shop that has just plugged a printer in must be able to
/// make it appear without restarting the counter.
#[derive(Debug, Default)]
pub struct Discovery {
    cached: Mutex<Option<Vec<Discovered>>>,
}

impl Discovery {
    #[must_use]
    pub fn new() -> Discovery {
        Discovery::default()
    }

    /// What the OS knows, from the cache if it has been asked before.
    pub fn list(&self) -> Result<Vec<Discovered>, TransportError> {
        {
            let cached = lock(&self.cached);
            if let Some(found) = cached.as_ref() {
                return Ok(found.clone());
            }
        }
        let fresh = spooler::enumerate()?;
        *lock(&self.cached) = Some(fresh.clone());
        Ok(fresh)
    }

    /// Forget the cache. The next [`list`](Discovery::list) asks the OS again.
    pub fn refresh(&self) {
        *lock(&self.cached) = None;
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shop_with_no_printer_can_still_print() {
        let mut null = NullTransport;
        assert!(null.send(b"anything", "bill").is_ok());
    }

    #[test]
    fn a_permanent_failure_is_told_apart_from_a_transient_one() {
        let off = TransportError::Connect {
            target: "192.168.1.50:9100".to_owned(),
            reason: "no route".to_owned(),
        };
        let gone = TransportError::Refused {
            target: "Kitchen".to_owned(),
            reason: "no such printer".to_owned(),
        };
        assert!(!off.is_permanent(), "a printer that is off comes back on");
        assert!(gone.is_permanent(), "a printer that does not exist will not");
    }
}
