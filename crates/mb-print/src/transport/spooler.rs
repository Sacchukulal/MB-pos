//! The Windows spooler, RAW.

use std::sync::{Arc, Mutex, MutexGuard};

use crate::transport::{Discovered, Interrupt, Transport, TransportError};

#[derive(Debug, Clone)]
pub struct SpoolerTransport {
    name: String,
    /// The Windows job the send in flight opened, while it is open — the handle by which a
    /// write that never returns is stopped from another thread.
    job: Arc<Mutex<Option<u32>>>,
}

impl SpoolerTransport {
    #[must_use]
    pub fn new(name: String) -> SpoolerTransport {
        SpoolerTransport {
            name,
            job: Arc::new(Mutex::new(None)),
        }
    }
}

impl Transport for SpoolerTransport {
    fn send(&mut self, bytes: &[u8], document: &str) -> Result<(), TransportError> {
        #[cfg(windows)]
        {
            use mb_winprint::WinPrintError;
            // A printer Windows already knows is out of paper or offline is not written to: the
            // write would only hang, and the words are what a person needs to see.
            if let Ok(Some(trouble)) = mb_winprint::printer_trouble(&self.name) {
                return Err(TransportError::Write {
                    target: self.name.clone(),
                    reason: trouble,
                });
            }
            let job = Arc::clone(&self.job);
            let sent = mb_winprint::write_raw_reporting(&self.name, document, bytes, move |id| {
                *lock(&job) = Some(id);
            });
            *lock(&self.job) = None;
            sent.map_err(|e| match e {
                // A printer somebody has renamed or removed in Windows will not come back by
                // being asked again, so this parks at once rather than retrying five times to
                // be sure.
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

    fn interrupter(&self) -> Option<Arc<dyn Interrupt>> {
        Some(Arc::new(SpoolerInterrupt {
            name: self.name.clone(),
            job: Arc::clone(&self.job),
        }))
    }

    fn purge_stale(&self, document_prefix: &str) -> usize {
        #[cfg(windows)]
        {
            mb_winprint::purge_jobs(&self.name, document_prefix).unwrap_or(0)
        }
        #[cfg(not(windows))]
        {
            let _ = document_prefix;
            0
        }
    }
}

/// Deletes the Windows job a blocked write is sitting in, which makes the write return.
#[derive(Debug)]
struct SpoolerInterrupt {
    name: String,
    job: Arc<Mutex<Option<u32>>>,
}

impl Interrupt for SpoolerInterrupt {
    fn interrupt(&self) {
        let job = *lock(&self.job);
        #[cfg(windows)]
        if let Some(id) = job {
            // A job that is already gone is the outcome wanted; nothing to report either way.
            let _ = mb_winprint::cancel_job(&self.name, id);
        }
        #[cfg(not(windows))]
        let _ = (job, &self.name);
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Ask the operating system which printers exist.
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
        // Not an error: a developer on another platform has no Windows printers and that is the
        // correct answer, not a failure to be handled.
        Ok(Vec::new())
    }
}
