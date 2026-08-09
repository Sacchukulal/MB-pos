//! What can go wrong on the road, and what a person is told about it.
//!
//! **R3.** Every variant here is either something a shopkeeper can act on or
//! something that must reach the log. There is no variant meaning "something
//! went wrong", because that is the sentence v1 showed and it is the reason
//! nobody could tell a dead router from a revoked phone.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LanError {
    /// The certificate or its key. The counter still serves — see
    /// [`crate::identity::Identity::load_or_create`] — so this only escapes
    /// when nothing at all could be made.
    #[error("this counter's network identity could not be created: {0}")]
    Identity(String),

    /// The port is taken, usually by a second copy of Magic Bill or by a
    /// development server. The panel says which port and suggests another.
    #[error("nothing could listen on port {port}: {why}")]
    Listen { port: u16, why: String },

    #[error("the network layer failed: {0}")]
    Io(String),

    /// The `Counter` implementation refused. The string is already a sentence
    /// a person can read — mb-lan does not rewrite it, because the layer that
    /// knows what happened is the layer that should say so (D75).
    #[error("{0}")]
    Counter(String),
}

impl From<std::io::Error> for LanError {
    fn from(e: std::io::Error) -> Self {
        LanError::Io(e.to_string())
    }
}
