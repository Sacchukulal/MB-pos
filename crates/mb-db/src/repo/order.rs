//! Orders: the aggregate the whole product turns around.
//!
//! An order is nine tables — the header, the typed lines, their modifiers, the
//! computed bill, its lines, its charges, its rate summary, the payments and
//! the kitchen ledger. [`OrderRepo::save`] writes all of them **inside the
//! caller's transaction, or none of them.**
//!
//! # The private-field wall
//!
//! Shredding an order into rows is the easy direction. Four mb-core types have
//! private fields and no `from_parts`, so [`OrderRepo::find`] rebuilds them by
//! replaying their public API:
//!
//! | type | replayed by |
//! |---|---|
//! | [`Cart`] | `add` in stored `seq` order, then `set_line_discount` by index |
//! | [`Settlement`] | `new`, `add` per payment, `set_tip` |
//! | [`KitchenLedger`] | `new`, then `mark_printed` with the stored pairs |
//! | [`TaxSummary`] | `new`, one synthesised `add` per stored rate row, then the two public totals |
//!
//! `Cart::add` **re-runs the merge rule**, which is why the rows are replayed
//! in `seq` order and why the round trip is asserted rather than assumed. A
//! cart can never hold two lines with the same identity — `add` merged them
//! when they were typed — so the replay is exact, and `t1` proves it across a
//! whole shop.
//!
//! `Counter` is deliberately not in that table. P04 decided it: the counter
//! lives on disk as columns and `numbering::claim` is one SQL statement.

use mb_core::{
    AnyOrder, Bill, BillCharge, BillLine, CancelledOrder, Cart, CategoryId, Claimed, DiscountEntry,
    DraftOrder, ItemId, ItemSnapshot, KitchenLedger, LineIdentity, Modifier, ModifierId, Money,
    OpenOrder, OrderCore, OrderId, Payment, SettledOrder, Settlement, StaffId, TableId, TaxAmounts,
    TaxOutcome, TaxSummary, TaxTreatment, VoidedOrder,
};
use rusqlite::Transaction;

use crate::encode::{self, order_state};
use crate::error::DbError;
use crate::repo::outbox::{Op, OutboxRepo};

