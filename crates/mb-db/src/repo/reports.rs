//! Every report, and every one of them groups by the STORED business day.

use mb_core::{BusinessDay, Money, Qty, Timestamp};
use rusqlite::Transaction;

use crate::encode;
use crate::error::DbError;

/// A stretch of business days, inclusive at both ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Period {
    pub from: BusinessDay,
    pub to: BusinessDay,
}

impl Period {
    #[must_use]
    pub const fn new(from: BusinessDay, to: BusinessDay) -> Period {
        Period { from, to }
    }

    #[must_use]
    pub const fn one_day(day: BusinessDay) -> Period {
        Period { from: day, to: day }
    }

    /// How many days this is, counting both ends.
    #[must_use]
    pub const fn days(self) -> i32 {
        self.from.days_until(self.to) + 1
    }

    /// The same number of days, ending the day before this one starts.
    #[must_use]
    pub fn previous(self) -> Period {
        let length = self.days();
        let to = self.from.previous();
        Period {
            from: BusinessDay::from_days_since_epoch(to.days_since_epoch() - (length - 1)),
            to,
        }
    }
}

/// What a sales report can be grouped by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SalesBy {
    Day,
    /// The peak hour, which the phone app had and the till did not.
    Hour,
    OrderType,
    PaymentMode,
    Cashier,
    Section,
    /// Which till took the money.
    Terminal,
    Item,
    Category,
}

/// One row of a grouped sales report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bucket {
    /// The stable key: a staff id, a rate in basis points, an hour number.
    pub key: String,
    /// What a person reads.
    pub label: String,
    pub bills: i64,
    /// The bill's own grand total.
    pub gross: Money,
    pub discount: Money,
    pub tax: Money,
    /// Only for item and category reports; `None` elsewhere, because a quantity of "dine-in" is
    /// not a thing.
    pub qty: Option<Qty>,
}

/// One rate in the rate-wise tax report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaxBucket {
    pub rate_bp: i64,
    /// `gst`, `exempt`, `outside_gst` or `untaxed` — a rate means a different thing in each, so
    /// the bucket carries the kind rather than the rate alone.
    pub tax_kind: String,
    pub taxable: Money,
    pub cgst: Money,
    pub sgst: Money,
    pub igst: Money,
    /// State VAT. Only ever non-zero on an `outside_gst` bucket.
    pub vat: Money,
}

/// One HSN code, for the summary a GSTR-1 wants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HsnBucket {
    pub hsn: String,
    pub qty: Qty,
    pub taxable: Money,
    pub cgst: Money,
    pub sgst: Money,
    pub igst: Money,
}

/// One person's tips over a period.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TipRow {
    pub who: String,
    /// Already in the drawer.
    pub cash: Money,
    /// Card, UPI, or on an account.
    pub other: Money,
    pub total: Money,
    pub bills: i64,
}

/// One line of the control report: something a person did that an owner may want to ask about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlRow {
    pub business_day: BusinessDay,
    pub at: i64,
    /// `void`, `cancel`, `discount`, `reprint`, `refund`.
    pub kind: String,
    pub reference: String,
    pub who: String,
    pub reason: String,
    pub amount: Money,
}

/// One item, with what it earned and what it cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemMargin {
    pub item_id: String,
    pub name: String,
    pub qty: Qty,
    pub revenue: Money,
    /// `None` means the cost price is not known, and the report says so rather than treating
    /// the item as pure margin.
    pub cost: Option<Money>,
}

