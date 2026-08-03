//! Raw TCP to port 9100 — **scope 7.2, the biggest single hardware gap in v1.**
//!
//! A kitchen printer on the shop's own switch, with no PC in front of it, is
//! how every kitchen in the market is wired and how none of v1's were. The
//! protocol is not a protocol: open a socket, write the ESC/POS, close it.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::transport::{Transport, TransportError};

/// Connect, send, close. **No pooling, and that is the decision here.**
///
/// A kitchen printer is idle for minutes at a time, and a socket held open
/// across that idle is a socket that is silently dead when it matters. This
/// product has already paid for that lesson once, on the cloud side — crown
/// jewel 6: *"a sleeping PC leaves half-dead sockets that never fail, they just
/// never answer; this deadlocked the entire cloud side of the counter."* A
/// three-millisecond reconnect per ticket is a bargain against finding that out
/// again in a kitchen.
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

        // Resolved first, and separately, so "there is no such name" reads
        // differently from "it did not answer". A shop that has typed the IP
        // wrongly should be told that, not told to wait.
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

        // **Connect is where a dead printer hangs.** Windows' own connect
        // timeout is over twenty seconds, which is longer than audit D3's
        // fifteen — the very number that made a rush lose an order. This is one
        // of the timeouts that can genuinely be enforced (P07 item 7.3), so it
        // is.
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
        // Nagle off: a receipt is one burst of a few kilobytes and then silence,
        // and waiting 200 ms to coalesce a packet that has nothing to coalesce
        // with is 200 ms of the kitchen waiting.
        let _ = stream.set_nodelay(true);

        stream
            .write_all(bytes)
            .map_err(|e| TransportError::Write {
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
