//! What a table is called, and when two of them are the same table.

use serde::{Deserialize, Serialize};

/// A sub-table letter.
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
    /// A single letter, upper-cased.
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

/// What a table prints as — the one string the whole system compares by.
#[must_use]
pub fn printed_name(section: Option<&str>, label: &str) -> String {
    let label = label.trim();
    match section.map(str::trim).filter(|s| !s.is_empty()) {
        Some(section) => format!("{section} {label}"),
        None => label.to_owned(),
    }
}

/// The same, with a sub-table letter: `AC 1A`.
#[must_use]
pub fn printed_seat(section: Option<&str>, label: &str, sub: Option<&SubTable>) -> String {
    match sub {
        Some(sub) => printed_name(section, &format!("{}{}", label.trim(), sub.as_str())),
        None => printed_name(section, label),
    }
}

/// Are these two the same table, as far as anybody reading a ticket is concerned?
#[must_use]
pub fn same_table(a: (Option<&str>, &str), b: (Option<&str>, &str)) -> bool {
    comparable(&printed_name(a.0, a.1)) == comparable(&printed_name(b.0, b.1))
}

/// The form two names are compared in: case-folded, with runs of whitespace collapsed to one
/// space.
#[must_use]
pub fn comparable(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Check a proposed name against what the shop already has.
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
pub fn range_labels(from: i64, to: i64, prefix: &str) -> Result<Vec<String>, TableError> {
    if to < from || to - from >= 200 {
        return Err(TableError::EmptyLabel);
    }
    let prefix = prefix.trim();
    Ok((from..=to)
        .map(|n| {
            if prefix.is_empty() {
                n.to_string()
            } else {
                format!("{prefix}{n}")
            }
        })
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

    #[test]
    fn one_in_ac_and_a_table_named_ac_one_are_the_same_table() {
        assert!(same_table((Some("AC"), "1"), (None, "AC 1")));
        assert!(same_table((Some("ac"), "1"), (None, "AC  1")));
        assert!(same_table((Some(" AC "), " 1 "), (None, "ac 1")));
    }

    /// The corollary, and the one a naive rule gets wrong: the most ordinary table layout in
    /// India has a table 1 in every room.
    #[test]
    fn the_same_number_in_two_rooms_is_two_tables() {
        assert!(!same_table((Some("AC"), "1"), (Some("Garden"), "1")));
        assert!(!same_table((Some("AC"), "1"), (Some("AC"), "2")));
    }

    #[test]
    fn a_clash_names_the_row_it_clashed_with() {
        let existing = vec![("tbl_1", Some("Main hall"), "1"), ("tbl_ac1", None, "AC 1")];
        assert_eq!(
            clashes_with(Some("AC"), "1", existing.clone()).expect("a name"),
            Some("tbl_ac1"),
        );
        assert_eq!(
            clashes_with(Some("AC"), "2", existing.clone()).expect("a name"),
            None
        );
        assert_eq!(
            clashes_with(None, "  ", existing),
            Err(TableError::EmptyLabel)
        );
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
