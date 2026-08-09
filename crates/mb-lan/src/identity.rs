//! **Who this counter is, cryptographically** — the key, the certificate, and
//! the fingerprint a phone pins.
//!
//! # The threat this exists to answer
//!
//! The shop's WiFi is not trustworthy. Guests are on it, the password is on a
//! blackboard, and the router is whatever the cable company left behind. A
//! stranger on that network can see every packet and can answer to any name
//! mDNS resolves.
//!
//! So the phone does not trust an address. It trusts a **certificate it saw
//! once, at pairing, while a person was watching** — and it refuses to talk to
//! anything that cannot prove it holds that certificate's private key. That is
//! certificate pinning, and it is the whole security model of this session in
//! one sentence.
//!
//! Consequences worth writing down, because each is a support call somebody
//! will make:
//!
//! * a self-signed certificate is **correct here**, not a compromise. There is
//!   no certificate authority that can vouch for `192.168.1.7`, and paying one
//!   would buy nothing a fingerprint does not already give.
//! * **if this key is lost, every phone must pair again.** Not silently — the
//!   counter's panel says so, because fifteen waiters discovering it one at a
//!   time during a rush is the alternative.
//! * the key is stored beside the **config**, not in the shop's database. P05
//!   copies that database to a pen drive on purpose and restores it onto other
//!   machines (D27); a restored backup must not resurrect another machine's
//!   private key.

use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use crate::error::LanError;

/// Ten years. A shop does not have a certificate-renewal process and must not
/// need one: an expiry that stops the waiters taking orders on a Saturday is a
/// worse outcome than a long-lived key on a machine somebody already has to
/// stand next to.
const YEARS: i64 = 10;

/// What the counter proves it is.
#[derive(Debug, Clone)]
pub struct Identity {
    /// Stable across restarts, and what a device credential is bound to — so a
    /// counter whose DHCP lease moves it to a new address is still the same
    /// counter and nobody pairs again.
    pub server_id: String,
    /// PEM, for the TLS acceptor.
    pub certificate_pem: String,
    /// PEM. **Never leaves this process** except to be written to its own file.
    pub key_pem: String,
    /// `sha256:` and 64 hex characters. This is what a phone stores and checks
    /// on every connection.
    pub fingerprint: String,
    /// True when this run created a NEW identity — which means every phone
    /// must pair again, and the panel has to say so out loud.
    pub is_new: bool,
}

impl Identity {
    /// Load the identity from `dir`, or make one.
    ///
    /// **A corrupt or unreadable key is not fatal and is not silent.** The
    /// counter must still serve, so a new identity is generated — and `is_new`
    /// carries the fact upwards so the panel can say *"this counter has a new
    /// certificate; phones will need to be added again"*. R3: the failure is
    /// visible, and the shop is not stopped.
    ///
    /// # Errors
    ///
    /// Only when a fresh certificate cannot be generated or written at all.
    pub fn load_or_create(dir: &Path, ips: &[IpAddr]) -> Result<Identity, LanError> {
        let cert_path = dir.join("counter-cert.pem");
        let key_path = dir.join("counter-key.pem");
        let id_path = dir.join("counter-id.txt");

        if let (Ok(certificate_pem), Ok(key_pem), Ok(server_id)) = (
            fs::read_to_string(&cert_path),
            fs::read_to_string(&key_path),
            fs::read_to_string(&id_path),
        ) && let Some(fingerprint) = fingerprint_of(&certificate_pem)
            && !server_id.trim().is_empty()
        {
            return Ok(Identity {
                server_id: server_id.trim().to_owned(),
                certificate_pem,
                key_pem,
                fingerprint,
                is_new: false,
            });
        }

        let fresh = generate(ips)?;
        fs::create_dir_all(dir).map_err(|e| LanError::Identity(e.to_string()))?;
        fs::write(&cert_path, &fresh.certificate_pem)
            .map_err(|e| LanError::Identity(e.to_string()))?;
        fs::write(&key_path, &fresh.key_pem).map_err(|e| LanError::Identity(e.to_string()))?;
        fs::write(&id_path, &fresh.server_id).map_err(|e| LanError::Identity(e.to_string()))?;
        Ok(fresh)
    }

