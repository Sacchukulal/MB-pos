//! **Every string in the product, once, in three languages.**
//!
//! # How to add one
//!
//! A row. There is no second step, no bundle to regenerate and no file to keep
//! in step — and **you cannot add a row without all three languages**, because
//! [`Entry`] has three fields and no default. "Every key exists in every
//! language" is therefore not a test that runs; it is the type.
//!
//! What the type CANNOT check is whether the Hindi field contains Hindi.
//! [`State`] is how that is counted rather than hidden:
//!
//! * [`State::Reviewed`] — a person who reads the language has signed it off.
//! * [`State::NeedsReview`] — plausible, and **not yet trustworthy**. A session
//!   can produce these; it cannot tell a plausible string from a wrong one, and
//!   "wrong word on a bill" includes *void*, *refund*, *credit* and *day close*
//!   — words a shopkeeper is answerable to a tax officer for.
//! * [`State::DeliberatelyEnglish`] — see the policy below.
//!
//! `cargo run -p mb-i18n --example missing` counts them, so the gap is a number
//! and not a feeling.
//!
//! # THE POLICY ON BORROWED WORDS — read before "fixing" one
//!
//! Where a term has no good local equivalent, **the English word in the local
//! script** beats a coined one nobody uses. बिल, not a Sanskritised
//! construction; ಬಿಲ್, not a translation committee's word.
//!
//! This is how the words are actually said in a shop in Bengaluru or Pune, and
//! a receipt is not the place to teach somebody new vocabulary. The next
//! person to read this file will want to "improve" these; that is why the rule
//! is written down here rather than assumed.

use crate::plural::Form;
use crate::Language;

/// How far a row's translations can be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    /// Signed off by somebody who reads the language.
    Reviewed,
    /// Plausible, unverified. **Counted, never hidden** — see the module note.
    NeedsReview,
    /// The same word in all three, on purpose: a borrowed term, a proper noun,
    /// or a format a person does not read as prose.
    DeliberatelyEnglish,
}

/// One string, in three languages.
///
/// **Three fields, no default, no `Option`.** That is the completeness rule,
/// and making it structural is why this crate has no "missing key" test to run
/// at build time — there is nothing to miss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// **Keyed by meaning.** `"cart.empty"`, never `"Cart is empty"`.
    pub key: &'static str,
    pub en: &'static str,
    pub hi: &'static str,
    pub kn: &'static str,
    /// The other two forms, when this string counts something. `None` for an
    /// ordinary string.
    pub plural: Option<Plurals>,
    pub state: State,
}

/// The `zero` and `other` forms. The `one` form is [`Entry`]'s own text, so an
/// ordinary string and a singular are the same field and cannot drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plurals {
    pub zero_en: &'static str,
    pub zero_hi: &'static str,
    pub zero_kn: &'static str,
    pub other_en: &'static str,
    pub other_hi: &'static str,
    pub other_kn: &'static str,
}

impl Entry {
    #[must_use]
    pub const fn in_language(&self, language: Language) -> &'static str {
        match language {
            Language::English => self.en,
            Language::Hindi => self.hi,
            Language::Kannada => self.kn,
        }
    }

    /// The right form, in the right language. Falls back to the singular when
    /// a row has no plural block — which is a caller using `count` on a string
    /// that does not count anything, and shows up as the singular rather than
    /// as a crash.
    #[must_use]
    pub const fn plural_in(&self, language: Language, form: Form) -> &'static str {
        let Some(plurals) = self.plural else {
            return self.in_language(language);
        };
        match (form, language) {
            (Form::One, _) => self.in_language(language),
            (Form::Zero, Language::English) => plurals.zero_en,
            (Form::Zero, Language::Hindi) => plurals.zero_hi,
            (Form::Zero, Language::Kannada) => plurals.zero_kn,
            (Form::Other, Language::English) => plurals.other_en,
            (Form::Other, Language::Hindi) => plurals.other_hi,
            (Form::Other, Language::Kannada) => plurals.other_kn,
        }
    }
}

/// A row with no plural forms.
macro_rules! say {
    ($key:literal, $state:ident, $en:literal, $hi:literal, $kn:literal) => {
        Entry {
            key: $key,
            en: $en,
            hi: $hi,
            kn: $kn,
            plural: None,
            state: State::$state,
        }
    };
}

