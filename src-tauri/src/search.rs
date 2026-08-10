//! Item search — **audit 2.3, and budget B2.**
//!
//! > *"Type the item name. Two match modes (setting): starts with or contains.
//! > In 'contains' mode results are ranked — name-start first, then word-start,
//! > then anywhere inside. Maximum 10 suggestions."*
//!
//! # Why this is in Rust and not in React
//!
//! The same shape of argument as P09's cart:
//!
//! * **ranking is a rule.** Name-start beats word-start beats inside-word, and
//!   ties break on the shorter name. A ranking in TypeScript is a second one
//!   the moment P13 adds short codes (scope 1.3) and categories to the same
//!   box — and then two of them disagree about what the first suggestion is.
//! * the menu is already here, and P13 makes it bigger.
//! * **the round trip is not the cost.** Tauri's IPC is well under a
//!   millisecond and B1's whole budget is 16 ms. `tests/perf.rs` measures B2
//!   over 2,000 items; if the boundary ever *is* the cost, that is a finding
//!   worth having rather than an assumption worth making.
//!
//! # Ten, and why
//!
//! Not a performance limit. **A list you can choose from without reading is
//! faster than a list that is complete**, and the eleventh result has never
//! been the one a cashier wanted.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::billing::{MenuItemView, menu_view};

/// The setting from audit 2.3. Shops differ: a short menu wants starts-with,
/// a long one wants contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    StartsWith,
    #[default]
    Contains,
}

/// How well a name matched, best first.
///
/// A plain enum rather than a score, because the ordering IS the rule and a
/// number would invite somebody to tune it into something nobody can predict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    /// "dos" in "Dosa" — the name starts with what was typed.
    NameStart,
    /// "but" in "Paneer Butter Masala" — a word starts with it.
    WordStart,
    /// "utter" in "Butter" — it is in there somewhere.
    Inside,
}

pub const MAX_SUGGESTIONS: usize = 10;

/// Rank the menu against what has been typed.
///
/// Empty text gives nothing: an empty search box means the cashier is about to
/// do something else entirely (open a table, print a ticket), and filling the
/// screen with the whole menu would be in the way of all of it.
#[must_use]
pub fn search(
    items: &[mb_db::repo::menu::MenuItem],
    text: &str,
    mode: MatchMode,
) -> Vec<MenuItemView> {
    let needle = text.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }

    let mut hits: Vec<(Rank, usize, &mb_db::repo::menu::MenuItem)> = items
        .iter()
        .filter_map(|item| rank(&item.name, &needle, mode).map(|r| (r, item.name.len(), item)))
        .collect();

    // Rank, then the shorter name — because a cashier scanning a list picks
    // the first thing that looks right, and "Dosa" looks more right than
    // "Dosa Special Family Pack" when you typed "dos".
    hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.name.cmp(&b.2.name)));
    hits.truncate(MAX_SUGGESTIONS);
    hits.iter().map(|(_, _, item)| menu_view(item)).collect()
}

fn rank(name: &str, needle: &str, mode: MatchMode) -> Option<Rank> {
    let haystack = name.to_lowercase();
    if haystack.starts_with(needle) {
        return Some(Rank::NameStart);
    }
    if mode == MatchMode::StartsWith {
        return None;
    }
    // A word start is the useful middle case: typing "but" should find
    // "Paneer Butter Masala" ahead of anything that merely contains "but".
    if haystack.split_whitespace().any(|word| word.starts_with(needle)) {
        return Some(Rank::WordStart);
    }
    if haystack.contains(needle) {
        return Some(Rank::Inside);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use mb_core::{ItemId, Money, TaxRate, TaxTreatment};
    use mb_db::repo::menu::MenuItem;

    fn item(name: &str) -> MenuItem {
        MenuItem {
            tax_class_id: None,
            course: None,
            id: ItemId::new(name.to_lowercase().replace(' ', "_")),
            category_id: None,
            name: name.to_owned(),
            unit_price: Money::from_paise(12_000),
            tax_rate: TaxRate::GST_5,
            tax_treatment: TaxTreatment::Exclusive,
            hsn: None,
            cost_price: None,
            short_code: None,
            prep_minutes: None,
            is_open_price: false,
            is_available: true,
            sort_order: 0,
        }
    }

    fn names(found: &[MenuItemView]) -> Vec<&str> {
        found.iter().map(|i| i.name.as_str()).collect()
    }

    /// **T11 — the ranking, which is the rule this module exists for.**
    #[test]
    fn name_start_beats_word_start_beats_inside() {
        let menu = [
            item("Paneer Butter Masala"), // word-start on "but"
            item("Butter Naan"),          // name-start
            item("Rebuttal Special"),     // inside
        ];
        let found = search(&menu, "but", MatchMode::Contains);
        assert_eq!(
            names(&found),
            ["Butter Naan", "Paneer Butter Masala", "Rebuttal Special"]
        );
    }

    #[test]
    fn a_tie_breaks_on_the_shorter_name() {
        // A cashier scanning a list picks the first thing that looks right.
        let menu = [item("Dosa Special Family Pack"), item("Dosa")];
        assert_eq!(names(&search(&menu, "dos", MatchMode::Contains))[0], "Dosa");
    }

    #[test]
    fn starts_with_mode_refuses_a_word_start() {
        let menu = [item("Paneer Butter Masala"), item("Butter Naan")];
        assert_eq!(
            names(&search(&menu, "but", MatchMode::StartsWith)),
            ["Butter Naan"]
        );
    }

    #[test]
    fn it_is_case_insensitive_because_nobody_uses_shift_at_a_counter() {
        let menu = [item("Masala Dosa")];
        assert_eq!(search(&menu, "MASALA", MatchMode::Contains).len(), 1);
        assert_eq!(search(&menu, "masala", MatchMode::Contains).len(), 1);
    }

    #[test]
    fn never_more_than_ten() {
        let menu: Vec<MenuItem> = (0..50).map(|n| item(&format!("Dosa {n}"))).collect();
        assert_eq!(search(&menu, "dosa", MatchMode::Contains).len(), MAX_SUGGESTIONS);
    }

    #[test]
    fn an_empty_box_suggests_nothing() {
        // An empty search box means the cashier is about to open a table or
        // print a ticket, and the whole menu would be in the way of both.
        let menu = [item("Masala Dosa")];
        assert!(search(&menu, "", MatchMode::Contains).is_empty());
        assert!(search(&menu, "   ", MatchMode::Contains).is_empty());
    }
}
