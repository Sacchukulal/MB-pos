//! What the machines plugged into a counter are saying.

use serde::{Deserialize, Serialize};

use crate::money::Money;
use crate::qty::Qty;

// Telling a SCAN from TYPING.

/// How fast characters have to arrive to be a scanner rather than a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanRule {
    /// The longest average gap, in milliseconds, that still counts as a machine.
    pub max_average_gap_ms: u32,
    /// A single gap longer than this ends the burst, whatever the average.
    pub max_single_gap_ms: u32,
    /// Shorter than this and it is not a barcode at all.
    pub min_length: usize,
}

impl Default for ScanRule {
    fn default() -> Self {
        ScanRule {
            max_average_gap_ms: 25,
            max_single_gap_ms: 80,
            min_length: 8,
        }
    }
}

/// What arrived in the search box, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keystrokes {
    pub text: String,
    /// One gap per character after the first.
    pub gaps_ms: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Typed {
    /// A machine. Look this up as a code.
    Scan,
    /// A person. Search the menu with it.
    Person,
}

/// Scan, or person?
#[must_use]
#[allow(
    clippy::integer_division,
    reason = "an average of whole milliseconds: the remainder is a fraction of a 
              millisecond and no decision here turns on one"
)]
pub fn how_it_arrived(keys: &Keystrokes, rule: ScanRule) -> Typed {
    let count = keys.text.chars().count();
    if count < rule.min_length {
        return Typed::Person;
    }
    // A code is digits, or digits and a letter or two.
    if keys.text.contains(' ') {
        return Typed::Person;
    }
    if !keys
        .text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Typed::Person;
    }
    if keys.gaps_ms.is_empty() {
        // Every character arrived in the same instant — a paste, or a scanner fast enough that
        // the clock could not tell.
        return Typed::Scan;
    }
    if keys.gaps_ms.iter().any(|gap| *gap > rule.max_single_gap_ms) {
        return Typed::Person;
    }
    let total: u64 = keys.gaps_ms.iter().map(|g| u64::from(*g)).sum();
    let average = total / keys.gaps_ms.len() as u64;
    if average <= u64::from(rule.max_average_gap_ms) {
        Typed::Scan
    } else {
        Typed::Person
    }
}

// Weight-encoded barcodes.

/// What a scale's printed label means.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScannedLabel {
    /// The code that identifies the item — matched against a menu item's barcode, not against
    /// the whole 13 digits.
    pub item_code: String,
    /// What was embedded.
    pub embedded: Embedded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Embedded {
    /// Grams, millilitres or pieces — the item's own dimension decides which, exactly as it
    /// does everywhere else in this product.
    Quantity(Qty),
    /// The money the scale worked out.
    Price(Money),
}

/// How one shop's scale labels are laid out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddedRule {
    /// The leading digits that mark a label as one of these.
    pub prefix: String,
    /// Where the item code starts and how long it is.
    pub item_from: usize,
    pub item_len: usize,
    /// Where the embedded number starts and how long it is.
    pub value_from: usize,
    pub value_len: usize,
    /// Whether the embedded number is a quantity or a price.
    pub value_is_price: bool,
    /// How many of the value's digits are after the decimal point.
    pub value_decimals: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LabelError {
    #[error("that is not one of this shop's scale labels")]
    NotOurs,
    #[error("that label is {found} digits long and this shop's are at least {needed}")]
    TooShort { found: usize, needed: usize },
    #[error("that label has something other than digits in it")]
    NotDigits,
    #[error("the number inside that label is too large to be a weight or a price")]
    OutOfRange,
}

/// Read a scale's label.
#[allow(
    clippy::integer_division,
    reason = "scaling a label's digits down to paise or thousandths: the division 
              IS the operation, and the digits it drops are below the unit this 
              product stores"
)]
pub fn read_label(code: &str, rule: &EmbeddedRule) -> Result<ScannedLabel, LabelError> {
    let digits: Vec<char> = code.trim().chars().collect();
    if !digits.iter().all(char::is_ascii_digit) {
        return Err(LabelError::NotDigits);
    }
    if !code.trim().starts_with(&rule.prefix) {
        return Err(LabelError::NotOurs);
    }

    let needed = (rule.item_from + rule.item_len).max(rule.value_from + rule.value_len);
    if digits.len() < needed {
        return Err(LabelError::TooShort {
            found: digits.len(),
            needed,
        });
    }

    let slice = |from: usize, len: usize| -> String { digits[from..from + len].iter().collect() };

    let item_code = slice(rule.item_from, rule.item_len);
    let raw: i64 = slice(rule.value_from, rule.value_len)
        .parse()
        .map_err(|_| LabelError::OutOfRange)?;

    let embedded = if rule.value_is_price {
        // Paise. Two decimals is the ordinary case; anything else is scaled to paise here so
        // the rest of the product never sees a different unit.
        let paise = match rule.value_decimals {
            0 => raw.checked_mul(100),
            1 => raw.checked_mul(10),
            2 => Some(raw),
            _ => Some(raw / 10_i64.pow(rule.value_decimals - 2)),
        }
        .ok_or(LabelError::OutOfRange)?;
        Embedded::Price(Money::from_paise(paise))
    } else {
        // `Qty` is thousandths, so a value with three decimals is already in its unit and
        // anything else is scaled to it.
        let thousandths = match rule.value_decimals {
            0 => raw.checked_mul(1_000),
            1 => raw.checked_mul(100),
            2 => raw.checked_mul(10),
            3 => Some(raw),
            _ => Some(raw / 10_i64.pow(rule.value_decimals - 3)),
        }
        .ok_or(LabelError::OutOfRange)?;
        Embedded::Quantity(Qty::from_thousandths(thousandths))
    };

    Ok(ScannedLabel {
        item_code,
        embedded,
    })
}