#[derive(Debug)]
pub struct OrderRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> OrderRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        OrderRepo { tx }
    }

    /// Write an order in whatever state it is in.
    ///
    /// **One transaction or nothing.** The header, every line, every modifier,
    /// the computed bill and all four of its child tables, the payments, the
    /// kitchen ledger and the outbox row commit together. An order that exists
    /// without its lines is a bill that cannot be reprinted and a GST return
    /// that will not tie; `t2` injects a failure halfway to prove it cannot
    /// happen.
    ///
    /// Idempotent: saving the same order again replaces its rows. That is what
    /// a state transition is — the same order, later.
    pub fn save(&self, outlet: &str, terminal: &str, order: &AnyOrder) -> Result<(), DbError> {
        let core = order.core();
        let id = core.id.as_str();

        // Replace rather than accumulate. The children go first, because the
        // foreign keys point upwards and nothing may cascade (D22 / P04).
        //
        // **The header itself is UPSERTED, not deleted and re-inserted**, and
        // P12 is what found the difference: `reprints` and `refunds` point at
        // `orders(id)`, so deleting the row to re-add it broke the foreign key
        // — voiding a bill that had been reprinted failed with "FOREIGN KEY
        // constraint failed" and the shop was told its data could not be read.
        //
        // The children have no such problem: nothing references an order line.
        self.delete_children(id)?;

        let numbers = order.bill_number();
        let token = match order {
            AnyOrder::Draft(_) => None,
            AnyOrder::Open(o) => Some(&o.token),
            AnyOrder::Settled(o) => Some(&o.token),
            AnyOrder::Cancelled(o) => Some(&o.token),
            AnyOrder::Voided(o) => Some(&o.token),
        };

        let (settled, cancelled, voided) = timestamps(order);

        self.tx.execute(
            "INSERT INTO orders (id, outlet_id, terminal_id, state, business_day, created_at,
                                 created_by, order_type, table_id, sub_table, covers, note,
                                 token_value, token_formatted,
                                 bill_number_value, bill_number_formatted,
                                 settled_at, settled_by,
                                 cancelled_at, cancelled_by, cancel_reason,
                                 voided_at, voided_by, void_reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?23, ?24, ?10,
                     ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)
             ON CONFLICT (id) DO UPDATE SET
                 state                 = excluded.state,
                 table_id              = excluded.table_id,
                 sub_table             = excluded.sub_table,
                 covers                = excluded.covers,
                 note                  = excluded.note,
                 token_value           = excluded.token_value,
                 token_formatted       = excluded.token_formatted,
                 bill_number_value     = excluded.bill_number_value,
                 bill_number_formatted = excluded.bill_number_formatted,
                 settled_at            = excluded.settled_at,
                 settled_by            = excluded.settled_by,
                 cancelled_at          = excluded.cancelled_at,
                 cancelled_by          = excluded.cancelled_by,
                 cancel_reason         = excluded.cancel_reason,
                 voided_at             = excluded.voided_at,
                 voided_by             = excluded.voided_by,
                 void_reason           = excluded.void_reason",
            rusqlite::params![
                id,
                outlet,
                terminal,
                state_tag(order),
                encode::business_day_to_sql(core.business_day),
                encode::timestamp_to_sql(core.created_at),
                core.created_by.as_str(),
                encode::order_type_to_sql(core.order_type),
                core.table.as_ref().map(TableId::as_str),
                core.note,
                token.map(claimed_value).transpose()?,
                token.map(|c| c.formatted.clone()),
                numbers.map(claimed_value).transpose()?,
                numbers.map(|c| c.formatted.clone()),
                settled.as_ref().map(|(at, _)| encode::timestamp_to_sql(*at)),
                settled.as_ref().map(|(_, by)| by.as_str().to_owned()),
                cancelled.as_ref().map(|(at, _, _)| encode::timestamp_to_sql(*at)),
                cancelled.as_ref().map(|(_, by, _)| by.as_str().to_owned()),
                cancelled.as_ref().map(|(_, _, why)| why.clone()),
                voided.as_ref().map(|(at, _, _)| encode::timestamp_to_sql(*at)),
                voided.as_ref().map(|(_, by, _)| by.as_str().to_owned()),
                voided.as_ref().map(|(_, _, why)| why.clone()),
                // P14: which seat of a shared table (scope 1.6) and how many
                // are eating (1.24). Both nullable, and covers is honestly
                // unknown rather than 1 — see `OrderCore::covers`.
                core.sub_table.as_ref().map(|s| s.as_str().to_owned()),
                core.covers,
            ],
        )?;

        self.save_lines(id, &core.cart)?;
        self.save_kitchen(id, &core.kitchen, core.created_at)?;

        if let Some((bill, settlement)) = bill_and_settlement(order) {
            self.save_bill(id, bill, core)?;
            self.save_payments(id, settlement, core)?;
        }

        // The outbox entry is written HERE, in the same transaction as the row
        // it describes. A2 and A3 were one forgotten enqueue each.
        OutboxRepo::new(self.tx).enqueue(outlet, "orders", id, Op::Upsert, core.created_at)?;
        Ok(())
    }

    /// Read one order back, in whatever state it was left.
    pub fn find(&self, id: &OrderId) -> Result<Option<AnyOrder>, DbError> {
        let header = self.read_header(id.as_str())?;
        let Some(header) = header else {
            return Ok(None);
        };

        let cart = self.read_cart(id.as_str())?;
        let kitchen = self.read_kitchen(id.as_str())?;

        // Cloned rather than moved: `header` is still needed below for the
        // state-specific columns, and a partial move would make the borrow
        // checker's complaint the thing a reader notices instead of the shape
        // of the match.
        let core = OrderCore {
            id: id.clone(),
            business_day: header.business_day,
            created_at: header.created_at,
            order_type: header.order_type,
            table: header.table.clone(),
            sub_table: header
                .sub_table
                .as_deref()
                .map(mb_core::SubTable::parse)
                .transpose()
                .map_err(|e| DbError::invariant(format!("orders.sub_table: {e}")))?,
            covers: header.covers,
            cart,
            created_by: header.created_by.clone(),
            note: header.note.clone(),
            kitchen,
        };

        let order = match header.state.as_str() {
            order_state::DRAFT => AnyOrder::Draft(DraftOrder { core }),
            order_state::OPEN => AnyOrder::Open(OpenOrder {
                core,
                token: header.token()?,
                bill_number: header.bill_number()?,
            }),
            order_state::CANCELLED => AnyOrder::Cancelled(CancelledOrder {
                token: header.token()?,
                bill_number: header.bill_number()?,
                reason: header.required("cancel_reason", header.cancel_reason.clone())?,
                cancelled_at: header.required("cancelled_at", header.cancelled_at)?,
                cancelled_by: header.required("cancelled_by", header.cancelled_by.clone())?,
                core,
            }),
            order_state::SETTLED => {
                let bill = self.read_bill(id.as_str())?;
                AnyOrder::Settled(SettledOrder {
                    token: header.token()?,
                    bill_number: header.bill_number()?,
                    bill,
                    settlement: self.read_settlement(id.as_str())?,
                    settled_at: header.required("settled_at", header.settled_at)?,
                    settled_by: header.required("settled_by", header.settled_by.clone())?,
                    core,
                })
            }
            order_state::VOIDED => {
                let bill = self.read_bill(id.as_str())?;
                AnyOrder::Voided(VoidedOrder {
                    token: header.token()?,
                    bill_number: header.bill_number()?,
                    bill,
                    settlement: self.read_settlement(id.as_str())?,
                    settled_at: header.required("settled_at", header.settled_at)?,
                    settled_by: header.required("settled_by", header.settled_by.clone())?,
                    reason: header.required("void_reason", header.void_reason.clone())?,
                    voided_at: header.required("voided_at", header.voided_at)?,
                    voided_by: header.required("voided_by", header.voided_by.clone())?,
                    core,
                })
            }
            other => {
                return Err(DbError::BadValue {
                    column: "orders.state",
                    value: other.to_owned(),
                });
            }
        };
        Ok(Some(order))
    }

    /// Everything on the floor right now — what P09's table grid draws.
    pub fn list_open(&self, outlet: &str) -> Result<Vec<AnyOrder>, DbError> {
        self.list_where(
            "outlet_id = ?1 AND state IN ('draft', 'open') ORDER BY created_at",
            rusqlite::params![outlet],
        )
    }

    /// One business day, every state — what the day report and the Z-report
    /// read. Grouped by the STORED day, never by a timestamp (D5).
    pub fn list_for_day(
        &self,
        outlet: &str,
        day: mb_core::BusinessDay,
    ) -> Result<Vec<AnyOrder>, DbError> {
        self.list_where(
            "outlet_id = ?1 AND business_day = ?2 ORDER BY created_at",
            rusqlite::params![outlet, encode::business_day_to_sql(day)],
        )
    }

    /// Every order, oldest first. For backup verification and export, not for a
    /// screen — nothing unbounded is ever rendered (`PERFORMANCE.md` §5 rule 4).
    pub fn list_all(&self) -> Result<Vec<AnyOrder>, DbError> {
        self.list_where("1 = 1 ORDER BY created_at, id", rusqlite::params![])
    }

    fn list_where(
        &self,
        predicate: &str,
        params: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<AnyOrder>, DbError> {
        let ids: Vec<String> = {
            let sql = format!("SELECT id FROM orders WHERE {predicate}");
            let mut stmt = self.tx.prepare(&sql)?;
            let rows = stmt.query_map(params, |r| r.get::<_, String>(0))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            out
        };

        let mut orders = Vec::with_capacity(ids.len());
        for id in ids {
            let id = OrderId::new(id);
            if let Some(order) = self.find(&id)? {
                orders.push(order);
            }
        }
        Ok(orders)
    }

    // -----------------------------------------------------------------------
    // Writing
    // -----------------------------------------------------------------------

    fn delete_children(&self, id: &str) -> Result<(), DbError> {
        // Deepest first. Nothing cascades in the money path (P04), so this is
        // the only way children go, and it is deliberate: the delete is spelled
        // out here where somebody can read it.
        self.tx.execute(
            "DELETE FROM order_line_modifiers
              WHERE order_line_id IN (SELECT id FROM order_lines WHERE order_id = ?1)",
            [id],
        )?;
        self.tx
            .execute("DELETE FROM bill_lines WHERE order_id = ?1", [id])?;
        self.tx
            .execute("DELETE FROM bill_charges WHERE order_id = ?1", [id])?;
        self.tx
            .execute("DELETE FROM bill_tax_rows WHERE order_id = ?1", [id])?;
        self.tx
            .execute("DELETE FROM payments WHERE order_id = ?1", [id])?;
        self.tx
            .execute("DELETE FROM kitchen_ledger WHERE order_id = ?1", [id])?;
        self.tx
            .execute("DELETE FROM order_lines WHERE order_id = ?1", [id])?;
        self.tx
            .execute("DELETE FROM bills WHERE order_id = ?1", [id])?;
        Ok(())
    }

    fn save_lines(&self, order_id: &str, cart: &Cart) -> Result<(), DbError> {
        for (seq, line) in cart.lines().iter().enumerate() {
            let line_id = line_id(order_id, seq);
            let seq_sql = i64::try_from(seq).unwrap_or(i64::MAX);
            let discount = line.line_discount.as_ref();
            let (kind, value) = match discount {
                Some(entry) => {
                    let (k, v) = encode::discount_to_sql(entry.discount);
                    (Some(k), Some(v))
                }
                None => (None, None),
            };

            self.tx.execute(
                "INSERT INTO order_lines (id, order_id, seq, item_id, name, unit_price,
                                          tax_rate_bp, tax_treatment, hsn, category_id, qty, note,
                                          course, prep_minutes,
                                          discount_kind, discount_value, discount_reason,
                                          discount_by, was_discount_capped)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                         ?17, ?18, 0)",
                rusqlite::params![
                    line_id,
                    order_id,
                    seq_sql,
                    // **An empty item id is NULL, not `''`.** The column is
                    // nullable on purpose — an ad-hoc line typed at the counter
                    // belongs to no menu row — and `''` would be a foreign key
                    // to an item that does not exist, which SQLite refuses and
                    // which would make an ad-hoc line impossible to bill.
                    // Found at P25, by settling one.
                    Some(line.snapshot.item_id.as_str()).filter(|id| !id.is_empty()),
                    line.snapshot.name,
                    encode::money_to_sql(line.snapshot.unit_price),
                    encode::tax_rate_to_sql(line.snapshot.tax_rate),
                    encode::tax_treatment_to_sql(line.snapshot.tax_treatment),
                    line.snapshot.hsn,
                    line.snapshot.category_id.as_ref().map(CategoryId::as_str),
                    encode::qty_to_sql(line.qty),
                    line.note,
                    // P24. Part of the snapshot, so they are written with it.
                    line.snapshot.course,
                    line.snapshot.prep_minutes.map(i64::from),
                    kind,
                    value,
                    discount.and_then(|d| d.reason.clone()),
                    discount.and_then(|d| d.authorised_by.as_ref().map(|s| s.as_str().to_owned())),
                ],
            )?;

            for (mseq, modifier) in line.modifiers.iter().enumerate() {
                self.tx.execute(
                    "INSERT INTO order_line_modifiers (order_line_id, seq, modifier_id, name,
                                                       price_delta)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![
                        line_id,
                        i64::try_from(mseq).unwrap_or(i64::MAX),
                        modifier.modifier_id.as_str(),
                        modifier.name,
                        encode::money_to_sql(modifier.price_delta),
                    ],
                )?;
            }
        }
        Ok(())
    }

    fn save_kitchen(
        &self,
        order_id: &str,
        ledger: &KitchenLedger,
        at: mb_core::Timestamp,
    ) -> Result<(), DbError> {
        for (identity, qty) in ledger.told() {
            self.tx.execute(
                "INSERT INTO kitchen_ledger (order_id, identity_key, item_id, note, qty_told,
                                             updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    order_id,
                    encode::line_identity_key(identity),
                    identity.item_id.as_str(),
                    identity.note,
                    encode::qty_to_sql(*qty),
                    encode::timestamp_to_sql(at),
                ],
            )?;
            // The modifier ids are inside identity_key; they are re-read from
            // there, which is why the key's format is part of encode.rs's
            // contract and not an implementation detail of this file.
            for (mseq, modifier) in identity.modifier_ids.iter().enumerate() {
                let _ = (mseq, modifier);
            }
        }
        Ok(())
    }

    fn save_bill(&self, order_id: &str, bill: &Bill, core: &OrderCore) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO bills (order_id, subtotal, total_line_discount, total_bill_discount,
                                total_discount, total_charges, was_bill_discount_capped,
                                total_taxable, total_cgst, total_sgst, total_igst,
                                non_gst_value, exempt_value, round_off, grand_total,
                                place_of_supply, rounding_mode, computed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                     ?18)",
            rusqlite::params![
                order_id,
                encode::money_to_sql(bill.subtotal),
                encode::money_to_sql(bill.total_line_discount),
                encode::money_to_sql(bill.total_bill_discount),
                encode::money_to_sql(bill.total_discount),
                encode::money_to_sql(bill.total_charges),
                encode::bool_to_sql(bill.bill_discount_capped),
                encode::money_to_sql(bill.total_taxable),
                encode::money_to_sql(bill.total_tax.cgst),
                encode::money_to_sql(bill.total_tax.sgst),
                encode::money_to_sql(bill.total_tax.igst),
                encode::money_to_sql(bill.non_gst_value),
                encode::money_to_sql(bill.exempt_value),
                encode::money_to_sql(bill.round_off),
                encode::money_to_sql(bill.grand_total),
                encode::place_of_supply_to_sql(bill.place_of_supply),
                encode::rounding_mode_to_sql(bill.rounding),
                encode::timestamp_to_sql(core.created_at),
            ],
        )?;

        for (seq, line) in bill.lines.iter().enumerate() {
            self.tx.execute(
                "INSERT INTO bill_lines (order_line_id, order_id, gross, line_discount,
                                         bill_discount_share, net, taxable, cgst, sgst, igst,
                                         gross_including_tax, rate_bp, treatment)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                rusqlite::params![
                    line_id(order_id, seq),
                    order_id,
                    encode::money_to_sql(line.gross),
                    encode::money_to_sql(line.line_discount),
                    encode::money_to_sql(line.bill_discount_share),
                    encode::money_to_sql(line.net),
                    encode::money_to_sql(line.taxable),
                    encode::money_to_sql(line.tax.cgst),
                    encode::money_to_sql(line.tax.sgst),
                    encode::money_to_sql(line.tax.igst),
                    encode::money_to_sql(line.gross_including_tax),
                    encode::tax_rate_to_sql(line.rate),
                    encode::tax_treatment_to_sql(line.treatment),
                ],
            )?;
        }

        for (seq, charge) in bill.charges.iter().enumerate() {
            let (basis, basis_value) = encode::charge_basis_to_sql(charge.basis);
            self.tx.execute(
                "INSERT INTO bill_charges (id, order_id, seq, kind, name, basis, basis_value,
                                           amount, taxable, cgst, sgst, igst, gross_including_tax,
                                           rate_bp, treatment)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                rusqlite::params![
                    format!("{order_id}_chg_{seq}"),
                    order_id,
                    i64::try_from(seq).unwrap_or(i64::MAX),
                    encode::charge_kind_to_sql(&charge.kind),
                    charge.name,
                    basis,
                    basis_value,
                    encode::money_to_sql(charge.amount),
                    encode::money_to_sql(charge.taxable),
                    encode::money_to_sql(charge.tax.cgst),
                    encode::money_to_sql(charge.tax.sgst),
                    encode::money_to_sql(charge.tax.igst),
                    encode::money_to_sql(charge.gross_including_tax),
                    encode::tax_rate_to_sql(charge.rate),
                    encode::tax_treatment_to_sql(charge.treatment),
                ],
            )?;
        }

        for row in bill.summary.rows() {
            self.tx.execute(
                "INSERT INTO bill_tax_rows (order_id, rate_bp, taxable, cgst, sgst, igst)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    order_id,
                    encode::tax_rate_to_sql(row.rate),
                    encode::money_to_sql(row.taxable),
                    encode::money_to_sql(row.tax.cgst),
                    encode::money_to_sql(row.tax.sgst),
                    encode::money_to_sql(row.tax.igst),
                ],
            )?;
        }
        Ok(())
    }

    fn save_payments(
        &self,
        order_id: &str,
        settlement: &Settlement,
        core: &OrderCore,
    ) -> Result<(), DbError> {
        for (seq, payment) in settlement.payments().iter().enumerate() {
            let cols = encode::payment_mode_to_sql(&payment.mode);
            // The tip belongs to the settlement, not to one payment. It rides
            // on the first row so `Settlement::with_tip` can be replayed, and
            // the day close reads SUM(tip) either way.
            let tip = if seq == 0 { settlement.tip() } else { Money::ZERO };
            self.tx.execute(
                "INSERT INTO payments (id, order_id, seq, mode, customer_id, mode_label, amount,
                                       tip, reference, settles_credit, received_at, received_by,
                                       business_day)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                rusqlite::params![
                    format!("{order_id}_pay_{seq}"),
                    order_id,
                    i64::try_from(seq).unwrap_or(i64::MAX),
                    cols.mode,
                    cols.customer_id,
                    cols.mode_label,
                    encode::money_to_sql(payment.amount),
                    encode::money_to_sql(tip),
                    payment.reference,
                    encode::bool_to_sql(payment.settles_credit),
                    encode::timestamp_to_sql(core.created_at),
                    core.created_by.as_str(),
                    encode::business_day_to_sql(core.business_day),
                ],
            )?;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Reading — the private-field wall
    // -----------------------------------------------------------------------

    fn read_header(&self, id: &str) -> Result<Option<Header>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            // New columns go on the END of this list, never in the middle: the
            // reader below indexes by position, and inserting `sub_table` after
            // `note` silently shifted every column after it by two. Caught by
            // two P12 tests reading `cancelled_at` as text (P14).
            "SELECT state, business_day, created_at, created_by, order_type, table_id, note,
                    token_value, token_formatted, bill_number_value, bill_number_formatted,
                    settled_at, settled_by, cancelled_at, cancelled_by, cancel_reason,
                    voided_at, voided_by, void_reason,
                    sub_table, covers
               FROM orders WHERE id = ?1",
        )?;
        let mut rows = stmt.query([id])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };

        let business_day =
            encode::business_day_from_sql(row.get(1)?, "orders.business_day")?;
        Ok(Some(Header {
            state: row.get(0)?,
            business_day,
            created_at: encode::timestamp_from_sql(row.get(2)?),
            created_by: StaffId::new(row.get::<_, String>(3)?),
            order_type: encode::order_type_from_sql(&row.get::<_, String>(4)?)?,
            table: row.get::<_, Option<String>>(5)?.map(TableId::new),
            note: row.get(6)?,
            token_value: row.get(7)?,
            token_formatted: row.get(8)?,
            bill_value: row.get(9)?,
            bill_formatted: row.get(10)?,
            settled_at: row.get::<_, Option<i64>>(11)?.map(encode::timestamp_from_sql),
            settled_by: row.get::<_, Option<String>>(12)?.map(StaffId::new),
            cancelled_at: row.get::<_, Option<i64>>(13)?.map(encode::timestamp_from_sql),
            cancelled_by: row.get::<_, Option<String>>(14)?.map(StaffId::new),
            cancel_reason: row.get(15)?,
            voided_at: row.get::<_, Option<i64>>(16)?.map(encode::timestamp_from_sql),
            voided_by: row.get::<_, Option<String>>(17)?.map(StaffId::new),
            void_reason: row.get(18)?,
            sub_table: row.get::<_, Option<String>>(19)?,
            covers: row.get(20)?,
        }))
    }

    /// Replays [`Cart::add`] in stored `seq` order.
    ///
    /// `add` re-runs the merge rule, which is exactly why the order matters. A
    /// cart cannot hold two lines with the same identity — `add` merged them
    /// when they were typed — so replaying in sequence reproduces it exactly,
    /// and `t1` asserts that rather than trusting it.
    fn read_cart(&self, order_id: &str) -> Result<Cart, DbError> {
        let lines = self.read_line_rows(order_id)?;
        let mut cart = Cart::new();

        for line in &lines {
            let modifiers = self.read_modifiers(&line.id)?;
            let mut snapshot = ItemSnapshot::new(
                ItemId::new(line.item_id.clone()),
                line.name.clone(),
                encode::money_from_sql(line.unit_price),
                encode::tax_rate_from_sql(line.tax_rate_bp, "order_lines.tax_rate_bp")?,
            )
            .with_treatment(encode::tax_treatment_from_sql(&line.tax_treatment)?);
            if let Some(hsn) = &line.hsn {
                snapshot = snapshot.with_hsn(hsn.clone());
            }
            if let Some(category) = &line.category_id {
                snapshot = snapshot.with_category(CategoryId::new(category.clone()));
            }
            // P24. The snapshot is not the snapshot if these come back empty:
            // an order read from disk is what `kitchen::send` fires from, so
            // losing them here would silently switch off every course and
            // every timer after a restart.
            snapshot.course = line.course.clone();
            snapshot.prep_minutes = line.prep_minutes.and_then(|m| u32::try_from(m).ok());

            let index = cart
                .add(
                    snapshot,
                    encode::qty_from_sql(line.qty),
                    line.note.clone(),
                    modifiers,
                )
                .map_err(|e| {
                    DbError::invariant(format!(
                        "order {order_id} line {} will not go back into a cart: {e}",
                        line.seq
                    ))
                })?;

            if let Some(kind) = &line.discount_kind {
                let value = line.discount_value.ok_or_else(|| {
                    DbError::invariant(format!(
                        "order {order_id} line {} has a discount kind and no value",
                        line.seq
                    ))
                })?;
                let discount =
                    encode::discount_from_sql(kind, value, "order_lines.discount_kind")?;
                let mut entry = DiscountEntry::new(discount);
                if let Some(reason) = &line.discount_reason {
                    entry = entry.with_reason(reason.clone());
                }
                if let Some(by) = &line.discount_by {
                    entry = entry.authorised_by(StaffId::new(by.clone()));
                }
                cart.set_line_discount(index, Some(entry)).map_err(|e| {
                    DbError::invariant(format!("order {order_id}: {e}"))
                })?;
            }
        }
        Ok(cart)
    }

    fn read_line_rows(&self, order_id: &str) -> Result<Vec<LineRow>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT id, seq, item_id, name, unit_price, tax_rate_bp, tax_treatment, hsn,
                    category_id, qty, note, course, prep_minutes,
                    discount_kind, discount_value, discount_reason, discount_by
               FROM order_lines WHERE order_id = ?1 ORDER BY seq",
        )?;
        let rows = stmt.query_map([order_id], |row| {
            Ok(LineRow {
                id: row.get(0)?,
                seq: row.get(1)?,
                item_id: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                name: row.get(3)?,
                unit_price: row.get(4)?,
                tax_rate_bp: row.get(5)?,
                tax_treatment: row.get(6)?,
                hsn: row.get(7)?,
                category_id: row.get(8)?,
                qty: row.get(9)?,
                note: row.get(10)?,
                course: row.get(11)?,
                prep_minutes: row.get(12)?,
                discount_kind: row.get(13)?,
                discount_value: row.get(14)?,
                discount_reason: row.get(15)?,
                discount_by: row.get(16)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn read_modifiers(&self, line_id: &str) -> Result<Vec<Modifier>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT modifier_id, name, price_delta FROM order_line_modifiers
              WHERE order_line_id = ?1 ORDER BY seq",
        )?;
        let rows = stmt.query_map([line_id], |row| {
            Ok(Modifier::new(
                ModifierId::new(row.get::<_, Option<String>>(0)?.unwrap_or_default()),
                row.get::<_, String>(1)?,
                encode::money_from_sql(row.get::<_, i64>(2)?),
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Replays [`KitchenLedger::mark_printed`].
    ///
    /// The identity's sorted modifier ids come back out of `identity_key`, and
    /// the sort is mb-core's rule — this only decodes it. Crown jewel 2 said
    /// the delta is remembered *in the database*; this is where that is true.
    fn read_kitchen(&self, order_id: &str) -> Result<KitchenLedger, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT identity_key, qty_told FROM kitchen_ledger WHERE order_id = ?1
              ORDER BY identity_key",
        )?;
        let rows = stmt.query_map([order_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;

        let mut told = Vec::new();
        for row in rows {
            let (key, qty) = row?;
            told.push((decode_identity(&key), encode::qty_from_sql(qty)));
        }

        let mut ledger = KitchenLedger::new();
        ledger
            .mark_printed(&told)
            .map_err(|e| DbError::invariant(format!("order {order_id} kitchen ledger: {e}")))?;
        Ok(ledger)
    }

    /// Replays [`Settlement`]: `new`, `add` per payment, then the tip.
    fn read_settlement(&self, order_id: &str) -> Result<Settlement, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT mode, customer_id, mode_label, amount, tip, reference, settles_credit
               FROM payments WHERE order_id = ?1 ORDER BY seq",
        )?;
        let rows = stmt.query_map([order_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })?;

        let mut settlement = Settlement::new();
        let mut tip = Money::ZERO;
        for row in rows {
            let (mode, customer, label, amount, row_tip, reference, credit) = row?;
            let mode = encode::payment_mode_from_sql(&mode, customer.as_deref(), label.as_deref())?;
            let mut payment = Payment::new(mode, encode::money_from_sql(amount))
                .map_err(|e| DbError::invariant(format!("order {order_id} payment: {e}")))?;
            if let Some(reference) = reference {
                payment = payment.with_reference(reference);
            }
            if encode::bool_from_sql(credit, "payments.settles_credit")? {
                payment = payment.settling_credit();
            }
            settlement
                .add(payment)
                .map_err(|e| DbError::invariant(format!("order {order_id} settlement: {e}")))?;
            tip = tip
                .add(encode::money_from_sql(row_tip))
                .map_err(|e| DbError::invariant(format!("order {order_id} tip: {e}")))?;
        }
        settlement
            .set_tip(tip)
            .map_err(|e| DbError::invariant(format!("order {order_id} tip: {e}")))?;
        Ok(settlement)
    }

    fn read_bill(&self, order_id: &str) -> Result<Bill, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT subtotal, total_line_discount, total_bill_discount, total_discount,
                    total_charges, was_bill_discount_capped, total_taxable, total_cgst,
                    total_sgst, total_igst, non_gst_value, exempt_value, round_off, grand_total,
                    place_of_supply, rounding_mode
               FROM bills WHERE order_id = ?1",
        )?;
        let mut rows = stmt.query([order_id])?;
        let row = rows.next()?.ok_or_else(|| {
            DbError::invariant(format!(
                "order {order_id} is settled but has no bill — the save was not one transaction"
            ))
        })?;

        let order_type = self.tx.query_row(
            "SELECT order_type FROM orders WHERE id = ?1",
            [order_id],
            |r| r.get::<_, String>(0),
        )?;

        let bill = Bill {
            lines: self.read_bill_lines(order_id)?,
            charges: self.read_bill_charges(order_id)?,
            summary: self.read_summary(
                order_id,
                encode::money_from_sql(row.get(10)?),
                encode::money_from_sql(row.get(11)?),
            )?,
            subtotal: encode::money_from_sql(row.get(0)?),
            total_line_discount: encode::money_from_sql(row.get(1)?),
            total_bill_discount: encode::money_from_sql(row.get(2)?),
            total_discount: encode::money_from_sql(row.get(3)?),
            total_charges: encode::money_from_sql(row.get(4)?),
            bill_discount_capped: encode::bool_from_sql(
                row.get(5)?,
                "bills.was_bill_discount_capped",
            )?,
            total_taxable: encode::money_from_sql(row.get(6)?),
            total_tax: TaxAmounts {
                cgst: encode::money_from_sql(row.get(7)?),
                sgst: encode::money_from_sql(row.get(8)?),
                igst: encode::money_from_sql(row.get(9)?),
            },
            non_gst_value: encode::money_from_sql(row.get(10)?),
            exempt_value: encode::money_from_sql(row.get(11)?),
            round_off: encode::money_from_sql(row.get(12)?),
            grand_total: encode::money_from_sql(row.get(13)?),
            order_type: encode::order_type_from_sql(&order_type)?,
            place_of_supply: encode::place_of_supply_from_sql(&row.get::<_, String>(14)?)?,
            rounding: encode::rounding_mode_from_sql(&row.get::<_, String>(15)?)?,
        };
        Ok(bill)
    }

    fn read_bill_lines(&self, order_id: &str) -> Result<Vec<BillLine>, DbError> {
        // The snapshot, the quantity, the note and the modifiers live on the
        // TYPED line; only the computed figures live on the bill line. Joining
        // them here is what keeps the line stored once (P04 item 4).
        let cart = self.read_cart(order_id)?;
        let mut stmt = self.tx.prepare_cached(
            "SELECT bl.gross, bl.line_discount, bl.bill_discount_share, bl.net, bl.taxable,
                    bl.cgst, bl.sgst, bl.igst, bl.gross_including_tax, bl.rate_bp, bl.treatment
               FROM bill_lines bl
               JOIN order_lines ol ON ol.id = bl.order_line_id
              WHERE bl.order_id = ?1
              ORDER BY ol.seq",
        )?;
        let rows = stmt.query_map([order_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, i64>(9)?,
                row.get::<_, String>(10)?,
            ))
        })?;

        let mut out = Vec::new();
        for (index, row) in rows.enumerate() {
            let row = row?;
            let source = cart.lines().get(index).ok_or_else(|| {
                DbError::invariant(format!(
                    "order {order_id} has more bill lines than typed lines"
                ))
            })?;
            out.push(BillLine {
                snapshot: source.snapshot.clone(),
                qty: source.qty,
                note: source.note.clone(),
                modifiers: source.modifiers.clone(),
                gross: encode::money_from_sql(row.0),
                line_discount: encode::money_from_sql(row.1),
                bill_discount_share: encode::money_from_sql(row.2),
                net: encode::money_from_sql(row.3),
                taxable: encode::money_from_sql(row.4),
                tax: TaxAmounts {
                    cgst: encode::money_from_sql(row.5),
                    sgst: encode::money_from_sql(row.6),
                    igst: encode::money_from_sql(row.7),
                },
                gross_including_tax: encode::money_from_sql(row.8),
                rate: encode::tax_rate_from_sql(row.9, "bill_lines.rate_bp")?,
                treatment: encode::tax_treatment_from_sql(&row.10)?,
            });
        }
        Ok(out)
    }

    fn read_bill_charges(&self, order_id: &str) -> Result<Vec<BillCharge>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT kind, name, basis, basis_value, amount, taxable, cgst, sgst, igst,
                    gross_including_tax, rate_bp, treatment
               FROM bill_charges WHERE order_id = ?1 ORDER BY seq",
        )?;
        let rows = stmt.query_map([order_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, i64>(9)?,
                row.get::<_, i64>(10)?,
                row.get::<_, String>(11)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let row = row?;
            out.push(BillCharge {
                kind: encode::charge_kind_from_sql(&row.0, &row.1)?,
                name: row.1.clone(),
                basis: encode::charge_basis_from_sql(&row.2, row.3)?,
                amount: encode::money_from_sql(row.4),
                taxable: encode::money_from_sql(row.5),
                tax: TaxAmounts {
                    cgst: encode::money_from_sql(row.6),
                    sgst: encode::money_from_sql(row.7),
                    igst: encode::money_from_sql(row.8),
                },
                gross_including_tax: encode::money_from_sql(row.9),
                rate: encode::tax_rate_from_sql(row.10, "bill_charges.rate_bp")?,
                treatment: encode::tax_treatment_from_sql(&row.11)?,
            });
        }
        Ok(out)
    }

    /// Replays [`TaxSummary`].
    ///
    /// `TaxSummary::add` routes `NonGst` and `Exempt` into their own public
    /// totals and everything else into a rate row, so the rate rows are
    /// replayed as `Exclusive` outcomes — the treatment only decides *which
    /// bucket*, and these are already in the taxed bucket — and the two totals
    /// are set directly from the bill header. Rows come back in ascending rate
    /// order, which is the order `add` would have built them in.
    fn read_summary(
        &self,
        order_id: &str,
        non_gst: Money,
        exempt: Money,
    ) -> Result<TaxSummary, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT rate_bp, taxable, cgst, sgst, igst FROM bill_tax_rows
              WHERE order_id = ?1 ORDER BY rate_bp",
        )?;
        let rows = stmt.query_map([order_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;

        let mut summary = TaxSummary::new();
        for row in rows {
            let (rate_bp, taxable, cgst, sgst, igst) = row?;
            let taxable = encode::money_from_sql(taxable);
            let tax = TaxAmounts {
                cgst: encode::money_from_sql(cgst),
                sgst: encode::money_from_sql(sgst),
                igst: encode::money_from_sql(igst),
            };
            let gross = taxable
                .add(tax.total().map_err(|e| {
                    DbError::invariant(format!("order {order_id} tax row: {e}"))
                })?)
                .map_err(|e| DbError::invariant(format!("order {order_id} tax row: {e}")))?;
            summary
                .add(TaxOutcome {
                    taxable,
                    tax,
                    gross,
                    rate: encode::tax_rate_from_sql(rate_bp, "bill_tax_rows.rate_bp")?,
                    treatment: TaxTreatment::Exclusive,
                })
                .map_err(|e| DbError::invariant(format!("order {order_id} tax summary: {e}")))?;
        }
        summary.non_gst_value = non_gst;
        summary.exempt_value = exempt;
        Ok(summary)
    }
}

