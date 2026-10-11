//! Printing a report: the whole way from the button to the bytes a printer receives.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

use std::io::Read;
use std::sync::mpsc;

use crate::licence_tests::{a_trading_shop, licence_in};
use crate::reports::{PeriodArg, report_print_on};
use crate::settings::printers::{PrinterEdit, save_printer_on};
use crate::signin_tests::{Scratch, slips_taken};
use crate::state::App;

/// A thermal printer that keeps the paper: it answers on a port and hands back what it was
/// sent. The one way to prove a report reached a PRINTER and not just a queue — the stand-in
/// printer a shop starts with takes every job and prints nothing.
fn a_printer_that_keeps_the_paper() -> (u16, mpsc::Receiver<Vec<u8>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port for a test printer");
    let port = listener.local_addr().expect("an address").port();
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut paper = Vec::new();
            // The queue closes the socket when the job is done, which ends this read.
            let _ = stream.read_to_end(&mut paper);
            if send.send(paper).is_err() {
                return;
            }
        }
    });
    (port, receive)
}

/// Point the shop's bill printer at it, through the same screen a shopkeeper would use, and in
/// the text engine so the paper reads as characters rather than as dots.
fn print_to(app: &App, port: u16) {
    save_printer_on(
        app,
        PrinterEdit {
            id: String::new(),
            name: "The test printer".to_owned(),
            kind: "network".to_owned(),
            address: format!("127.0.0.1:{port}"),
            paper_mm: 80,
            is_default: true,
            role: "both".to_owned(),
            engine: "text".to_owned(),
            is_bold_dark: false,
            can_kick_drawer: false,
        },
    )
    .expect("the printer is saved");
}

fn today() -> PeriodArg {
    let day = crate::flows::today(crate::flows::now()).to_string();
    PeriodArg {
        from: day.clone(),
        to: day,
    }
}

fn a_licensed_shop(scratch: &Scratch, name: &str) -> App {
    let app = a_trading_shop(scratch, name);
    app.use_licensing(licence_in(scratch, name, mb_license::Status::Active, 90));
    app
}

/// One tea, paid for in cash — so Item sales has a line on it and the paper has a figure to
/// carry. A report of nothing proves nothing.
fn sell_a_tea(app: &App) {
    app.with_cart_mut(|state| {
        state.set_order_type(mb_core::OrderType::Parcel);
        Ok(())
    })
    .expect("parcel");
    crate::ipc::cart_add_on(app, "itm_tea".to_owned(), Some("2".to_owned()), None).expect("added");
    let total = app
        .with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total))
        .expect("a bill");
    app.with_cart_mut(|state| {
        let payment =
            mb_core::Payment::new(mb_core::PaymentMode::Cash, total).expect("a cash payment");
        state.settlement.add(payment).map_err(|e| {
            crate::words::UiError::new("bill.pay", "That payment could not be taken.")
                .with_detail(e.to_string())
        })
    })
    .expect("paid");
    crate::flows::complete_bill_on(app, None).expect("settled");
}

/// The button queues a job, and the job is a REPORT — which is the half migration 0016 exists
/// for: before it the database refused the kind and nothing reached paper at all.
#[test]
fn printing_a_report_puts_a_report_job_on_the_queue() {
    let scratch = Scratch::new("report_print");
    let app = a_licensed_shop(&scratch, "report_print");

    let (said, slips) = slips_taken(&app, || report_print_on(&app, "items".to_owned(), today()));
    let said = said.expect("the report printed");
    assert!(said.contains("printing"), "{said}");
    assert!(
        slips
            .iter()
            .any(|(kind, _)| *kind == mb_print::queue::JobKind::Report),
        "no report reached the queue: {slips:?}"
    );
}

