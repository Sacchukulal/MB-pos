//! **What the machines plugged into a counter are saying** — P29.
//!
//! A scanner, a scale, and the barcodes a scale prints. Every function here is
//! pure: bytes and timings in, a decision out. The transports live in
//! `src-tauri`, because a serial port is an operating system's business and a
//! protocol is not.
//!
//! # Why this is worth a module of its own
//!
//! **None of these devices exists on the machine this was written on.** No
//! scanner, no scale, no pole display. So the only honest way to build them is
//! to put every decision that can be made from data into a pure function and
//! test it exhaustively — and leave the part that genuinely needs hardware as a
//! thin transport that does nothing but move bytes.
//!
//! What that buys: when the owner finally plugs a scale in and it does not
//! work, the question is *"what is it sending?"* rather than *"where is the
//! bug?"* — and the device manager screen answers the first one by showing the
//! raw bytes.

use serde::{Deserialize, Serialize};

use crate::money::Money;
use crate::qty::Qty;

// ===========================================================================
// Telling a SCAN from TYPING
// ===========================================================================

/// How fast characters have to arrive to be a scanner rather than a person.
///
/// **A barcode scanner is a keyboard.** It emulates one, types the code, and
/// presses Enter — so the billing search box gets exactly what a fast cashier
/// typing "dosa" gets, and has to tell them apart or it will look up a menu
/// item called `8901234567890`.
///
/// The signal is the GAP between keystrokes. A scanner emits characters a few
/// milliseconds apart, evenly; a human, however fast, cannot hold under about
/// 40 ms for a whole word, and their gaps vary wildly because some letter pairs
/// are easy and some are not.
///
/// **The failure that matters is the wrong one.** Missing a scan costs a
/// cashier one re-scan. Misreading a fast typist as a scan throws away what
/// they typed and searches for a barcode that does not exist — so every
/// threshold here is chosen to be safe in that direction, and
/// `a_fast_human_typist_is_never_read_as_a_scan` is the test that says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanRule {
    /// The longest average gap, in milliseconds, that still counts as a
    /// machine. 25 ms is roughly 480 characters a minute sustained — about
    /// twice what a very fast typist manages, and half what a slow scanner
    /// does.
    pub max_average_gap_ms: u32,
    /// A single gap longer than this ends the burst, whatever the average.
    /// Somebody typing, pausing to think, and typing again must not have the
    /// two halves joined into one "scan".
    pub max_single_gap_ms: u32,
    /// Shorter than this and it is not a barcode at all. The shortest real
    /// symbology in a shop is EAN-8.
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
///
/// The caller collects `(character, milliseconds since the previous one)` and
/// hands the run over when Enter is pressed. Keeping the timing OUT of this
/// module's own clock is what makes it testable: a test supplies the gaps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keystrokes {
    pub text: String,
    /// One gap per character after the first. `text.len() - 1` entries.
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