#[derive(Debug)]
pub struct ReportsRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> ReportsRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        ReportsRepo { tx }
    }

    /// What counts as a sale, written once.
    const SOLD: &'static str = "o.state = 'settled'";

    /// Every grouped sales report.
    pub fn sales_by(
        &self,
        outlet: &str,
        period: Period,
        by: SalesBy,
    ) -> Result<Vec<Bucket>, DbError> {
        let (from, to) = (
            encode::business_day_to_sql(period.from),
            encode::business_day_to_sql(period.to),
        );
        let sold = Self::SOLD;

        // Item and category read `order_lines`, so they are a different shape: a bill's grand
        // total cannot be attributed to one line, and pretending otherwise is how an item
        // report stops adding up.
        let sql = match by {
            SalesBy::Item | SalesBy::Category => {
                let (key, label) = match by {
                    SalesBy::Item => ("COALESCE(l.item_id, l.name)", "l.name"),
                    _ => (
                        "COALESCE(l.category_id, '')",
                        "COALESCE(c.name, 'No category')",
                    ),
                };
                format!(
                    "SELECT {key} AS k, {label} AS lbl,
                            COUNT(DISTINCT o.id),
                            COALESCE(SUM(bl.gross_including_tax), 0),
                            COALESCE(SUM(bl.line_discount + bl.bill_discount_share), 0),
                            COALESCE(SUM(bl.cgst + bl.sgst + bl.igst), 0),
                            COALESCE(SUM(l.qty), 0)
                       FROM orders o
                       JOIN order_lines l ON l.order_id = o.id
                       JOIN bill_lines bl ON bl.order_line_id = l.id
                  LEFT JOIN categories c  ON c.id = l.category_id
                      WHERE o.outlet_id = ?1 AND o.business_day BETWEEN ?2 AND ?3
                        AND {sold}
                   GROUP BY k
                   ORDER BY SUM(bl.gross_including_tax) DESC"
                )
            }
            _ => {
                let (key, label, extra_join) = match by {
                    SalesBy::Day => ("o.business_day", "o.business_day", ""),
                    // Local time: `settled_at` is UTC milliseconds and the shop's clock is
                    // +05:30, so the offset is added before the hour is taken.
                    SalesBy::Hour => (
                        "CAST(((o.settled_at + 19800000) / 3600000) % 24 AS INTEGER)",
                        "CAST(((o.settled_at + 19800000) / 3600000) % 24 AS INTEGER)",
                        "",
                    ),
                    SalesBy::OrderType => ("o.order_type", "o.order_type", ""),
                    SalesBy::Cashier => (
                        "o.settled_by",
                        "COALESCE(s.name, 'Unknown')",
                        "LEFT JOIN staff s ON s.id = o.settled_by",
                    ),
                    SalesBy::Terminal => (
                        "o.terminal_id",
                        // The till's NAME, resolved here — the same rule the cashier report
                        // follows, and for the same reason: a report that hands back ids is a
                        // screen writing SQL by post.
                        "COALESCE(tm.name, o.terminal_id)",
                        "LEFT JOIN terminals tm ON tm.id = o.terminal_id",
                    ),
                    SalesBy::Section => (
                        "COALESCE(t.section_id, '')",
                        "COALESCE(sec.name, 'No table')",
                        "LEFT JOIN dining_tables t ON t.id = o.table_id \
                         LEFT JOIN sections sec ON sec.id = t.section_id",
                    ),
                    // A payment mode is a property of the PAYMENT, not of the bill: one bill
                    // can be cash and UPI at once.
                    _ => ("p.mode", "p.mode", "JOIN payments p ON p.order_id = o.id"),
                };
                let amount = if by == SalesBy::PaymentMode {
                    "COALESCE(SUM(p.amount), 0)"
                } else {
                    "COALESCE(SUM(b.grand_total), 0)"
                };
                format!(
                    "SELECT {key} AS k, {label} AS lbl,
                            COUNT(DISTINCT o.id),
                            {amount},
                            COALESCE(SUM(DISTINCT b.total_discount), 0),
                            COALESCE(SUM(DISTINCT b.total_cgst + b.total_sgst + b.total_igst), 0),
                            0
                       FROM orders o
                       JOIN bills b ON b.order_id = o.id
                       {extra_join}
                      WHERE o.outlet_id = ?1 AND o.business_day BETWEEN ?2 AND ?3
                        AND {sold}
                   GROUP BY k
                   ORDER BY k"
                )
            }
        };

        let mut stmt = self.tx.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params![outlet, from, to], |row| {
            Ok((
                row.get::<_, rusqlite::types::Value>(0)?,
                row.get::<_, rusqlite::types::Value>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
            ))
        })?;

        let wants_qty = matches!(by, SalesBy::Item | SalesBy::Category);
        let mut out = Vec::new();
        for row in rows {
            let (key, label, bills, gross, discount, tax, qty) = row?;
            out.push(Bucket {
                key: text_of(&key),
                label: label_for(by, &label),
                bills,
                gross: encode::money_from_sql(gross),
                discount: encode::money_from_sql(discount),
                tax: encode::money_from_sql(tax),
                qty: wants_qty.then(|| Qty::from_thousandths(qty.max(0))),
            });
        }
        // A shop brought down from the cloud has only its last 30 days as bills; the older days
        // come from the totals the cloud kept. A day with bills of its own is never doubled.
        if by == SalesBy::Day {
            let have: std::collections::BTreeSet<String> = out.iter().map(|b| b.key.clone()).collect();
            let older = crate::repo::wire::WireRepo::new(self.tx).cloud_days(outlet, period)?;
            out.extend(older.into_iter().filter(|b| !have.contains(&b.key)));
            out.sort_by_key(|b| b.key.parse::<i64>().unwrap_or(0));
        }
        Ok(out)
    }

    /// Rate-wise taxable value and tax, from the per-line figures `compute_bill` produced —
    /// never recomputed here.
    pub fn tax_by_rate(&self, outlet: &str, period: Period) -> Result<Vec<TaxBucket>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT bl.rate_bp, bl.tax_kind,
                    COALESCE(SUM(bl.taxable), 0),
                    COALESCE(SUM(bl.cgst), 0),
                    COALESCE(SUM(bl.sgst), 0),
                    COALESCE(SUM(bl.igst), 0),
                    COALESCE(SUM(bl.vat), 0)
               FROM orders o
               JOIN bill_lines bl ON bl.order_id = o.id
              WHERE o.outlet_id = ?1 AND o.business_day BETWEEN ?2 AND ?3
                AND o.state = 'settled'
           GROUP BY bl.rate_bp, bl.tax_kind
           ORDER BY bl.rate_bp",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![
                outlet,
                encode::business_day_to_sql(period.from),
                encode::business_day_to_sql(period.to),
            ],
            |row| {
                Ok(TaxBucket {
                    rate_bp: row.get(0)?,
                    tax_kind: row.get(1)?,
                    taxable: encode::money_from_sql(row.get(2)?),
                    cgst: encode::money_from_sql(row.get(3)?),
                    sgst: encode::money_from_sql(row.get(4)?),
                    igst: encode::money_from_sql(row.get(5)?),
                    vat: encode::money_from_sql(row.get(6)?),
                })
            },
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// The HSN summary a GSTR-1 asks for.
    pub fn tax_by_hsn(&self, outlet: &str, period: Period) -> Result<Vec<HsnBucket>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT COALESCE(l.hsn, ''),
                    COALESCE(SUM(l.qty), 0),
                    COALESCE(SUM(bl.taxable), 0),
                    COALESCE(SUM(bl.cgst), 0),
                    COALESCE(SUM(bl.sgst), 0),
                    COALESCE(SUM(bl.igst), 0)
               FROM orders o
               JOIN order_lines l ON l.order_id = o.id
               JOIN bill_lines bl ON bl.order_line_id = l.id
              WHERE o.outlet_id = ?1 AND o.business_day BETWEEN ?2 AND ?3
                AND o.state = 'settled'
           GROUP BY COALESCE(l.hsn, '')
           ORDER BY 1",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![
                outlet,
                encode::business_day_to_sql(period.from),
                encode::business_day_to_sql(period.to),
            ],
            |row| {
                Ok(HsnBucket {
                    hsn: row.get(0)?,
                    qty: Qty::from_thousandths(row.get::<_, i64>(1)?.max(0)),
                    taxable: encode::money_from_sql(row.get(2)?),
                    cgst: encode::money_from_sql(row.get(3)?),
                    sgst: encode::money_from_sql(row.get(4)?),
                    igst: encode::money_from_sql(row.get(5)?),
                })
            },
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// The report an owner uses to spot a problem at the counter (10.5).
    /// Tips, by whoever settled the bill.
    /// The arithmetic was already right: a tip changes what is DUE and never what the bill IS,
    /// so it is outside the taxable value and outside every sales figure in this product.
    pub fn tips_by_staff(&self, outlet: &str, period: Period) -> Result<Vec<TipRow>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT COALESCE(s.name, 'Not recorded'),
                    COALESCE(SUM(CASE WHEN p.mode = 'cash' THEN p.tip ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN p.mode <> 'cash' THEN p.tip ELSE 0 END), 0),
                    COALESCE(SUM(p.tip), 0),
                    COUNT(DISTINCT p.order_id)
               FROM payments p
               JOIN orders o ON o.id = p.order_id
          LEFT JOIN staff s ON s.id = o.settled_by
              WHERE o.outlet_id = ?1
                AND p.business_day BETWEEN ?2 AND ?3
                AND o.state = 'settled'
                AND p.tip <> 0
           GROUP BY s.id
           ORDER BY SUM(p.tip) DESC",
        )?;
        let mut rows = stmt.query(rusqlite::params![
            outlet,
            encode::business_day_to_sql(period.from),
            encode::business_day_to_sql(period.to)
        ])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(TipRow {
                who: row.get(0)?,
                cash: encode::money_from_sql(row.get(1)?),
                other: encode::money_from_sql(row.get(2)?),
                total: encode::money_from_sql(row.get(3)?),
                bills: row.get(4)?,
            });
        }
        Ok(out)
    }

    pub fn control_log(&self, outlet: &str, period: Period) -> Result<Vec<ControlRow>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT o.business_day, o.voided_at, 'void',
                    COALESCE(o.bill_number_formatted, o.id),
                    COALESCE(s.name, ''), COALESCE(o.void_reason, ''),
                    COALESCE(b.grand_total, 0)
               FROM orders o
          LEFT JOIN bills b ON b.order_id = o.id
          LEFT JOIN staff s ON s.id = o.voided_by
              WHERE o.outlet_id = ?1 AND o.business_day BETWEEN ?2 AND ?3
                AND o.state = 'voided'
             UNION ALL
             SELECT o.business_day, o.cancelled_at, 'cancel',
                    COALESCE(o.token_formatted, o.id),
                    COALESCE(s.name, ''), COALESCE(o.cancel_reason, ''), 0
               FROM orders o
          LEFT JOIN staff s ON s.id = o.cancelled_by
              WHERE o.outlet_id = ?1 AND o.business_day BETWEEN ?2 AND ?3
                AND o.state = 'cancelled' AND o.merged_into IS NULL
             UNION ALL
             SELECT r.business_day, r.refunded_at, 'refund',
                    COALESCE(o.bill_number_formatted, r.order_id),
                    COALESCE(s.name, ''), COALESCE(r.reason, ''), r.amount
               FROM refunds r
               JOIN orders o ON o.id = r.order_id
          LEFT JOIN staff s ON s.id = r.refunded_by
              WHERE r.outlet_id = ?1 AND r.business_day BETWEEN ?2 AND ?3
             UNION ALL
             SELECT p.business_day, p.printed_at, 'reprint',
                    COALESCE(o.bill_number_formatted, p.order_id),
                    COALESCE(s.name, ''), COALESCE(p.reason, ''), 0
               FROM reprints p
               JOIN orders o ON o.id = p.order_id
          LEFT JOIN staff s ON s.id = p.printed_by
              WHERE p.business_day BETWEEN ?2 AND ?3
             UNION ALL
             SELECT o.business_day, o.settled_at, 'discount',
                    COALESCE(o.bill_number_formatted, o.id),
                    COALESCE(s.name, ''), COALESCE(b.bill_discount_reason, ''),
                    b.total_discount
               FROM orders o
               JOIN bills b ON b.order_id = o.id
          LEFT JOIN staff s ON s.id = b.bill_discount_by
              WHERE o.outlet_id = ?1 AND o.business_day BETWEEN ?2 AND ?3
                AND o.state = 'settled' AND b.total_discount > 0
           ORDER BY 2 DESC",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![
                outlet,
                encode::business_day_to_sql(period.from),
                encode::business_day_to_sql(period.to),
            ],
            |row| {
                Ok(ControlRow {
                    business_day: encode::business_day_from_sql(row.get(0)?, "orders.business_day")
                        .unwrap_or_else(|_| BusinessDay::from_days_since_epoch(0)),
                    at: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                    kind: row.get(2)?,
                    reference: row.get(3)?,
                    who: row.get(4)?,
                    reason: row.get(5)?,
                    amount: encode::money_from_sql(row.get(6)?),
                })
            },
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn menu_engineering(
        &self,
        outlet: &str,
        period: Period,
    ) -> Result<Vec<ItemMargin>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT COALESCE(l.item_id, l.name), l.name,
                    COALESCE(SUM(l.qty), 0),
                    COALESCE(SUM(bl.gross_including_tax), 0),
                    -- MAX rather than an aggregate over rows: the cost is a
                    -- property of the ITEM, and it is NULL for an item nobody
                    -- has costed. `SUM` over a NULL would read as zero cost,
                    -- which is the lie this column exists to avoid.
                    MAX(i.cost_price),
                    COUNT(i.cost_price)
               FROM orders o
               JOIN order_lines l ON l.order_id = o.id
               JOIN bill_lines bl ON bl.order_line_id = l.id
          LEFT JOIN items i ON i.id = l.item_id
              WHERE o.outlet_id = ?1 AND o.business_day BETWEEN ?2 AND ?3
                AND o.state = 'settled'
           GROUP BY COALESCE(l.item_id, l.name)
           ORDER BY SUM(bl.gross_including_tax) DESC",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![
                outlet,
                encode::business_day_to_sql(period.from),
                encode::business_day_to_sql(period.to),
            ],
            |row| {
                let qty: i64 = row.get(2)?;
                let cost: Option<i64> = row.get(4)?;
                let costed: i64 = row.get(5)?;
                Ok(ItemMargin {
                    item_id: row.get(0)?,
                    name: row.get(1)?,
                    qty: Qty::from_thousandths(qty.max(0)),
                    revenue: encode::money_from_sql(row.get(3)?),
                    // The count guards the MAX: an item with no costed row at all must be
                    // `None`, not `Some(0)`.
                    cost: if costed > 0 {
                        cost.map(encode::money_from_sql)
                    } else {
                        None
                    },
                })
            },
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// "top items that stopped selling".
    pub fn stopped_selling(&self, outlet: &str, period: Period) -> Result<Vec<Bucket>, DbError> {
        let before = period.previous();
        let mut stmt = self.tx.prepare(
            "SELECT COALESCE(l.item_id, l.name) AS k, l.name,
                    COUNT(DISTINCT o.id),
                    COALESCE(SUM(bl.gross_including_tax), 0),
                    COALESCE(SUM(l.qty), 0)
               FROM orders o
               JOIN order_lines l ON l.order_id = o.id
               JOIN bill_lines bl ON bl.order_line_id = l.id
              WHERE o.outlet_id = ?1 AND o.business_day BETWEEN ?2 AND ?3
                AND o.state = 'settled'
                AND COALESCE(l.item_id, l.name) NOT IN (
                        SELECT COALESCE(l2.item_id, l2.name)
                          FROM orders o2 JOIN order_lines l2 ON l2.order_id = o2.id
                         WHERE o2.outlet_id = ?1
                           AND o2.business_day BETWEEN ?4 AND ?5
                           AND o2.state = 'settled')
           GROUP BY k
           ORDER BY SUM(bl.gross_including_tax) DESC",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![
                outlet,
                encode::business_day_to_sql(before.from),
                encode::business_day_to_sql(before.to),
                encode::business_day_to_sql(period.from),
                encode::business_day_to_sql(period.to),
            ],
            |row| {
                Ok(Bucket {
                    key: row.get(0)?,
                    label: row.get(1)?,
                    bills: row.get(2)?,
                    gross: encode::money_from_sql(row.get(3)?),
                    discount: Money::ZERO,
                    tax: Money::ZERO,
                    qty: Some(Qty::from_thousandths(row.get::<_, i64>(4)?.max(0))),
                })
            },
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// The profit statement.
    ///
    /// ```text
    ///   Sales (net of tax)
    /// − Cost of food actually used     <- the stock ledger
    /// = Gross margin
    /// − Running costs                  <- expenses
    /// = What is left
    /// ```
    pub fn profit(&self, outlet: &str, period: Period) -> Result<Profit, DbError> {
        let from = encode::business_day_to_sql(period.from);
        let to = encode::business_day_to_sql(period.to);

        let (gross, tax): (i64, i64) = self.tx.query_row(
            // There is no `bills.tax_total` and there must not be: the three GST columns are
            // what a rate-wise return is filed from, and a fourth column holding their sum is a
            // fourth place for them to disagree.
            "SELECT COALESCE(SUM(b.grand_total), 0),
                    COALESCE(SUM(b.total_cgst + b.total_sgst + b.total_igst), 0)
               FROM bills b JOIN orders o ON o.id = b.order_id
              WHERE o.outlet_id = ?1 AND o.business_day BETWEEN ?2 AND ?3
                AND o.state = 'settled'",
            rusqlite::params![outlet, from, to],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        // What the kitchen actually used, from the ledger and nothing else: what a sale took,
        // what went in the bin, and what a count found missing.
        let cost_of = |kinds: &str| -> Result<i64, DbError> {
            Ok(self.tx.query_row(
                &format!(
                    "SELECT COALESCE(-SUM(total_cost), 0) FROM stock_movements
                      WHERE outlet_id = ?1 AND business_day BETWEEN ?2 AND ?3
                        AND base_qty < 0 AND kind IN ({kinds})"
                ),
                rusqlite::params![outlet, from, to],
                |row| row.get(0),
            )?)
        };
        let food = cost_of("'sale', 'reversal'")?;
        let wastage = cost_of("'wastage'")?;
        let shrinkage = cost_of("'adjustment'")?;

        let expenses: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM expenses
              WHERE outlet_id = ?1 AND business_day BETWEEN ?2 AND ?3",
            rusqlite::params![outlet, from, to],
            |row| row.get(0),
        )?;

        // The double count. An expense in a category the shop also buys through Purchases is
        // money that may be in both blocks.
        let double_counted: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(e.amount), 0)
               FROM expenses e JOIN expense_categories c ON c.id = e.category_id
              WHERE e.outlet_id = ?1 AND e.business_day BETWEEN ?2 AND ?3
                AND EXISTS (SELECT 1 FROM materials m
                             WHERE m.outlet_id = ?1 AND m.category <> ''
                               AND lower(m.category) = lower(c.name))",
            rusqlite::params![outlet, from, to],
            |row| row.get(0),
        )?;

        let net_sales = gross - tax;
        let used = food + wastage + shrinkage;
        Ok(Profit {
            gross_sales: encode::money_from_sql(gross),
            tax: encode::money_from_sql(tax),
            net_sales: encode::money_from_sql(net_sales),
            food_used: encode::money_from_sql(food),
            wastage: encode::money_from_sql(wastage),
            shrinkage: encode::money_from_sql(shrinkage),
            cost_of_food: encode::money_from_sql(used),
            gross_margin: encode::money_from_sql(net_sales - used),
            running_costs: encode::money_from_sql(expenses),
            left: encode::money_from_sql(net_sales - used - expenses),
            double_counted: encode::money_from_sql(double_counted),
        })
    }
}

