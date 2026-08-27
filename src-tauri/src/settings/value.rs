//! What a setting's value is, what shapes it may take, and the one place a bad one is refused.

use mb_core::Money;

/// A setting's value, in the four shapes the storage layer can hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Money(Money),
    Text(String),
}

impl Value {
    pub fn as_bool(&self) -> Result<bool, Invalid> {
        match self {
            Value::Bool(b) => Ok(*b),
            other => Err(Invalid::wrong_shape("a tick", other)),
        }
    }

    pub fn as_int(&self) -> Result<i64, Invalid> {
        match self {
            Value::Int(n) => Ok(*n),
            other => Err(Invalid::wrong_shape("a number", other)),
        }
    }

    pub fn as_money(&self) -> Result<Money, Invalid> {
        match self {
            Value::Money(m) => Ok(*m),
            other => Err(Invalid::wrong_shape("an amount", other)),
        }
    }

    pub fn as_text(&self) -> Result<&str, Invalid> {
        match self {
            Value::Text(t) => Ok(t),
            other => Err(Invalid::wrong_shape("some words", other)),
        }
    }

    /// The word for what this is, for a refusal sentence.
    #[must_use]
    pub const fn shape(&self) -> &'static str {
        match self {
            Value::Bool(_) => "a tick",
            Value::Int(_) => "a number",
            Value::Money(_) => "an amount",
            Value::Text(_) => "some words",
        }
    }
}

/// One option in a `Kind::Choice`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    pub value: &'static str,
    pub label: &'static str,
}

/// The extra shape a piece of text has to hold, beyond a length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Free,
    /// Ten digits. India, no country code — the bill has 32 columns.
    Phone,
    /// Fifteen characters, and the state code has to match the shop's own state, which is the
    /// check nobody does and everybody needs.
    Gstin,
    /// Fourteen digits.
    Fssai,
    UpiId,
    /// A folder that may be empty (meaning "the usual place").
    Folder,
}

/// What a setting may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bool,
    Int {
        min: i64,
        max: i64,
        /// "minutes", "%", "characters" — printed in the refusal and shown beside the field, so
        /// a number never appears without its unit.
        unit: &'static str,
    },
    Money {
        min_paise: i64,
        max_paise: i64,
    },
    Text {
        max_len: usize,
        shape: Shape,
    },
    Choice(&'static [Choice]),
}

impl Kind {
    /// The one gate. Everything that reaches a setting comes through here first, including an
    /// import and including a value this program wrote itself — the second one matters, because
    /// a build that changes a limit must fail loudly on the old value rather than printing it.
    pub fn check(self, value: &Value) -> Result<(), Invalid> {
        match self {
            Kind::Bool => {
                value.as_bool()?;
                Ok(())
            }
            Kind::Int { min, max, unit } => {
                let n = value.as_int()?;
                if n < min || n > max {
                    return Err(Invalid::new(format!(
                        "{n} is outside what this can be. Choose between {min} and {max} {unit}."
                    )));
                }
                Ok(())
            }
            Kind::Money {
                min_paise,
                max_paise,
            } => {
                let m = value.as_money()?;
                if m.paise() < min_paise || m.paise() > max_paise {
                    return Err(Invalid::new(format!(
                        "{} is outside what this can be. Choose between {} and {}.",
                        m.to_plain_string(),
                        Money::from_paise(min_paise).to_plain_string(),
                        Money::from_paise(max_paise).to_plain_string()
                    )));
                }
                Ok(())
            }
            Kind::Text { max_len, shape } => {
                let text = value.as_text()?;
                if text.chars().count() > max_len {
                    return Err(Invalid::new(format!(
                        "That is {} characters and the most this can hold is {max_len}.",
                        text.chars().count()
                    )));
                }
                check_shape(shape, text)
            }
            Kind::Choice(options) => {
                let text = value.as_text()?;
                if options.iter().any(|o| o.value == text) {
                    return Ok(());
                }
                Err(Invalid::new(format!(
                    "\"{text}\" is not one of the choices here. Pick one of: {}.",
                    options
                        .iter()
                        .map(|o| o.label)
                        .collect::<Vec<_>>()
                        .join(", ")
                )))
            }
        }
    }
}