// ---------------------------------------------------------------------------

struct Header {
    state: String,
    business_day: mb_core::BusinessDay,
    created_at: mb_core::Timestamp,
    created_by: StaffId,
    order_type: mb_core::OrderType,
    table: Option<TableId>,
    note: Option<String>,
    token_value: Option<i64>,
    token_formatted: Option<String>,
    bill_value: Option<i64>,
    bill_formatted: Option<String>,
    settled_at: Option<mb_core::Timestamp>,
    settled_by: Option<StaffId>,
    cancelled_at: Option<mb_core::Timestamp>,
    cancelled_by: Option<StaffId>,
    cancel_reason: Option<String>,
    voided_at: Option<mb_core::Timestamp>,
    voided_by: Option<StaffId>,
    void_reason: Option<String>,
    /// P14, scope 1.6. Stored as the letter; parsed on the way out so a hand-
    /// edited database cannot put "front-left" on a kitchen ticket.
    sub_table: Option<String>,
    /// P14, scope 1.24.
    covers: Option<u32>,
}

impl Header {
    fn token(&self) -> Result<Claimed, DbError> {
        claimed(
            self.token_value,
            self.token_formatted.clone(),
            self.business_day,
            "orders.token_value",
        )
    }

    fn bill_number(&self) -> Result<Claimed, DbError> {
        claimed(
            self.bill_value,
            self.bill_formatted.clone(),
            self.business_day,
            "orders.bill_number_value",
        )
    }

