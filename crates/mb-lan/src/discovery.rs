//! MDNS — the phone finds the counter with no typing.

use crate::error::LanError;

/// The service type. `_magicbill._tcp` — ours, not a borrowed one, so a phone browsing for it
/// cannot find somebody else's printer.
pub const SERVICE: &str = "_magicbill._tcp.local.";

/// A live advertisement. Dropping it withdraws the service, which is what makes a stopped
/// counter stop being findable.
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
    pub fn start(
        instance: &str,
        port: u16,
        ips: &[std::net::IpAddr],
        properties: &[(&str, &str)],
    ) -> Result<Advertisement, LanError> {
        let daemon = mdns_sd::ServiceDaemon::new()
            .map_err(|e| LanError::Io(format!("mDNS could not start: {e}")))?;

        // The host name has to end in `.local.` and must not contain spaces — a shop called
        // "Anna Kuteera (Jayanagar)" is an entirely ordinary name and an entirely invalid host
        // name.
        let safe: String = instance
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let trimmed = safe.trim_matches('-');
        let name = if trimmed.is_empty() {
            "magic-bill"
        } else {
            trimmed
        };
        let host = format!("{name}.local.");
        // The instance is what a phone SHOWS in its list, so it keeps the shop's real name —
        // brackets, spaces and all — unless there isn't one.
        let shown = if instance.trim().is_empty() {
            "Magic Bill counter"
        } else {
            instance
        };

        let info = mdns_sd::ServiceInfo::new(
            SERVICE,
            shown,
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
        // Withdraw it, then shut the daemon down.
        if let Some(daemon) = self.daemon.take() {
            let _ = daemon.unregister(&self.full_name);
            let _ = daemon.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shop name with brackets and spaces in it is an ordinary shop name and an invalid host
    /// name.
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

    /// A shop that has not typed its name in yet.
    #[test]
    fn a_shop_with_no_name_still_advertises() {
        let ip: std::net::IpAddr = "127.0.0.1".parse().expect("an address");
        match Advertisement::start("", 7331, &[ip], &[("v", "1")]) {
            Ok(ad) => assert!(
                ad.full_name().contains("Magic Bill counter"),
                "{}",
                ad.full_name()
            ),
            Err(e) => {
                let said = e.to_string();
                assert!(
                    !said.contains("cannot be empty"),
                    "an unnamed shop still cannot advertise: {said}"
                );
            }
        }
    }
}