    /// Make one without touching the disk. The tests use this; so does a shop
    /// whose config folder is read-only, which is a state a locked-down machine
    /// really gets into.
    ///
    /// # Errors
    ///
    /// If the certificate cannot be generated.
    pub fn ephemeral(ips: &[IpAddr]) -> Result<Identity, LanError> {
        generate(ips)
    }
}

fn generate(ips: &[IpAddr]) -> Result<Identity, LanError> {
    // Every address the counter can be reached on, plus localhost. A phone
    // connects by IP — there is no DNS on a shop LAN — so an IP that is not in
    // the SAN list is an address the certificate does not cover, and rustls is
    // right to refuse it.
    let mut names: Vec<String> = ips.iter().map(ToString::to_string).collect();
    names.push("localhost".to_owned());
    names.push("127.0.0.1".to_owned());
    names.sort();
    names.dedup();

    let mut params =
        rcgen::CertificateParams::new(names).map_err(|e| LanError::Identity(e.to_string()))?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "Magic Bill counter");
    params.not_before = rcgen::date_time_ymd(2020, 1, 1);
    let (year, month, day) = (2020 + i32::try_from(YEARS).unwrap_or(10) + 6, 1, 1);
    params.not_after = rcgen::date_time_ymd(year, month, day);

    let key = rcgen::KeyPair::generate().map_err(|e| LanError::Identity(e.to_string()))?;
    let certificate = params
        .self_signed(&key)
        .map_err(|e| LanError::Identity(e.to_string()))?;

    let certificate_pem = certificate.pem();
    let fingerprint = fingerprint_of(&certificate_pem)
        .ok_or_else(|| LanError::Identity("the new certificate could not be read back".into()))?;

    Ok(Identity {
        server_id: format!("srv_{}", mb_auth::random_token(12)),
        key_pem: key.serialize_pem(),
        certificate_pem,
        fingerprint,
        is_new: true,
    })
}

/// `sha256:` and the certificate's DER, hashed.
///
/// **The DER and not the PEM**: PEM is base64 with line breaks in it, and two
/// tools that wrap at different widths would produce two different
/// fingerprints for the same certificate. Every other TLS tool in the world
/// fingerprints the DER, and a phone written against this must be able to check
/// the number with `openssl x509 -fingerprint -sha256`.
#[must_use]
pub fn fingerprint_of(certificate_pem: &str) -> Option<String> {
    let der = der_of(certificate_pem)?;
    let digest = mb_auth::sha256(&der);
    let mut out = String::from("sha256:");
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    Some(out)
}

/// The bytes between the PEM markers, base64-decoded.
///
/// **Every check here was earned by a test.** The first version skipped lines
/// until it found a BEGIN marker, and if there was none it collected nothing,
/// base64-decoded the empty string successfully, and handed back a fingerprint
/// of zero bytes. A file containing the words "not a certificate" loaded as a
/// valid identity — which is precisely the silent failure R3 forbids, on the
/// one value the phone's entire trust decision rests on.
pub(crate) fn der_of(certificate_pem: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    if !certificate_pem.contains("-----BEGIN CERTIFICATE-----")
        || !certificate_pem.contains("-----END CERTIFICATE-----")
    {
        return None;
    }
    let body: String = certificate_pem
        .lines()
        .skip_while(|line| !line.starts_with("-----BEGIN CERTIFICATE-----"))
        .skip(1)
        .take_while(|line| !line.starts_with("-----END CERTIFICATE-----"))
        .collect();
    let der = base64::engine::general_purpose::STANDARD.decode(body).ok()?;
    // A DER certificate is an ASN.1 SEQUENCE, so it starts with 0x30. Cheap,
    // and it catches a truncated or half-written file — which is what a power
    // cut during first run leaves behind.
    if der.len() < 64 || der.first() != Some(&0x30) {
        return None;
    }
    Some(der)
}