/// The five numbers, and the two warnings that go under them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profit {
    pub gross_sales: Money,
    pub tax: Money,
    /// What the shop kept of what it billed, before anything was spent.
    pub net_sales: Money,
    /// What the bills' recipes actually took off the shelf, at cost.
    pub food_used: Money,
    pub wastage: Money,
    /// What a stock count found missing — the honest part of the cost, and the one nobody wants
    /// to look at.
    pub shrinkage: Money,
    /// Food + wastage + shrinkage.
    pub cost_of_food: Money,
    pub gross_margin: Money,
    pub running_costs: Money,
    pub left: Money,
    /// The catch, not an assumption.
    pub double_counted: Money,
}

impl Profit {
    /// Gross margin as a percentage of net sales, in basis points.
    #[must_use]
    #[allow(
        clippy::integer_division,
        reason = "a percentage in basis points IS a division; the guard above \
                  is the only case that could lose anything"
    )]
    pub fn margin_bp(&self) -> Option<i64> {
        if self.net_sales.is_zero() {
            return None;
        }
        let scaled = i128::from(self.gross_margin.paise()) * 10_000;
        i64::try_from(scaled / i128::from(self.net_sales.paise())).ok()
    }
}

/// A SQLite value as text, whatever type it came back as.
fn text_of(value: &rusqlite::types::Value) -> String {
    match value {
        rusqlite::types::Value::Integer(n) => n.to_string(),
        rusqlite::types::Value::Real(f) => f.to_string(),
        rusqlite::types::Value::Text(t) => t.clone(),
        _ => String::new(),
    }
}

