//! Raw TCP to port 9100.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::transport::{Transport, TransportError};

/// Connect, send, close. No pooling, and that is the decision here.
#[derive(Debug, Clone)]
pub struct NetworkTransport {
    host: String,
    port: u16,
    timeout: Duration,
}

impl NetworkTransport {
    #[must_use]
    pub fn new(host: String, port: u16, timeout: Duration) -> NetworkTransport {
        NetworkTransport {
            host,
            port,
            timeout,
        }
    }

    fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

impl Transport for NetworkTransport {
    fn send(&mut self, bytes: &[u8], _document: &str) -> Result<(), TransportError> {
        let address = self.address();

        // Resolved first, and separately, so "there is no such name" reads differently from "it
        // did not answer".
        let mut resolved = address
            .to_socket_addrs()
            .map_err(|e| TransportError::Refused {
                target: address.clone(),
                reason: format!("that address cannot be understood: {e}"),
            })?;
        let Some(socket) = resolved.next() else {
            return Err(TransportError::Refused {
                target: address.clone(),
                reason: "that address resolves to nothing".to_owned(),
            });
        };

        // Connect is where a dead printer hangs.
        let mut stream = TcpStream::connect_timeout(&socket, self.timeout).map_err(|e| {
            TransportError::Connect {
                target: address.clone(),
                reason: e.to_string(),
            }
        })?;
        stream
            .set_write_timeout(Some(self.timeout))
            .map_err(|e| TransportError::Connect {
                target: address.clone(),
                reason: e.to_string(),
            })?;
        // Nagle off: a receipt is one burst of a few kilobytes and then silence, and waiting
        // 200 ms to coalesce a packet that has nothing to coalesce with is 200 ms of the
        // kitchen waiting.
        let _ = stream.set_nodelay(true);

        stream.write_all(bytes).map_err(|e| TransportError::Write {
            target: address.clone(),
            reason: e.to_string(),
        })?;
        stream.flush().map_err(|e| TransportError::Write {
            target: address,
            reason: e.to_string(),
        })
    }

    fn describe(&self) -> String {
        format!("network {}", self.address())
    }
}
