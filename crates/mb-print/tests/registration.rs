//! What the paper says about who the shop is — P33 Phase 6.
//!
//! Audit 3.2: `is_composition` changed only the title, so a bill of supply
//! printed CGST lines under it. That is a contradiction in law.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

mod common;

use common::Fixture;
use mb_core::{
    BillInput, Cart, ItemSnapshot, Money, Qty, Registration, StateTax, TaxRate, TaxSpec,
    compute_bill,
};
use mb_print::layout::{LaidContent, layout};
use mb_print::paper::PaperKind;
use mb_print::template::{BillContext, Copy};

fn pc(percent: u32) -> TaxRate {
    TaxRate::from_percent(percent).expect("a real rate")
}

fn cart_of(specs: &[(&str, i64, TaxSpec)]) -> Cart {
    let mut cart = Cart::new();
    for (name, paise, tax) in specs {
        let snapshot = ItemSnapshot::new(
            mb_core::ItemId::new(*name),
            *name,
            Money::from_paise(*paise),
            tax.rate,
        )
        .with_tax(*tax);
        cart.add(snapshot, Qty::ONE, None, Vec::new()).expect("adds");
    }
    cart
}

/// The whole bill, as printable text.
fn paper_for(
    registration: Registration,
    state_tax: StateTax,
    specs: &[(&str, i64, TaxSpec)],
) -> String {
    let cart = cart_of(specs);
    let bill = compute_bill(
        BillInput::new(&cart, registration)
            .with_state_tax(state_tax)
            .with_rounding(mb_core::RoundingMode::NearestRupee),
    )
    .expect("computes");

    let mut fixture = Fixture::new();
    fixture.store.registration = registration;
    fixture.bill = bill;

    let ctx = BillContext {
        bill: &fixture.bill,
        ..fixture.context(Copy::Original)
    };
    let doc = mb_print::template::bill_document(&common::metrics(PaperKind::Mm80), &ctx)
        .expect("builds");
    let laid = layout(&doc).expect("lays out");
    laid.lines
        .iter()
        .filter_map(|l| match &l.content {
            LaidContent::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("
")
}

/// **The illegal document, as a guard.**
///
/// A composition dealer may not collect or show GST.
#[test]
fn a_bill_of_supply_can_never_contain_the_word_cgst() {
    let shapes: [&[(&str, i64, TaxSpec)]; 4] = [
        &[("Dosa", 10_000, TaxSpec::gst(pc(5)))],
        &[
            ("Dosa", 10_000, TaxSpec::gst(pc(5))),
            ("Water", 2_000, TaxSpec::gst_inclusive(pc(18))),
        ],
        &[("Rice", 9_000, TaxSpec::gst(pc(5))), ("Papad", 2_000, TaxSpec::exempt())],
        &[("Dosa", 10_000, TaxSpec::gst(pc(5))), ("Coffee", 3_000, TaxSpec::gst(pc(18)))],
    ];
    for shape in shapes {
        let paper = paper_for(Registration::Composition, StateTax::Sgst, shape);
        assert!(paper.contains("BILL OF SUPPLY"), "wrong title:\n{paper}");
        for word in ["CGST", "SGST", "IGST", "UTGST"] {
            assert!(!paper.contains(word), "a bill of supply printed {word}:\n{paper}");
        }
    }
}

/// An unregistered shop charges nothing and claims no title.
#[test]
fn an_unregistered_bill_has_no_title_and_no_tax() {
    let paper = paper_for(
        Registration::Unregistered,
        StateTax::Sgst,
        &[("Dosa", 10_000, TaxSpec::gst(pc(5)))],
    );
    assert!(!paper.contains("TAX INVOICE"), "{paper}");
    assert!(!paper.contains("BILL OF SUPPLY"), "{paper}");
    for word in ["CGST", "SGST", "IGST"] {
        assert!(!paper.contains(word), "an unregistered bill printed {word}:\n{paper}");
    }
}

/// A regular shop still gets its tax invoice and its GST lines.
#[test]
fn a_regular_bill_still_prints_a_tax_invoice_with_gst() {
    let paper = paper_for(
        Registration::Regular,
        StateTax::Sgst,
        &[("Dosa", 10_000, TaxSpec::gst(pc(5)))],
    );
    assert!(paper.contains("TAX INVOICE"), "{paper}");
    assert!(paper.contains("CGST"), "{paper}");
    assert!(paper.contains("SGST"), "{paper}");
}

/// A union territory without a legislature calls the state half UTGST.
#[test]
fn a_union_territory_prints_utgst() {
    let paper = paper_for(
        Registration::Regular,
        StateTax::Utgst,
        &[("Dosa", 10_000, TaxSpec::gst(pc(5)))],
    );
    assert!(paper.contains("UTGST"), "{paper}");
    assert!(paper.contains("CGST"), "the central half is unchanged:\n{paper}");
}

/// **The bar bill.** Liquor VAT prints, and never inside a GST figure.
#[test]
fn a_bar_bill_prints_its_vat_separately() {
    let paper = paper_for(
        Registration::Regular,
        StateTax::Sgst,
        &[
            ("Dosa", 10_000, TaxSpec::gst(pc(5))),
            ("Beer", 25_000, TaxSpec::liquor(pc(20))),
        ],
    );
    assert!(paper.contains("VAT"), "the VAT never reached the paper:\n{paper}");
    assert!(paper.contains("CGST"), "the food's GST is still there:\n{paper}");
    // It printed twice once, and `contains` could not see it.
    assert_eq!(
        paper.matches("(includes VAT").count(),
        1,
        "the VAT memo is not printed exactly once:\n{paper}"
    );
}
