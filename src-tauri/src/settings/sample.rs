//! The bill the preview shows, and the bill the test print prints.

use mb_core::{
    AnyOrder, Bill, BillInput, BusinessDay, Cart, Claimed, Discount, DiscountEntry, DraftOrder,
    ItemId, ItemSnapshot, Money, OpenOrder, OrderId, OrderType, Payment, PaymentMode, Qty,
    Registration, Settlement, StaffId, TableId, Timestamp,
};
use mb_print::error::PrintError;
use mb_print::layout::layout_for;
use mb_print::metrics::Metrics;
use mb_print::paper::Paper;

use super::{Billing, ShopConfig};
use crate::preview::{PreviewDoc, to_preview};

/// A fixed moment, so the preview does not flicker every time it is redrawn.
const AT: Timestamp = Timestamp::from_millis(1_770_000_000_000);

/// The cart, the bill and the settled order the preview renders.
fn pc(percent: u32) -> mb_core::TaxRate {
    mb_core::TaxRate::from_percent(percent).unwrap_or(mb_core::TaxRate::ZERO)
}

pub fn sample_order(registration: Registration) -> Result<(Bill, AnyOrder), PrintError> {
    let mut cart = Cart::new();
    cart.add(
        ItemSnapshot::new(
            ItemId::new("itm_sample_1"),
            // Long on purpose: 32-column paper has to wrap this and lose nothing, and a shop
            // choosing a bigger item size needs to see what that costs.
            "Paneer Butter Masala (Half) - Extra Spicy",
            Money::from_paise(24_000),
            pc(5),
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
            pc(18),
        )
        .with_tax(mb_core::TaxSpec::gst_inclusive(pc(18)))
        .with_hsn("2101"),
        Qty::from_whole(3).unwrap_or_default(),
        None,
        vec![],
    )
    .ok();
    cart.add(
        // 3, and the line that makes this product usable by a bar.
        ItemSnapshot::new(
            ItemId::new("itm_sample_3"),
            "Beer 650ml",
            Money::from_paise(22_000),
            // A real VAT rate.
            pc(20),
        )
        .with_tax(mb_core::TaxSpec::liquor(pc(20))),
        Qty::from_whole(1).unwrap_or_default(),
        None,
        vec![],
    )
    .ok();

    // The seeded slabs, priced tax-added — the sample does not read the shop's own book, so
    // the preview takes the same path as a fresh shop.
    let book = mb_core::TaxBook::new(mb_core::starting_classes(), mb_core::PriceBasis::Exclusive);
    let charges = Billing {
        service_charge_bp: 500,
        ..Billing::default()
    }
    .charges_for(OrderType::DineIn, &book)
    .map_err(|e| PrintError::Invalid(format!("the sample bill's charges will not compute: {e}")))?;

    // Billed as whatever this shop is.
    let mut input = BillInput::new(&cart, registration)
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
        mb_core::Placement::on_table(TableId::new("6")),
        StaffId::new("staff_sample"),
    )
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
            // Not a plausible number.
            formatted: "SAMPLE".to_owned(),
            business_day: day,
        },
    };

    let mut settlement = Settlement::new();
    // Two payments, so `show.payment_lines` has something to show, and cash first so
    // `change_due` can be non-zero.
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

/// A real one-bit picture, for a shop that has not chosen a logo.
fn sample_logo() -> Vec<u8> {
    let mut image = mb_print::image::Monochrome::blank(64, 32);
    for x in 0..64 {
        image.set(x, 0);
        image.set(x, 31);
    }
    for y in 0..32 {
        image.set(0, y);
        image.set(63, y);
        image.set(y * 2, y);
    }
    image.encode()
}

/// Everything a preview needs that a REAL print resolves from the shop.
#[derive(Debug, Clone)]
pub struct Around {
    pub paper: Paper,
    /// The face and the sizes the chosen printer will actually draw with.
    pub metrics: Metrics,
    /// `raster` or `text` — laid out for the engine the printer is set to, so a shop on the
    /// text engine is not shown the graphics engine's bill.
    pub engine: &'static str,
    pub logo: Option<Vec<u8>>,
    /// The table's own name, through `flows::table_label`.
    pub table: Option<String>,
    /// Through `flows::clock_time`, against a fixed moment — see `AT`.
    pub time: String,
    pub waiter: Option<String>,
    pub cashier: Option<String>,
}

