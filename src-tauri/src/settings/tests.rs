//! What keeps the catalogue honest.

use super::catalog::{CATALOG, Group};
use super::value::{Kind, Value};
use super::*;

/// Every leaf of `ShopConfig`, as a dotted path.
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

/// The completeness guard, and the reason this module is one table.
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
        assert!(
            seen.insert(entry.key),
            "{} is in the catalogue twice",
            entry.key
        );
    }
}

/// A default that its own validation refuses would make `reset` impossible and `load` fail on a
/// fresh shop.
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

/// Reading a setting and writing it straight back must be a no-op.
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

/// Every entry says something a person can read, and every choice's stored value is stable
/// enough to be a stored value.
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
                assert!(
                    !option.label.is_empty(),
                    "{} has an unlabelled choice",
                    entry.key
                );
            }
        }
    }
}

/// Every setting sits under a heading, and headings come in runs.
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

/// An owner must never hunt.
#[test]
fn search_finds_a_setting_by_the_word_a_person_would_type() {
    for (typed, expected) in [
        ("QR", "receipt.qr"),
        ("thank you", "receipt.footer"),
        ("roundoff", "billing.rounding"),
        ("round off", "billing.rounding"),
        ("logo", "receipt.logo"),
        ("5 am", "day.starts_at_minutes"),
        ("utgst", "store.state_code"),
        ("composition", "store.registration"),
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

#[test]
fn a_gstin_that_does_not_match_its_state_is_refused() {
    let mut config = ShopConfig::default();
    config.store.state_code = "29".to_owned(); // Karnataka
    config.store.gstin = "29ABCDE1234F1ZW".to_owned();
    assert!(catalog::check_gstin_against_state(&config).is_ok());

    // A real Kerala number on a Karnataka shop.
    config.store.gstin = "32ABCDE1234F1Z9".to_owned();
    let error = catalog::check_gstin_against_state(&config).expect_err("it was allowed");
    assert!(error.message.contains("Kerala"), "{}", error.message);
    assert_eq!(error.key, Some("store.gstin"));
}

/// A charge is added for the order type that earned it, and for no other.
#[test]
fn charges_follow_the_order_type() {
    use mb_core::{ChargeKind, OrderType};
    let book = mb_core::TaxBook::new(mb_core::starting_classes(), mb_core::PriceBasis::Exclusive);
    let mut billing = Billing::default();
    assert!(
        billing
            .charges_for(OrderType::DineIn, &book)
            .expect("charges")
            .is_empty(),
        "a shop that has not been asked charges nothing"
    );

    billing.service_charge_bp = 500;
    billing.packing_charge = mb_core::Money::from_paise(1_000);
    billing.delivery_charge = mb_core::Money::from_paise(3_000);

    let dine_in = billing.charges_for(OrderType::DineIn, &book).expect("charges");
    assert_eq!(dine_in.len(), 1);
    assert_eq!(dine_in[0].kind, ChargeKind::Service);

    let parcel = billing.charges_for(OrderType::Parcel, &book).expect("charges");
    assert_eq!(parcel.len(), 1);
    assert_eq!(parcel[0].kind, ChargeKind::Packing);
    // A parcel is not a table: no service charge on food nobody served.
    assert!(!parcel.iter().any(|c| c.kind == ChargeKind::Service));

    let delivery = billing.charges_for(OrderType::Delivery, &book).expect("charges");
    assert_eq!(delivery.len(), 1);
    assert_eq!(delivery[0].kind, ChargeKind::Delivery);

    // The service charge reads its slab: 18% on a bill of 5% food, and a slab that is gone is
    // an error rather than a silent zero.
    assert_eq!(
        dine_in[0].tax.rate,
        mb_core::TaxRate::from_percent(18).expect("18%")
    );
    billing.service_charge_tax = "tax_nope".to_owned();
    assert!(billing.charges_for(OrderType::DineIn, &book).is_err());
}

/// The day rule reaches `mb_core` intact, and a corrupt one does not stop the shop billing.
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
    // Past the end of a day: refused by the screen, so reaching this means the row is corrupt —
    // and a corrupt row must not stop a bill being dated.
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

/// The two font lists cannot drift apart.
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

/// Six faces, named as a Windows user knows them, key for key with the print crate's list.
#[test]
fn the_typefaces_are_six_plain_family_names() {
    use super::catalog::FONTS;

    let offered: Vec<&str> = FONTS.iter().map(|c| c.value).collect();
    assert_eq!(
        offered,
        ["monospace", "sans_serif", "serif", "arial", "courier", "times"]
    );
    let families: Vec<&str> = mb_print::font::FAMILIES.iter().map(|f| f.key).collect();
    assert_eq!(offered, families);
    for (choice, family) in FONTS.iter().zip(mb_print::font::FAMILIES) {
        assert_eq!(choice.label, family.label);
    }
    for choice in FONTS {
        assert!(
            !choice.label.contains(" — "),
            "{:?} carries an explainer",
            choice.label
        );
    }
}

/// Ten sizes, numbered 1 to 10, and nothing else on the label.
#[test]
fn the_text_sizes_are_plain_numbers() {
    use super::catalog::SIZES;

    assert!(
        (5..=10).contains(&SIZES.len()),
        "there are {} sizes — the owner asked for five to ten",
        SIZES.len()
    );

    for (index, choice) in SIZES.iter().enumerate() {
        assert_eq!(
            choice.label,
            (index + 1).to_string(),
            "a size label is not its position on the list"
        );
        assert!(
            !choice.label.contains("px") && !choice.label.contains(' '),
            "{:?} is not a plain number",
            choice.label
        );
    }
}

/// Every step is a different size on paper, and they only go up.
#[test]
fn every_size_is_bigger_than_the_one_before_it() {
    use super::catalog::SIZES;

    let dots: Vec<u16> = SIZES
        .iter()
        .map(|c| c.value.parse().expect("a size is a number of dots"))
        .collect();

    assert!(
        dots.windows(2).all(|w| w[0] < w[1]),
        "the sizes are out of order or repeat: {dots:?}"
    );
    // And it is the same list the printer resolves against.
    assert_eq!(
        dots,
        mb_print::Style::LADDER.to_vec(),
        "the screen offers different sizes from the ones the printer draws"
    );

    // Deliberately disjoint from every value an older build stored — the multipliers 1/2/3 and
    // the nominal heights.
    for old in [1_u16, 2, 3, 16, 20, 24, 28, 32, 36, 40, 48, 60, 72] {
        assert!(
            !dots.contains(&old),
            "{old} is on both the old list and the new one, so a stored row is ambiguous"
        );
    }
}

/// A size is described to a shop by its number, not by dots and not by "×".
#[test]
fn a_capped_size_is_reported_as_a_number_a_shop_can_pick() {
    // Exactly on the list.
    assert_eq!(super::size_label(mb_print::Style::LADDER[2]), "3");
    assert_eq!(super::size_label(mb_print::Style::LARGEST), "10");
    // A size between two rungs reports as the nearest below, because that is what a person
    // would have had to choose to get it.
    assert_eq!(super::size_label(mb_print::Style::LADDER[4] + 1), "5");
    // And below the smallest is still the smallest anybody can ask for.
    assert_eq!(super::size_label(4), "1");
}

/// A setting nobody reads is a promise the screen cannot keep.
#[test]
fn every_setting_has_a_reader() {
    fn gather(dir: &std::path::Path, into: &mut String) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                gather(&path, into);
                continue;
            }
            let name = path.to_string_lossy().replace('\\', "/");
            // The catalogue names every field; a reader is anywhere else, the settings module
            // included (a charge is read through a method on its own struct).
            let is_the_catalogue =
                name.ends_with("/settings/catalog.rs") || name.ends_with("/settings/tests.rs");
            let is_a_test = name.contains("/tests/") || name.ends_with("_tests.rs");
            if is_the_catalogue || is_a_test || name.contains("/generated/") {
                continue;
            }
            let source_code = matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("rs" | "ts" | "tsx")
            );
            if source_code && let Ok(text) = std::fs::read_to_string(&path) {
                into.push_str(&text);
            }
        }
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut sources = String::new();
    for dir in ["src-tauri/src", "crates", "ui/src"] {
        gather(&root.join(dir), &mut sources);
    }
    let unread: Vec<&str> = CATALOG
        .iter()
        .filter(|entry| {
            let field = entry.key.rsplit('.').next().unwrap_or(entry.key);
            !sources.contains(&format!(".{field}"))
        })
        .map(|entry| entry.key)
        .collect();
    assert!(
        unread.is_empty(),
        "nothing reads these settings: {unread:?}"
    );
}
