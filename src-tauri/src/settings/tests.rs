//! What keeps the catalogue honest.
//!
//! The first test is the important one. Everything else in `settings/` is
//! ordinary code that either works or does not; the catalogue is a **second
//! list**, and audit E6 is what happens to a second list nobody checks.

use super::catalog::{CATALOG, Group};
use super::value::{Kind, Value};
use super::*;

/// Every leaf of `ShopConfig`, as a dotted path.
///
/// Built from serde rather than from a hand-written list, because a
/// hand-written list is a *third* copy and would need its own guard.
fn leaf_paths(value: &serde_json::Value, prefix: &str, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                leaf_paths(child, &path, out);
            }
        }
        _ => out.push(prefix.to_owned()),
    }
}

fn config_leaves() -> Vec<String> {
    let json = serde_json::to_value(ShopConfig::default()).expect("the config serialises");
    let mut out = Vec::new();
    leaf_paths(&json, "", &mut out);
    out.sort();
    out
}

/// **T10 — the completeness guard, and the reason this module is one table.**
///
/// A field added to `ReceiptSettings` with no catalogue line fails here. So
/// does a catalogue line for a field that has been removed. Both directions,
/// because only one direction is the half that lets a setting go dark.
#[test]
fn the_catalogue_is_the_whole_of_the_configuration() {
    let leaves = config_leaves();
    let mut keys: Vec<String> = CATALOG.iter().map(|e| e.key.to_owned()).collect();
    keys.sort();

    let missing: Vec<&String> = leaves.iter().filter(|l| !keys.contains(l)).collect();
    assert!(
        missing.is_empty(),
        "these parts of the configuration have no setting, so nobody can ever \
         change them — add a line to catalog.rs: {missing:?}"
    );

    let ghosts: Vec<&String> = keys.iter().filter(|k| !leaves.contains(k)).collect();
    assert!(
        ghosts.is_empty(),
        "these settings point at nothing — delete the line: {ghosts:?}"
    );
}

#[test]
fn nothing_is_in_the_catalogue_twice() {
    let mut seen = std::collections::BTreeSet::new();
    for entry in CATALOG {
        assert!(seen.insert(entry.key), "{} is in the catalogue twice", entry.key);
    }
}

/// A default that its own validation refuses would make `reset` impossible and
/// `load` fail on a fresh shop.
#[test]
fn every_default_is_valid() {
    let defaults = ShopConfig::default();
    for entry in CATALOG {
        let value = (entry.read)(&defaults);
        assert!(
            entry.kind.check(&value).is_ok(),
            "the default for {} is refused by its own rule: {:?}",
            entry.key,
            entry.kind.check(&value)
        );
    }
}

/// Reading a setting and writing it straight back must be a no-op. If it is
/// not, the pair disagree — which is the bug the whole file exists to prevent.
#[test]
fn every_entry_round_trips_through_its_own_pair() {
    let defaults = ShopConfig::default();
    for entry in CATALOG {
        let mut config = defaults.clone();
        let value = (entry.read)(&config);
        (entry.write)(&mut config, &value)
            .unwrap_or_else(|e| panic!("{} refused its own value: {}", entry.key, e.message));
        assert_eq!(
            (entry.read)(&config),
            value,
            "{} does not read back what it was written",
            entry.key
        );
    }
}

/// Every entry says something a person can read, and every choice's stored
/// value is stable enough to be a stored value.
#[test]
fn every_entry_is_written_for_a_shopkeeper() {
    for entry in CATALOG {
        assert!(!entry.label.is_empty(), "{} has no label", entry.key);
        assert!(
            entry.label.chars().next().is_some_and(char::is_uppercase),
            "{}'s label should start like a sentence: {}",
            entry.key,
            entry.label
        );
        for synonym in entry.synonyms {
            assert_eq!(
                *synonym,
                synonym.to_lowercase(),
                "{}'s synonyms are matched lower-case, so they must be stored that way",
                entry.key
            );
        }
        if let Kind::Choice(options) = entry.kind {
            assert!(!options.is_empty(), "{} is a choice of nothing", entry.key);
            for option in options {
                assert!(!option.label.is_empty(), "{} has an unlabelled choice", entry.key);
            }
        }
    }
}