/// **Scan, or person?**
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
    // A code is digits, or digits and a letter or two. A word with spaces in it
    // is somebody searching, however fast they typed it.
    if keys.text.contains(' ') {
        return Typed::Person;
    }
    if !keys.text.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Typed::Person;
    }
    if keys.gaps_ms.is_empty() {
        // Every character arrived in the same instant — a paste, or a scanner
        // fast enough that the clock could not tell. Either way not typing.
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

// ===========================================================================
// Weight-encoded barcodes
// ===========================================================================

/// What a scale's printed label means.
///
/// **Extremely common in Indian retail** and the reason this is worth a type:
/// a sweet shop weighs 450 g of laddu, the scale prints a label, and the
/// barcode on it carries both the item and either the weight or the price.
///
/// The convention is a 13-digit EAN with a reserved prefix — usually 2x — then
/// an item code, then the embedded value, then a check digit. **Which digits
/// mean what is not standardised**, so it is a per-shop setting rather than a
/// guess; this type is the shape, and [`EmbeddedRule`] is the shop's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScannedLabel {
    /// The code that identifies the item — matched against a menu item's
    /// barcode, not against the whole 13 digits.
    pub item_code: String,
    /// What was embedded.
    pub embedded: Embedded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Embedded {
    /// Grams, millilitres or pieces — the item's own dimension decides which,
    /// exactly as it does everywhere else in this product.
    Quantity(Qty),
    /// The money the scale worked out. **The shop's own price is still used to
    /// bill**; this is compared against it, and a disagreement is worth saying
    /// out loud rather than silently trusting a label printed last Tuesday.
    Price(Money),
}

/// How one shop's scale labels are laid out.
///
/// Positions are zero-based indices into the digits, and every one of them
/// differs by brand — which is why this is data a dealer sets and not a
/// constant somebody has to recompile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddedRule {
    /// The leading digits that mark a label as one of these. "21", "22", "20".
    pub prefix: String,
    /// Where the item code starts and how long it is.
    pub item_from: usize,
    pub item_len: usize,
    /// Where the embedded number starts and how long it is.
    pub value_from: usize,
    pub value_len: usize,
    /// Whether the embedded number is a quantity or a price.
    pub value_is_price: bool,
    /// How many of the value's digits are after the decimal point. A weight in
    /// grams printed as `00450` with `value_decimals: 3` is 0.450 kg; a price
    /// printed as `01250` with `2` is ₹12.50.
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

/// **Read a scale's label.**
///
/// Pure, so it is fully testable without a scale — which matters, because
/// there is no scale on the machine this was written on and there may not be
/// one for months.
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

    let slice = |from: usize, len: usize| -> String {
        digits[from..from + len].iter().collect()
    };

    let item_code = slice(rule.item_from, rule.item_len);
    let raw: i64 = slice(rule.value_from, rule.value_len)
        .parse()
        .map_err(|_| LabelError::OutOfRange)?;

    let embedded = if rule.value_is_price {
        // Paise. Two decimals is the ordinary case; anything else is scaled to
        // paise here so the rest of the product never sees a different unit.
        let paise = match rule.value_decimals {
            0 => raw.checked_mul(100),
            1 => raw.checked_mul(10),
            2 => Some(raw),
            _ => Some(raw / 10_i64.pow(rule.value_decimals - 2)),
        }
        .ok_or(LabelError::OutOfRange)?;
        Embedded::Price(Money::from_paise(paise))
    } else {
        // `Qty` is thousandths (P01), so a value with three decimals is already
        // in its unit and anything else is scaled to it.
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

// ===========================================================================
// The scale itself
// ===========================================================================

/// One line a scale sent down the wire.
///
/// **Every brand differs**, which is why the protocol is a per-scale setting
/// and not a guess. Two are shipped and a third mode shows the raw bytes, so a
/// dealer can work out an unknown scale without a code change — and without a
/// phone call to us.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleProtocol {
    /// `ST,GS,+  1.234kg` — a status word, then a signed number and a unit.
    /// The commonest shape on Indian counter scales.
    StatusThenWeight,
    /// A bare number and unit per line: `1.234 kg`. Some cheap scales send
    /// nothing else at all, and then STABILITY has to come from the reading
    /// not changing rather than from the scale saying so — see
    /// [`Reading::stable`].
    WeightOnly,
    /// Show the bytes and decide nothing. **Not a fallback — a tool.** It is
    /// how an unknown scale gets configured, and it is the difference between
    /// "we support your scale" and "we support scales".
    Raw,
}

/// What one line meant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reading {
    pub qty: Qty,
    /// The unit as the scale said it: "kg", "g", "l".
    pub unit: String,
    /// **Whether the scale says the weight has settled.**
    ///
    /// A bouncing weight is the single thing that makes a scale integration
    /// untrustworthy: a shop puts a bag down, the software grabs 0.2 kg on the
    /// way to 1.4, and a customer is undercharged for ever. So a reading is
    /// only ever taken when this is true (T4).
    ///
    /// On a protocol that does not report it, the caller compares consecutive
    /// readings instead — the honest fallback, and it is slower on purpose.
    pub stable: bool,
}

/// A thousand kilogrammes. A counter scale that reports more than this is a
/// counter scale that has come unplugged.
const MAX_THOUSANDTHS: f64 = 1_000_000.0;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScaleError {
    #[error("the scale sent something this program could not read")]
    Unreadable,
    #[error("the scale is showing a negative weight — take everything off it and try again")]
    Negative,
}

