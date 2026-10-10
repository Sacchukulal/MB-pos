#![allow(clippy::expect_used, clippy::panic, reason = "tests: expect is the assertion")]
mod common;

use common::{Scratch, OUTLET, TERMINAL};
use mb_core::{AnyOrder, BillInput, BusinessDay, Cart, DraftOrder, ItemId, ItemSnapshot, Money,
    OrderId, Payment, PaymentMode, Placement, Qty, Registration, Settlement, StaffId, TaxRate, TaxSpec, Timestamp, compute_bill};
use mb_db::{Db, Repos};
use mb_db::repo::{DayKind, DayRow, Refund};
use mb_db::repo::reports::{Period, SalesBy};
use mb_db::repo::returns::FullReturn;

fn day(offset: i32) -> BusinessDay { BusinessDay::from_days_since_epoch(20_700 + offset) }
fn at(offset: i64) -> Timestamp { Timestamp::from_millis(1_788_480_000_000 + offset) }
fn masters(db: &Db) {
    db.transaction(|tx| { tx.execute_batch(common::STAFF_SQL)?; tx.execute_batch(common::MENU_SQL)?; Ok(()) }).expect("masters");
}
fn sale(db: &Db) -> mb_core::SettledOrder {
    sale_with_credit(db, None)
}
fn sale_with_credit(db: &Db, customer: Option<mb_core::CustomerId>) -> mb_core::SettledOrder {
    let mut cart = Cart::new();
    let mut item = ItemSnapshot::new(ItemId::new("itm_dosa"), "Dosa", Money::from_paise(10_000), TaxRate::from_percent(5).expect("rate"))
        .with_tax(TaxSpec::gst(TaxRate::from_percent(5).expect("rate")));
    item.hsn = Some("2106".to_owned());
    cart.add(item, Qty::ONE, None, vec![]).expect("item");
    let bill = compute_bill(BillInput::new(&cart, Registration::Regular).with_order_type(mb_core::OrderType::Parcel)).expect("bill");
    let mut payments = Settlement::new();
    if let Some(customer) = customer {
        payments.add(Payment::new(PaymentMode::Credit(customer), bill.grand_total).expect("credit")).expect("credit added");
    } else {
        payments.add(Payment::new(PaymentMode::Cash, Money::from_paise(6000)).expect("cash")).expect("cash added");
        payments.add(Payment::new(PaymentMode::Card, bill.grand_total.sub(Money::from_paise(6000)).expect("remainder")).expect("card")).expect("card added");
    }
    let mut draft = DraftOrder::new(OrderId::new("ord_historical"), day(0), at(0), Placement::Parcel, StaffId::new("staff_1"));
    draft.core.cart = cart;
    let till = mb_db::Till::new(OUTLET, TERMINAL);
    let open = mb_db::open_draft(db, till, draft).expect("open");
    mb_db::settle(db, till, open, bill, payments, at(1), StaffId::new("staff_1")).expect("settle")
}
fn lock(db: &Db, on: BusinessDay) {
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        let figures = repos.days().figures(OUTLET, on)?;
        repos.days().lock(OUTLET, &DayRow { day: on, kind: DayKind::Trading, is_locked: true,
            closed_at: Some(at(2)), closed_by: Some(StaffId::new("staff_1")), reopened_at: None,
            reopened_by: None, note: None, bills: figures.bills, net: figures.net, cash_taken: figures.cash })
    }).expect("close");
}
fn request() -> FullReturn { FullReturn { id: "ret_historical".to_owned(), order_id: OrderId::new("ord_historical"),
    business_day: day(1), terminal_id: TERMINAL.to_owned(), at: at(90_000_000), by: StaffId::new("staff_1"), reason: "Returned meal".to_owned() } }
