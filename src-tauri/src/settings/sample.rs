//! **The bill the preview shows, and the bill the test print prints.**
//!
//! # Why it is one sample and not two
//!
//! The settings screen needs something to render, and the test print (P17 part
//! 4) needs something to put on paper. If those were two samples they would
//! drift, and the drift would be invisible: a shop would tune its receipt
//! against one and print the other.
//!
//! # Why it is deliberately awkward
//!
//! **A preview that only ever shows two cups of tea is how a setting ships
//! broken.** Half the settings on that screen change nothing on a simple bill:
//!
//! | setting | needs |
//! |---|---|
//! | `show.hsn` | an item with an HSN |
//! | `show.tax_summary` | more than one tax rate |
//! | `show.payment_lines` | more than one payment |
//! | `separators.below_column_names` | paper wide enough to have columns |
//! | `composition_note` | a composition dealer |
//! | `qr_width_pct` | a UPI id |
//! | `logo_width_pct` | a logo |
//! | round-off | a total with paise in it |
//!
//! So the sample carries a long name that wraps, a per-line note, an HSN, a
//! non-GST line (a bar's beer — scope 2.3), a bill discount, a service charge,
//! and a split payment. Every one of those is a setting somebody can otherwise
//! toggle and see nothing happen.
//!
//! # It is not a real bill and says so
//!
//! The bill number is `SAMPLE`. A preview that showed a plausible bill number
//! is a preview somebody can photograph and hand to a customer.

use mb_core::{
    AnyOrder, Bill, BillInput, BusinessDay, Cart, Claimed, Discount, DiscountEntry, DraftOrder,
    ItemSnapshot, ItemId, Money, OpenOrder, OrderId, OrderType, Payment, PaymentMode, Qty,
    Settlement, StaffId, TableId, TaxRate, TaxTreatment, Timestamp,
};
use mb_print::error::PrintError;
use mb_print::layout::layout;
use mb_print::paper::Paper;

use super::{Billing, ShopConfig};
use crate::preview::{PreviewDoc, to_preview};

/// A fixed moment, so the preview does not flicker every time it is redrawn.
///
/// It is also why the sample carries no clock: two renders a second apart must
/// differ only where a setting differs, or "did that change anything?" has no
/// answer.
const AT: Timestamp = Timestamp::from_millis(1_770_000_000_000);

/// The cart, the bill and the settled order the preview renders.
///
/// **A `Result`, like a real bill**, because it is built by the same engine as
/// one — `compute_bill` is D4's pipeline and this does not get a private door
/// past it. It cannot fail in practice, and
/// `a_preview_is_produced_for_every_paper_and_every_shop` is what says so.
pub fn sample_order() -> Result<(Bill, AnyOrder), PrintError> {
    let mut cart = Cart::new();
    cart.add(
        ItemSnapshot::new(
            ItemId::new("itm_sample_1"),
            // Long on purpose: 32-column paper has to wrap this and lose
            // nothing, and a shop choosing a bigger item size needs to see what
            // that costs.
            "Paneer Butter Masala (Half) - Extra Spicy",
            Money::from_paise(24_000),
            TaxRate::GST_5,
        )
        .with_hsn("2106"),
        Qty::from_whole(2).unwrap_or_default(),
        Some("no onion".to_owned()),
        vec![],
    )
    .ok();
    cart.add(
        ItemSnapshot::new(
            ItemId::new("itm_sample_2"),
            "Filter Coffee",
            Money::from_paise(3_000),
            TaxRate::GST_18,
        )
        .with_treatment(TaxTreatment::Inclusive)
        .with_hsn("2101"),
        Qty::from_whole(3).unwrap_or_default(),
        None,
        vec![],
    )
    .ok();
    cart.add(
        // Scope 2.3, and the line that makes this product usable by a bar. A
        // shop that turns the tax summary off should see what it is losing.
        ItemSnapshot::new(
            ItemId::new("itm_sample_3"),
            "Beer 650ml",
            Money::from_paise(22_000),
            TaxRate::ZERO,
        )
        .with_treatment(TaxTreatment::NonGst),
        Qty::from_whole(1).unwrap_or_default(),
        None,
        vec![],
    )
    .ok();

    let charges = Billing {
        service_charge_bp: 500,
        ..Billing::default()
    }
    .charges_for(OrderType::DineIn);

    let mut input = BillInput::new(&cart)
        .with_order_type(OrderType::DineIn)
        .with_charges(&charges);
    if let Some(five_percent) = Discount::percent_bp(500) {
        input = input.with_bill_discount(DiscountEntry::new(five_percent));
    }
    let bill = mb_core::compute_bill(input)
        .map_err(|e| PrintError::Invalid(format!("the sample bill will not compute: {e}")))?;

    let day = BusinessDay::of(AT, mb_core::DayRule::DEFAULT, mb_core::UtcOffset::INDIA);
    let core = DraftOrder::new(
        OrderId::new("ord_sample"),
        day,
        AT,
        OrderType::DineIn,
        StaffId::new("staff_sample"),
    )
    .on_table(TableId::new("6"))
    .core;
    let open = OpenOrder {
        core,
        token: Claimed {
            value: 7,
            formatted: "7".to_owned(),
            business_day: day,
        },
        bill_number: Claimed {
            value: 0,
            // **Not a plausible number.** A preview somebody can photograph and
            // hand to a customer is a preview that has become a bill.
            formatted: "SAMPLE".to_owned(),
            business_day: day,
        },
    };

    let mut settlement = Settlement::new();
    // Two payments, so `show.payment_lines` has something to show, and cash
    // first so `change_due` can be non-zero.
    if let Ok(payment) = Payment::new(PaymentMode::Cash, Money::from_paise(50_000)) {
        settlement.add(payment).ok();
    }
    if let Ok(rest) = bill.grand_total.sub(Money::from_paise(50_000))
        && let Ok(payment) = Payment::new(PaymentMode::Upi, rest)
    {
        settlement.add(payment).ok();
    }

    let settled = open
        .settle(bill.clone(), settlement, AT, StaffId::new("staff_sample"))
        .map_err(|e| PrintError::Invalid(format!("the sample bill will not settle: {e}")))?;

    Ok((bill, AnyOrder::Settled(settled)))
}