/// The words a person reads for a grouped key.
fn label_for(by: SalesBy, raw: &rusqlite::types::Value) -> String {
    let text = text_of(raw);
    match by {
        SalesBy::Day => {
            encode::business_day_from_sql(text.parse().unwrap_or(0), "orders.business_day")
                .map_or_else(|_| text.clone(), |day| day.to_string())
        }
        SalesBy::Hour => {
            let hour: i64 = text.parse().unwrap_or(0);
            let next = (hour + 1) % 24;
            format!("{hour:02}:00–{next:02}:00")
        }
        SalesBy::OrderType => match text.as_str() {
            "dine_in" => "Dine in".to_owned(),
            "parcel" => "Parcel".to_owned(),
            "self_service" => "Self service".to_owned(),
            "delivery" => "Delivery".to_owned(),
            other => other.to_owned(),
        },
        SalesBy::PaymentMode => match text.as_str() {
            "cash" => "Cash".to_owned(),
            "card" => "Card".to_owned(),
            "upi" => "UPI".to_owned(),
            "credit" => "Credit".to_owned(),
            other => other.to_owned(),
        },
        _ => text,
    }
}

/// One drawer, counted at the end of one shift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoverRow {
    pub day: BusinessDay,
    /// The till's own name, or "The shop" for the roll-up row.
    pub till: String,
    pub shift_no: i64,
    /// Who counted it. The whole point of a handover: two people and a name.
    pub closed_by: String,
    /// "11:14 pm", formatted by the caller — this crate owns no clock.
    pub closed_at: Timestamp,
    pub expected: Money,
    pub counted: Money,
    /// Counted − expected, as it was STORED at the close (never recomputed, so a void three
    /// weeks later cannot rewrite what was handed over).
    pub variance: Money,
    pub note: Option<String>,
    /// Who was clocked in on that till's day.
    pub who_was_on: Vec<String>,
}

