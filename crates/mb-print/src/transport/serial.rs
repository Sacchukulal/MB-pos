//! A COM port.

use crate::transport::{Transport, TransportError};

#[derive(Debug, Clone)]
pub struct SerialTransport {
    port: String,
    baud: u32,
}

impl SerialTransport {
    #[must_use]
    pub fn new(port: String, baud: u32) -> SerialTransport {
        SerialTransport { port, baud }
    }
}

impl Transport for SerialTransport {
    fn send(&mut self, bytes: &[u8], _document: &str) -> Result<(), TransportError> {
        #[cfg(windows)]
        {
            use std::io::Write;

            // Opened per job and closed after it.
            let mut port = mb_winprint::open_serial(&self.port, self.baud).map_err(|e| {
                TransportError::Connect {
                    target: self.port.clone(),
                    reason: e.to_string(),
                }
            })?;
            port.write_all(bytes).map_err(|e| TransportError::Write {
                target: self.port.clone(),
                reason: e.to_string(),
            })?;
            port.flush().map_err(|e| TransportError::Write {
                target: self.port.clone(),
                reason: e.to_string(),
            })
        }
        #[cfg(not(windows))]
        {
            let _ = bytes;
            Err(TransportError::Unavailable {
                what: format!("the serial port {}", self.port),
            })
        }
    }

    fn describe(&self) -> String {
        format!("serial {} at {} baud", self.port, self.baud)
    }
}
