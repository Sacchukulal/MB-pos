//! **The road a phone's order travels, and the gate on it** — decision D9.
//!
//! # Why this crate exists
//!
//! In v1 every tap on a waiter's phone travelled to Mumbai and back, and the
//! server refused the order if the counter had not checked in within five
//! minutes. When the internet dropped — which happens weekly in an Indian
//! restaurant — the waiter simply could not take the order, while standing ten
//! feet from the till.
//!
//! From here on the phone talks to the **counter**, over the shop's own WiFi.
//! The cloud is a backup and the owner's remote window; it is never the road an
//! order travels. That closes ANDROID-A1/A3/A5 and BACKEND-F1, and it removes
//! the single largest cloud cost in the system.
//!
//! # What is in this crate, and what deliberately is not
//!
//! In: sockets, TLS, framing, authentication, rate limits, discovery, pairing.
//!
//! Not in: any business rule at all. This crate does not know what an order is,
//! what a table is, or what money is. Everything it can ask the shop arrives
//! through the [`Counter`](counter::Counter) trait, which `magic-bill`
//! implements — D1 and audit E3 applied to a network layer:
//!
//! > *"business rules live inside screen files… to answer 'what exactly
//! > happens when a bill is settled?' you must read four files at once."*
//!
//! That seam is what will let **P20 add the messages without touching a
//! socket**, and it is what lets every test here run with no database, no
//! Tauri and no shop.
//!
//! # The security model in one paragraph
//!
//! The shop's WiFi is not trustworthy: guests are on it, the password is on a
//! blackboard, and the router is whatever the cable company left behind. So the
//! counter generates its own certificate, the phone **pins its fingerprint at
//! pairing while a person is watching**, and every request afterwards carries a
//! credential the counter issued and stores only as an Argon2 hash. A stranger
//! on that WiFi can see the counter exists and can call `/v1/hello`. Everything
//! else needs a credential they do not have. The long version, including what
//! they *can* still do, is in [`server`]'s module note.

/// P27. **The other end of the wire** â the first client this crate has ever
/// had, because a second till is a client too (D136, D137).
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

pub use counter::{Counter, Device, DeviceRow, PairRequest, PairedDevice, Refusal};
pub use error::LanError;
pub use intent::{Batch, BatchResult, Catalogue, Intent, LineView, Outcome, What};
pub use identity::Identity;
pub use limit::{Limiter, Rate};
pub use pairing::{Desk, Waiting};
pub use client::{ClientError, Credential, Master};
pub use intent::{Forwarded, Receipt};
pub use server::{
    start_on,
    Missed, PROTOCOL_VERSION, Push, Running, Shared, TlsConfig, Where, require, router, start,
    upgrade_message,
};

/// The port the counter listens on by default.
///
/// Deliberately high and deliberately not 8080: a shop PC has a printer utility
/// or a bank's software on 8080 more often than not, and "the port is taken" on
/// first run is a support call for something that should just work. 7331 is in
/// the registered range but unassigned in practice, and it is configurable.
pub const DEFAULT_PORT: u16 = 7331;
