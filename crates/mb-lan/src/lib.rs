//! The road a phone's order travels, and the gate on it.

/// The other end of the wire â the first client this crate has ever had, because a second
/// till is a client too.
pub mod client;
pub mod counter;
pub mod discovery;
pub mod error;
pub mod identity;
pub mod intent;
pub mod limit;
pub mod pairing;
pub mod qr;
pub mod server;

pub use client::{ClientError, Credential, Master};
pub use counter::{
    ClaimRequest, Counter, Device, DeviceRow, PairRequest, PairedDevice, Person, Refusal,
};
pub use error::LanError;
pub use identity::Identity;
pub use intent::{Batch, BatchResult, Catalogue, Intent, LineView, Outcome, What};
pub use intent::{Forwarded, Receipt};
pub use limit::{Limiter, Rate};
pub use pairing::{Desk, Waiting};
pub use server::{
    Missed, PROTOCOL_VERSION, Push, Running, Shared, TlsConfig, Where, require, router, start,
    start_on, upgrade_message,
};

/// The port the counter listens on by default.
pub const DEFAULT_PORT: u16 = 7331;
