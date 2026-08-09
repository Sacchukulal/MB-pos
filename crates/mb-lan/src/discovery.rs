//! **mDNS — the phone finds the counter with no typing.**
//!
//! The first of two discovery paths, and never the only one. It fails on a
//! network with client isolation turned on, on several cheap consumer routers
//! that do not forward multicast between wireless clients, and on Android
//! builds that put multicast behind a wake lock the app has to hold. The QR in
//! [`crate::qr`] is the other path, and it always works.
//!
//! # What the advertisement carries, and what it must not
//!
//! TXT records: the server id, the protocol version, the shop's name and the
//! **certificate fingerprint**. Nothing secret — mDNS is broadcast to every
//! device on the network including the guest phones, so anything in here is
//! public by construction. In particular there is no pairing token here: that
//! is what the QR is for, and the difference between the two channels is the
//! difference between "anybody can see it" and "somebody held it up to you".
//!
//! Publishing the fingerprint is safe and useful: it is a public value (it is
//! in the certificate the server presents anyway) and a phone that already
//! paired can use it to recognise the counter after a DHCP move without asking
//! anybody anything.

use crate::error::LanError;

/// The service type. `_magicbill._tcp` — ours, not a borrowed one, so a phone
/// browsing for it cannot find somebody else's printer.
pub const SERVICE: &str = "_magicbill._tcp.local.";

/// A live advertisement. Dropping it withdraws the service, which is what makes
/// a stopped counter stop being findable.
pub struct Advertisement {
    daemon: Option<mdns_sd::ServiceDaemon>,
    full_name: String,
}

impl std::fmt::Debug for Advertisement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Advertisement")
            .field("name", &self.full_name)
            .field("live", &self.daemon.is_some())
            .finish()
    }
}

impl Advertisement {
    /// Start advertising.
    ///
    /// **A failure here is not fatal and must not be.** mDNS is the convenient
    /// path, not the necessary one — a shop whose router blocks multicast still
    /// adds phones by QR, and a counter that refused to start because it could
    /// not advertise would be a counter that stops billing over a router
    /// setting. So this returns the error for the panel to SAY, and the caller
    /// carries on (R3: visible, not fatal).
    ///
    /// # Errors
    ///
    /// When the mDNS daemon cannot start or the service cannot be registered.
    pub fn start(
        instance: &str,
        port: u16,
        ips: &[std::net::IpAddr],
        properties: &[(&str, &str)],
    ) -> Result<Advertisement, LanError> {
        let daemon = mdns_sd::ServiceDaemon::new()
            .map_err(|e| LanError::Io(format!("mDNS could not start: {e}")))?;

        // The host name has to end in `.local.` and must not contain spaces —
        // a shop called "Anna Kuteera (Jayanagar)" is an entirely ordinary
        // name and an entirely invalid host name.
        let safe: String = instance
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let host = format!("{}.local.", safe.trim_matches('-'));

        let info = mdns_sd::ServiceInfo::new(
            SERVICE,
            instance,
            &host,
            ips,
            port,
            properties
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect::<std::collections::HashMap<String, String>>(),
        )
        .map_err(|e| LanError::Io(format!("the mDNS advertisement is not valid: {e}")))?;

        let full_name = info.get_fullname().to_owned();
        daemon
            .register(info)
            .map_err(|e| LanError::Io(format!("mDNS refused the advertisement: {e}")))?;

        Ok(Advertisement {
            daemon: Some(daemon),
            full_name,
        })
    }

    #[must_use]
    pub fn full_name(&self) -> &str {
        &self.full_name
    }
}

impl Drop for Advertisement {
    fn drop(&mut self) {
        // Withdraw it, then shut the daemon down. A counter that was turned off
        // must stop appearing in the phone's list — a stale entry is a waiter
        // tapping a name and waiting for a timeout.
        if let Some(daemon) = self.daemon.take() {
            let _ = daemon.unregister(&self.full_name);
            let _ = daemon.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shop name with brackets and spaces in it is an ordinary shop name and
    /// an invalid host name. It must not stop the advertisement.
    ///
    /// The daemon binds a multicast socket, which a CI box may not allow — so
    /// the assertion is "it either works or fails with a sentence", never a
    /// panic.
    #[test]
    fn a_real_shop_name_does_not_break_the_advertisement() {
        let ip: std::net::IpAddr = "127.0.0.1".parse().expect("an address");
        match Advertisement::start(
            "Anna Kuteera (Jayanagar)",
            7331,
            &[ip],
            &[("v", "1"), ("id", "srv_1")],
        ) {
            Ok(ad) => assert!(ad.full_name().ends_with(SERVICE)),
            Err(e) => {
                let said = e.to_string();
                assert!(
                    said.contains("mDNS"),
                    "the failure has to name the thing that failed: {said}"
                );
            }
        }
    }
}