    /// A column the state's CHECK constraint guarantees is present. Reaching
    /// the error means the row was written by something other than this
    /// program — so it is an error, never a default (D7).
    fn required<T>(&self, column: &'static str, value: Option<T>) -> Result<T, DbError> {
        value.ok_or_else(|| {
            DbError::invariant(format!(
                "a {} order has no {column} — the row is not one this program wrote",
                self.state
            ))
        })
    }
}

struct LineRow {
    id: String,
    seq: i64,
    item_id: String,
    name: String,
    unit_price: i64,
    tax_rate_bp: i64,
    tax_treatment: String,
    hsn: Option<String>,
    category_id: Option<String>,
    qty: i64,
    note: Option<String>,
    course: Option<String>,
    prep_minutes: Option<i64>,
    discount_kind: Option<String>,
    discount_value: Option<i64>,
    discount_reason: Option<String>,
    discount_by: Option<String>,
}

/// **The id of one order line, derived rather than generated.**
///
/// P25 reads it from outside this module: a stock movement points at the line it
/// came from, so a per-line void reverses only its own share. Derived means the
/// stock walk can name a line before the row is read back.
pub(crate) fn line_id(order_id: &str, seq: usize) -> String {
    format!("{order_id}_ln_{seq}")
}

fn state_tag(order: &AnyOrder) -> &'static str {
    match order {
        AnyOrder::Draft(_) => order_state::DRAFT,
        AnyOrder::Open(_) => order_state::OPEN,
        AnyOrder::Settled(_) => order_state::SETTLED,
        AnyOrder::Cancelled(_) => order_state::CANCELLED,
        AnyOrder::Voided(_) => order_state::VOIDED,
    }
}