fn post(db: &Db) {
    db.transaction(|tx| {
        let repos = Repos::new(tx);
        if repos.returns().record_full(OUTLET, &request())? {
            for (mode, amount) in repos.corrections().refundable_payments(&request().order_id)? {
                repos.corrections().record_refund(OUTLET, &Refund { id: format!("ref_{mode}"),
                    order_id: request().order_id, amount, mode, reason: "Returned meal".to_owned(),
                    refunded_at: request().at, refunded_by: Some(request().by) }, day(1))?;
            }
        }
        Ok(())
    }).expect("post return and actual refunds together");
}

#[test]
fn closed_sale_stays_intact_and_return_posts_exact_sales_tax_and_actual_tenders_once() {
    let scratch = Scratch::new("historical-return"); let db = scratch.open(); masters(&db);
    let original = sale(&db); lock(&db, day(0)); post(&db); post(&db);
    db.read_transaction(|tx| {
        let repos = Repos::new(tx);
        let old = repos.corrections().day_totals(OUTLET, day(0))?;
        assert_eq!(old.net, original.bill.grand_total);
        assert_eq!(old.returned, Money::ZERO);
        let today = repos.corrections().day_totals(OUTLET, day(1))?;
        assert_eq!(today.returned, original.bill.grand_total);
        assert_eq!(today.net.paise(), -original.bill.grand_total.paise());
        assert_eq!(today.refunded, original.bill.grand_total);
        assert_eq!(today.bills, 0);
        let saved = repos.orders().find(&request().order_id)?.expect("original still exists");
        assert_eq!(saved, AnyOrder::Settled(original.clone()));
        assert!(repos.days().is_locked(OUTLET, day(0))?);
        assert_eq!(repos.money().cash_position(OUTLET, day(1))?.cash_refunds.paise(), 6000);
        assert_eq!(repos.corrections().refunds_for(&request().order_id)?.len(), 2);
        assert!(repos.corrections().refundable_payments(&request().order_id)?.iter().all(|(_, amount)| amount.is_zero()));
        for by in [SalesBy::Day, SalesBy::Hour, SalesBy::OrderType, SalesBy::Cashier, SalesBy::Terminal, SalesBy::Section, SalesBy::Item, SalesBy::Category, SalesBy::PaymentMode] {
            let buckets = repos.reports().sales_by(OUTLET, Period::one_day(day(1)), by)?;
            assert_eq!(buckets.iter().map(|b| b.gross.paise()).sum::<i64>(), -original.bill.grand_total.paise(), "{by:?}");
            assert!(buckets.iter().all(|b| b.bills == 0));
        }
        let taxes = repos.reports().tax_by_rate(OUTLET, Period::one_day(day(1)))?;
        assert_eq!(taxes[0].cgst.paise(), -original.bill.total_gst.central.paise());
        assert_eq!(taxes[0].sgst.paise(), -original.bill.total_gst.state.paise());
        let hsn = repos.reports().tax_by_hsn(OUTLET, Period::one_day(day(1)))?;
        assert_eq!(hsn[0].qty.thousandths(), -1000);
        assert_eq!(repos.reports().profit(OUTLET, Period::one_day(day(1)))?.net_sales.paise(), -10_000);
        let both = repos.reports().sales_by(OUTLET, Period::new(day(0), day(1)), SalesBy::Item)?;
        assert_eq!(both[0].gross, Money::ZERO);
        assert_eq!(both[0].qty.expect("quantity").thousandths(), 0);
        assert_eq!(repos.days().figures(OUTLET, day(1))?.cash.paise(), -6000);
        assert!(repos.reports().control_log(OUTLET, Period::one_day(day(1)))?.iter().any(|r| r.kind == "return"));
        Ok(())
    }).expect("all reporting paths reconcile");
}

