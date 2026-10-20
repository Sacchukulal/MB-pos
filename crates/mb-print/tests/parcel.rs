#![allow(clippy::expect_used, clippy::panic, reason = "tests: receipt assertions")]
mod common;

use mb_core::{BillInput, Cart, Charge, ChargeKind, Discount, DiscountEntry, ItemId,
    ItemSnapshot, Money, OrderType, Qty, Registration, RoundingMode, TaxRate, TaxSpec, compute_bill};
use mb_print::{layout::{LaidContent, layout}, paper::PaperKind, template::Copy};

#[test]
fn parcel_receipts_fold_charges_into_items_without_changing_tax_discounts_or_total() {
    for kind in [PaperKind::Mm58, PaperKind::Mm80] {
        for basis in [TaxSpec::gst(TaxRate::from_percent(5).expect("tax")),
                      TaxSpec::gst_inclusive(TaxRate::from_percent(5).expect("tax"))] {
            let mut cart = Cart::new();
            let mut item = ItemSnapshot::new(ItemId::new("tea"), "Tea", Money::from_paise(2000), TaxRate::ZERO);
            item.parcel_charge = Money::from_paise(200);
            cart.add(item, Qty::from_whole(3).expect("quantity"), None, vec![]).expect("add");
            let charges = [Charge::flat(ChargeKind::Packing, "Parcel charges", Money::from_paise(600), basis.rate).with_tax(basis)];
            let bill = compute_bill(BillInput::new(&cart, Registration::Regular)
                .with_order_type(OrderType::Parcel).with_charges(&charges)
                .with_rounding(RoundingMode::None)
                .with_bill_discount(DiscountEntry::new(Discount::percent_bp(1000).expect("discount")))).expect("bill");
            let original = bill.clone();
            let folded = bill.display_with_parcel_in_items().expect("display");
            assert_eq!(folded.subtotal, Money::from_paise(6600));
            assert_eq!(folded.lines[0].snapshot.unit_price, Money::from_paise(2200));
            assert_eq!(folded.lines[0].net, Money::from_paise(6000));
            assert_eq!(folded.total_discount, bill.total_discount);
            assert_eq!(folded.gst_added, bill.gst_added);
            assert_eq!(folded.summary, bill.summary);
            assert_eq!(folded.grand_total, bill.grand_total);
            let mut fixture = common::Fixture::new();
            fixture.bill = bill;
            let render = |fixture: &common::Fixture| {
                let doc = mb_print::template::bill_document(&common::metrics(kind), &fixture.context(Copy::Original)).expect("document");
                layout(&doc).expect("layout").lines.iter().filter_map(|line| match &line.content {
                    LaidContent::Text { text } => Some(text.as_str()), _ => None,
                }).collect::<Vec<_>>().join("\n")
            };
            let separate = render(&fixture);
            assert!(separate.contains("Parcel charges"));
            fixture.settings.parcel_charge_separate = false;
            let combined = render(&fixture);
            assert!(!combined.contains("Parcel charges"), "{combined}");
            assert!(combined.contains("22.00"), "{combined}");
            assert!(combined.contains("66.00"), "{combined}");
            assert!(combined.contains(&original.grand_total.to_plain_string()), "{combined}");
            assert_eq!(fixture.bill, original, "rendering cannot alter the issued bill");
        }
    }
}

#[test]
fn historical_flat_packing_bills_keep_their_saved_charge() {
    let mut fixture = common::Fixture::new();
    fixture.bill.order_type = OrderType::Parcel;
    let charge = &mut fixture.bill.charges[0];
    charge.kind = ChargeKind::Packing;
    charge.name = "Packing".to_owned();
    charge.basis = mb_core::ChargeBasis::Flat(charge.amount);
    let before = fixture.bill.clone();
    assert_eq!(fixture.bill.display_with_parcel_in_items().expect("old bill"), before);
}