type Settled = (mb_core::Timestamp, StaffId);
type Reasoned = (mb_core::Timestamp, StaffId, String);

fn timestamps(order: &AnyOrder) -> (Option<Settled>, Option<Reasoned>, Option<Reasoned>) {
    match order {
        AnyOrder::Draft(_) | AnyOrder::Open(_) => (None, None, None),
        AnyOrder::Settled(o) => (Some((o.settled_at, o.settled_by.clone())), None, None),
        AnyOrder::Cancelled(o) => (
            None,
            Some((o.cancelled_at, o.cancelled_by.clone(), o.reason.clone())),
            None,
        ),
        AnyOrder::Voided(o) => (
            Some((o.settled_at, o.settled_by.clone())),
            None,
            Some((o.voided_at, o.voided_by.clone(), o.reason.clone())),
        ),
    }
}

/// A voided order keeps its bill and its amounts — P03 decided that
/// deliberately, so reports can show gross takings and takings net of voids
/// side by side.
fn bill_and_settlement(order: &AnyOrder) -> Option<(&Bill, &Settlement)> {
    match order {
        AnyOrder::Settled(o) => Some((&o.bill, &o.settlement)),
        AnyOrder::Voided(o) => Some((&o.bill, &o.settlement)),
        _ => None,
    }
}

