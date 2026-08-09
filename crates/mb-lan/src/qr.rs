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
/// `magicbill://pair?h=<host>&p=<port>&f=<fingerprint>&t=<token>`
///
/// **The server id is deliberately not in it.** The phone learns that from the
/// pairing response, so putting it in the code only makes the code denser —
/// and the code has to be readable by a five-year-old phone camera held at
/// arm length.
///
/// **The fingerprint is in the QR on purpose.** It is the phone's only chance
/// to learn which certificate to pin, and it learns it from a code held up by a
/// person at the counter rather than from the network — which is exactly the
/// out-of-band channel that makes pinning worth anything on a WiFi a stranger
/// is also on.
///
/// # Why the fingerprint is base64url here and hex on the screen
///
/// Found by looking at it: `sha256:` plus 64 hex characters is 71 of the
/// payload's 150, which pushed the code to 45 modules — big enough that it ran
/// off the bottom of the panel, and a QR a person has to scroll to see is a QR
/// a camera sees half of. The same 32 bytes in base64url are 43 characters, and
/// the code drops two versions.
///
/// The **screen** still shows the hex form, because that is what
/// `openssl x509 -fingerprint -sha256` prints and somebody comparing the two by
/// eye has to be able to.
#[must_use]
pub fn pairing_uri(host: &str, port: u16, fingerprint: &str, token: &str) -> String {
    let compact = compact_fingerprint(fingerprint).unwrap_or_else(|| fingerprint.to_owned());
    format!("magicbill://pair?h={host}&p={port}&f={compact}&t={token}")
}

/// `sha256:aabb…` as base64url of the same bytes. `None` when it is not one.
#[must_use]
pub fn compact_fingerprint(fingerprint: &str) -> Option<String> {
    use base64::Engine as _;
    let hex = fingerprint.strip_prefix("sha256:")?;
    if hex.len() != 64 {
        return None;
    }
    let mut bytes = Vec::with_capacity(32);
    for pair in hex.as_bytes().chunks(2) {
        let text = std::str::from_utf8(pair).ok()?;
        bytes.push(u8::from_str_radix(text, 16).ok()?);
    }
    Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

/// A QR as rows of booleans — `true` is a dark module.
///
/// # Errors
///
/// If the payload will not fit in a QR code at all.
pub fn matrix(payload: &str) -> Result<Vec<Vec<bool>>, LanError> {
    // **The LOWEST correction level, and that is the right call here.** This is
    // read off a bright, clean, backlit screen from six inches away — not off a
    // smudged printed label in a warehouse. Every level above L buys robustness
    // nothing here needs and pays for it in modules, and a code with more
    // modules than the panel is tall is a code the camera sees half of.
    let code = QrCode::with_error_correction_level(payload.as_bytes(), EcLevel::L)
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
        let uri = pairing_uri("192.168.1.7", 7331, "sha256:aa", "tok");
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
        let uri = pairing_uri("192.168.1.7", 7331, "sha256:beef", "tok123");
        assert!(uri.contains("h=192.168.1.7"));
        assert!(uri.contains("p=7331"));
        // Not a real fingerprint, so it travels unchanged rather than being
        // silently mangled into something a phone would pin.
        assert!(uri.contains("f=sha256:beef"));
        assert!(uri.contains("t=tok123"));
        assert!(
            !uri.contains("srv"),
            "the server id is in the code, which only makes it denser"
        );
    }

    /// **Found by looking at the screen.** A real fingerprint pushed the code
    /// to 45 modules, which ran off the bottom of the panel — and a QR a person
    /// has to scroll to see is a QR a camera sees half of.
    #[test]
    fn a_real_pairing_code_fits_on_the_panel() {
        let real = format!("sha256:{}", "d1a26a75616db687a2d96294144659af".repeat(2));
        let compact = compact_fingerprint(&real).expect("it is a fingerprint");
        assert_eq!(compact.len(), 43, "base64url of 32 bytes is 43 characters");
        assert!(
            compact.len() < real.len(),
            "the compact form is not smaller, so the QR did not shrink"
        );

        let uri = pairing_uri("192.168.1.104", 7331, &real, &"t".repeat(22));
        let m = matrix(&uri).expect("drawn");
        assert!(
            m.len() <= 41,
            "the pairing code is {} modules across, which does not fit",
            m.len()
        );

        // And the phone can still get the exact bytes back out.
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&compact)
            .expect("decodes");
        assert_eq!(bytes.len(), 32);
    }
}
