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
    let stat = |label: &str| dashboard.stats.iter().find(|s| s.label == label).expect("stat");
    assert_eq!(stat("Net sales").value, amount);
    for id in ["trend", "payment", "types", "cashiers"] {
        let chart = dashboard.charts.iter().find(|chart| chart.id == id).expect("chart");
        let sum: i64 = chart.points.iter().map(|point|
            mb_core::Money::parse(&point.value).expect("chart money").paise()).sum();
        assert_eq!(sum, paise, "{id} chart disagrees with report totals");
    }
    let average = if count > 0 { paise.checked_div(count).expect("nonzero") } else { 0 };
    assert_eq!(stat("Average bill").value, mb_core::Money::from_paise(average).to_plain_string());
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
    let totals = report.totals.expect("totals");
    assert_eq!(totals[1], "2");
    assert_eq!(totals.last().map(String::as_str), Some("130.00"));
    let dashboard = crate::reports::dashboard_on(&app, Some(today())).expect("dashboard");
    let stat = |label: &str| dashboard.stats.iter().find(|s| s.label == label).expect("stat");
    assert_eq!(stat("Net sales").value, "130.00");
    assert_eq!(stat("Average bill").value, "65.00");
    assert_eq!(stat("Expected cash in drawer").value, "—", "a summary cannot reconstruct a drawer");
    assert!(dashboard.attention.iter().any(|a| a.title.contains("Archived")));
}