impl<'a> ReportsRepo<'a> {
    /// Every drawer handed over in a period.
    pub fn handovers(&self, outlet: &str, period: Period) -> Result<Vec<HandoverRow>, DbError> {
        let from = encode::business_day_to_sql(period.from);
        let to = encode::business_day_to_sql(period.to);
        let mut stmt = self.tx.prepare(
            "SELECT c.business_day,
                    COALESCE(t.name, 'The shop'),
                    c.shift_no,
                    COALESCE(s.name, 'Not recorded'),
                    c.closed_at,
                    c.expected_cash,
                    c.counted_cash,
                    c.variance,
                    c.note,
                    c.terminal_id
               FROM day_closes c
          LEFT JOIN terminals t ON t.id = c.terminal_id
          LEFT JOIN staff s ON s.id = c.closed_by
              WHERE c.outlet_id = ?1 AND c.business_day BETWEEN ?2 AND ?3
           ORDER BY c.business_day DESC, c.terminal_id IS NULL, t.name, c.shift_no",
        )?;
        let mut rows = stmt.query(rusqlite::params![outlet, from, to])?;

        let mut out: Vec<HandoverRow> = Vec::new();
        while let Some(row) = rows.next()? {
            let day = encode::business_day_from_sql(row.get(0)?, "day_closes.business_day")?;
            out.push(HandoverRow {
                day,
                till: row.get(1)?,
                shift_no: row.get(2)?,
                closed_by: row.get(3)?,
                closed_at: encode::timestamp_from_sql(row.get(4)?),
                expected: encode::money_from_sql(row.get(5)?),
                counted: encode::money_from_sql(row.get(6)?),
                variance: encode::money_from_sql(row.get(7)?),
                note: row.get(8)?,
                who_was_on: Vec::new(),
            });
        }

        // Who was on, per day.
        let mut people = self.tx.prepare(
            "SELECT a.business_day, s.name
               FROM attendance a JOIN staff s ON s.id = a.staff_id
              WHERE a.outlet_id = ?1 AND a.business_day BETWEEN ?2 AND ?3
           GROUP BY a.business_day, s.name
           ORDER BY s.name",
        )?;
        let mut by_day: std::collections::BTreeMap<i32, Vec<String>> =
            std::collections::BTreeMap::new();
        let mut who = people.query(rusqlite::params![outlet, from, to])?;
        while let Some(row) = who.next()? {
            let day = encode::business_day_from_sql(row.get(0)?, "attendance.business_day")?;
            by_day
                .entry(day.days_since_epoch())
                .or_default()
                .push(row.get(1)?);
        }
        for row in &mut out {
            if let Some(names) = by_day.get(&row.day.days_since_epoch()) {
                row.who_was_on = names.clone();
            }
        }

        Ok(out)
    }
}
