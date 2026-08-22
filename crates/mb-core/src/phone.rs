//! **A phone number, and the one place that decides what one is.**
//!
//! The owner, 2026-08-22, from a real install: *"i noticed while adding a
//! credit customer, shop details, i can enter alphabets, more than 10 numbers,
//! fix it, this app is india only so only 10 digits needed."*
//!
//! # Why this is a type and not a check in four screens
//!
//! It was four places, and none of them agreed. `settings::value::Shape::Phone`
//! held the ten-digit rule and refused on save. `mb_core::credit::phone_key`
//! took the *last* ten digits of anything with ten or more, because a customer
//! is identified by their number and two spellings of one number are one
//! person. The supplier form and the staff emergency contact checked **nothing
//! at all** — a name typed into the phone box was stored as a phone number.
//!
//! So the rule lives here, every command that stores a phone parses through it,
//! and adding a fifth phone field to the product means calling [`Phone::parse`]
//! rather than remembering a regex.
//!
//! # What it accepts, and why it is not stricter
//!
//! Ten digits after the punctuation is thrown away. `+91`, a leading trunk `0`,
//! spaces, dashes and brackets are all how a number arrives from somebody's
//! contact list, and refusing a paste is how you make a person retype
//! something the program could have understood.
//!
//! It does **not** insist the first digit is 6–9. That is the rule for an
//! Indian *mobile*, and a shop's landline is a phone number too — a counter
//! that argues with the number on the shutter is wrong more often than the shop
//! is. Same reasoning as `Shape::Gstin` after the owner's ruling of 2026-08-16.

use std::fmt;

/// India, and the owner's instruction. The bill is 32 columns wide.
pub const PHONE_DIGITS: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PhoneError {
    #[error("a phone number here is {PHONE_DIGITS} digits — this has {had}")]
    WrongLength { had: usize },
    #[error("a phone number is digits only")]
    NotDigits,
}

/// Exactly ten digits. Constructed only by [`Phone::parse`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Phone(String);

impl Phone {
    /// **The rule.**
    ///
    /// # Errors
    ///
    /// [`PhoneError::NotDigits`] when there is a letter in it, and
    /// [`PhoneError::WrongLength`] when what is left is not ten digits.
    pub fn parse(typed: &str) -> Result<Phone, PhoneError> {
        let trimmed = typed.trim();

        // **A letter is a different mistake from a wrong length**, and saying
        // which one is the difference between "you typed your name in the wrong
        // box" and "you missed a digit". Punctuation a person genuinely writes
        // in a phone number is not a letter and is dropped below.
        if trimmed
            .chars()
            .any(|c| c.is_alphabetic() || (!c.is_ascii_digit() && !is_punctuation(c)))
        {
            return Err(PhoneError::NotDigits);
        }

        let digits: String = trimmed.chars().filter(char::is_ascii_digit).collect();

        // The two shapes a number arrives in from a phone: with the country
        // code, and with the trunk prefix. Only when the rest is exactly ten,
        // so a real ten-digit number that happens to start 91 survives.
        let digits = match digits.len() {
            12 if digits.starts_with("91") => digits[2..].to_owned(),
            11 if digits.starts_with('0') => digits[1..].to_owned(),
            _ => digits,
        };

        if digits.len() != PHONE_DIGITS {
            return Err(PhoneError::WrongLength { had: digits.len() });
        }
        Ok(Phone(digits))
    }

    /// The same rule for a box somebody may leave empty.
    ///
    /// Most phone fields in this product are optional — a shop with no supplier
    /// number is a shop, and refusing to save the supplier over it would be the
    /// counter arguing about something it does not need.
    ///
    /// # Errors
    ///
    /// As [`Phone::parse`], for anything that is not blank.
    pub fn parse_optional(typed: &str) -> Result<Option<Phone>, PhoneError> {
        if typed.trim().is_empty() {
            return Ok(None);
        }
        Phone::parse(typed).map(Some)
    }

    /// The ten digits. What is stored, printed and compared.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Phone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What somebody may write between the digits of a phone number.
const fn is_punctuation(c: char) -> bool {
    matches!(c, ' ' | '-' | '+' | '(' | ')' | '.' | '/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_digits_is_a_phone_number() {
        assert_eq!(Phone::parse("9840011223").expect("ten").as_str(), "9840011223");
    }

    /// **However it arrives from somebody's contact list.** Refusing a paste is
    /// how you make a person retype what the program could have understood.
    #[test]
    fn the_punctuation_people_actually_write_is_thrown_away() {
        for typed in [
            "+91 98400 11223",
            "+919840011223",
            "098400-11223",
            "(98400) 11223",
            " 98400 11223 ",
            "98400.11223",
        ] {
            assert_eq!(
                Phone::parse(typed).map(|p| p.as_str().to_owned()),
                Ok("9840011223".to_owned()),
                "{typed:?}",
            );
        }
    }

    /// The owner's complaint, in one test: *"i can enter alphabets."*
    #[test]
    fn a_name_typed_into_the_phone_box_is_refused() {
        for typed in ["Ravi Kumar", "98400abcde", "nine eight four"] {
            assert_eq!(Phone::parse(typed), Err(PhoneError::NotDigits), "{typed:?}");
        }
    }

    /// And the other half: *"more than 10 numbers."*
    #[test]
    fn nine_digits_and_eleven_digits_are_both_refused() {
        assert_eq!(
            Phone::parse("984001122"),
            Err(PhoneError::WrongLength { had: 9 }),
        );
        assert_eq!(
            Phone::parse("98400112233"),
            Err(PhoneError::WrongLength { had: 11 }),
            "eleven that does not start with a trunk zero is eleven",
        );
        assert_eq!(
            Phone::parse("9840011223344"),
            Err(PhoneError::WrongLength { had: 13 }),
        );
    }

    /// **A ten-digit number starting 91 is not a country code**, and the
    /// stripping rule must not eat it.
    #[test]
    fn a_number_that_begins_ninety_one_survives() {
        assert_eq!(Phone::parse("9188776655").expect("ten").as_str(), "9188776655");
    }

    /// **A landline is a phone number.** The 6-to-9 rule is for mobiles, and a
    /// counter that argues with the number on the shutter is wrong more often
    /// than the shop is — the same ruling the owner gave about GST numbers on
    /// 2026-08-16.
    #[test]
    fn a_landline_is_not_argued_with() {
        assert!(Phone::parse("0442345678").is_ok(), "Chennai, with the trunk 0");
        assert!(Phone::parse("4412345678").is_ok());
    }

    #[test]
    fn blank_is_no_phone_rather_than_a_bad_one() {
        assert_eq!(Phone::parse_optional("   "), Ok(None));
        assert_eq!(Phone::parse_optional(""), Ok(None));
        assert!(Phone::parse_optional("nope").is_err());
        assert_eq!(
            Phone::parse_optional("+91 98400 11223")
                .expect("valid")
                .map(|p| p.as_str().to_owned()),
            Some("9840011223".to_owned()),
        );
    }

    /// The refusal has to say which mistake it was — see `Phone::parse`.
    #[test]
    fn the_two_refusals_say_different_things() {
        let letters = Phone::parse("Ravi").expect_err("letters").to_string();
        let short = Phone::parse("98400").expect_err("short").to_string();
        assert!(letters.contains("digits only"), "{letters}");
        assert!(short.contains("10 digits"), "{short}");
        assert!(short.contains('5'), "it must say what was actually typed: {short}");
    }
}
