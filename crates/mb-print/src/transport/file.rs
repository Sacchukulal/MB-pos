//! A file.
//!
//! Two jobs: "print to file", which a shop occasionally wants, and **every test
//! in this crate**. The queue, its retries, its parking, its parallelism and
//! its bytes are all provable against this, on a machine with no printer — and
//! that is the difference between a session that can be verified and one that
//! has to be believed.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use crate::transport::{Transport, TransportError};

#[derive(Debug, Clone)]
pub struct FileTransport {
    path: PathBuf,
}

impl FileTransport {
    #[must_use]
    pub fn new(path: PathBuf) -> FileTransport {
        FileTransport { path }
    }
}

impl Transport for FileTransport {
    fn send(&mut self, bytes: &[u8], _document: &str) -> Result<(), TransportError> {
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            return Err(TransportError::Refused {
                target: self.path.display().to_string(),
                reason: e.to_string(),
            });
        }

        // Appended, so a second job does not erase the first — a file target is
        // a paper roll, and a roll does not rewind.
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| TransportError::Refused {
                target: self.path.display().to_string(),
                reason: e.to_string(),
            })?;

        file.write_all(bytes).map_err(|e| TransportError::Write {
            target: self.path.display().to_string(),
            reason: e.to_string(),
        })?;
        // Flushed rather than left to the drop, because a job that reports
        // success and then loses its bytes in a buffer is the same lie the whole
        // queue exists to stop.
        file.flush().map_err(|e| TransportError::Write {
            target: self.path.display().to_string(),
            reason: e.to_string(),
        })
    }

    fn describe(&self) -> String {
        format!("file {}", self.path.display())
    }
}
