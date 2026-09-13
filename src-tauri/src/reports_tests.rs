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
