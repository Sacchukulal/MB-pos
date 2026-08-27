//! A secret cannot be in the log, and this is the half that is enforced by a script.

use std::sync::OnceLock;

use regex::Regex;

/// What was found, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub kind: Kind,
    /// 1-based, so it matches what an editor shows.
    pub line: usize,
    /// The offending text, itself redacted — a report about a leaked secret must not be a
    /// second copy of it.
    pub sample: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    LicenceKey,
    PasswordHash,
    DeviceCredential,
    EmergencyCode,
    Pin,
    CustomerPhone,
}

impl Kind {
    #[cfg(test)]
    pub const ALL: &'static [Kind] = &[
        Kind::LicenceKey,
        Kind::PasswordHash,
        Kind::DeviceCredential,
        Kind::EmergencyCode,
        Kind::Pin,
        Kind::CustomerPhone,
    ];

    #[must_use]
    #[cfg(test)]
    pub const fn what(self) -> &'static str {
        match self {
            Kind::LicenceKey => "a licence key",
            Kind::PasswordHash => "a password hash",
            Kind::DeviceCredential => "a phone's credential",
            Kind::EmergencyCode => "an emergency unlock code",
            Kind::Pin => "a PIN",
            Kind::CustomerPhone => "a customer's phone number",
        }
    }
}

/// One line of every pattern.
#[cfg(test)]
pub const THE_FIXTURE: &str = "\
12:04:11.001 INFO  [magic_bill::licensing] activated MB-4KQ7-9WTX-2100 for Anna's Kitchen
12:04:11.002 DEBUG [mb_auth::pin] stored $argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQ$aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789abc
12:04:11.003 INFO  [mb_lan::pairing] issued credential dGhpcy1pcy1hLTMyLWJ5dGUtc2VjcmV0LXZhbHVlLXg
12:04:11.004 WARN  [magic_bill::licensing] emergency code K7M2Q-9XR4T-BW8HN-3PZ6D refused
12:04:11.005 INFO  [magic_bill::ipc] login pin 483920 accepted
12:04:11.006 INFO  [magic_bill::credit] customer +91 9845012345 now owes 1200.00
";

/// A line that must NOT trip anything — the other half of the fixture.
#[cfg(test)]
pub const THE_INNOCENT_FIXTURE: &str = "\
12:04:11.007 INFO  [magic_bill::flows] settled bill INV-000241 order ord_9f8e at 1786340060438
12:04:11.008 INFO  [mb_auth::audit] seq 41 hash ffc56e8e51e8 prev 9a1b2c3d4e5f
12:04:11.009 INFO  [magic_bill::lan] counter at 192.168.1.7:7331 with 4 phones
12:04:11.010 INFO  [magic_bill::licensing] licence standing expired, grace 30 days
12:04:11.011 INFO  [magic_bill::dayclose] counted 100000 expected 12600 variance 87400
";

struct Rules {
    licence_key: Regex,
    password_hash: Regex,
    credential: Regex,
    emergency: Regex,
    pin: Regex,
    phone: Regex,
}

fn rules() -> &'static Rules {
    static RULES: OnceLock<Rules> = OnceLock::new();
    RULES.get_or_init(|| Rules {
        // A licence key: MB- and then groups.
        licence_key: build(r"\bMB-[A-Z0-9]{4}-[A-Z0-9]{4}(-[A-Z0-9]{4})*\b"),
        // Argon2 and bcrypt both start with a `$`-delimited identifier.
        password_hash: build(r"\$(argon2[a-z]*|2[aby])\$"),
        // 32 bytes as base64url, which is what `mb_auth::device` issues.
        credential: build(r"\b[A-Za-z0-9_\-]{43}\b"),
        emergency: build(r"\b[0-9A-HJKMNP-TV-Z]{5}-[0-9A-HJKMNP-TV-Z]{5}-[0-9A-HJKMNP-TV-Z]{5}-[0-9A-HJKMNP-TV-Z]{5}\b"),
        // Only next to the word.
        pin: build(r"(?i)\bpins?\b[^0-9\n]{0,12}\b\d{4,8}\b"),
        // An Indian mobile, with or without the country code.
        phone: build(r"\b(\+91[\s\-]?)?[6-9]\d{9}\b"),
    })
}