/// A row that counts something. `{count}` is filled automatically.
macro_rules! counted {
    ($key:literal, $state:ident,
     $zero_en:literal / $one_en:literal / $other_en:literal,
     $zero_hi:literal / $one_hi:literal / $other_hi:literal,
     $zero_kn:literal / $one_kn:literal / $other_kn:literal) => {
        Entry {
            key: $key,
            en: $one_en,
            hi: $one_hi,
            kn: $one_kn,
            plural: Some(Plurals {
                zero_en: $zero_en,
                zero_hi: $zero_hi,
                zero_kn: $zero_kn,
                other_en: $other_en,
                other_hi: $other_hi,
                other_kn: $other_kn,
            }),
            state: State::$state,
        }
    };
}

/// **The catalogue.**
///
/// P23 delivers the whole mechanism, English complete, and Hindi and Kannada
/// for **the receipt and the counter's own words** — the strings a waiter and a
/// customer actually read. Everything here that is not `Reviewed` is waiting
/// for a person; `examples/missing.rs` counts them.
pub const CATALOGUE: &[Entry] = &[
    // --- the paper. The half a CUSTOMER reads, and the half that matters
    //     most: a receipt is the only part of this product that leaves the
    //     shop.
    say!("receipt.subtotal", NeedsReview, "Subtotal", "उप-कुल", "ಉಪಮೊತ್ತ"),
    say!("receipt.total", NeedsReview, "Total", "कुल", "ಒಟ್ಟು"),
    say!("receipt.round_off", NeedsReview, "Round off", "राउंड ऑफ", "ರೌಂಡ್ ಆಫ್"),
    say!("receipt.discount", NeedsReview, "Discount", "छूट", "ರಿಯಾಯಿತಿ"),
    say!("receipt.qty", NeedsReview, "Qty", "मात्रा", "ಪ್ರಮಾಣ"),
    say!("receipt.rate", NeedsReview, "Rate", "दर", "ದರ"),
    say!("receipt.amount", NeedsReview, "Amount", "राशि", "ಮೊತ್ತ"),
    say!("receipt.item", NeedsReview, "Item", "वस्तु", "ವಸ್ತು"),
    say!("receipt.thank_you", NeedsReview,
        "Thank you, please visit again",
        "धन्यवाद, फिर से पधारें",
        "ಧನ್ಯವಾದಗಳು, ಮತ್ತೆ ಬನ್ನಿ"),
    say!("receipt.table", NeedsReview, "Table", "टेबल", "ಟೇಬಲ್"),
    say!("receipt.cashier", NeedsReview, "Cashier", "कैशियर", "ಕ್ಯಾಷಿಯರ್"),
    say!("receipt.duplicate", NeedsReview, "DUPLICATE", "डुप्लिकेट", "ನಕಲು"),
    say!("receipt.voided", NeedsReview, "VOIDED", "रद्द", "ರದ್ದು"),
    // **The borrowed words**, and the policy note above is about exactly
    // these. "GST" is GST on a bill in Pune and in Bengaluru; nobody says
    // anything else, and a translated tax heading is a bill a tax officer
    // queries.
    say!("receipt.gst", DeliberatelyEnglish, "GST", "GST", "GST"),
    say!("receipt.cgst", DeliberatelyEnglish, "CGST", "CGST", "CGST"),
    say!("receipt.sgst", DeliberatelyEnglish, "SGST", "SGST", "SGST"),
    say!("receipt.hsn", DeliberatelyEnglish, "HSN", "HSN", "HSN"),
    say!("receipt.gstin", DeliberatelyEnglish, "GSTIN", "GSTIN", "GSTIN"),
    // "Bill" is बिल and ಬಿಲ್ — the English word in the local script, which is
    // what a shopkeeper says. Not a coined equivalent.
    say!("receipt.bill_no", NeedsReview, "Bill No", "बिल नं", "ಬಿಲ್ ನಂ"),

    // --- the kitchen ticket. Read by a cook, at speed, in a hot room.
    say!("kitchen.new", NeedsReview, "NEW", "नया", "ಹೊಸದು"),
    say!("kitchen.added", NeedsReview, "ADDED", "और जोड़ा", "ಸೇರಿಸಲಾಗಿದೆ"),
    say!("kitchen.cancelled", NeedsReview, "CANCELLED", "रद्द", "ರದ್ದು"),
    say!("kitchen.token", NeedsReview, "Token", "टोकन", "ಟೋಕನ್"),

    // --- the counter's own words. The half a WAITER reads all shift.
    say!("billing.dine_in", NeedsReview, "Dine in", "यहीं खाना", "ಇಲ್ಲಿ ಊಟ"),
    say!("billing.parcel", NeedsReview, "Parcel", "पार्सल", "ಪಾರ್ಸೆಲ್"),
    say!("billing.self_service", NeedsReview, "Self service", "स्वयं सेवा", "ಸ್ವಯಂ ಸೇವೆ"),
    say!("billing.delivery", NeedsReview, "Delivery", "डिलीवरी", "ಡೆಲಿವರಿ"),
    say!("billing.complete", NeedsReview, "Complete bill", "बिल पूरा करें", "ಬಿಲ್ ಪೂರ್ಣಗೊಳಿಸಿ"),
    say!("billing.new_order", NeedsReview, "New order", "नया ऑर्डर", "ಹೊಸ ಆರ್ಡರ್"),
    say!("billing.cancel_order", NeedsReview, "Cancel order", "ऑर्डर रद्द करें", "ಆರ್ಡರ್ ರದ್ದುಮಾಡಿ"),
    say!("billing.kitchen_ticket", NeedsReview, "Kitchen ticket", "किचन टिकट", "ಅಡುಗೆಮನೆ ಟಿಕೆಟ್"),
    say!("billing.empty", NeedsReview,
        "Nothing on this bill yet",
        "इस बिल में अभी कुछ नहीं है",
        "ಈ ಬಿಲ್‌ನಲ್ಲಿ ಇನ್ನೂ ಏನೂ ಇಲ್ಲ"),
    say!("billing.press_an_item", NeedsReview,
        "Press an item to add it.",
        "जोड़ने के लिए किसी वस्तु को दबाएँ।",
        "ಸೇರಿಸಲು ಒಂದು ವಸ್ತುವನ್ನು ಒತ್ತಿರಿ."),
    say!("payment.cash", NeedsReview, "Cash", "नकद", "ನಗದು"),
    say!("payment.card", NeedsReview, "Card", "कार्ड", "ಕಾರ್ಡ್"),
    say!("payment.credit", NeedsReview, "Credit", "उधार", "ಸಾಲ"),
    // UPI is UPI. Nobody has ever said anything else.
    say!("payment.upi", DeliberatelyEnglish, "UPI", "UPI", "UPI"),

    // --- counting things. D78's rule, three times over.
    counted!("bills.settled", NeedsReview,
        "no bills" / "1 bill" / "{count} bills",
        "कोई बिल नहीं" / "1 बिल" / "{count} बिल",
        "ಬಿಲ್‌ಗಳಿಲ್ಲ" / "1 ಬಿಲ್" / "{count} ಬಿಲ್‌ಗಳು"),
    counted!("items.on_bill", NeedsReview,
        "no items" / "1 item" / "{count} items",
        "कोई वस्तु नहीं" / "1 वस्तु" / "{count} वस्तुएँ",
        "ವಸ್ತುಗಳಿಲ್ಲ" / "1 ವಸ್ತು" / "{count} ವಸ್ತುಗಳು"),
    counted!("days.left", NeedsReview,
        "no days" / "1 day" / "{count} days",
        "कोई दिन नहीं" / "1 दिन" / "{count} दिन",
        "ದಿನಗಳಿಲ್ಲ" / "1 ದಿನ" / "{count} ದಿನಗಳು"),
];