#[test]
fn returns_require_a_closed_original_an_open_current_day_and_rollback_with_failed_refund() {
    let scratch = Scratch::new("historical-return-guards"); let db = scratch.open(); masters(&db); sale(&db);
    assert!(db.transaction(|tx| Repos::new(tx).returns().record_full(OUTLET, &request())).is_err());
    lock(&db, day(0));
    let result = db.transaction(|tx| {
        let repos = Repos::new(tx); repos.returns().record_full(OUTLET, &request())?;
        repos.corrections().record_refund(OUTLET, &Refund { id: "bad".to_owned(), order_id: request().order_id,
            amount: Money::from_paise(999999), mode: "cash".to_owned(), reason: "Wrong amount".to_owned(),
            refunded_at: request().at, refunded_by: Some(request().by) }, day(1))
    });
    assert!(result.is_err());
    assert!(!db.read_transaction(|tx| Repos::new(tx).returns().contains(&request().order_id)).expect("no half-return"));
    lock(&db, day(1));
    assert!(db.transaction(|tx| Repos::new(tx).returns().record_full(OUTLET, &request())).is_err());
}

#[test]
fn paying_a_return_later_changes_that_days_drawer_without_rewriting_closed_sales() {
    let scratch = Scratch::new("historical-return-delayed-payment"); let db = scratch.open(); masters(&db);
    let original = sale(&db); lock(&db, day(0));
    db.transaction(|tx| Repos::new(tx).returns().record_full(OUTLET, &request())).expect("return, payment still owed");
    let before = db.read_transaction(|tx| {
        let repos = Repos::new(tx);
        let figures = repos.days().figures(OUTLET, day(1))?;
        let by_payment = repos.reports().sales_by(OUTLET, Period::one_day(day(1)), SalesBy::PaymentMode)?;
        assert_eq!(by_payment.iter().map(|bucket| bucket.gross.paise()).sum::<i64>(), -original.bill.grand_total.paise());
        assert_eq!(figures.cash.paise(), -6000, "sales tender attribution reverses the full original payment");
        assert_eq!(repos.money().cash_position(OUTLET, day(1))?.cash_refunds, Money::ZERO, "no cash handed back yet");
        Ok(figures)
    }).expect("return day before payout");
    lock(&db, day(1));
    assert!(db.transaction(|tx| Repos::new(tx).corrections().record_refund(OUTLET, &Refund {
        id: "closed_day_cash".to_owned(), order_id: request().order_id, amount: Money::from_paise(6000),
        mode: "cash".to_owned(), reason: "Late request for closed day".to_owned(),
        refunded_at: request().at, refunded_by: Some(request().by),
    }, day(1))).is_err(), "the refund transaction rechecks its business-day lock");
    db.transaction(|tx| Repos::new(tx).corrections().record_refund(OUTLET, &Refund {
        id: "later_cash".to_owned(), order_id: request().order_id, amount: Money::from_paise(6000),
        mode: "cash".to_owned(), reason: "Collected next day".to_owned(),
        refunded_at: at(180_000_000), refunded_by: Some(request().by),
    }, day(2))).expect("actual cash returned later");
    db.read_transaction(|tx| {
        let repos = Repos::new(tx);
        assert_eq!(repos.days().figures(OUTLET, day(1))?, before, "closed return-day sales are immutable");
        assert_eq!(repos.days().find(OUTLET, day(1))?.expect("closed return day").cash_taken, before.cash);
        assert_eq!(repos.money().cash_position(OUTLET, day(1))?.cash_refunds, Money::ZERO);
        assert_eq!(repos.money().cash_position(OUTLET, day(2))?.cash_refunds.paise(), 6000);
        assert!(repos.reports().sales_by(OUTLET, Period::one_day(day(2)), SalesBy::PaymentMode)?.is_empty());
        Ok(())
    }).expect("payout date and sale adjustment date stay separate");
}

