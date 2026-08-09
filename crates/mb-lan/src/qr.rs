//! **The pairing QR — the discovery path that always works.**
//!
//! mDNS is first and this is second, and "second" does not mean optional. mDNS
//! is absent on a network with client isolation turned on, unreliable on cheap
//! consumer routers, and flaky on several Android builds. A shop whose router
//! does not carry multicast still has to be able to add a phone, and this is
//! how.
//!
//! # It hands back a matrix, not a picture
//!
//! A bitmap crossing IPC to be scaled by a browser is three problems where one
//! will do: an encoder here, a base64 blob on the wire, and a browser scaling a
//! 25-pixel image up to 200 without turning the modules to mush. So this
//! returns booleans and the counter's panel draws them as a CSS grid — sharp at
//! any size, no image decoding, and no `dangerouslySetInnerHTML`.

use qrcode::{EcLevel, QrCode};

use crate::error::LanError;

/// Everything a phone needs to find this counter and to know it is the right
/// one, in a form that survives being photographed.
///
/// `magicbill://pair?h=<host>&p=<port>&f=<fingerprint>&t=<token>&s=<server id>`
///
/// **The fingerprint is in the QR on purpose.** It is the phone's only chance
/// to learn which certificate to pin, and it learns it from a code held up by a
/// person at the counter rather than from the network — which is exactly the
/// out-of-band channel that makes pinning worth anything on a WiFi a stranger
/// is also on.
#[must_use]
pub fn pairing_uri(host: &str, port: u16, fingerprint: &str, token: &str, server_id: &str) -> String {
    format!("magicbill://pair?h={host}&p={port}&f={fingerprint}&t={token}&s={server_id}")
}

/// A QR as rows of booleans — `true` is a dark module.
///
/// # Errors
///
/// If the payload will not fit in a QR code at all.
pub fn matrix(payload: &str) -> Result<Vec<Vec<bool>>, LanError> {
    // Medium correction: this is read off a bright screen from six inches
    // away, not off a printed label in a warehouse. High correction would make
    // the code denser for a robustness nothing here needs.
    let code = QrCode::with_error_correction_level(payload.as_bytes(), EcLevel::M)
        .map_err(|e| LanError::Identity(format!("the pairing code could not be drawn: {e}")))?;
    let width = code.width();
    let colours = code.into_colors();
    Ok(colours
        .chunks(width)
        .map(|row| {
            row.iter()
                .map(|c| *c == qrcode::Color::Dark)
                .collect::<Vec<bool>>()
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pairing_code_is_square_and_has_its_finder_patterns() {
        let uri = pairing_uri(
            "192.168.1.7",
            7331,
            "sha256:aa",
            "tok",
            "srv_1",
        );
        let m = matrix(&uri).expect("drawn");
        assert!(m.len() >= 21, "a QR is at least 21 modules across");
        for row in &m {
            assert_eq!(row.len(), m.len(), "it is not square");
        }
        // The top-left finder pattern is a 7x7 ring. Its outer row is solid
        // dark, and its second row is dark-light-…-light-dark. If this is
        // wrong the phone will not see a code at all.
        assert!(m[0][0..7].iter().all(|d| *d), "no finder pattern");
        assert!(m[1][0] && !m[1][1] && m[1][6]);
    }

    /// The URI carries the four facts a phone cannot get any other way, and
    /// the fingerprint is one of them.
    #[test]
    fn the_uri_carries_the_fingerprint_because_that_is_the_whole_point() {
        let uri = pairing_uri("192.168.1.7", 7331, "sha256:beef", "tok123", "srv_9");
        assert!(uri.contains("h=192.168.1.7"));
        assert!(uri.contains("p=7331"));
        assert!(uri.contains("f=sha256:beef"));
        assert!(uri.contains("t=tok123"));
        assert!(uri.contains("s=srv_9"));
    }
}