/// And the paper itself, off the port of a printer that is really listening.
#[test]
fn the_report_comes_out_of_the_printer() {
    let scratch = Scratch::new("report_paper");
    let app = a_licensed_shop(&scratch, "report_paper");
    sell_a_tea(&app);
    let (port, paper) = a_printer_that_keeps_the_paper();
    print_to(&app, port);

    report_print_on(&app, "items".to_owned(), today()).expect("the report printed");

    let sheet = paper
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("the printer was sent nothing at all");
    let text = String::from_utf8_lossy(&sheet);

    assert!(text.contains("Item sales"), "the title is missing:\n{text}");
    // The figures, not just the heading.
    assert!(text.contains("Masala Tea"), "the dish is missing:\n{text}");
    assert!(text.contains("Quantity 2"), "the quantity is missing:\n{text}");
    assert!(text.contains("Total"), "the total is missing:\n{text}");
    assert!(text.contains("Printed"), "the date is missing:\n{text}");
}

fn assert_live_sales(app: &App, paise: i64, count: i64) {
    let amount = mb_core::Money::from_paise(paise).to_plain_string();
    for id in ["sales_day", "sales_hour", "sales_type", "sales_mode",
        "sales_cashier", "sales_section", "sales_terminal"] {
        let report = crate::reports::report_on(app, id.to_owned(), today()).expect("report");
        let totals = report.totals.expect("totals");
        assert_eq!(totals[1], count.to_string(), "{id} double-counted a bill");
        assert_eq!(totals.last(), Some(&amount), "{id} has the wrong sales");
    }
    let dashboard = crate::reports::dashboard_on(app, Some(today())).expect("dashboard");
    let summary = &dashboard.summary;
    assert_eq!(mb_core::Money::parse(&summary.sales).expect("sales").paise(), paise);
    let allocated: i64 = summary.payments.iter().map(|row|
        mb_core::Money::parse(&row.value).expect("payment").paise()).sum();
    assert_eq!(allocated, paise, "connected summary must reconcile");
    let cash = summary.drawer.as_ref().expect("one day drawer");
    let movements: i64 = cash.rows.iter().map(|row|
        mb_core::Money::parse(&row.value).expect("movement").paise()).sum();
    assert_eq!(movements, mb_core::Money::parse(&cash.expected).expect("expected").paise());
    for id in ["trend", "payment", "types", "cashiers"] {
        let chart = dashboard.charts.iter().find(|chart| chart.id == id).expect("chart");
        let sum: i64 = chart.points.iter().map(|point|
            mb_core::Money::parse(&point.value).expect("chart money").paise()).sum();
        assert_eq!(sum, paise, "{id} chart disagrees with report totals");
    }
    let average = if count > 0 { paise.checked_div(count).expect("nonzero") } else { 0 };
    assert_eq!(summary.details.iter().find(|s| s.id == "average").expect("average").value,
        mb_core::Money::from_paise(average).to_indian_string());
}

fn drawer(app: &App) -> mb_db::repo::money::CashPosition {
    app.with_shop(|shop| shop.db.read_transaction(|tx| {
        mb_db::Repos::new(tx).money().cash_position(crate::state::OUTLET,
            crate::flows::today(crate::flows::now()))
    }).map_err(|e| crate::words::from_db(&e))).expect("drawer")
}

