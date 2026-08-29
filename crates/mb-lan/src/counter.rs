//! The seam. Everything this crate knows about the shop arrives through here, and it is a
//! trait.

use serde::{Deserialize, Serialize};

/// What a phone says about itself when it asks to be let in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairRequest {
    /// What the phone calls itself.
    pub name: String,
    /// `android`, `ios`, `web`.
    pub platform: String,
    /// The short-lived token from the QR or the counter's screen.
    pub token: String,
    /// A stable id for this install of the app, so a phone that pairs again takes its OWN seat
    /// back instead of a second one. Absent from an old app: then every pairing is a new phone.
    #[serde(default)]
    pub install: Option<String>,
}

/// Somebody a phone can be given to. Names only — the shop's WiFi already carries them on
/// every kitchen ticket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Person {
    pub id: String,
    pub name: String,
}

/// What the counter hands back when a person approves it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairedDevice {
    pub device_id: String,
    /// The one moment this exists in plain text.
    pub secret: String,
    /// So the phone can recognise this counter again after a DHCP move — the credential is
    /// bound to the SERVER, never to an address.
    pub server_id: String,
}

/// Who is on the other end of an authenticated request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// Nullable, and that is the point.
    pub staff_id: Option<String>,
    /// The person's name, for the phone's own screen. None for a shared tablet.
    pub staff_name: Option<String>,
    pub permissions: mb_auth::PermissionSet,
}

/// Why a request was refused, in the words the phone shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// No credential, or one that does not verify.
    NotPaired,
    /// The device was revoked.
    Revoked,
    /// The staff member on this device may not do this.
    NotAllowed(String),
    /// The shop is at its device limit.
    TooManyDevices(String),
    /// The token was wrong, used, or has expired.
    BadToken,
    /// A person has not approved it yet.
    WaitingForApproval,
    /// Anything the counter itself refused, already written as a sentence.
    Refused(String),
}

impl Refusal {
    /// What the phone shows.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Refusal::NotPaired => "This phone is not connected to the counter. Ask somebody at \
                 the till to add it."
                .to_owned(),
            Refusal::Revoked => "This phone has been removed from the counter. Ask somebody at \
                 the till to add it again."
                .to_owned(),
            Refusal::NotAllowed(what) => {
                format!("You do not have permission to {what}. Ask somebody who can.")
            }
            Refusal::TooManyDevices(sentence) | Refusal::Refused(sentence) => sentence.clone(),
            Refusal::BadToken => "That is not the code on the counter's screen now — it may \
                 have expired or been used. Check it, or scan it."
                .to_owned(),
            Refusal::WaitingForApproval => {
                "Waiting for somebody at the counter to allow this phone.".to_owned()
            }
        }
    }

    /// The HTTP status. `403` for everything a person could fix by asking somebody, `429` is
    /// the rate limiter's and lives elsewhere.
    #[must_use]
    pub const fn status(&self) -> u16 {
        match self {
            // 401 and not 403: the phone has no credential the counter accepts, which is what
            // 401 means.
            Refusal::NotPaired | Refusal::Revoked => 401,
            Refusal::WaitingForApproval => 202,
            Refusal::BadToken => 400,
            Refusal::NotAllowed(_) | Refusal::TooManyDevices(_) | Refusal::Refused(_) => 403,
        }
    }
}

/// What mb-lan may ask the shop.
pub trait Counter: Send + Sync + 'static {
    fn shop_name(&self) -> String;

    /// How many phones this shop's licence allows.
    fn device_limit(&self) -> u32;

    /// Every device that is not revoked.
    fn devices(&self) -> Vec<DeviceRow>;

    fn till_room(&self) -> Result<(), String>;

    /// Find a live device and check its credential.
    fn authenticate(&self, device_id: &str, secret: &str) -> Option<Device>;

    /// Record that a device was heard from, for the panel's "last seen".
    fn seen(&self, device_id: &str, ip: &str);

    /// How many phones are on the stream right now. Told on every change, so the counter's
    /// screen can show it without asking.
    fn presence(&self, _connected: usize) {}

    /// The phone's login to the cloud, minted by the counter for the person this device belongs
    /// to (LAN_PROTOCOL.md §3). The whole reply the cloud gave, passed through as-is; a shared
    /// tablet has no person and so no login. Blocking: the server calls it off its own threads.
    fn cloud_login(&self, _device: &Device) -> Result<serde_json::Value, Refusal> {
        Err(Refusal::Refused(
            "This counter cannot sign a phone in to the cloud.".to_owned(),
        ))
    }

    /// The phone chose to leave: the counter forgets it, so its seat on the plan is free again.
    fn leave(&self, _device: &Device) {}

    /// Issue a credential. `staff_id` is the person the approver bound the device to — the
    /// waiter whose phone it is — or `None` for a shared tablet that belongs to nobody.
    fn pair(
        &self,
        request: &PairRequest,
        name: &str,
        platform: &str,
        staff_id: Option<&str>,
    ) -> Result<PairedDevice, Refusal>;

    // What a phone actually came here to do.

    /// Apply one intent. Idempotent by its `id` — see `LAN_PROTOCOL.md` §6.
    fn apply(&self, device: &Device, intent: &crate::intent::Intent) -> crate::intent::Outcome;

    /// Apply a queued batch, in order.
    fn apply_batch(
        &self,
        device: &Device,
        batch: &crate::intent::Batch,
    ) -> crate::intent::BatchResult;

    /// Take a settled bill from another till.
    fn receive(
        &self,
        device: &Device,
        forwarded: &crate::intent::Forwarded,
    ) -> crate::intent::Receipt;

    /// The menu and the floor, with a version.
    fn catalogue(&self, held: Option<&str>) -> Option<crate::intent::Catalogue>;
    /// What the floor is doing right now — which tables are taken and the open orders — as the
    /// same JSON the `floor` push carries. A phone asks for it once after `too_far_behind`.
    fn floor(&self) -> serde_json::Value;
}

/// One row of the panel's device list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceRow {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub staff: Option<String>,
    /// Already written: "2 minutes ago", "never".
    pub last_seen: String,
    pub last_ip: String,
}
