//! One bill with everything on it.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::integer_division,
    reason = "tests: expect is the assertion, and the fixture splits a fake total three ways"
)]
// Shared by five test binaries, each of which uses a different subset.
#![allow(
    dead_code,
    reason = "shared by five test binaries, each using a different subset"
)]

use mb_core::{
    AnyOrder, Bill, BillInput, BusinessDay, Cart, Charge, ChargeKind, Claimed, CustomerId,
    Discount, DiscountEntry, DraftOrder, ItemId, ItemSnapshot, Money, OpenOrder, OrderId,
    OrderType, Payment, PaymentMode, PlaceOfSupply, PriceBasis, Qty, Registration, RoundingMode,
    Settlement, StaffId, TableId, TaxKind, TaxRate, TaxSpec, Timestamp, compute_bill,
};
use mb_print::settings::ReceiptSettings;
use mb_print::template::{BillContext, BillCustomer, Copy, EInvoice, Store};

/// A whole-percent rate.
pub fn pc(percent: u32) -> TaxRate {
    TaxRate::from_percent(percent).expect("a real rate")
}

pub fn store() -> Store {
    Store {
        name: "Anna Kuteera".to_owned(),
        address: "12 MG Road, Jayanagar, Bengaluru 560011".to_owned(),
        phone: Some("9880012345".to_owned()),
        // A GSTIN that would actually pass — the check character is W, not.
        gstin: Some("29ABCDE1234F1ZW".to_owned()),
        fssai: Some("11223344556677".to_owned()),
        state_code: Some("29".to_owned()),
        upi_id: Some("anna@upi".to_owned()),
        upi_merchant_name: Some("Anna Kuteera".to_owned()),
        upi_reference: Some("MB1".to_owned()),
        registration: mb_core::Registration::Regular,
    }
}

pub fn customer() -> BillCustomer {
    BillCustomer {
        name: "Suresh Traders".to_owned(),
        phone: Some("9845098450".to_owned()),
        gstin: Some("29ZYXWV9876K1Z2".to_owned()),
    }
}

/// A cart with the four cases that matter: a long name, mixed rates, an inclusive line and a
/// non-GST line.
pub fn cart() -> Cart {
    let mut cart = Cart::new();

    let dosa = ItemSnapshot::new(
        ItemId::new("itm_dosa"),
        // Long on purpose: 32-column paper has to wrap this and lose nothing.
        "Paneer Butter Masala (Half) - Extra Spicy",
        Money::from_paise(24_000),
        pc(5),
    )
    .with_hsn("2106");

    let water = ItemSnapshot::new(
        ItemId::new("itm_water"),
        "Water 1L",
        Money::from_paise(2_000),
        pc(18),
    )
    .with_tax(TaxSpec::gst_inclusive(pc(18)))
    .with_hsn("2201");

    // Alcohol at a zero VAT rate, which is what the old `NonGst` meant.
    let beer = ItemSnapshot::new(
        ItemId::new("itm_beer"),
        "Beer 650ml",
        Money::from_paise(22_000),
        TaxRate::ZERO,
    )
    .with_tax(TaxSpec {
        kind: TaxKind::OutsideGst,
        rate: TaxRate::ZERO,
        basis: PriceBasis::Exclusive,
    });

    cart.add(
        dosa,
        Qty::from_whole(2).expect("qty"),
        Some("no onion".to_owned()),
        vec![],
    )
    .expect("add");
    cart.add(water, Qty::from_whole(3).expect("qty"), None, vec![])
        .expect("add");
    cart.add(beer, Qty::from_whole(2).expect("qty"), None, vec![])
        .expect("add");

    // A discount larger than the line, so `was_capped` reaches the paper.
    cart.set_line_discount(
        1,
        Some(
            DiscountEntry::new(Discount::amount(Money::from_paise(999_999)).expect("valid"))
                .with_reason("goodwill"),
        ),
    )
    .expect("discount");

    cart
}

pub fn bill(cart: &Cart) -> Bill {
    let charges = [
        Charge::percent(ChargeKind::Service, "Service Charge", 500, pc(5)),
        Charge::flat(
            ChargeKind::Other("Donation".to_owned()),
            "Donation",
            Money::from_paise(500),
            TaxRate::ZERO,
        )
        .with_tax(TaxSpec::exempt()),
    ];
    // A regular taxpayer, which is what `Store::is_composition = false` and a GSTIN on the
    // letterhead have always meant.
    compute_bill(
        BillInput::new(cart, Registration::Regular)
            .with_bill_discount(DiscountEntry::new(
                Discount::percent_bp(500).expect("valid"),
            ))
            .with_charges(&charges)
            .with_place_of_supply(PlaceOfSupply::Intra)
            .with_order_type(OrderType::DineIn)
            .with_rounding(RoundingMode::NearestRupee),
    )
    .expect("compute")
}