#[test]
fn reports_follow_one_bill_through_repeated_edits_void_and_partial_refunds() {
    let scratch = Scratch::new("report_correction_lifecycle");
    let app = a_licensed_shop(&scratch, "report_correction_lifecycle");
    sell_a_tea(&app);
    let row = crate::corrections::list_bills_on(&app).expect("bills").remove(0);
    let original = drawer(&app).expected.paise();
    assert_live_sales(&app, original, 1);

    let mut issued = original;
    for qty in [4, 1, 1] {
        crate::corrections::revert_bill_on(&app, row.order_id.clone(),
            "Correct quantity".to_owned(), None, None).expect("revert");
        app.with_cart_mut(|state| {
            state.cart.set_qty(0, mb_core::Qty::from_whole(qty).expect("qty"))
                .map_err(|e| crate::words::UiError::new("test", e.to_string()))
        }).expect("edit");
        // Editing has no financial effect until the replacement is committed.
        assert_live_sales(&app, issued, 1);
        assert_eq!(drawer(&app).expected.paise(), issued);
        let replacement = app.with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total.paise())).expect("bill");
        let number = crate::flows::complete_bill_with_return_on(&app,
            Some("Cash".to_owned()), Some("Cash".to_owned())).expect("finish correction");
        assert_eq!(number, row.number);
        issued = replacement;
        assert_live_sales(&app, issued, 1);
        assert_eq!(drawer(&app).expected.paise(), issued, "adjustment refund deducted twice");
        assert_eq!(crate::corrections::list_bills_on(&app).expect("bills").len(), 1);
    }

    // Another equal bill makes the average sensitive to including voids in its denominator.
    sell_a_tea(&app);
    crate::corrections::void_bill_on(&app, row.order_id.clone(),
        "Customer cancelled".to_owned(), None, None).expect("void");
    assert_live_sales(&app, original, 1);
    assert_eq!(drawer(&app).expected.paise(), original + issued,
        "voiding is not itself a cash refund");
    crate::corrections::refund_on(&app, row.order_id.clone(), 100,
        "Cash".to_owned(), "Part returned".to_owned()).expect("partial refund");
    assert_eq!(drawer(&app).expected.paise(), original + issued - 100);
    assert_live_sales(&app, original, 1);
    crate::corrections::refund_on(&app, row.order_id, issued - 100,
        "cash".to_owned(), "Remainder returned".to_owned()).expect("remaining refund");
    assert_eq!(drawer(&app).expected.paise(), original);
    assert_live_sales(&app, original, 1);
}

#[test]
fn split_payments_count_one_bill_and_do_not_repeat_bill_tax() {
    let scratch = Scratch::new("report_split_counts");
    let app = a_licensed_shop(&scratch, "report_split_counts");
    crate::ipc::cart_add_on(&app, "itm_tea".to_owned(), Some("4".to_owned()), None).expect("tea");
    let total = app.with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total)).expect("bill");
    app.with_cart_mut(|state| {
        for (mode, amount) in [(mb_core::PaymentMode::Cash, 1_000),
            (mb_core::PaymentMode::Cash, 1_000), (mb_core::PaymentMode::Card, total.paise() - 2_000)] {
            state.settlement.add(mb_core::Payment::new(mode, mb_core::Money::from_paise(amount)).expect("payment"))
                .map_err(|e| crate::words::UiError::new("test", e.to_string()))?;
        }
        Ok(())
    }).expect("paid");
    crate::flows::complete_bill_on(&app, None).expect("settled");
    assert_live_sales(&app, total.paise(), 1);
    let report = crate::reports::report_on(&app, "sales_mode".to_owned(), today()).expect("report");
    assert_eq!(report.rows.len(), 2);
    assert_eq!(report.columns.len(), 3, "payment modes have no tax allocation");
}

#[test]
fn archived_daily_totals_agree_with_dashboard_sales_and_active_bill_count() {
    let scratch = Scratch::new("report_archived_dashboard");
    let app = a_licensed_shop(&scratch, "report_archived_dashboard");
    let at = crate::flows::now();
    let day = crate::flows::today(at);
    app.with_shop(|shop| shop.db.transaction(|tx| {
        mb_db::Repos::new(tx).wire().restore_row(crate::state::OUTLET, "day_totals",
            &day.to_string(), at, &serde_json::json!({
                "business_day": day.days_since_epoch(), "bills": 3, "voids": 1,
                "gross_paise": 13_000, "net_paise": 13_000
            }))
    }).map_err(|e| crate::words::from_db(&e))).expect("archive");
    let report = crate::reports::report_on(&app, "sales_day".to_owned(), today()).expect("report");
    assert_eq!(report.rows[0][0], crate::words::date(day));
    assert_eq!(report.subtitle, crate::words::date(day));
    assert_eq!(today().from, day.to_string(), "queries must remain ISO dates");
    let totals = report.totals.expect("totals");
    assert_eq!(totals[1], "2");
    assert_eq!(totals.last().map(String::as_str), Some("130.00"));
    let dashboard = crate::reports::dashboard_on(&app, Some(today())).expect("dashboard");
    assert_eq!(dashboard.summary.sales, "₹130.00");
    assert_eq!(dashboard.summary.details.iter().find(|s| s.id == "average").expect("average").value, "₹65.00");
    assert!(dashboard.attention.iter().any(|a| a.title.contains("Archived")));
    assert!(dashboard.summary.payments.is_empty());
    assert!(dashboard.summary.drawer.is_none());
    assert!(dashboard.summary.payment_note.contains("archived"));
}