/// **Three tiny black squares.** A logo has to exist for the logo settings to
/// be able to do anything, and P07's one-bit format is width, height, bits.
const SAMPLE_LOGO: &[u8] = &[8, 8, 0xFF, 0x81, 0x81, 0xFF, 0xFF, 0x81, 0x81, 0xFF];

/// The bill, laid out with this shop's settings, ready for the screen.
pub fn bill_preview(
    config: &ShopConfig,
    paper: Paper,
    logo: Option<Vec<u8>>,
) -> Result<PreviewDoc, PrintError> {
    let (bill, order) = sample_order()?;
    let store = config.store.to_print_store();
    let document = mb_print::template::bill_document(
        paper,
        &mb_print::template::BillContext {
            bill: &bill,
            order: &order,
            store: &store,
            settings: &config.receipt,
            customer: None,
            cashier: Some("Ravi"),
            copy: mb_print::template::Copy::Original,
            einvoice: mb_print::template::EInvoice::default(),
            // **This shop's own logo when it has chosen one** (P31), and the
            // three squares when it has not. The preview is what proves the
            // logo settings do anything at all, so showing a stand-in to
            // somebody who has a real one would be showing them the wrong bill.
            logo: Some(logo.unwrap_or_else(|| SAMPLE_LOGO.to_vec())),
        },
    )?;
    Ok(to_preview(&layout(&document)?))
}