/// Find a row. Linear, and deliberately: the catalogue is a few hundred rows,
/// a lookup happens when a screen paints rather than per bill, and a map would
/// be a lazily-initialised global to save a microsecond nobody is waiting on.
#[must_use]
pub fn find(key: &str) -> Option<&'static Entry> {
    CATALOGUE.iter().find(|entry| entry.key == key)
}

/// How many rows still need a person, per language. The `missing` example
/// prints it; a future session can gate a release on it.
#[must_use]
pub fn needing_review() -> usize {
    CATALOGUE
        .iter()
        .filter(|entry| entry.state == State::NeedsReview)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_key_appears_twice() {
        let mut seen = std::collections::BTreeSet::new();
        for entry in CATALOGUE {
            assert!(seen.insert(entry.key), "{} is in the catalogue twice", entry.key);
        }
    }

    /// **Keyed by meaning.** A key that is the English text is the failure mode
    /// this whole design exists to prevent, and it looks like a key.
    #[test]
    fn keys_are_meanings_and_not_sentences() {
        for entry in CATALOGUE {
            assert!(
                entry.key.contains('.'),
                "{} is not a dotted key",
                entry.key
            );
            assert!(
                !entry.key.contains(' '),
                "{} looks like a sentence, not a key",
                entry.key
            );
            assert!(
                entry.key.chars().all(|c| c.is_ascii_lowercase() || c == '.' || c == '_'),
                "{} is not a stable key",
                entry.key
            );
        }
    }

    /// **Nothing is empty.** The type makes a translation impossible to forget;
    /// this makes an empty one impossible to sneak in.
    #[test]
    fn every_row_says_something_in_every_language() {
        for entry in CATALOGUE {
            for language in Language::ALL {
                let text = entry.in_language(*language);
                assert!(
                    !text.trim().is_empty(),
                    "{} is empty in {language:?}",
                    entry.key
                );
            }
        }
    }

    /// **A translated row is actually translated.** Three identical strings
    /// means somebody pasted English into all three fields, and the only
    /// legitimate reason for that is `DeliberatelyEnglish` — which has to be
    /// chosen, not defaulted into.
    #[test]
    fn a_row_that_is_english_three_times_says_it_meant_to_be() {
        for entry in CATALOGUE {
            if entry.en == entry.hi && entry.en == entry.kn {
                assert_eq!(
                    entry.state,
                    State::DeliberatelyEnglish,
                    "{} is English in all three languages. If that is right \
                     (GST, UPI, a proper noun), mark it DeliberatelyEnglish and \
                     say why beside it; if it is not, translate it.",
                    entry.key
                );
            }
        }
    }

    /// And the reverse: a row marked `DeliberatelyEnglish` that is NOT the same
    /// three times is a mislabelled row.
    #[test]
    fn a_deliberately_english_row_is_english_three_times() {
        for entry in CATALOGUE {
            if entry.state == State::DeliberatelyEnglish {
                assert_eq!(entry.en, entry.hi, "{}", entry.key);
                assert_eq!(entry.en, entry.kn, "{}", entry.key);
            }
        }
    }

    /// The Hindi and Kannada fields must be in Hindi and Kannada — a Latin
    /// string in the Kannada column is the mistake that produces English on a
    /// Kannada receipt, and it is invisible to a reviewer skimming.
    #[test]
    fn the_indic_columns_are_in_indic_scripts() {
        for entry in CATALOGUE {
            if entry.state == State::DeliberatelyEnglish {
                continue;
            }
            assert!(
                entry.hi.chars().any(|c| ('\u{0900}'..='\u{097F}').contains(&c)),
                "{}'s Hindi has no Devanagari in it: {:?}",
                entry.key,
                entry.hi
            );
            assert!(
                entry.kn.chars().any(|c| ('\u{0C80}'..='\u{0CFF}').contains(&c)),
                "{}'s Kannada has no Kannada script in it: {:?}",
                entry.key,
                entry.kn
            );
        }
    }

    /// A counted row must differ between its forms, or the plural does nothing.
    #[test]
    fn a_counted_row_actually_counts() {
        for entry in CATALOGUE {
            let Some(plurals) = entry.plural else { continue };
            for language in Language::ALL {
                let one = entry.plural_in(*language, Form::One);
                let other = entry.plural_in(*language, Form::Other);
                let zero = entry.plural_in(*language, Form::Zero);
                assert_ne!(one, other, "{} in {language:?}", entry.key);
                assert_ne!(one, zero, "{} in {language:?}", entry.key);
            }
            // And `(s)` is still banned — D78.
            for text in [plurals.other_en, plurals.other_hi, plurals.other_kn] {
                assert!(!text.contains("(s)"), "{}: {text}", entry.key);
            }
        }
    }

    /// **The borrowed-word policy, as a test.** GST is GST.
    #[test]
    fn tax_headings_are_not_translated() {
        for key in ["receipt.gst", "receipt.cgst", "receipt.sgst", "receipt.hsn"] {
            let entry = find(key).unwrap_or_else(|| panic!("{key} is missing"));
            assert_eq!(entry.state, State::DeliberatelyEnglish, "{key}");
        }
    }

    /// **The gap is a number.** If this ever reads zero, somebody has reviewed
    /// the catalogue and the release note should say so.
    #[test]
    fn the_review_gap_is_countable() {
        let waiting = needing_review();
        assert!(waiting > 0, "if this is zero, update the master plan and RELEASE.md");
        assert!(
            waiting < CATALOGUE.len(),
            "some rows are settled — the tax headings at least"
        );
    }
}
