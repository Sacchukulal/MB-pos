//! **What a table is called, and when two of them are the same table.**
//!
//! Audit loophole I5, kept in full because it names the exact half-fix:
//!
//! > *"Table numbers accept any text; two people can create "1" and "AC 1"
//! > that print identically (this is now guarded in the table master, but the
//! > billing screen's free-text table box is not guarded the same way)."*
//!
//! v1 had the right idea and guarded one of the two doors. So the rule lives
//! here, in the domain crate, with no database and no screen anywhere near it,
//! and **everything that can name a table calls it** — the master editor, the
//! bulk range add, and the billing screen's table box.
//!
//! # A section is part of the name
//!
//! That is the whole point. "1" in the AC room prints as `AC 1`, and a table
//! literally *named* "AC 1" with no section prints the same string. Those two
//! are one table as far as a cook holding a ticket is concerned, and the
//! second one must be refused.
//!
//! The corollary matters just as much: **"1" in AC and "1" in Garden are two
//! different tables** and both must be allowed. A rule that compares bare
//! labels forbids the most ordinary table layout in India.

use serde::{Deserialize, Serialize};

/// A sub-table letter — scope 1.6's `6A` / `6B`.
///
/// Two parties at one big table, each with their own order and their own bill.
/// The table is `6`; this is the letter, and it is stored on the ORDER
/// (`orders.sub_table`) rather than on the table, because it exists only while
/// somebody is sitting there.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SubTable(String);

/// What went wrong naming a table.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TableError {
    #[error("a table needs a name")]
    EmptyLabel,
    #[error("a sub-table is a single letter, A to Z")]
    BadSubTable,
}