/// The kitchen ticket, the same way.
pub fn kitchen_preview(config: &ShopConfig, paper: Paper) -> Result<PreviewDoc, PrintError> {
    let lines = vec![
        mb_print::template::TicketLine {
            name: "Paneer Butter Masala (Half)".to_owned(),
            qty: Qty::from_whole(2).unwrap_or_default(),
            note: Some("no onion".to_owned()),
            modifiers: vec!["Extra chutney".to_owned()],
        },
        mb_print::template::TicketLine {
            name: "Filter Coffee".to_owned(),
            qty: Qty::from_whole(3).unwrap_or_default(),
            note: None,
            modifiers: vec![],
        },
    ];
    let document = mb_print::template::kitchen_document(
        paper,
        &mb_print::template::KitchenContext {
            kind: mb_print::template::TicketKind::New,
            token: Some("7"),
            bill_number: Some("SAMPLE"),
            order_type: OrderType::DineIn,
            table: Some("6"),
            // A fixed time, for the reason `AT` gives.
            time: Some("19:40"),
            station: None,
            lines: &lines,
            settings: &config.kitchen,
        },
    )?;
    Ok(to_preview(&layout(&document)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mb_print::paper::PaperKind;

    /// The sample has to carry every case a setting can turn on, or the preview
    /// is a lie about half the screen.
    #[test]
    fn the_sample_exercises_every_setting_that_needs_a_case() {
        let (bill, _) = sample_order().expect("the sample computes");
        assert!(bill.lines.len() >= 3, "one item cannot show a tax summary");
        assert!(
            bill.lines.iter().any(|l| l.snapshot.hsn.is_some()),
            "no HSN, so show.hsn changes nothing"
        );
        assert!(
            bill.lines.iter().any(|l| l.note.is_some()),
            "no note, so the item block never wraps"
        );
        assert!(
            !bill.non_gst_value.is_zero(),
            "no non-GST line, so scope 2.3's line never prints"
        );
        assert!(!bill.total_discount.is_zero(), "no discount");
        assert!(!bill.charges.is_empty(), "no charge");
        assert!(
            bill.summary.rows().count() >= 2,
            "one rate cannot show a RATE-WISE summary"
        );
        assert!(
            bill.lines
                .iter()
                .any(|l| l.snapshot.name.chars().count() > 30),
            "no long name, so nothing ever wraps"
        );
    }

    /// It is a sample, and it says so where it counts.
    #[test]
    fn the_sample_cannot_be_mistaken_for_a_bill() {
        let (_, order) = sample_order().expect("the sample computes");
        assert_eq!(
            order.bill_number().map(|n| n.formatted.clone()),
            Some("SAMPLE".to_owned())
        );
    }

    /// **It must not be able to fail.** A preview that errors leaves the design
    /// screen blank at the moment somebody is trying to fix it.
    #[test]
    fn a_preview_is_produced_for_every_paper_and_every_shop() {
        for kind in [PaperKind::Mm58, PaperKind::Mm80, PaperKind::Mm100] {
            let paper = Paper::new(kind);
            // A shop that has filled in nothing at all — its first day.
            let empty = ShopConfig::default();
            assert!(bill_preview(&empty, paper, None).is_ok(), "{kind:?} empty");
            assert!(kitchen_preview(&empty, paper).is_ok(), "{kind:?} empty");

            // And one with everything turned on.
            let mut full = ShopConfig::default();
            full.store.name = "Anna Kuteera".to_owned();
            full.store.upi_id = "anna@upi".to_owned();
            full.store.is_composition = true;
            full.receipt.show.hsn = true;
            full.receipt.qr = mb_print::settings::QrMode::Dynamic;
            full.receipt.logo = mb_print::settings::LogoPosition::Top;
            full.receipt.sections.store_name = mb_print::Style::new(3, true);
            full.kitchen.two_column = true;
            full.kitchen.show_column_names = true;
            assert!(bill_preview(&full, paper, None).is_ok(), "{kind:?} full");
            assert!(kitchen_preview(&full, paper).is_ok(), "{kind:?} full");
        }
    }

    /// **T2 — the anti-drift test, across the settings path.**
    ///
    /// P06's rule is that every renderer is a sink over one laid-out document.
    /// This asserts it for the document the settings screen shows: the text the
    /// preview carries is character-for-character the text the printer's own
    /// sink would emit, for a shop's REAL settings rather than the defaults.
    #[test]
    fn the_preview_is_the_same_document_the_printer_gets() {
        let mut config = ShopConfig::default();
        config.store.name = "Anna Kuteera".to_owned();
        config.receipt.footer = "Come back soon".to_owned();
        config.receipt.show.hsn = true;
        config.receipt.pattern = mb_print::Pattern::Dotted;

        let paper = Paper::new(PaperKind::Mm80);
        let (bill, order) = sample_order().expect("the sample computes");
        let store = config.store.to_print_store();
        let document = mb_print::template::bill_document(
            paper,
            &mb_print::template::BillContext {
                bill: &bill,
                order: &order,
                store: &store,
                settings: &config.receipt,
                customer: None,
                cashier: Some("Ravi"),
                copy: mb_print::template::Copy::Original,
                einvoice: mb_print::template::EInvoice::default(),
                logo: Some(SAMPLE_LOGO.to_vec()),
            },
        )
        .expect("builds");
        let laid = layout(&document).expect("lays out");

        // What the printer's text sink emits.
        let from_printer = laid.text_lines();
        // What the screen is handed.
        let from_screen: Vec<String> = bill_preview(&config, paper, None)
            .expect("previews")
            .lines
            .iter()
            .filter_map(|line| match line {
                crate::preview::PreviewLine::Text { text, indent, .. } => {
                    Some(format!("{}{text}", " ".repeat(*indent)))
                }
                _ => None,
            })
            .collect();

        assert_eq!(
            from_printer, from_screen,
            "the preview and the paper have drifted, which is audit D1"
        );
    }
}