/// The shape checks, and every one of them says what is wrong rather than "invalid format".
fn check_shape(shape: Shape, text: &str) -> Result<(), Invalid> {
    if text.is_empty() {
        return Ok(());
    }
    match shape {
        Shape::Free | Shape::Folder => Ok(()),
        // What is STORED is ten bare digits, and this says so.
        Shape::Phone => {
            if text.len() == mb_core::PHONE_DIGITS && text.chars().all(|c| c.is_ascii_digit()) {
                Ok(())
            } else {
                Err(Invalid::new(
                    "A phone number here is ten digits, with no spaces and no +91.",
                ))
            }
        }
        // Whatever the shop types is what the shop meant.
        Shape::Fssai | Shape::Gstin => Ok(()),
        Shape::UpiId => match text.split_once('@') {
            Some((name, handle)) if !name.is_empty() && !handle.is_empty() => Ok(()),
            _ => Err(Invalid::new(
                "A UPI id looks like name@bank — for example 9880012345@okhdfcbank.",
            )),
        },
    }
}

/// A GSTIN, checked properly.
pub(crate) fn check_gstin(text: &str) -> Result<(), Invalid> {
    if text.chars().count() != 15 {
        return Err(Invalid::new(format!(
            "A GST number is fifteen characters and that one is {}.",
            text.chars().count()
        )));
    }
    if !text
        .chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return Err(Invalid::new(
            "A GST number is capital letters and digits only.",
        ));
    }
    let bytes = text.as_bytes();
    if bytes.get(13) != Some(&b'Z') {
        return Err(Invalid::new(
            "The fourteenth character of a GST number is always the letter Z.",
        ));
    }
    if gstin_check_character(&text[..14]) != text.chars().nth(14) {
        return Err(Invalid::new(
            "That GST number's last character does not match the rest of it, so \
             one of the other fourteen has been typed wrongly.",
        ));
    }
    Ok(())
}