impl SubTable {
    /// A single letter, upper-cased. `6a` and `6A` are the same seat.
    ///
    /// Deliberately not "any text": the letter is printed on a ticket beside a
    /// number, and `6Front-left-by-the-window` is not a thing a cook can read.
    pub fn parse(text: &str) -> Result<Self, TableError> {
        let trimmed = text.trim();
        let mut chars = trimmed.chars();
        match (chars.next(), chars.next()) {
            (Some(letter), None) if letter.is_ascii_alphabetic() => {
                Ok(SubTable(letter.to_ascii_uppercase().to_string()))
            }
            _ => Err(TableError::BadSubTable),
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// **What a table prints as** — the one string the whole system compares by.
///
/// Not stored anywhere. Storing it would make it a second copy of the truth
/// that could disagree with its own parts, which is the mistake D56 was
/// written to avoid on the other side of the app.
#[must_use]
pub fn printed_name(section: Option<&str>, label: &str) -> String {
    let label = label.trim();
    match section.map(str::trim).filter(|s| !s.is_empty()) {
        Some(section) => format!("{section} {label}"),
        None => label.to_owned(),
    }
}

/// The same, with a sub-table letter: `AC 1A`.
///
/// The letter joins the LABEL, not the section — `AC 1A`, never `AC A1` and
/// never `AC 1 A`. It reads as one seat identifier because that is what it is.
#[must_use]
pub fn printed_seat(section: Option<&str>, label: &str, sub: Option<&SubTable>) -> String {
    match sub {
        Some(sub) => printed_name(section, &format!("{}{}", label.trim(), sub.as_str())),
        None => printed_name(section, label),
    }
}

/// Are these two the same table, as far as anybody reading a ticket is
/// concerned?
#[must_use]
pub fn same_table(a: (Option<&str>, &str), b: (Option<&str>, &str)) -> bool {
    comparable(&printed_name(a.0, a.1)) == comparable(&printed_name(b.0, b.1))
}

/// The form two names are compared in: **case-folded, with runs of whitespace
/// collapsed to one space.**
///
/// `"AC  1"`, `"ac 1"` and `"AC 1"` are one table.
///
/// **This form is never stored.** The label prints verbatim — the same
/// argument `cart::normalise_note` makes about a kitchen note: quietly
/// changing what a cook reads is worse than the duplicate it prevents. So the
/// shop keeps typing `AC` in capitals if it wants to, and the comparison
/// simply does not care.
#[must_use]
pub fn comparable(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Check a proposed name against what the shop already has.
///
/// `existing` is `(id, section, label)` for every table that is not this one —
/// the caller filters itself out, because renaming a table to its own name has
/// to be allowed.
///
/// Returns the id of the table it would collide with, so the message can name
/// it: *"AC 1 already exists in Main hall"* rather than *"duplicate"*.
pub fn clashes_with<'a>(
    section: Option<&str>,
    label: &str,
    existing: impl IntoIterator<Item = (&'a str, Option<&'a str>, &'a str)>,
) -> Result<Option<&'a str>, TableError> {
    if label.trim().is_empty() {
        return Err(TableError::EmptyLabel);
    }
    let wanted = comparable(&printed_name(section, label));
    Ok(existing
        .into_iter()
        .find(|(_, their_section, their_label)| {
            comparable(&printed_name(*their_section, their_label)) == wanted
        })
        .map(|(id, _, _)| id))
}

/// The labels a "add tables 1 to 20" range would create.
///
/// Rejects a backwards or absurd range rather than creating nothing and
/// claiming success. The cap is 200 because a range is a convenience for a
/// dining room, not a bulk import: a shop with 500 tables has a different
/// problem and should be typing them into a spreadsheet.
pub fn range_labels(from: i64, to: i64, prefix: &str) -> Result<Vec<String>, TableError> {
    if to < from || to - from >= 200 {
        return Err(TableError::EmptyLabel);
    }
    let prefix = prefix.trim();
    Ok((from..=to)
        .map(|n| if prefix.is_empty() { n.to_string() } else { format!("{prefix}{n}") })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_section_is_part_of_what_a_table_prints_as() {
        assert_eq!(printed_name(Some("AC"), "1"), "AC 1");
        assert_eq!(printed_name(None, "AC 1"), "AC 1");
        assert_eq!(printed_name(None, "Counter"), "Counter");
        // An empty section is no section, not a leading space.
        assert_eq!(printed_name(Some("   "), "5"), "5");
    }

    /// **Loophole I5, as a test.** These two rows are different in the master
    /// and identical to the kitchen.
    #[test]
    fn one_in_ac_and_a_table_named_ac_one_are_the_same_table() {
        assert!(same_table((Some("AC"), "1"), (None, "AC 1")));
        assert!(same_table((Some("ac"), "1"), (None, "AC  1")));
        assert!(same_table((Some(" AC "), " 1 "), (None, "ac 1")));
    }

    /// The corollary, and the one a naive rule gets wrong: the most ordinary
    /// table layout in India has a table 1 in every room.
    #[test]
    fn the_same_number_in_two_rooms_is_two_tables() {
        assert!(!same_table((Some("AC"), "1"), (Some("Garden"), "1")));
        assert!(!same_table((Some("AC"), "1"), (Some("AC"), "2")));
    }

    #[test]
    fn a_clash_names_the_row_it_clashed_with() {
        let existing = vec![
            ("tbl_1", Some("Main hall"), "1"),
            ("tbl_ac1", None, "AC 1"),
        ];
        assert_eq!(
            clashes_with(Some("AC"), "1", existing.clone()).expect("a name"),
            Some("tbl_ac1"),
        );
        assert_eq!(clashes_with(Some("AC"), "2", existing.clone()).expect("a name"), None);
        assert_eq!(clashes_with(None, "  ", existing), Err(TableError::EmptyLabel));
    }

    #[test]
    fn a_sub_table_is_one_letter_and_prints_beside_the_number() {
        let a = SubTable::parse("a").expect("a letter");
        assert_eq!(a.as_str(), "A");
        assert_eq!(printed_seat(Some("AC"), "1", Some(&a)), "AC 1A");
        assert_eq!(printed_seat(None, "6", Some(&a)), "6A");
        assert_eq!(printed_seat(Some("AC"), "1", None), "AC 1");

        assert_eq!(SubTable::parse("AB"), Err(TableError::BadSubTable));
        assert_eq!(SubTable::parse("1"), Err(TableError::BadSubTable));
        assert_eq!(SubTable::parse(""), Err(TableError::BadSubTable));
    }

    #[test]
    fn a_range_makes_the_labels_it_says_it_will() {
        assert_eq!(range_labels(1, 3, "").expect("labels"), ["1", "2", "3"]);
        assert_eq!(range_labels(1, 2, "G").expect("labels"), ["G1", "G2"]);
        // Backwards, and absurd, both refused rather than silently empty.
        assert!(range_labels(5, 1, "").is_err());
        assert!(range_labels(1, 5_000, "").is_err());
    }
}