/// `Regex::new` on a literal that is tested.
fn build(pattern: &str) -> Regex {
    #[allow(
        clippy::expect_used,
        reason = "a constant pattern, and a scanner that silently does not scan is the failure this file exists to prevent"
    )]
    Regex::new(pattern).expect("a pattern in this file does not compile")
}

/// Read `text` and report anything that looks like a secret.
#[must_use]
pub fn scan(text: &str) -> Vec<Finding> {
    let rules = rules();
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        for (kind, rule) in [
            (Kind::LicenceKey, &rules.licence_key),
            (Kind::PasswordHash, &rules.password_hash),
            (Kind::DeviceCredential, &rules.credential),
            (Kind::EmergencyCode, &rules.emergency),
            (Kind::Pin, &rules.pin),
            (Kind::CustomerPhone, &rules.phone),
        ] {
            if let Some(hit) = rule.find(line) {
                found.push(Finding {
                    kind,
                    line: line_number,
                    // Redacted. A report about a leak must not be the leak.
                    sample: middle(hit.as_str()),
                });
            }
        }
    }
    found
}

/// The findings as one sentence, for a test failure and for the bundle screen.
#[must_use]
#[cfg(test)]
pub fn describe(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(|f| format!("line {}: {} ({})", f.line, f.kind.what(), f.sample))
        .collect::<Vec<_>>()
        .join("; ")
}

// Making things safe to write down.

/// Keep the ends, hide the middle.
fn middle(value: &str) -> String {
    let characters: Vec<char> = value.chars().collect();
    if characters.len() <= 8 {
        return "•".repeat(characters.len().max(1));
    }
    let head: String = characters.iter().take(4).collect();
    let tail: String = characters.iter().skip(characters.len() - 2).collect();
    format!(
        "{head}{}{tail}",
        "•".repeat(characters.len().saturating_sub(6))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The test that stops this file becoming a function returning nothing.
    #[test]
    fn the_scanner_catches_its_own_fixture() {
        let found = scan(THE_FIXTURE);
        let mut kinds: Vec<Kind> = found.iter().map(|f| f.kind).collect();
        kinds.sort_unstable();
        kinds.dedup();
        for kind in Kind::ALL {
            assert!(
                kinds.contains(kind),
                "the rule for {} did not fire on the fixture — the scanner is \
                 broken, not the log. Found: {}",
                kind.what(),
                describe(&found)
            );
        }
    }

    /// And it does not cry wolf.
    #[test]
    fn ordinary_log_lines_are_not_secrets() {
        let found = scan(THE_INNOCENT_FIXTURE);
        assert!(
            found.is_empty(),
            "the scanner flagged ordinary output: {}",
            describe(&found)
        );
    }

    /// The specific near miss that broke the first version of the phone rule.
    #[test]
    fn a_millisecond_timestamp_is_not_a_phone_number() {
        // 1786340060438 contains "6340060438" — ten digits starting with 6.
        assert!(scan("settled at 1786340060438").is_empty());
        // But a real one is caught, with and without the country code.
        assert_eq!(scan("rang 9845012345").len(), 1);
        assert_eq!(scan("rang +91 9845012345").len(), 1);
    }

    /// A quantity is not a PIN, and neither is a bill number.
    #[test]
    fn a_bare_number_is_not_a_pin() {
        assert!(scan("counted 483920 paise").is_empty());
        assert!(scan("bill INV-483920").is_empty());
        // Next to the word, it is.
        assert_eq!(scan("pin 483920 accepted").len(), 1);
        assert_eq!(scan("PIN: 4839").len(), 1);
    }

    /// A finding must not be a second copy of the secret.
    #[test]
    fn a_report_about_a_leak_is_not_the_leak() {
        let found = scan("activated MB-4KQ7-9WTX-2100");
        assert_eq!(found.len(), 1);
        assert!(!found[0].sample.contains("9WTX"), "{:?}", found[0].sample);
        assert!(found[0].sample.contains('•'));
    }
}