fn claimed_value(c: &Claimed) -> Result<i64, DbError> {
    i64::try_from(c.value).map_err(|_| DbError::OutOfRange {
        column: "orders.bill_number_value",
        expected: "a claimed number",
    })
}

fn claimed(
    value: Option<i64>,
    formatted: Option<String>,
    day: mb_core::BusinessDay,
    column: &'static str,
) -> Result<Claimed, DbError> {
    let value = value.ok_or(DbError::OutOfRange {
        column,
        expected: "a claimed number on a non-draft order",
    })?;
    let value = u64::try_from(value).map_err(|_| DbError::OutOfRange {
        column,
        expected: "a claimed number",
    })?;
    Ok(Claimed {
        value,
        formatted: formatted.unwrap_or_default(),
        business_day: day,
    })
}

/// The inverse of [`crate::encode::line_identity_key`].
///
/// Item, then note, then the sorted modifier ids, separated by the unit
/// separator. An empty note field means `None` — a line whose note is the empty
/// string is a line with no note as far as the cart's merge rule is concerned,
/// because `Cart::add` normalises it.
fn decode_identity(key: &str) -> LineIdentity {
    const US: char = '\u{1f}';
    let mut parts = key.split(US);
    let item_id = ItemId::new(parts.next().unwrap_or_default());
    let note = parts.next().filter(|s| !s.is_empty()).map(str::to_owned);
    let modifier_ids = parts.map(ModifierId::new).collect();
    LineIdentity {
        item_id,
        note,
        modifier_ids,
    }
}
