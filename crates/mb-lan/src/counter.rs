//! **The seam.** Everything this crate knows about the shop arrives through
//! here, and it is a trait.
//!
//! mb-lan owns sockets, TLS, framing, authentication and rate limits. It owns
//! **no business rule**: it does not know what an order is, what a table is, or
//! what money is. That is D1 and audit E3 applied to a network layer —
//! *"business rules live inside screen files… to answer 'what exactly happens
//! when a bill is settled?' you must read four files at once"* — and it is what
//! will let P20 add messages without touching a socket.
//!
//! It also buys the tests. Every one of T1–T12 runs against a fake `Counter`
//! with no database, no Tauri and no shop, which is why they take milliseconds
//! and why they can simulate a revocation landing between two requests.

use serde::{Deserialize, Serialize};

/// What a phone says about itself when it asks to be let in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairRequest {
    /// What the phone calls itself. Shown to the person who approves it.
    pub name: String,
    /// `android`, `ios`, `web`.
    pub platform: String,
    /// The short-lived token from the QR or the counter's screen.
    pub token: String,
}

/// What the counter hands back when a person approves it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairedDevice {
    pub device_id: String,
    /// **The one moment this exists in plain text.** The phone stores it; the
    /// counter stored only its Argon2 hash.
    pub secret: String,
    /// So the phone can recognise this counter again after a DHCP move — the
    /// credential is bound to the SERVER, never to an address.
    pub server_id: String,
}

/// Who is on the other end of an authenticated request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// **Nullable, and that is the point.** A shared tablet at the pass belongs
    /// to no one person; each ACTION on it still names the staff member who did
    /// it, which is how a shop keeps audit C1's promise on a device four people
    /// touch in an hour.
    pub staff_id: Option<String>,
    pub permissions: mb_auth::PermissionSet,
}

/// Why a request was refused, in the words the phone shows.
///
/// **Not a code and not a tag** (crown jewel 14, audit F8). Every variant
/// carries the sentence, because the layer that knows what happened is the
/// layer that should say it — and a phone that turns a number into its own
/// guess at a sentence is a second product with a second vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// No credential, or one that does not verify. Deliberately one variant
    /// for both: telling them apart is a way to enumerate the shop's devices.
    NotPaired,
    /// The device was revoked. Said plainly, because the waiter needs to go and
    /// ask rather than reinstall the app.
    Revoked,
    /// The staff member on this device may not do this.
    NotAllowed(String),
    /// The shop is at its device limit. The sentence has the number in it.
    TooManyDevices(String),
    /// The token was wrong, used, or has expired.
    BadToken,
    /// A person has not approved it yet. Not an error — the phone waits.
    WaitingForApproval,
    /// Anything the counter itself refused, already written as a sentence.
    Refused(String),
}

impl Refusal {
    /// What the phone shows. One sentence, in a shopkeeper's words.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Refusal::NotPaired => {
                "This phone is not connected to the counter. Ask somebody at \
                 the till to add it."
                    .to_owned()
            }
            Refusal::Revoked => {
                "This phone has been removed from the counter. Ask somebody at \
                 the till to add it again."
                    .to_owned()
            }
            Refusal::NotAllowed(what) => {
                format!("You do not have permission to {what}. Ask somebody who can.")
            }
            Refusal::TooManyDevices(sentence) | Refusal::Refused(sentence) => sentence.clone(),
            Refusal::BadToken => {
                "That code has expired or has already been used. Ask the \
                 counter to show a new one."
                    .to_owned()
            }
            Refusal::WaitingForApproval => {
                "Waiting for somebody at the counter to allow this phone.".to_owned()
            }
        }
    }

    /// The HTTP status. `403` for everything a person could fix by asking
    /// somebody, `429` is the rate limiter's and lives elsewhere.
    #[must_use]
    pub const fn status(&self) -> u16 {
        match self {
            // 401 and not 403: the phone has no credential the counter accepts,
            // which is what 401 means. A phone that sees 401 clears its stored
            // credential and asks to be paired again; one that sees 403 does
            // not, and 403 is right for "you, specifically, may not".
            Refusal::NotPaired | Refusal::Revoked => 401,
            Refusal::WaitingForApproval => 202,
            Refusal::BadToken => 400,
            Refusal::NotAllowed(_) | Refusal::TooManyDevices(_) | Refusal::Refused(_) => 403,
        }
    }
}

