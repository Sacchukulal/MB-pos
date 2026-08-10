//! **Magic Bill in English, Hindi and Kannada.**
//!
//! > The people least likely to read English are the ones using this all day.
//! > An owner reads English. A waiter at 9 pm on a Saturday, a cashier three
//! > weeks into the job, the person who takes over the till when somebody goes
//! > home early — those are the people this crate is for.
//!
//! # Why this is one crate and not a hundred call sites
//!
//! `src-tauri/src/words.rs` is *"the one place a machine state becomes words"*
//! — crown jewel 14 — and sixteen sessions have kept it. `mb-license` and
//! `mb-lan` carry the same rule for their own refusals. So there is one place
//! per crate to key, and this is what they all key against.
//!
//! # The three rules, and each of them is a bug that would otherwise ship
//!
//! **1. Keyed by MEANING, never by English text.** `"cart.empty"`, not
//! `"Cart is empty"`. An English string used as a key means every English
//! wording change silently orphans two translations — and an orphan shows up
//! as English on a Kannada receipt, which is the exact failure this crate
//! exists to prevent.
//!
//! **2. A key exists in all three languages, or it does not exist.**
//! [`Entry`] has three `&'static str` fields and no default. There is no test
//! for completeness because there is nothing to test: a translation cannot be
//! forgotten, because the struct will not build without it. What CAN be
//! forgotten is putting real Hindi in the Hindi field, and
//! [`State::NeedsReview`] is how that is counted rather than hidden — see
//! `examples/missing.rs`.
//!
//! **3. Interpolation by NAMED placeholder, never by concatenation.**
//! `{name} owes {amount}`. Hindi and Kannada both put the verb last; a
//! sentence assembled from fragments cannot be reordered, and one assembled
//! from fragments in English word order is a sentence a Kannada reader parses
//! twice.
//!
//! # Money does not move
//!
//! Indian grouping, the rupee sign, and **Latin digits in every interface
//! language**. The currency does not change with the reader; a shopkeeper
//! reading a total is reading a number whose shape they already know; and
//! Devanagari digits on a bill would be a novelty nobody asked for. D2 and R2
//! are untouched — the string still comes from Rust, formatted once, and this
//! crate never sees an amount.

pub mod catalogue;
pub mod plural;

pub use catalogue::{CATALOGUE, Entry, State};
pub use plural::Form;

use serde::{Deserialize, Serialize};

/// The languages this build speaks.
///
/// **Not a locale.** There is no `en-IN` versus `en-GB` here and there must not
/// be: the product is sold in one country, money is formatted one way (see the
/// module note), and a locale string is a door onto a problem this shop does
/// not have.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    /// **The default, and it is a decision rather than an accident of
    /// ordering.** A counter whose language setting has never been touched — or
    /// whose setting will not parse — shows English, because English is the
    /// only one of the three this session can vouch for (§7). Defaulting to a
    /// language nobody has reviewed would put unverified words on a bill for
    /// every shop that never opened Settings.
    #[default]
    English,
    Hindi,
    Kannada,
}

impl Language {
    pub const ALL: &'static [Language] =
        &[Language::English, Language::Hindi, Language::Kannada];

    /// The stored form. **This is a database value**: changing one is a
    /// migration, not a rename.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Hindi => "hi",
            Language::Kannada => "kn",
        }
    }

    /// **The language's own name, in its own script.** A person choosing a
    /// language cannot read the list in a language they do not read, which is
    /// why this is never "Hindi" and "Kannada" in English.
    #[must_use]
    pub const fn endonym(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Hindi => "हिन्दी",
            Language::Kannada => "ಕನ್ನಡ",
        }
    }

    /// Whether this language needs a shaper and a face the thermal printer's
    /// built-in font does not have — see `mb-print` and P23 part 5.
    #[must_use]
    pub const fn is_latin(self) -> bool {
        matches!(self, Language::English)
    }

    #[must_use]
    pub fn from_code(code: &str) -> Option<Language> {
        Language::ALL.iter().copied().find(|l| l.code() == code)
    }
}

/// What can be put into a placeholder.
///
/// Deliberately small. **There is no `Money` variant and there must not be**:
/// an amount reaches this crate already formatted, as a string, by the one
/// function in `mb-core` that formats one (D2). A second formatter behind a
/// translation is a second answer to "how much is this".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Count(i64),
}

impl From<&str> for Value {
    fn from(text: &str) -> Value {
        Value::Text(text.to_owned())
    }
}

impl From<String> for Value {
    fn from(text: String) -> Value {
        Value::Text(text)
    }
}

impl From<i64> for Value {
    fn from(count: i64) -> Value {
        Value::Count(count)
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Text(text) => f.write_str(text),
            Value::Count(count) => write!(f, "{count}"),
        }
    }
}

