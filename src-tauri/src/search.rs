//! Item search.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::billing::{MenuItemView, menu_view};
use mb_core::TaxBook;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "snake_case")]
pub enum MatchMode {
    StartsWith,
    #[default]
    Contains,
}

/// How well a name matched, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    /// The short code, exactly — what a cashier who knows the menu types.
    Code,
    /// "dos" in "Dosa" — the name starts with what was typed.
    NameStart,
    /// The code starts with it: "d1" finds "d12".
    CodeStart,
    /// "but" in "Paneer Butter Masala" — a word starts with it.
    WordStart,
    /// "utter" in "Butter" — it is in there somewhere.
    Inside,
}

pub const MAX_SUGGESTIONS: usize = 10;

/// Rank the menu against what has been typed.
#[must_use]
pub fn search(
    items: &[mb_db::repo::menu::MenuItem],
    text: &str,
    mode: MatchMode,
    book: &TaxBook,
) -> Vec<MenuItemView> {
    let needle = text.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }

    let mut hits: Vec<(Rank, usize, &mb_db::repo::menu::MenuItem)> = items
        .iter()
        .filter_map(|item| rank(item, &needle, mode).map(|r| (r, item.name.len(), item)))
        .collect();

    // Rank, then the shorter name — because a cashier scanning a list picks the first thing
    // that looks right, and "Dosa" looks more right than "Dosa Special Family Pack" when you
    // typed "dos".
    hits.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then(a.2.name.cmp(&b.2.name))
    });
    hits.truncate(MAX_SUGGESTIONS);
    hits.iter().map(|(_, _, item)| menu_view(item, book)).collect()
}

fn rank(item: &mb_db::repo::menu::MenuItem, needle: &str, mode: MatchMode) -> Option<Rank> {
    let code = item.short_code.as_deref().map(str::to_lowercase);
    if code.as_deref() == Some(needle) {
        return Some(Rank::Code);
    }
    let haystack = item.name.to_lowercase();
    if haystack.starts_with(needle) {
        return Some(Rank::NameStart);
    }
    if code.is_some_and(|c| c.starts_with(needle)) {
        return Some(Rank::CodeStart);
    }
    if mode == MatchMode::StartsWith {
        return None;
    }
    // A word start is the useful middle case: typing "but" should find "Paneer Butter Masala"
    // ahead of anything that merely contains "but".
    if haystack
        .split_whitespace()
        .any(|word| word.starts_with(needle))
    {
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
    use mb_core::{ItemId, Money, TaxRate};
    use mb_db::repo::menu::MenuItem;

    fn item(name: &str) -> MenuItem {
        MenuItem {
            course: None,
            id: ItemId::new(name.to_lowercase().replace(' ', "_")),
            category_id: None,
            name: name.to_owned(),
            unit_price: Money::from_paise(12_000),
            tax_class_id: mb_core::seeded_placement(mb_core::TaxSpec::gst(TaxRate::from_percent(5).expect("5%"))).expect("a seeded slab").0,
            price_basis: mb_core::seeded_placement(mb_core::TaxSpec::gst(TaxRate::from_percent(5).expect("5%"))).expect("a seeded slab").1,
            hsn: None,
            cost_price: None,
            short_code: None,
            prep_minutes: None,
            is_open_price: false,
            is_available: true,
            sort_order: 0,
        }
    }

    fn book() -> TaxBook {
        TaxBook::new(mb_core::starting_classes(), mb_core::PriceBasis::Exclusive)
    }

    fn names(found: &[MenuItemView]) -> Vec<&str> {
        found.iter().map(|i| i.name.as_str()).collect()
    }

    /// The ranking, which is the rule this module exists for.
    #[test]
    fn name_start_beats_word_start_beats_inside() {
        let menu = [
            item("Paneer Butter Masala"), // word-start on "but"
            item("Butter Naan"),          // name-start
            item("Rebuttal Special"),     // inside
        ];
        let found = search(&menu, "but", MatchMode::Contains, &book());
        assert_eq!(
            names(&found),
            ["Butter Naan", "Paneer Butter Masala", "Rebuttal Special"]
        );
    }

    /// A code is what a cashier who knows the menu types, and it wins outright.
    #[test]
    fn a_short_code_beats_every_name() {
        let mut coded = item("Dosa");
        coded.short_code = Some("D1".to_owned());
        let mut longer = item("Dosa Special");
        longer.short_code = Some("D12".to_owned());
        let menu = [item("D1 Sauce"), longer, coded];
        assert_eq!(
            names(&search(&menu, "d1", MatchMode::Contains, &book())),
            ["Dosa", "D1 Sauce", "Dosa Special"],
            "the exact code, then the name, then the code that starts with it"
        );
        assert_eq!(
            names(&search(&menu, "d12", MatchMode::StartsWith, &book()))[0],
            "Dosa Special",
            "a code is found in either mode"
        );
    }

    #[test]
    fn a_tie_breaks_on_the_shorter_name() {
        // A cashier scanning a list picks the first thing that looks right.
        let menu = [item("Dosa Special Family Pack"), item("Dosa")];
        assert_eq!(names(&search(&menu, "dos", MatchMode::Contains, &book()))[0], "Dosa");
    }

    #[test]
    fn starts_with_mode_refuses_a_word_start() {
        let menu = [item("Paneer Butter Masala"), item("Butter Naan")];
        assert_eq!(
            names(&search(&menu, "but", MatchMode::StartsWith, &book())),
            ["Butter Naan"]
        );
    }

    #[test]
    fn it_is_case_insensitive_because_nobody_uses_shift_at_a_counter() {
        let menu = [item("Masala Dosa")];
        assert_eq!(search(&menu, "MASALA", MatchMode::Contains, &book()).len(), 1);
        assert_eq!(search(&menu, "masala", MatchMode::Contains, &book()).len(), 1);
    }

    #[test]
    fn never_more_than_ten() {
        let menu: Vec<MenuItem> = (0..50).map(|n| item(&format!("Dosa {n}"))).collect();
        assert_eq!(
            search(&menu, "dosa", MatchMode::Contains, &book()).len(),
            MAX_SUGGESTIONS
        );
    }

    #[test]
    fn an_empty_box_suggests_nothing() {
        // An empty search box means the cashier is about to open a table or print a ticket, and
        // the whole menu would be in the way of both.
        let menu = [item("Masala Dosa")];
        assert!(search(&menu, "", MatchMode::Contains, &book()).is_empty());
        assert!(search(&menu, "   ", MatchMode::Contains, &book()).is_empty());
    }
}