// The scale itself.

/// One line a scale sent down the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleProtocol {
    /// `ST,GS,+ 1.234kg` — a status word, then a signed number and a unit.
    StatusThenWeight,
    /// A bare number and unit per line: `1.234 kg`.
    WeightOnly,
    /// Show the bytes and decide nothing.
    Raw,
}

/// What one line meant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reading {
    pub qty: Qty,
    /// The unit as the scale said it: "kg", "g", "l".
    pub unit: String,
    /// Whether the scale says the weight has settled.
    pub stable: bool,
}

/// A thousand kilogrammes. A counter scale that reports more than this is a counter scale that
/// has come unplugged.
const MAX_THOUSANDTHS: f64 = 1_000_000.0;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScaleError {
    #[error("the scale sent something this program could not read")]
    Unreadable,
    #[error("the scale is showing a negative weight — take everything off it and try again")]
    Negative,
}

/// Read one line from a scale.
#[allow(
    clippy::float_arithmetic,
    reason = "the ONE boundary where a decimal arrives from outside this 
              product: a scale sends decimal text. It becomes thousandths on 
              the next line and no float touches it again (D2)"
)]
pub fn read_scale(line: &str, protocol: ScaleProtocol) -> Result<Reading, ScaleError> {
    let line = line.trim();
    if line.is_empty() {
        return Err(ScaleError::Unreadable);
    }

    let (stable, rest) = match protocol {
        ScaleProtocol::StatusThenWeight => {
            // `ST` is settled, `US` is unsettled.
            let (status, rest) = line.rsplit_once(',').ok_or(ScaleError::Unreadable)?;
            (status.to_ascii_uppercase().starts_with("ST"), rest)
        }
        // Nothing in the line says whether it settled, so this alone can never claim it did.
        ScaleProtocol::WeightOnly => (false, line),
        ScaleProtocol::Raw => return Err(ScaleError::Unreadable),
    };

    let cleaned: String = rest
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '+')
        .collect();
    let split = cleaned
        .find(|c: char| c.is_ascii_alphabetic())
        .unwrap_or(cleaned.len());
    let (number, unit) = cleaned.split_at(split);

    let value: f64 = number.parse().map_err(|_| ScaleError::Unreadable)?;
    if value < 0.0 {
        return Err(ScaleError::Negative);
    }

    // Float here and integer everywhere after, deliberately: the scale sent decimal TEXT and
    // something has to parse it.
    let scaled = (value * 1_000.0).round();
    if !scaled.is_finite() || scaled > MAX_THOUSANDTHS {
        return Err(ScaleError::Unreadable);
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "guarded by the range check on the line above"
    )]
    let thousandths = scaled as i64;

    Ok(Reading {
        qty: Qty::from_thousandths(thousandths),
        unit: if unit.is_empty() {
            "kg".to_owned()
        } else {
            unit.to_ascii_lowercase()
        },
        stable,
    })
}