impl Around {
    /// Plain 80 mm in the built-in face, for a test that only wants a document.
    #[cfg(test)]
    #[must_use]
    pub fn plain(paper: Paper) -> Around {
        let metrics = mb_print::font::Font::builtin().map_or_else(
            |_| Metrics::printer_font(paper),
            |font| Metrics::face(paper, std::sync::Arc::new(font)),
        );
        Around {
            paper,
            metrics,
            engine: "raster",
            logo: None,
            table: Some("6".to_owned()),
            time: "19:40".to_owned(),
            waiter: Some("Suresh".to_owned()),
            cashier: Some("Ravi".to_owned()),
        }
    }
}

/// Build `Around` the way a real print builds it.
pub fn around_for(app: &crate::state::App, paper: Paper, group: &str) -> Around {
    // The same question the queue asks, through the same function — the kitchen ticket and the
    // bill each have their own face and their own engine, and a preview that guessed either
    // would be a preview that lies.
    let kind = if group == "kitchen" {
        mb_print::queue::JobKind::Kitchen
    } else {
        mb_print::queue::JobKind::Bill
    };
    let printer = crate::flows::default_printer(app)
        .unwrap_or_else(|_| {
            mb_print::printer::PrinterConfig::new(
                "prn_preview",
                "Preview",
                mb_print::printer::Target::None,
            )
        })
        .with_paper(paper.kind);
    let (metrics, engine) = app.metrics_for(kind, &printer);

    // The shop's own first table, through the real lookup.
    let table = crate::flows::first_table_label(app);

    Around {
        paper,
        metrics,
        engine,
        logo: crate::logo::stored(app),
        table,
        // A fixed moment, for the reason `AT` gives: two renders a second apart must differ
        // only where a setting differs.
        time: crate::flows::clock_time(AT),
        waiter: crate::flows::current_staff_name(app),
        cashier: crate::flows::current_staff_name(app),
    }
}

/// The bill, laid out with this shop's settings, ready for the screen.
pub fn bill_preview(config: &ShopConfig, around: &Around) -> Result<PreviewDoc, PrintError> {
    let (bill, order) = sample_order(config.store.registration())?;
    let store = config.store.to_print_store();
    let document = mb_print::template::bill_document(
        &around.metrics,
        &mb_print::template::BillContext {
            bill: &bill,
            order: &order,
            store: &store,
            settings: &config.receipt,
            customer: None,
            cashier: around.cashier.as_deref(),
            table: around.table.as_deref(),
            time: Some(around.time.as_str()),
            waiter: around.waiter.as_deref(),
            copy: mb_print::template::Copy::Original,
            einvoice: mb_print::template::EInvoice::default(),
            // This shop's own logo when it has chosen one, and a real sample picture when it
            // has not.
            logo: Some(around.logo.clone().unwrap_or_else(sample_logo)),
        },
    )?;
    Ok(to_preview(
        &layout_for(&document, &around.metrics)?,
        &around.metrics,
        around.engine,
    ))
}