/// **Every setting sits under a heading, and headings come in runs.**
///
/// The screen draws a heading when it changes, so a topic that appears, stops
/// and comes back again would draw the same heading twice with unrelated
/// settings between. Found by looking at thirty-nine receipt settings in one
/// flat grid; this is what stops it happening again.
#[test]
fn every_setting_has_a_heading_and_the_headings_do_not_interleave() {
    for group in Group::ALL {
        let mut seen: Vec<&str> = Vec::new();
        let mut previous = "";
        for entry in CATALOG.iter().filter(|e| e.group == *group) {
            let topic = catalog::topic_for(entry);
            assert!(!topic.is_empty(), "{} has no heading", entry.key);
            if topic == previous {
                continue;
            }
            assert!(
                !seen.contains(&topic),
                "\"{topic}\" comes back after something else in {}, so the screen \
                 would draw it twice — move {} beside its own kind in CATALOG",
                group.label(),
                entry.key
            );
            seen.push(topic);
            previous = topic;
        }
    }
}

/// **T9.** An owner must never hunt.
#[test]
fn search_finds_a_setting_by_the_word_a_person_would_type() {
    for (typed, expected) in [
        ("QR", "receipt.qr"),
        ("thank you", "receipt.footer"),
        ("roundoff", "billing.rounding"),
        ("round off", "billing.rounding"),
        ("logo", "receipt.logo"),
        ("5 am", "day.starts_at_minutes"),
        ("igst", "store.default_place_of_supply"),
        ("pen drive", "backup.second_folder"),
        ("hsn", "receipt.show.hsn"),
        ("kot", "kitchen.show_title"),
    ] {
        let found = super::search(typed);
        assert!(
            found.iter().any(|e| e.key == expected),
            "searching for {typed:?} did not find {expected} — it found {:?}",
            found.iter().map(|e| e.key).collect::<Vec<_>>()
        );
    }
}

/// A section's name finds the whole section.
///
/// It finds a little more than the section — "This shop has no kitchen ticket"
/// is a Billing setting whose label contains the words — and that is right. A
/// person typing "kitchen ticket" wants everything about kitchen tickets, not
/// everything filed under a heading.
#[test]
fn searching_by_section_finds_that_section() {
    let found = super::search("kitchen ticket");
    let in_section = CATALOG.iter().filter(|e| e.group == Group::Kitchen).count();
    assert_eq!(
        found.iter().filter(|e| e.group == Group::Kitchen).count(),
        in_section,
        "a section search must find every setting in that section"
    );
    assert!(
        found.iter().any(|e| e.key == "billing.kitchen_ticket_off"),
        "and the ones outside it that are about the same thing"
    );
}

/// **T4, on the two halves that can only be checked together.**
#[test]
fn a_gstin_that_does_not_match_its_state_is_refused() {
    let mut config = ShopConfig::default();
    config.store.state_code = "29".to_owned(); // Karnataka
    config.store.gstin = "29ABCDE1234F1ZW".to_owned();
    assert!(catalog::check_gstin_against_state(&config).is_ok());

    // A real Kerala number on a Karnataka shop. Both halves are individually
    // valid, which is exactly why nobody catches this until a return bounces.
    config.store.gstin = "32ABCDE1234F1Z9".to_owned();
    let error = catalog::check_gstin_against_state(&config).expect_err("it was allowed");
    assert!(error.message.contains("Kerala"), "{}", error.message);
    assert_eq!(error.key, Some("store.gstin"));
}