/// Two readings agreeing is what stability means on a scale that will not say.
#[must_use]
pub fn settled(previous: Option<&Reading>, current: &Reading) -> bool {
    if current.stable {
        return true;
    }
    previous.is_some_and(|p| p.qty == current.qty && p.unit == current.unit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(text: &str, gap: u32) -> Keystrokes {
        Keystrokes {
            text: text.to_owned(),
            gaps_ms: vec![gap; text.chars().count().saturating_sub(1)],
        }
    }

    // Scan or person.

    #[test]
    fn a_scanner_burst_is_a_scan() {
        // A real scanner: thirteen digits, a few milliseconds apart.
        assert_eq!(
            how_it_arrived(&keys("8901234567890", 4), ScanRule::default()),
            Typed::Scan
        );
    }

    /// The failure that matters.
    #[test]
    fn a_fast_human_typist_is_never_read_as_a_scan() {
        // 60 ms a character is about 200 characters a minute — genuinely fast.
        assert_eq!(
            how_it_arrived(&keys("paneerbutter", 60), ScanRule::default()),
            Typed::Person
        );
        // Even an implausible 35 ms sustained is still a person.
        assert_eq!(
            how_it_arrived(&keys("paneerbutter", 35), ScanRule::default()),
            Typed::Person
        );
        // A word with a space is somebody searching, however fast.
        assert_eq!(
            how_it_arrived(&keys("paneer butter", 5), ScanRule::default()),
            Typed::Person
        );
    }

    #[test]
    fn a_pause_in_the_middle_ends_the_burst() {
        // Typed fast, thought about it, typed fast again.
        let mut k = keys("89012345678", 5);
        k.gaps_ms[4] = 400;
        assert_eq!(how_it_arrived(&k, ScanRule::default()), Typed::Person);
    }

    #[test]
    fn something_too_short_to_be_a_barcode_is_typing() {
        assert_eq!(
            how_it_arrived(&keys("890123", 3), ScanRule::default()),
            Typed::Person
        );
    }

    #[test]
    fn a_paste_with_no_gaps_at_all_is_not_typing() {
        let pasted = Keystrokes {
            text: "8901234567890".to_owned(),
            gaps_ms: Vec::new(),
        };
        assert_eq!(how_it_arrived(&pasted, ScanRule::default()), Typed::Scan);
    }

    // Scale labels.

    fn weight_rule() -> EmbeddedRule {
        // `21` + five item digits + five weight digits + check digit.
        EmbeddedRule {
            prefix: "21".to_owned(),
            item_from: 2,
            item_len: 5,
            value_from: 7,
            value_len: 5,
            value_is_price: false,
            value_decimals: 3,
        }
    }

    #[test]
    fn a_weight_label_reads_as_an_item_and_a_quantity() {
        // 21 · 12345 · 00450 · 6 → item 12345, 0.450.
        let label = read_label("2112345004506", &weight_rule()).expect("reads");
        assert_eq!(label.item_code, "12345");
        assert_eq!(
            label.embedded,
            Embedded::Quantity(Qty::from_thousandths(450))
        );
    }

    #[test]
    fn a_price_label_reads_as_an_item_and_money() {
        let rule = EmbeddedRule {
            value_is_price: true,
            value_decimals: 2,
            ..weight_rule()
        };
        // 21 · 12345 · 01250 · 6 → item 12345, ₹12.50.
        let label = read_label("2112345012506", &rule).expect("reads");
        assert_eq!(label.embedded, Embedded::Price(Money::from_paise(1_250)));
    }

    #[test]
    fn a_label_that_is_not_ours_says_so_rather_than_guessing() {
        // An ordinary product barcode, not a scale label.
        assert_eq!(
            read_label("8901234567890", &weight_rule()),
            Err(LabelError::NotOurs)
        );
        assert_eq!(
            read_label("21abc45004506", &weight_rule()),
            Err(LabelError::NotDigits)
        );
        assert!(matches!(
            read_label("211234", &weight_rule()),
            Err(LabelError::TooShort { .. })
        ));
    }

    // The scale.

    #[test]
    fn a_settled_reading_reads() {
        let r = read_scale("ST,GS,+  1.234kg", ScaleProtocol::StatusThenWeight).expect("reads");
        assert_eq!(r.qty, Qty::from_thousandths(1_234));
        assert_eq!(r.unit, "kg");
        assert!(r.stable);
    }

    /// A weight on its way up is never taken.
    #[test]
    fn a_bouncing_weight_is_never_taken() {
        let moving =
            read_scale("US,GS,+  0.200kg", ScaleProtocol::StatusThenWeight).expect("reads");
        assert!(!moving.stable);
        assert!(!settled(None, &moving), "an unsettled scale was believed");

        // On a scale that will not say, two agreeing readings are the answer.
        let first = read_scale("0.200 kg", ScaleProtocol::WeightOnly).expect("reads");
        let second = read_scale("0.200 kg", ScaleProtocol::WeightOnly).expect("reads");
        let different = read_scale("0.850 kg", ScaleProtocol::WeightOnly).expect("reads");
        assert!(!settled(None, &first), "one reading is never settled");
        assert!(!settled(Some(&first), &different), "it was still moving");
        assert!(
            settled(Some(&first), &second),
            "two agreeing readings settled"
        );
    }

    #[test]
    fn a_negative_weight_says_what_to_do_about_it() {
        // A scale that was tared with something on it.
        let err =
            read_scale("ST,GS,-  0.500kg", ScaleProtocol::StatusThenWeight).expect_err("refused");
        assert_eq!(err, ScaleError::Negative);
        assert!(err.to_string().contains("take everything off it"));
    }

    #[test]
    fn raw_mode_decides_nothing_because_that_is_its_job() {
        // It exists so a dealer can SEE what an unknown scale sends.
        assert_eq!(
            read_scale("ST,GS,+  1.234kg", ScaleProtocol::Raw),
            Err(ScaleError::Unreadable)
        );
    }

    #[test]
    fn rubbish_from_the_wire_is_refused_rather_than_believed() {
        assert!(read_scale("", ScaleProtocol::WeightOnly).is_err());
        assert!(read_scale("hello", ScaleProtocol::WeightOnly).is_err());
        assert!(read_scale("ST,GS,+  abc kg", ScaleProtocol::StatusThenWeight).is_err());
    }
}
