//! **Which form of the noun**, decided by the language and not by English.
//!
//! D78 said: *one function, and `(s)` is banned.* That function hard-coded
//! English's rule — `0 => "no bills", 1 => "1 bill", n => "n bills"` — and that
//! was right for one language and is wrong for three.
//!
//! # The three rules, and where they differ
//!
//! All three languages have **two** forms, which makes this look simpler than
//! it is. The forms are not chosen the same way:
//!
//! | | 0 | 1 | 2+ |
//! |---|---|---|---|
//! | English | other (*no bills*) | one | other |
//! | Hindi | **other** (*कोई बिल नहीं*) | one | other |
//! | Kannada | **other** | one | other |
//!
//! The table agrees, and that is worth writing down rather than assuming:
//! Unicode CLDR gives Hindi `one` for both 0 and 1 in its *ordinal*-adjacent
//! rules, but for **cardinal** counts — which is what a bill count is — Hindi's
//! plural rule is `i = 0 or n = 1 → one`. That means **0.5 kg is "one"** in
//! Hindi and "other" in English, which is a real difference this product can
//! reach: a quantity in kilograms on a kitchen ticket.
//!
//! So the rule is per language, and [`form`] is where it lives. It is short on
//! purpose — a plural engine is a thing to be able to read.
//!
//! # Zero says "no", and that is a wording decision, not a plural one
//!
//! D78 chose *"no bills"* over *"0 bills"* because that is how a person reads a
//! figure out loud and because a screen full of zeroes is harder to scan. That
//! is still true, and it is handled in the CATALOGUE — a `zero` string beside
//! the two forms — rather than here, because "no bills" is a translation and
//! not a grammatical form. Hindi and Kannada each have their own way of saying
//! it and neither is "0 " plus a noun.

use crate::Language;

/// Which of a noun's forms to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Form {
    /// The catalogue's `zero` string, when it has one — see the module note.
    Zero,
    One,
    Other,
}

/// **The rule, per language.**
///
/// `how_many` is an integer because every count in this product is one: bills,
/// items, days, phones, tables. A fractional quantity is a `Qty` and is
/// formatted by `mb-core`, which does not come through here.
#[must_use]
pub const fn form(language: Language, how_many: i64) -> Form {
    if how_many == 0 {
        return Form::Zero;
    }
    match language {
        // "1 bill", "3 bills". The rule sixteen sessions have used.
        Language::English => {
            if how_many == 1 {
                Form::One
            } else {
                Form::Other
            }
        }
        // CLDR `hi`: cardinal `one` when the integer part is 0 or n is 1.
        // With an integer count that reduces to n == 1 — and it is written out
        // rather than shared with English so that the day somebody adds a
        // fractional count, the difference is here to be found.
        Language::Hindi => {
            if how_many == 1 {
                Form::One
            } else {
                Form::Other
            }
        }
        // CLDR `kn`: `one` when n is 1.
        Language::Kannada => {
            if how_many == 1 {
                Form::One
            } else {
                Form::Other
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_its_own_form_in_every_language() {
        for language in Language::ALL {
            assert_eq!(form(*language, 0), Form::Zero, "{language:?}");
        }
    }

    #[test]
    fn one_is_one_and_the_rest_are_other() {
        for language in Language::ALL {
            assert_eq!(form(*language, 1), Form::One, "{language:?}");
            for n in [2_i64, 3, 11, 100, -1] {
                assert_eq!(form(*language, n), Form::Other, "{language:?} {n}");
            }
        }
    }

    /// A negative count is not a thing this product produces, but a variance on
    /// a day close is negative and somebody will one day count something with
    /// it. It must not be `One`.
    #[test]
    fn a_negative_count_is_not_singular() {
        assert_eq!(form(Language::English, -1), Form::Other);
    }
}