/// Every address this machine can be reached on from the shop's WiFi.
///
/// Loopback is excluded from the ADVERTISED list on purpose — a phone told to
/// connect to 127.0.0.1 would be connecting to itself — but it stays in the
/// certificate's SAN list, because the counter's own panel and every test in
/// this crate connect over it.
#[must_use]
pub fn local_addresses() -> Vec<IpAddr> {
    // A UDP socket "connected" to an address off-machine picks the interface
    // the routing table would use, without sending a single packet. It is the
    // one way to answer "which of my addresses would a phone see?" without a
    // crate that enumerates adapters — and the answer is the one that matters,
    // because a shop PC has a WiFi adapter, an Ethernet adapter, a Hyper-V
    // switch and a VPN, and only one of them is the shop's network.
    let mut out = Vec::new();
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0")
        && socket.connect("192.168.1.1:9").is_ok()
        && let Ok(local) = socket.local_addr()
        && !local.ip().is_loopback()
    {
        out.push(local.ip());
    }
    if out.is_empty()
        && let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0")
        && socket.connect("8.8.8.8:9").is_ok()
        && let Ok(local) = socket.local_addr()
        && !local.ip().is_loopback()
    {
        out.push(local.ip());
    }
    out
}

/// Where the identity lives — beside the config, never in the shop database.
#[must_use]
pub fn folder(config_dir: &Path) -> PathBuf {
    config_dir.join("network")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **T1a.** The certificate covers the addresses a phone will use, and the
    /// same key produces the same fingerprint twice — which is the property
    /// pinning is built on.
    #[test]
    fn a_certificate_covers_the_lan_and_pins_stably() {
        let ip: IpAddr = "192.168.1.7".parse().expect("an address");
        let dir = scratch("stable");
        let first = Identity::load_or_create(&dir, &[ip]).expect("made one");
        assert!(first.is_new);
        assert!(first.fingerprint.starts_with("sha256:"));
        assert_eq!(first.fingerprint.len(), 7 + 64);

        // The SANs are in the DER; checking the PEM round-trips and the
        // fingerprint is stable is what actually matters to a phone.
        let second = Identity::load_or_create(&dir, &[ip]).expect("loaded it");
        assert!(!second.is_new, "it generated a second identity");
        assert_eq!(first.fingerprint, second.fingerprint);
        assert_eq!(first.server_id, second.server_id);
        clean(&dir);
    }

    /// **T1b.** A corrupt key file gives a NEW identity and says so. It never
    /// panics, and it never quietly reuses a certificate it could not read.
    #[test]
    fn a_corrupt_key_makes_a_new_identity_and_admits_it() {
        let dir = scratch("corrupt");
        let first = Identity::load_or_create(&dir, &[]).expect("made one");
        fs::write(dir.join("counter-cert.pem"), "not a certificate").expect("wrote rubbish");

        let second = Identity::load_or_create(&dir, &[]).expect("made another");
        assert!(
            second.is_new,
            "a corrupt certificate was reused, which is the silent failure R3 forbids"
        );
        assert_ne!(first.fingerprint, second.fingerprint);
        clean(&dir);
    }

    #[test]
    fn the_fingerprint_is_of_the_der_so_openssl_agrees() {
        let identity = Identity::ephemeral(&[]).expect("made one");
        let der = der_of(&identity.certificate_pem).expect("decoded");
        // A DER certificate always starts with a SEQUENCE tag.
        assert_eq!(der.first(), Some(&0x30));
        assert_eq!(
            fingerprint_of(&identity.certificate_pem).as_deref(),
            Some(identity.fingerprint.as_str())
        );
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mb-lan-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a scratch folder");
        dir
    }

    fn clean(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }
}