/// **Look a string up and fill it in.**
///
/// # A missing key is visible, not silent
///
/// It comes back as `⟦key⟧`. Not the English text: falling back to English is
/// how a half-translated app hides its gaps, and the whole premise of this
/// session is that *a half-translated app is worse than an untranslated one,
/// because the user cannot predict which half*. A bracketed key on a screen is
/// a bug report; English on a Kannada screen is a shrug.
///
/// The build catches these first — see `every_key_used_in_the_product_exists`
/// — so this is the belt to that braces.
#[must_use]
pub fn t(language: Language, key: &str, args: &[(&str, Value)]) -> String {
    let Some(entry) = catalogue::find(key) else {
        return format!("⟦{key}⟧");
    };
    fill(entry.in_language(language), args)
}

/// The plural form, chosen by the LANGUAGE's own rule.
///
/// D78 unchanged: *one function, and `(s)` is banned.* What changes is that
/// "which form" is no longer English's answer for everybody — see [`plural`].
#[must_use]
pub fn count(language: Language, how_many: i64, key: &str, args: &[(&str, Value)]) -> String {
    let Some(entry) = catalogue::find(key) else {
        return format!("⟦{key}⟧");
    };
    let text = entry.plural_in(language, plural::form(language, how_many));
    let mut filled = fill(text, args);
    // `{count}` is implicit, so a caller never has to pass the number it just
    // passed. Every plural string may use it and most do.
    filled = filled.replace("{count}", &how_many.to_string());
    filled
}

/// Replace `{name}` with what the caller gave.
///
/// **An unfilled placeholder is left visible**, for the same reason a missing
/// key is: `{amount}` on a screen is a bug somebody reports, and an empty gap
/// is one nobody notices until a customer does.
fn fill(text: &str, args: &[(&str, Value)]) -> String {
    if args.is_empty() || !text.contains('{') {
        return text.to_owned();
    }
    let mut out = text.to_owned();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), &value.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_language_round_trips_through_its_code() {
        for language in Language::ALL {
            assert_eq!(Language::from_code(language.code()), Some(*language));
        }
        assert_eq!(Language::from_code("ta"), None);
    }

    /// **A person cannot read a language list in a language they do not read.**
    #[test]
    fn every_language_names_itself_in_its_own_script() {
        assert_eq!(Language::Hindi.endonym(), "हिन्दी");
        assert_eq!(Language::Kannada.endonym(), "ಕನ್ನಡ");
        // And none of them is named in English.
        assert!(!Language::Hindi.endonym().is_ascii());
        assert!(!Language::Kannada.endonym().is_ascii());
    }

    /// **A missing key is loud.** Falling back to English is how a
    /// half-translated app hides its gaps.
    #[test]
    fn a_missing_key_is_visible_and_not_english() {
        let missing = t(Language::Kannada, "nothing.here", &[]);
        assert_eq!(missing, "⟦nothing.here⟧");
        assert!(missing.contains('⟦'), "a missing key must be obvious");
    }

    #[test]
    fn placeholders_are_filled_by_name() {
        let text = fill(
            "{who} owes {amount}",
            &[
                ("who", Value::from("Ravi")),
                ("amount", Value::from("1,200.00")),
            ],
        );
        assert_eq!(text, "Ravi owes 1,200.00");
    }

    /// **Order does not matter**, which is the whole reason placeholders are
    /// named: Hindi and Kannada put the verb last and the arguments arrive in
    /// whatever order the caller had them.
    #[test]
    fn the_order_of_the_arguments_is_not_the_order_of_the_sentence() {
        let args = [
            ("amount", Value::from("1,200.00")),
            ("who", Value::from("Ravi")),
        ];
        assert_eq!(fill("{who} owes {amount}", &args), "Ravi owes 1,200.00");
        // The same arguments, a sentence built the other way round.
        assert_eq!(fill("{amount} — {who}", &args), "1,200.00 — Ravi");
    }

    /// An unfilled placeholder stays visible rather than becoming a gap.
    #[test]
    fn an_unfilled_placeholder_is_left_where_somebody_will_see_it() {
        assert_eq!(fill("{who} owes {amount}", &[("who", Value::from("Ravi"))]), "Ravi owes {amount}");
    }

    /// `{count}` is implicit — a caller never passes the number twice.
    #[test]
    fn a_plural_fills_its_own_number() {
        let one = count(Language::English, 1, "bills.settled", &[]);
        let many = count(Language::English, 4, "bills.settled", &[]);
        assert!(one.contains('1'), "{one}");
        assert!(many.contains('4'), "{many}");
        assert_ne!(one, many);
    }
}