/// **Reset says what it will do, and does only that.**
#[test]
fn resetting_one_section_leaves_the_others_alone() {
    let mut config = ShopConfig::default();
    config.receipt.footer = "Come back soon".to_owned();
    config.billing.idle_lock_minutes = 45;

    let after = super::reset_group(&config, Group::Receipt);
    assert_eq!(after.receipt.footer, ShopConfig::default().receipt.footer);
    assert_eq!(after.billing.idle_lock_minutes, 45, "the billing section moved");
}

/// **T7's pure half** — the whole configuration survives a round trip through
/// the export format. The database half is in `settings_tests.rs`.
#[test]
fn a_configuration_survives_being_written_out_and_read_back() {
    let mut config = ShopConfig::default();
    // Move every kind of value, so the round trip is not proving that booleans
    // work.
    config.receipt.footer = "Visit again!".to_owned();
    config.receipt.qr_width_pct = 55;
    config.billing.packing_charge = mb_core::Money::from_paise(1_500);
    config.billing.search_mode = crate::search::MatchMode::StartsWith;
    config.store.name = "Anna Kuteera".to_owned();
    config.store.is_composition = true;
    config.day.starts_at_minutes = 240;

    let file = super::to_map(&config);
    let (restored, plan) = super::plan_import(&ShopConfig::default(), &file);
    assert!(plan.is_usable(), "{:?}", plan.problems);
    assert!(plan.unknown.is_empty());
    assert_eq!(restored, config);
}

/// **T15.** One bad value and the file changes nothing at all.
#[test]
fn an_import_with_one_bad_value_changes_nothing() {
    let current = ShopConfig::default();
    let mut file = super::to_map(&current);
    file.insert("receipt.footer".to_owned(), serde_json::json!("Come back soon"));
    file.insert("receipt.logo_width_pct".to_owned(), serde_json::json!(400));

    let (wanted, plan) = super::plan_import(&current, &file);
    assert!(!plan.is_usable());
    assert!(
        plan.problems.iter().any(|p| p.contains("Logo width")),
        "the problem should name the setting in words: {:?}",
        plan.problems
    );
    // And the good value in the same file did NOT get through.
    assert_eq!(wanted, current);
}

/// A configuration written by a NEWER build must still be usable.
#[test]
fn an_unknown_key_is_reported_and_not_fatal() {
    let current = ShopConfig::default();
    let mut file = super::to_map(&current);
    file.insert(
        "receipt.show.horoscope".to_owned(),
        serde_json::json!(true),
    );
    let (_, plan) = super::plan_import(&current, &file);
    assert!(plan.is_usable(), "{:?}", plan.problems);
    assert_eq!(plan.unknown, vec!["receipt.show.horoscope".to_owned()]);
}

/// The wrong sort of value in the file is a problem, not a coercion.
#[test]
fn a_value_of_the_wrong_sort_in_a_file_is_refused() {
    let current = ShopConfig::default();
    let mut file = super::to_map(&current);
    file.insert("receipt.show.token".to_owned(), serde_json::json!("yes"));
    let (_, plan) = super::plan_import(&current, &file);
    assert!(!plan.is_usable());
    assert!(
        plan.problems.iter().any(|p| p.contains("wrong sort")),
        "{:?}",
        plan.problems
    );
}

/// A charge is added for the order type that earned it, and for no other.
#[test]
fn charges_follow_the_order_type() {
    use mb_core::{ChargeKind, OrderType};
    let mut billing = Billing::default();
    assert!(
        billing.charges_for(OrderType::DineIn).is_empty(),
        "a shop that has not been asked charges nothing"
    );

    billing.service_charge_bp = 500;
    billing.packing_charge = mb_core::Money::from_paise(1_000);
    billing.delivery_charge = mb_core::Money::from_paise(3_000);

    let dine_in = billing.charges_for(OrderType::DineIn);
    assert_eq!(dine_in.len(), 1);
    assert_eq!(dine_in[0].kind, ChargeKind::Service);

    let parcel = billing.charges_for(OrderType::Parcel);
    assert_eq!(parcel.len(), 1);
    assert_eq!(parcel[0].kind, ChargeKind::Packing);
    // A parcel is not a table: no service charge on food nobody served.
    assert!(!parcel.iter().any(|c| c.kind == ChargeKind::Service));

    let delivery = billing.charges_for(OrderType::Delivery);
    assert_eq!(delivery.len(), 1);
    assert_eq!(delivery[0].kind, ChargeKind::Delivery);
}