/// **Read one line from a scale.**
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
            // `ST` is settled, `US` is unsettled. Everything before the last
            // comma is status; what follows is the number.
            let (status, rest) = line.rsplit_once(',').ok_or(ScaleError::Unreadable)?;
            (status.to_ascii_uppercase().starts_with("ST"), rest)
        }
        // Nothing in the line says whether it settled, so this alone can never
        // claim it did. The caller decides by watching two readings agree.
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

    // **Float here and integer everywhere after**, deliberately: the scale sent
    // decimal TEXT and something has to parse it. The moment it becomes a
    // quantity it becomes thousandths (P01) and no float touches it again —
    // which is the whole of D2's argument, applied at the one boundary where a
    // decimal genuinely arrives from outside.
    //
    // The range check is not ceremony: a scale that has been unplugged and is
    // sending noise can produce a number that does not fit, and `as` would
    // wrap it into a plausible-looking weight rather than refusing.
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

/// **Two readings agreeing is what stability means on a scale that will not
/// say.**
///
/// Slower than asking, and honest about it: a shop with a cheap scale waits
/// half a second longer, and never gets a weight taken on the way up.
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

    // -- scan or person ----------------------------------------------------

    #[test]
    fn a_scanner_burst_is_a_scan() {
        // A real scanner: thirteen digits, a few milliseconds apart.
        assert_eq!(
            how_it_arrived(&keys("8901234567890", 4), ScanRule::default()),
            Typed::Scan
        );
    }

    /// **The failure that matters.** Missing a scan costs one re-scan;
    /// misreading a typist throws away what they typed and looks up a barcode
    /// that does not exist.
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
        // Typed fast, thought about it, typed fast again. Not one scan.
        let mut k = keys("89012345678", 5);
        k.gaps_ms[4] = 400;
        assert_eq!(how_it_arrived(&k, ScanRule::default()), Typed::Person);
    }

    #[test]
    fn something_too_short_to_be_a_barcode_is_typing() {
        assert_eq!(how_it_arrived(&keys("890123", 3), ScanRule::default()), Typed::Person);
    }

    #[test]
    fn a_paste_with_no_gaps_at_all_is_not_typing() {
        let pasted = Keystrokes {
            text: "8901234567890".to_owned(),
            gaps_ms: Vec::new(),
        };
        assert_eq!(how_it_arrived(&pasted, ScanRule::default()), Typed::Scan);
    }

    // -- scale labels ------------------------------------------------------

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
        // 21 · 12345 · 00450 · 6  →  item 12345, 0.450
        let label = read_label("2112345004506", &weight_rule()).expect("reads");
        assert_eq!(label.item_code, "12345");
        assert_eq!(label.embedded, Embedded::Quantity(Qty::from_thousandths(450)));
    }

    #[test]
    fn a_price_label_reads_as_an_item_and_money() {
        let rule = EmbeddedRule {
            value_is_price: true,
            value_decimals: 2,
            ..weight_rule()
        };
        // 21 · 12345 · 01250 · 6  →  item 12345, ₹12.50
        let label = read_label("2112345012506", &rule).expect("reads");
        assert_eq!(label.embedded, Embedded::Price(Money::from_paise(1_250)));
    }

    #[test]
    fn a_label_that_is_not_ours_says_so_rather_than_guessing() {
        // An ordinary product barcode, not a scale label. Guessing would bill
        // somebody 890 kg of something.
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

    // -- the scale ---------------------------------------------------------

    #[test]
    fn a_settled_reading_reads() {
        let r = read_scale("ST,GS,+  1.234kg", ScaleProtocol::StatusThenWeight).expect("reads");
        assert_eq!(r.qty, Qty::from_thousandths(1_234));
        assert_eq!(r.unit, "kg");
        assert!(r.stable);
    }

    /// **T4.** A weight on its way up is never taken.
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
        assert!(settled(Some(&first), &second), "two agreeing readings settled");
    }

    #[test]
    fn a_negative_weight_says_what_to_do_about_it() {
        // A scale that was tared with something on it. The message tells a
        // shopkeeper what to do rather than reporting a minus sign (§6).
        let err = read_scale("ST,GS,-  0.500kg", ScaleProtocol::StatusThenWeight)
            .expect_err("refused");
        assert_eq!(err, ScaleError::Negative);
        assert!(err.to_string().contains("take everything off it"));
    }

    #[test]
    fn raw_mode_decides_nothing_because_that_is_its_job() {
        // It exists so a dealer can SEE what an unknown scale sends. A reading
        // out of it would be a guess dressed as a fact.
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
