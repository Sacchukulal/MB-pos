#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "tests: expect is the assertion"
)]

//! A report on paper: on the roll a bill comes off, and on a sheet.

use mb_print::layout::layout;
use mb_print::paper::{Paper, PaperKind};
use mb_print::template::{ReportColumn, ReportContext, Store, report_document};
use mb_print::text;

fn a_store() -> Store {
    Store {
        name: "Anna Kuteera".to_owned(),
        address: "12 MG Road, Bengaluru".to_owned(),
        phone: None,
        gstin: None,
        fssai: None,
        state_code: None,
        upi_id: None,
        upi_merchant_name: None,
        upi_reference: None,
        registration: mb_core::Registration::Regular,
    }
}

/// The six columns of Item sales, which is the report a shop prints most.
fn columns() -> Vec<ReportColumn> {
    [
        ("Item", false),
        ("Bills", true),
        ("Quantity", true),
        ("Discount", true),
        ("Tax", true),
        ("Total", true),
    ]
    .into_iter()
    .map(|(header, numeric)| ReportColumn {
        header: header.to_owned(),
        numeric,
    })
    .collect()
}

fn rows() -> Vec<Vec<String>> {
    vec![
        ["Baby corn manchurian", "3", "3", "0.00", "22.50", "472.50"]
            .map(str::to_owned)
            .to_vec(),
        ["BLACK TEA", "1", "2", "0.00", "1.50", "31.50"]
            .map(str::to_owned)
            .to_vec(),
    ]
}

fn totals() -> Vec<String> {
    ["Total", "10", "11", "0.00", "68.50", "1438.50"]
        .map(str::to_owned)
        .to_vec()
}

fn printed(kind: PaperKind, ctx: &ReportContext<'_>) -> String {
    text::to_text(&layout(&report_document(Paper::new(kind), ctx)).expect("lays out"))
}

/// Six columns do not fit an 80 mm roll. The roll gets the name and the total on one row and
/// the rest underneath in words — never a table squeezed to four characters a column.
#[test]
fn a_till_roll_stacks_what_it_cannot_fit_across() {
    let store = a_store();
    let columns = columns();
    let rows = rows();
    let totals = totals();
    let ctx = ReportContext {
        store: &store,
        title: "Item sales",
        subtitle: "2026-09-13",
        columns: &columns,
        rows: &rows,
        totals: Some(&totals),
        notes: &[],
        printed: Some("Printed 13 Sep, 9:41 am"),
    };
    let paper = printed(PaperKind::Mm80, &ctx);

    // Whose figures, what they are, and as at when: a report leaves the building.
    assert!(paper.contains("Anna Kuteera"), "{paper}");
    assert!(paper.contains("Item sales"), "{paper}");
    assert!(paper.contains("Printed 13 Sep"), "{paper}");

    // The name and the money a person is looking for, on one line.
    assert!(
        paper
            .lines()
            .any(|line| line.contains("Baby corn manchurian") && line.contains("472.50")),
        "{paper}"
    );
    // The columns the roll could not carry are labelled, not left as bare numbers.
    assert!(paper.contains("Quantity 3"), "{paper}");
    assert!(paper.contains("Tax 22.50"), "{paper}");
    // And the total is on it.
    assert!(
        paper
            .lines()
            .any(|line| line.contains("Total") && line.contains("1438.50")),
        "{paper}"
    );
    // Nothing was shrunk to fit.
    assert!(
        !layout(&report_document(Paper::new(PaperKind::Mm80), &ctx))
            .expect("lays out")
            .was_capped(),
        "the report was shrunk to fit rather than restacked"
    );
}

/// 58 mm is narrower still, and the same shape holds.
#[test]
fn the_narrowest_roll_still_reads() {
    let store = a_store();
    let columns = columns();
    let rows = rows();
    let ctx = ReportContext {
        store: &store,
        title: "Item sales",
        subtitle: "2026-09-13",
        columns: &columns,
        rows: &rows,
        totals: None,
        notes: &[],
        printed: None,
    };
    let paper = printed(PaperKind::Mm58, &ctx);
    assert!(paper.contains("BLACK TEA"), "{paper}");
    assert!(paper.contains("Bills 1"), "{paper}");
}

/// A sheet has room for the table, and a table is what a sheet is for.
#[test]
fn a_sheet_gets_the_table() {
    let store = a_store();
    let columns = columns();
    let rows = rows();
    let totals = totals();
    let ctx = ReportContext {
        store: &store,
        title: "Item sales",
        subtitle: "2026-09-13",
        columns: &columns,
        rows: &rows,
        totals: Some(&totals),
        notes: &["Two items have no cost price.".to_owned()],
        printed: None,
    };
    let paper = printed(PaperKind::A4, &ctx);

    // One row of headings carries them all.
    assert!(
        paper
            .lines()
            .any(|line| line.contains("Quantity") && line.contains("Discount")),
        "{paper}"
    );
    // And one line carries a whole row.
    assert!(
        paper
            .lines()
            .any(|line| line.contains("Baby corn manchurian") && line.contains("472.50")),
        "{paper}"
    );
    // The note came written.
    assert!(paper.contains("Two items have no cost price."), "{paper}");
}

/// An empty period says so, rather than printing a heading with nothing under it.
#[test]
fn an_empty_report_says_so() {
    let store = a_store();
    let columns = columns();
    let ctx = ReportContext {
        store: &store,
        title: "Item sales",
        subtitle: "2026-09-13",
        columns: &columns,
        rows: &[],
        totals: None,
        notes: &[],
        printed: None,
    };
    assert!(printed(PaperKind::Mm80, &ctx).contains("Nothing in this period"));
}
