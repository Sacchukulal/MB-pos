//! What can go wrong on the road, and what a person is told about it.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LanError {
    /// The certificate or its key.
    #[error("this counter's network identity could not be created: {0}")]
    Identity(String),

    /// The port is taken, usually by a second copy of Magic Bill or by a development server.
    #[error("nothing could listen on port {port}: {why}")]
    Listen { port: u16, why: String },

    #[error("the network layer failed: {0}")]
    Io(String),

    /// The `Counter` implementation refused.
    #[error("{0}")]
    Counter(String),
}

impl From<std::io::Error> for LanError {
    fn from(e: std::io::Error) -> Self {
        LanError::Io(e.to_string())
    }
}