pub fn settlement(bill: &Bill) -> Settlement {
    let mut settlement = Settlement::with_tip(Money::from_paise(2_000)).expect("tip");
    let due = settlement
        .amount_due(bill.grand_total)
        .expect("due")
        .paise();
    let a = Money::from_paise(due / 2);
    let b = Money::from_paise(due / 4);
    let c = Money::from_paise(due - a.paise() - b.paise());

    settlement
        .add(
            Payment::new(PaymentMode::Card, a)
                .expect("payment")
                .with_reference("APPR-77113"),
        )
        .expect("add");
    settlement
        .add(Payment::new(PaymentMode::Other("Sodexo".to_owned()), b).expect("payment"))
        .expect("add");
    settlement
        .add(
            Payment::new(PaymentMode::Credit(CustomerId::new("cus_1")), c)
                .expect("payment")
                .settling_credit(),
        )
        .expect("add");
    settlement
}

/// A settled order carrying that bill.
pub fn order(bill: Bill, settlement: Settlement) -> AnyOrder {
    let day = BusinessDay::from_ymd(2026, 8, 3);
    let core = DraftOrder::new(
        OrderId::new("ord_0001"),
        day,
        Timestamp::from_millis(1_770_000_000_000),
        OrderType::DineIn,
        StaffId::new("staff_1"),
    )
    .on_table(TableId::new("6"))
    .core;

    let open = OpenOrder {
        core,
        token: Claimed {
            value: 42,
            formatted: "42".to_owned(),
            business_day: day,
        },
        bill_number: Claimed {
            value: 1_207,
            formatted: "BIR/1207".to_owned(),
            business_day: day,
        },
    };

    AnyOrder::Settled(
        open.settle(
            bill,
            settlement,
            Timestamp::from_millis(1_770_000_900_000),
            StaffId::new("staff_1"),
        )
        .expect("settles"),
    )
}

/// Everything on: HSN column, tax summary, QR with the amount in it, a logo.
pub fn everything_on() -> ReceiptSettings {
    let mut s = ReceiptSettings::default();
    s.show.hsn = true;
    s.show.tax_summary = true;
    s.show.payment_lines = true;
    s.qr = mb_print::settings::QrMode::Dynamic;
    s.logo = mb_print::settings::LogoPosition::Top;
    s.footer = "Thank you, visit again".to_owned();
    s
}

/// A bill with every feature turned on, ready to render.
pub struct Fixture {
    pub bill: Bill,
    pub order: AnyOrder,
    pub store: Store,
    pub customer: BillCustomer,
    pub settings: ReceiptSettings,
    pub logo: Vec<u8>,
}

impl Fixture {
    pub fn new() -> Self {
        let cart = cart();
        let bill = bill(&cart);
        let settlement = settlement(&bill);
        let order = order(bill.clone(), settlement);
        Fixture {
            bill,
            order,
            store: store(),
            customer: customer(),
            settings: everything_on(),
            // Not a real PNG; nothing in this crate decodes one.
            logo: vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
        }
    }

    pub fn context(&self, copy: Copy) -> BillContext<'_> {
        BillContext {
            bill: &self.bill,
            order: &self.order,
            store: &self.store,
            settings: &self.settings,
            customer: Some(&self.customer),
            cashier: Some("Ravi"),
            // The table's own name, a time and a waiter — the values a real print resolves, so
            // the fixture exercises the same shape.
            table: Some("6"),
            time: Some("19:42"),
            waiter: Some("Suresh"),
            copy,
            einvoice: EInvoice::default(),
            logo: Some(self.logo.clone()),
        }
    }
}

impl Default for Fixture {
    fn default() -> Self {
        Fixture::new()
    }
}

// A scratch database, for the tests that need the queue to survive a restart.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use mb_db::{Db, Repos};

pub const OUTLET: &str = "outlet_default";

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A directory that deletes itself when the test ends.
pub struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    pub fn new(label: &str) -> Scratch {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("mb-print-{label}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch directory");
        Scratch { dir }
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A printer row, so `print_jobs.printer_id` has something to reference.
pub fn seed_printer(db: &Db, id: &str) {
    db.transaction(|tx| {
        Repos::new(tx).settings().save_printer(
            OUTLET,
            &mb_db::repo::settings::Printer {
                id: id.to_owned(),
                name: id.to_owned(),
                kind: "none".to_owned(),
                address: None,
                paper_mm: 80,
                is_default: true,
                can_kick_drawer: false,
                offset_x_mm: 0,
                offset_y_mm: 0,
                role: "both".to_owned(),
                engine: "raster".to_owned(),
                is_bold_dark: false,
            },
            mb_core::Timestamp::from_millis(1_770_000_000_000),
        )
    })
    .expect("seeds a printer");
}

/// Wait for a condition, polling briefly.
pub fn until(mut check: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if check() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    false
}

/// The built-in face's metrics for a roll — what a test that only wants a document hands
/// `bill_document`.
pub fn metrics(kind: mb_print::paper::PaperKind) -> mb_print::metrics::Metrics {
    mb_print::metrics::Metrics::face(
        mb_print::paper::Paper::new(kind),
        std::sync::Arc::new(mb_print::font::Font::builtin().expect("the shipped face loads")),
    )
}

/// The same, for a roll that carries a print offset.
pub fn metrics_on(paper: mb_print::paper::Paper) -> mb_print::metrics::Metrics {
    mb_print::metrics::Metrics::face(
        paper,
        std::sync::Arc::new(mb_print::font::Font::builtin().expect("the shipped face loads")),
    )
}