#[test]
fn return_only_day_archives_and_restores_the_original_and_frozen_return_rows() {
    let scratch = Scratch::new("historical-return-archive"); let db = scratch.open(); masters(&db);
    sale(&db); lock(&db, day(0)); post(&db);
    let file = db.read_transaction(|tx| {
        let archive = Repos::new(tx).archive();
        assert!(archive.pending(OUTLET, day(5), 100)?.contains(&day(1)));
        archive.day_file(OUTLET, day(1), at(500_000_000), "test")
    }).expect("return-only archive");
    assert_eq!(file.bills, 0);
    let text = mb_db::archive::gunzip(&file.gz).expect("unpack");
    assert!(text.contains("bill_returns")); assert!(text.contains("bill_return_lines"));
    let target = Scratch::new("historical-return-restore"); let restored = target.open(); masters(&restored);
    let mut report = mb_db::repo::wire::RestoreReport::default();
    restored.transaction(|tx| Repos::new(tx).archive().restore_text(OUTLET, &text, &mut report)).expect("restore");
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    restored.read_transaction(|tx| {
        let repos = Repos::new(tx);
        assert!(repos.returns().contains(&request().order_id)?);
        assert_eq!(repos.corrections().day_totals(OUTLET, day(0))?.net.paise(), 10500);
        assert_eq!(repos.corrections().day_totals(OUTLET, day(1))?.net.paise(), -10500);
        assert!(repos.wire().cloud_days(OUTLET, Period::one_day(day(1)))?.is_empty(), "restored header cannot count the local return twice");
        assert_eq!(repos.money().cash_position(OUTLET, day(1))?.cash_refunds.paise(), 6000);
        Ok(())
    }).expect("restored rows reconcile");
}

#[test]
fn verified_backup_counts_and_restores_return_headers_and_frozen_lines() {
    let scratch = Scratch::new("historical-return-backup"); let db = scratch.open(); masters(&db);
    sale(&db); lock(&db, day(0)); post(&db);
    let taken = mb_db::backup::take(&db, &scratch.db_path().with_file_name("returned-sale.db"), "test").expect("backup with return");
    for table in ["bill_returns", "bill_return_lines"] {
        assert_eq!(taken.manifest.counts.iter().find(|(name, _)| name == table).map(|(_, rows)| *rows), Some(1));
    }
    assert!(mb_db::backup::verify(&taken.path).expect("verify complete backup").is_ok());
    let target = Scratch::new("historical-return-backup-restored");
    let restored_report = mb_db::backup::restore(&taken.path, &target.db_path()).expect("restore backup");
    assert!(!restored_report.rolled_back, "{:?}", restored_report.failure);
    let restored = target.open();
    restored.read_transaction(|tx| {
        let repos = Repos::new(tx);
        assert!(repos.returns().contains(&request().order_id)?);
        assert_eq!(repos.corrections().day_totals(OUTLET, day(1))?.net.paise(), -10500);
        assert_eq!(repos.reports().tax_by_rate(OUTLET, Period::one_day(day(1)))?[0].cgst.paise(), -250);
        Ok(())
    }).expect("restored return still reconciles");
    let manifest = std::fs::read_to_string(taken.manifest_path()).expect("manifest");
    let missing_rows = manifest.replace("count bill_returns 1", "count bill_returns 2")
        .replace("count bill_return_lines 1", "count bill_return_lines 2");
    std::fs::write(taken.manifest_path(), missing_rows).expect("simulate promised rows absent from backup");
    let rejected = mb_db::backup::verify(&taken.path).expect("detect missing return rows");
    for table in ["bill_returns", "bill_return_lines"] {
        assert!(rejected.count_mismatches.iter().any(|(name, promised, actual)| name == table && *promised == 2 && *actual == 1));
    }
    assert!(!rejected.is_ok());
}