#[test]
fn dashboard_keeps_every_tender_tips_and_credit_collections_distinct() {
    use mb_core::{CustomerId, Money, Payment, PaymentMode};
    let scratch = Scratch::new("dashboard_modes");
    let app = a_licensed_shop(&scratch, "dashboard_modes");
    crate::credit::save_customer_on(&app, crate::credit::CustomerEdit {
        id: "cus_dashboard".to_owned(), name: "Regular".to_owned(), phone: "9000000088".to_owned(),
        gstin: String::new(), address: String::new(), credit_limit: "5000".to_owned(), is_active: true,
    }).expect("customer");
    crate::ipc::cart_add_on(&app, "itm_tea".to_owned(), Some("10".to_owned()), None).expect("tea");
    let total = app.with_cart(|state| Ok(state.bill(&app.shop_config())?.grand_total)).expect("bill");
    app.with_cart_mut(|state| {
        state.settlement.set_tip(Money::from_paise(500)).expect("tip");
        for (mode, amount) in [
            (PaymentMode::Cash, 1500), (PaymentMode::Upi, 1000), (PaymentMode::Card, 1000),
            (PaymentMode::Other("Meal voucher".to_owned()), 1000),
            (PaymentMode::Credit(CustomerId::new("cus_dashboard")), total.paise() - 4000),
        ] {
            state.settlement.add(Payment::new(mode, Money::from_paise(amount)).expect("payment")).expect("added");
        }
        Ok(())
    }).expect("split");
    crate::flows::complete_bill_on(&app, None).expect("settled");
    let dashboard = crate::reports::dashboard_on(&app, Some(today())).expect("dashboard");
    let summary = dashboard.summary;
    let ids: Vec<_> = summary.payments.iter().map(|p| p.id.as_str()).collect();
    for id in ["cash", "upi", "card", "other:meal voucher", "credit", "tips"] { assert!(ids.contains(&id), "missing {id}"); }
    assert!(!ids.contains(&"unallocated"));
    assert_eq!(summary.payments.iter().find(|p| p.id == "credit").expect("credit").label, "On credit");
    assert_eq!(summary.payments.iter().find(|p| p.id == "other:meal voucher").expect("custom").label, "meal voucher");
    let sum: i64 = summary.payments.iter().map(|p| Money::parse(&p.value).expect("money").paise()).sum();
    assert_eq!(sum, total.paise());
    assert_eq!(summary.drawer.expect("drawer").expected, Money::from_paise(1500).to_indian_string());
    crate::credit::record_repayment_on(&app, "cus_dashboard".to_owned(), "10".to_owned(), "cash".to_owned(), String::new()).expect("repayment");
    let repaid = crate::reports::dashboard_on(&app, Some(today())).expect("dashboard after repayment").summary;
    assert_eq!(repaid.sales, total.to_indian_string(), "repayment is not another sale");
    assert_eq!(repaid.secondary.iter().find(|r| r.id == "collections").expect("collection").value, "₹10.00");
    assert_eq!(repaid.drawer.expect("drawer").expected, "₹25.00");
}

#[test]
fn dashboard_empty_and_multi_day_periods_do_not_invent_drawer_balances() {
    let scratch = Scratch::new("dashboard_period_shapes");
    let app = a_licensed_shop(&scratch, "dashboard_period_shapes");
    let view = crate::reports::dashboard_on(&app, Some(today())).expect("empty");
    assert_eq!(view.summary.sales, "₹0.00");
    assert!(view.summary.payments.is_empty());
    let day = crate::flows::today(crate::flows::now());
    let view = crate::reports::dashboard_on(&app, Some(PeriodArg { from: day.previous().to_string(), to: day.to_string() })).expect("range");
    assert!(view.summary.drawer.is_none());
    assert!(view.summary.drawer_unavailable.contains("one day"));
}