/// The day rule reaches `mb_core` intact, and a corrupt one does not stop the
/// shop billing.
#[test]
fn the_day_start_becomes_a_day_rule() {
    assert_eq!(Day::default().rule(), mb_core::DayRule::DEFAULT);
    assert_eq!(
        Day {
            starts_at_minutes: 240,
            ..Day::default()
        }
        .rule()
        .starts_at_minutes(),
        240
    );
    // Past the end of a day: refused by the screen (T4), so reaching this means
    // the row is corrupt — and a corrupt row must not stop a bill being dated.
    assert_eq!(
        Day {
            starts_at_minutes: 5_000,
            ..Day::default()
        }
        .rule(),
        mb_core::DayRule::DEFAULT
    );
}

/// Empty means "not filled in yet", and that is the only rule about it.
#[test]
fn an_empty_field_becomes_null_and_comes_back_empty() {
    let store = Store::default();
    let profile = store.to_profile();
    assert_eq!(profile.phone, None);
    assert_eq!(profile.gstin, None);
    assert_eq!(Store::from_profile(&profile), store);
}

#[test]
fn describing_a_value_never_shows_a_tag() {
    assert_eq!(super::describe(&Value::Bool(true)), "on");
    assert_eq!(super::describe(&Value::Bool(false)), "off");
    assert_eq!(super::describe(&Value::Text(String::new())), "(nothing)");
    assert_eq!(
        super::describe(&Value::Money(mb_core::Money::from_paise(1_250))),
        "12.50"
    );
}

/// **P31 — the two font lists cannot drift apart.**
///
/// `catalog::FONTS` is what the settings screen offers; `mb_print::font::FAMILIES`
/// is what the print queue resolves against. They are two tables because one is
/// a `&'static [Choice]` read at start-up and the other is a fact about
/// typefaces that belongs in the printing crate — and two tables of the same
/// thing is exactly how a shop ends up choosing a face that silently does not
/// exist. So the build fails instead.
#[test]
fn every_typeface_on_the_settings_screen_is_one_the_printer_knows() {
    use super::catalog::FONTS;

    let offered: Vec<&str> = FONTS.iter().map(|c| c.value).collect();
    let known: Vec<&str> = mb_print::font::FAMILIES.iter().map(|f| f.key).collect();
    assert_eq!(
        offered, known,
        "the settings screen and mb-print disagree about which faces exist"
    );

    for choice in FONTS {
        let family = mb_print::font::family(choice.value)
            .unwrap_or_else(|| panic!("{} is offered and unknown to mb-print", choice.value));
        assert_eq!(
            choice.label, family.label,
            "{} is called two different things in two places",
            choice.value
        );
    }
}

/// The owner asked for sizes in px. This is that, made mechanical: a label that
/// goes back to "Large" fails the build.
#[test]
fn the_text_sizes_are_given_in_px() {
    use super::catalog::SIZES;

    for choice in SIZES {
        assert!(
            choice.label.contains(" px"),
            "the owner asked for sizes in px, and this one says {:?}",
            choice.label
        );
    }
    // 24, 48 and 72 — the paper's own cell (12x24 dots) times the ESC/POS
    // multiplier. If a paper size ever changes those, this is where it is said.
    let labels: Vec<&str> = SIZES.iter().map(|c| c.label).collect();
    assert!(labels[0].starts_with("24 px"), "{labels:?}");
    assert!(labels[1].starts_with("48 px"), "{labels:?}");
    assert!(labels[2].starts_with("72 px"), "{labels:?}");
}