#[test]
fn backup_before_historical_returns_still_verifies_and_migrates_on_restore() {
    let scratch = Scratch::new("before-return-schema-backup"); let db = scratch.open(); masters(&db);
    // Isolated legacy fixture: remove precisely the schema introduced by 22. Migration23
    // only projects receipt data, so this empty shop now has the schema of a pre22 backup.
    db.transaction(|tx| {
        tx.execute_batch("DROP VIEW report_sale_payments; DROP VIEW report_bill_lines;
            DROP VIEW report_order_lines; DROP VIEW report_sale_bills; DROP VIEW report_sale_orders;
            DROP TABLE bill_return_lines; DROP TABLE bill_returns;
            DELETE FROM schema_version WHERE version >= 22;")?;
        Ok(())
    }).expect("legacy fixture schema");
    let taken = mb_db::backup::take(&db, &scratch.db_path().with_file_name("legacy.db"), "before-returns").expect("legacy backup");
    assert_eq!(taken.manifest.schema_version, 21);
    assert!(!taken.manifest.counts.iter().any(|(table, _)| table == "bill_returns" || table == "bill_return_lines"));
    assert!(mb_db::backup::verify(&taken.path).expect("verify old backup without new tables").is_ok());
    let target = Scratch::new("before-return-schema-restored");
    let report = mb_db::backup::restore(&taken.path, &target.db_path()).expect("restore and migrate old backup");
    assert!(!report.rolled_back, "{:?}", report.failure);
    assert_eq!(report.migrated_to, mb_db::migrate::latest_version());
    target.open().read_transaction(|tx| {
        assert!(!Repos::new(tx).returns().contains(&request().order_id)?);
        Ok(())
    }).expect("new return schema available after old backup restore");
}

#[test]
fn a_historical_credit_return_reduces_the_account_without_inventing_a_cash_refund() {
    let scratch = Scratch::new("historical-credit-return"); let db = scratch.open(); masters(&db);
    let customer = mb_core::CustomerId::new("cus_return");
    db.transaction(|tx| {
        Repos::new(tx).money().save_customer(OUTLET, &mb_db::repo::money::Customer {
            id: customer.clone(), name: "Customer".to_owned(), phone: None, gstin: None,
            address: None, credit_limit: None, is_active: true,
        }, at(0))?;
        Ok(())
    }).expect("credit bill");
    sale_with_credit(&db, Some(customer.clone()));
    lock(&db, day(0)); post(&db);
    let history = db.read_transaction(|tx| {
        let repos = Repos::new(tx);
        assert_eq!(repos.money().customer_balance(&customer)?, Money::ZERO);
        let movements = repos.money().credit_movements(&customer)?;
        assert_eq!(mb_core::credit::balance(&movements).expect("ledger sum"), Money::ZERO);
        assert!(movements.iter().any(|row| row.day == day(1)));
        assert_eq!(repos.corrections().refunds_for(&request().order_id)?.len(), 0);
        assert_eq!(repos.money().cash_position(OUTLET, day(1))?.cash_refunds, Money::ZERO);
        let file = repos.archive().day_file(OUTLET, day(1), at(500_000_000), "test")?;
        let history = mb_db::archive::gunzip(&file.gz)?;
        assert!(history.contains("credit_adjustments"), "the return's account reduction is archived with its bill");
        let figures = repos.days().figures(OUTLET, day(1))?;
        assert_eq!(figures.credit_given.paise(), -10500);
        assert!(!figures.is_empty(), "a return-only day cannot be marked as a holiday");
        Ok(history)
    }).expect("account and drawer agree");
    let target = Scratch::new("historical-credit-restore"); let restored = target.open(); masters(&restored);
    restored.transaction(|tx| {
        Repos::new(tx).money().save_customer(OUTLET, &mb_db::repo::money::Customer {
            id: customer.clone(), name: "Customer".to_owned(), phone: None, gstin: None,
            address: None, credit_limit: None, is_active: true,
        }, at(0))?;
        let mut report = mb_db::repo::wire::RestoreReport::default();
        Repos::new(tx).archive().restore_text(OUTLET, &history, &mut report)?;
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        assert_eq!(Repos::new(tx).money().customer_balance(&customer)?, Money::ZERO);
        Ok(())
    }).expect("archived credit return restores with the original ledger reduction");
}