/// The GST portal's checksum: Luhn mod 36 over 0-9 then A-Z.
#[allow(
    clippy::integer_division,
    reason = "Luhn's digit-sum: the quotient and the remainder are BOTH added, \
              so nothing is discarded — this is the algorithm, not a rounding"
)]
fn gstin_check_character(first_fourteen: &str) -> Option<char> {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut sum = 0_u32;
    for (index, ch) in first_fourteen.chars().enumerate() {
        let position = u32::try_from(ALPHABET.iter().position(|c| *c == ch as u8)?).ok()?;
        // Every second character counts double, from the left, because the string is a fixed
        // fifteen and the portal counts it that way.
        let factor = if index % 2 == 0 { 1 } else { 2 };
        let product = position * factor;
        sum += (product / 36) + (product % 36);
    }
    let remainder = sum % 36;
    let check = (36 - remainder) % 36;
    ALPHABET
        .get(usize::try_from(check).ok()?)
        .map(|b| *b as char)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalid {
    pub key: Option<&'static str>,
    pub message: String,
}

impl Invalid {
    pub fn new(message: impl Into<String>) -> Invalid {
        Invalid {
            key: None,
            message: message.into(),
        }
    }

    fn wrong_shape(wanted: &str, got: &Value) -> Invalid {
        Invalid::new(format!(
            "This setting holds {wanted} and it was given {}.",
            got.shape()
        ))
    }

    #[must_use]
    pub fn about(mut self, key: &'static str) -> Invalid {
        self.key = Some(key);
        self
    }
}

impl From<Invalid> for crate::words::UiError {
    fn from(invalid: Invalid) -> Self {
        let error = crate::words::UiError::new("settings.invalid", invalid.message);
        match invalid.key {
            Some(key) => error.with_detail(format!("setting {key}")),
            None => error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_number_outside_its_range_is_refused_and_says_the_range() {
        let kind = Kind::Int {
            min: 0,
            max: 8,
            unit: "characters",
        };
        let error = kind.check(&Value::Int(40)).expect_err("40 was allowed");
        assert!(
            error.message.contains("between 0 and 8"),
            "{}",
            error.message
        );
        // And it is refused, not clamped.
        assert!(kind.check(&Value::Int(8)).is_ok());
    }

    #[test]
    fn a_choice_outside_its_list_names_the_choices() {
        const OPTIONS: &[Choice] = &[
            Choice {
                value: "dashed",
                label: "Dashed",
            },
            Choice {
                value: "dotted",
                label: "Dotted",
            },
        ];
        let error = Kind::Choice(OPTIONS)
            .check(&Value::Text("wavy".to_owned()))
            .expect_err("wavy was allowed");
        assert!(
            error.message.contains("Dashed, Dotted"),
            "{}",
            error.message
        );
    }

    #[test]
    fn the_wrong_shape_entirely_is_refused_rather_than_parsed() {
        // A `"1"` is not a tick.
        let error = Kind::Bool
            .check(&Value::Text("1".to_owned()))
            .expect_err("a string was taken as a tick");
        assert!(error.message.contains("a tick"), "{}", error.message);
    }

    /// Real GST numbers, and they are the whole evidence for this checksum.
    #[test]
    fn a_real_gstin_passes_and_a_typo_does_not() {
        for real in ["27AAPFU0939F1ZV", "24AAACC1206D1ZM", "29ABCDE1234F1ZW"] {
            assert!(check_gstin(real).is_ok(), "{real} was refused");
        }
        // One character changed, the rest identical.
        let error = check_gstin("29ABCDE1234F1Z6").expect_err("a bad checksum passed");
        assert!(
            error.message.contains("does not match"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_gstin_says_which_of_the_four_things_is_wrong() {
        assert!(
            check_gstin("29ABCDE1234F1Z")
                .expect_err("short")
                .message
                .contains("fifteen")
        );
        assert!(
            check_gstin("29abcde1234f1z5")
                .expect_err("lowercase")
                .message
                .contains("capital letters")
        );
        assert!(
            check_gstin("29ABCDE1234F1Y5")
                .expect_err("no Z")
                .message
                .contains("letter Z")
        );
    }

    #[test]
    fn nothing_filled_in_yet_is_allowed_everywhere() {
        // The state every shop is in on its first day.
        for shape in [
            Shape::Phone,
            Shape::Gstin,
            Shape::Fssai,
            Shape::UpiId,
            Shape::Folder,
            Shape::Free,
        ] {
            assert!(
                check_shape(shape, "").is_ok(),
                "{shape:?} refused an empty value"
            );
        }
    }

    #[test]
    fn a_phone_number_is_ten_digits_and_the_refusal_says_so() {
        assert!(check_shape(Shape::Phone, "9880012345").is_ok());
        let error = check_shape(Shape::Phone, "+919880012345").expect_err("allowed");
        assert!(error.message.contains("ten digits"), "{}", error.message);
    }

    #[test]
    fn a_upi_id_needs_both_halves() {
        assert!(check_shape(Shape::UpiId, "9880012345@okhdfcbank").is_ok());
        for bad in ["okhdfcbank", "@okhdfcbank", "9880012345@"] {
            assert!(check_shape(Shape::UpiId, bad).is_err(), "{bad} was allowed");
        }
    }

    #[test]
    fn an_amount_outside_its_range_is_refused_in_rupees_not_paise() {
        let kind = Kind::Money {
            min_paise: 0,
            max_paise: 100_000,
        };
        let error = kind
            .check(&Value::Money(Money::from_paise(500_000)))
            .expect_err("allowed");
        assert!(error.message.contains("5000.00"), "{}", error.message);
        assert!(error.message.contains("1000.00"), "{}", error.message);
        assert!(!error.message.contains("100000"), "{}", error.message);
    }
}