/// The kitchen ticket, the same way.
pub fn kitchen_preview(config: &ShopConfig, around: &Around) -> Result<PreviewDoc, PrintError> {
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
        around.paper,
        &mb_print::template::KitchenContext {
            kind: mb_print::template::TicketKind::New,
            token: Some("7"),
            bill_number: Some("SAMPLE"),
            kot_number: Some("14"),
            order_type: OrderType::DineIn,
            table: around.table.as_deref(),
            time: Some(around.time.as_str()),
            waiter: around.waiter.as_deref(),
            station: None,
            reprint: false,
            lines: &lines,
            settings: &config.kitchen,
        },
    )?;
    Ok(to_preview(
        &layout_for(&document, &around.metrics)?,
        &around.metrics,
        around.engine,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mb_print::paper::PaperKind;

    /// The sample has to carry every case a setting can turn on, or the preview is a lie about
    /// half the screen.
    #[test]
    fn the_sample_exercises_every_setting_that_needs_a_case() {
        let (bill, _) = sample_order(Registration::Regular).expect("the sample computes");
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

    /// The text of the sample bill, as the screen shows it.
    fn preview_text(config: &ShopConfig) -> String {
        bill_preview(config, &Around::plain(Paper::new(PaperKind::Mm80)))
            .expect("previews")
            .lines
            .iter()
            .filter_map(|line| match line {
                crate::preview::PreviewLine::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(
                "
",
            )
    }

    /// The preview shows the shop's own document.
    #[test]
    fn the_preview_follows_the_shop_s_registration() {
        let mut config = ShopConfig::default();
        // A title needs a GSTIN: `title_of` prints none for a shop that has not said it is
        // registered.
        config.store.gstin = "29ABCDE1234F1Z5".to_owned();
        config.store.registration = "composition".to_owned();
        let text = preview_text(&config);
        assert!(text.contains("BILL OF SUPPLY"), "{text}");
        for word in ["CGST", "SGST", "IGST"] {
            assert!(
                !text.contains(word),
                "a bill of supply preview printed {word}:
{text}"
            );
        }

        config.store.registration = "regular".to_owned();
        let text = preview_text(&config);
        assert!(text.contains("TAX INVOICE"), "{text}");
        assert!(text.contains("CGST"), "{text}");
    }

    /// The bar's VAT reaches the paper.
    #[test]
    fn the_sample_shows_a_bar_its_vat() {
        let text = preview_text(&ShopConfig::default());
        assert!(
            text.contains("VAT"),
            "the sample beer charges no VAT:
{text}"
        );
    }

    /// It is a sample, and it says so where it counts.
    #[test]
    fn the_sample_cannot_be_mistaken_for_a_bill() {
        let (_, order) = sample_order(Registration::Regular).expect("the sample computes");
        assert_eq!(
            order.bill_number().map(|n| n.formatted.clone()),
            Some("SAMPLE".to_owned())
        );
    }

    /// It must not be able to fail.
    #[test]
    fn a_preview_is_produced_for_every_paper_and_every_shop() {
        for kind in [PaperKind::Mm58, PaperKind::Mm80, PaperKind::Mm100] {
            let paper = Paper::new(kind);
            // A shop that has filled in nothing at all — its first day.
            let empty = ShopConfig::default();
            let around = Around::plain(paper);
            assert!(bill_preview(&empty, &around).is_ok(), "{kind:?} empty");
            assert!(kitchen_preview(&empty, &around).is_ok(), "{kind:?} empty");

            // And one with everything turned on.
            let mut full = ShopConfig::default();
            full.store.name = "Anna Kuteera".to_owned();
            full.store.upi_id = "anna@upi".to_owned();
            full.store.registration = "composition".to_owned();
            full.receipt.show.hsn = true;
            full.receipt.qr = mb_print::settings::QrMode::Dynamic;
            full.receipt.logo = mb_print::settings::LogoPosition::Left;
            full.receipt.sections.store_name = mb_print::Style::new(3, true);
            full.kitchen.two_column = true;
            full.kitchen.show_column_names = true;
            assert!(bill_preview(&full, &around).is_ok(), "{kind:?} full");
            assert!(kitchen_preview(&full, &around).is_ok(), "{kind:?} full");
        }
    }

    /// The anti-drift test, across the settings path.
    #[test]
    fn the_preview_is_the_same_document_the_printer_gets() {
        let mut config = ShopConfig::default();
        config.store.name = "Anna Kuteera".to_owned();
        config.receipt.footer = "Come back soon".to_owned();
        config.receipt.show.hsn = true;
        config.receipt.pattern = mb_print::Pattern::Dotted;

        let paper = Paper::new(PaperKind::Mm80);
        let around = Around::plain(paper);
        // The same registration the preview reads — a shop with no GST number is unregistered.
        let (bill, order) =
            sample_order(config.store.registration()).expect("the sample computes");
        let store = config.store.to_print_store();
        let document = mb_print::template::bill_document(
            &around.metrics,
            &mb_print::template::BillContext {
                bill: &bill,
                order: &order,
                store: &store,
                settings: &config.receipt,
                customer: None,
                cashier: around.cashier.as_deref(),
                table: around.table.as_deref(),
                time: Some(around.time.as_str()),
                waiter: around.waiter.as_deref(),
                copy: mb_print::template::Copy::Original,
                einvoice: mb_print::template::EInvoice::default(),
                logo: Some(sample_logo()),
            },
        )
        .expect("builds");
        let laid = layout_for(&document, &around.metrics).expect("lays out");

        // What the printer's text sink emits.
        let from_printer = laid.text_lines();
        // What the screen is handed.
        let from_screen: Vec<String> = bill_preview(&config, &around)
            .expect("previews")
            .lines
            .iter()
            .filter_map(|line| match line {
                crate::preview::PreviewLine::Text { text, indent, .. } => {
                    Some(format!("{}{text}", " ".repeat(laid.columns_of(*indent))))
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
