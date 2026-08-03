//! The Windows spooler, RAW — scope 7.1.
//!
//! What almost every shop runs today, and the only thing v1 could do. All the
//! `unsafe` is in `mb-winprint` (D34); this file is the safe half.

use crate::transport::{Discovered, Transport, TransportError};

#[derive(Debug, Clone)]
pub struct SpoolerTransport {
    name: String,
}

impl SpoolerTransport {
    #[must_use]
    pub fn new(name: String) -> SpoolerTransport {
        SpoolerTransport { name }
    }
}

impl Transport for SpoolerTransport {
    fn send(&mut self, bytes: &[u8], document: &str) -> Result<(), TransportError> {
        #[cfg(windows)]
        {
            use mb_winprint::WinPrintError;
            mb_winprint::write_raw(&self.name, document, bytes).map_err(|e| match e {
                // A printer somebody has renamed or removed in Windows will not
                // come back by being asked again, so this parks at once rather
                // than retrying five times to be sure.
                WinPrintError::NoSuchPrinter { .. } | WinPrintError::BadName { .. } => {
                    TransportError::Refused {
                        target: self.name.clone(),
                        reason: e.to_string(),
                    }
                }
                other => TransportError::Write {
                    target: self.name.clone(),
                    reason: other.to_string(),
                },
            })
        }
        #[cfg(not(windows))]
        {
            let _ = (bytes, document);
            Err(TransportError::Unavailable {
                what: format!("the Windows spooler printer \"{}\"", self.name),
            })
        }
    }

    fn describe(&self) -> String {
        format!("spooler \"{}\"", self.name)
    }
}

/// Ask the operating system which printers exist.
///
/// **Not a PowerShell process** — audit D2, and the reason [`super::Discovery`]
/// caches the answer.
pub fn enumerate() -> Result<Vec<Discovered>, TransportError> {
    #[cfg(windows)]
    {
        let found = mb_winprint::list_printers().map_err(|e| TransportError::Refused {
            target: "Windows".to_owned(),
            reason: e.to_string(),
        })?;
        Ok(found
            .into_iter()
            .map(|p| Discovered {
                name: p.name,
                is_default: p.is_default,
                is_network: p.is_network,
            })
            .collect())
    }
    #[cfg(not(windows))]
    {
        // Not an error: a developer on another platform has no Windows printers
        // and that is the correct answer, not a failure to be handled.
        Ok(Vec::new())
    }
}