/// **What mb-lan may ask the shop.** Nothing else, ever.
///
/// Every method returns owned data and takes `&self`. That is not politeness:
/// it is the rule that keeps a slow phone from touching the billing path. An
/// implementation holds whatever lock it needs for the length of one short
/// call and hands back a value — it never lends mb-lan a guard that a
/// half-connected socket could then hold across an await.
pub trait Counter: Send + Sync + 'static {
    /// The shop's name, for `/v1/hello` and for the phone's own screen.
    fn shop_name(&self) -> String;

    /// How many phones this shop's licence allows. P21 owns licensing; until
    /// then an implementation answers with the plan's number or a default, and
    /// mb-lan only enforces it.
    fn device_limit(&self) -> u32;

    /// Every device that is not revoked.
    fn devices(&self) -> Vec<DeviceRow>;

    /// Find a live device and check its credential.
    ///
    /// **This is read on EVERY request**, and that is deliberate: revocation
    /// has to bite on the next request, not on the next login (T3). An
    /// implementation may cache, but the cache is invalidated by the revoke
    /// itself.
    fn authenticate(&self, device_id: &str, secret: &str) -> Option<Device>;

    /// Record that a device was heard from, for the panel's "last seen".
    ///
    /// Deliberately fire-and-forget and deliberately allowed to be cheap or
    /// batched: a write on every request would put the WRITER connection on
    /// the path of every phone poll, which is exactly what must never happen.
    fn seen(&self, device_id: &str, ip: &str);

    /// Store an approved device. Returns its credential, once.
    ///
    /// # Errors
    ///
    /// A sentence, when the counter refuses.
    fn pair(&self, request: &PairRequest, name: &str, platform: &str)
    -> Result<PairedDevice, Refusal>;

    // -----------------------------------------------------------------------
    // P20. What a phone actually came here to do.
    //
    // Still no business rule in this crate: these hand the message over and
    // take an answer back. Every decision — the numbers, the money, what the
    // kitchen has been told, every conflict — is made on the other side of
    // this trait, which is what lets P20's tests run against a real database
    // and mb-lan's run against no database at all.
    // -----------------------------------------------------------------------

    /// Apply one intent. **Idempotent by its `id`** — see `LAN_PROTOCOL.md` §6.
    fn apply(&self, device: &Device, intent: &crate::intent::Intent) -> crate::intent::Outcome;

    /// Apply a queued batch, in order.
    fn apply_batch(
        &self,
        device: &Device,
        batch: &crate::intent::Batch,
    ) -> crate::intent::BatchResult;

    /// **P27 — take a settled bill from another till** (D136).
    ///
    /// A fact, not a request: the money and the number were decided by the
    /// sender, from a series only it issues, so there is nothing here to
    /// resolve. Idempotent on each order's id — a repeat is a success, which is
    /// what lets a secondary retry for ever without keeping track.
    ///
    /// Still no business rule in this crate: this hands the message over.
    fn receive(
        &self,
        device: &Device,
        forwarded: &crate::intent::Forwarded,
    ) -> crate::intent::Receipt;

    /// The menu and the floor, with a version.
    ///
    /// `held` is the version the phone already has; `None` means it has none.
    /// Returning `None` means **unchanged** — which is the whole reason the
    /// version exists.
    fn catalogue(&self, held: Option<&str>) -> Option<crate::intent::Catalogue>;
}

/// One row of the panel's device list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceRow {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub staff: Option<String>,
    /// Already written: "2 minutes ago", "never". R8 — the panel does no date
    /// arithmetic.
    pub last_seen: String,
    pub last_ip: String,
}
