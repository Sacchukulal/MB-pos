//! Rows as the cloud expects them, and back again.
//!
//! The outbox names a table and a row; this reads the row at send time and shapes it the way
//! `MB-backend/docs/SYNC_PROTOCOL.md` says. Typed tables get the cloud's column names; every
//! other table goes into the cloud's box exactly as it is stored here, column for column.
//! The same module brings a shop back down onto a new computer.

use base64::Engine as _;
use mb_core::{
    AnyOrder, BusinessDay, Cart, CartLine, Charge, DiscountEntry, KitchenLedger, Money, OrderCore,
    OrderId, Placement, SettledOrder, StaffId, TableId, Timestamp,
};
use rusqlite::Transaction;
use rusqlite::types::ValueRef;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::encode;
use crate::error::DbError;
use crate::repo::outbox::{Op, OutboxRow};
use crate::repo::reports::{Period, SalesBy};

/// The three per-day rows a settled bill changes. Queued by day, computed when sent.
pub const TOTALS_TABLES: &[&str] = &["day_totals", "day_item_totals", "day_category_totals"];

/// One row on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireRow {
    pub table: String,
    pub id: String,
    pub updated_at: Timestamp,
    pub deleted: bool,
    pub data: Value,
}

impl WireRow {
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({
            "table": self.table,
            "id": self.id,
            "updated_at": self.updated_at.millis(),
            "deleted": self.deleted,
            "data": self.data,
        })
    }
}

/// A blob crosses as `{"$b64": "…"}`, because JSON has no bytes.
const BLOB_KEY: &str = "$b64";

/// A typed row also carries the whole counter row under this key, so a restore puts back every
/// column and not only the ones the cloud has names for. `$table` inside it names the table.
pub const ROW_KEY: &str = "row";
pub const ROW_TABLE_KEY: &str = "$table";

/// Which column names a row. Everything not listed is keyed by `id`.
fn key_column(table: &str) -> &'static str {
    match table {
        "settings" => "key",
        "store_profile" => "outlet_id",
        "category_printers" => "category_id",
        _ => "id",
    }
}

fn is_a_table_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b == b'_' || b.is_ascii_digit())
}

fn ms(t: Timestamp) -> i64 {
    t.millis()
}

fn days(d: BusinessDay) -> i64 {
    encode::business_day_to_sql(d)
}

fn sql_value(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::from(i),
        ValueRef::Real(f) => Value::from(f),
        ValueRef::Text(t) => Value::from(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => {
            json!({ BLOB_KEY: base64::engine::general_purpose::STANDARD.encode(b) })
        }
    }
}

#[derive(Debug)]
pub struct WireRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> WireRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        WireRepo { tx }
    }

    // ------------------------------------------------------------------ up

    /// What to send for one outbox entry. Empty means nothing travels and the entry is done.
    pub fn read(&self, outlet: &str, row: &OutboxRow) -> Result<Vec<WireRow>, DbError> {
        if row.op == Op::Delete {
            return Ok(vec![WireRow {
                table: row.table_name.clone(),
                id: row.row_id.clone(),
                updated_at: row.created_at,
                deleted: true,
                data: row
                    .tombstone
                    .as_deref()
                    .and_then(|t| serde_json::from_str(t).ok())
                    .unwrap_or_else(|| json!({})),
            }]);
        }
        match row.table_name.as_str() {
            "orders" => Ok(self.order(outlet, &row.row_id)?.into_iter().collect()),
            "day_totals" => Ok(self.day_totals(outlet, &row.row_id)?.into_iter().collect()),
            "day_item_totals" => self.day_group_totals(outlet, &row.row_id, SalesBy::Item),
            "day_category_totals" => self.day_group_totals(outlet, &row.row_id, SalesBy::Category),
            "expenses" => self.with_row(self.one(row, "SELECT e.id, e.category_id, c.name AS category_name, e.amount AS amount_paise, e.description, e.note, e.business_day, e.paid_by AS paid_by_staff_id, e.paid_at AS created_at, e.mode, e.paid_to, e.reference, e.gst_rate_bp, e.gst_amount AS gst_paise FROM expenses e LEFT JOIN expense_categories c ON c.id = e.category_id WHERE e.id = ?1", Some(Self::expense_note))?, "expenses", &row.row_id),
            "expense_categories" => self.with_row(self.one(row, "SELECT id, name, sort_order FROM expense_categories WHERE id = ?1", None)?, "expense_categories", &row.row_id),
            "cash_movements" => self.with_row(self.one(row, "SELECT id, kind, amount AS amount_paise, business_day, reason AS note, moved_by AS staff_id, at AS created_at FROM cash_movements WHERE id = ?1", None)?, "cash_movements", &row.row_id),
            "customers" => self.with_row(self.customer(outlet, &row.row_id, row.created_at)?, "customers", &row.row_id),
            "credit_adjustments" => self.with_row(self.ledger(row, "SELECT id, customer_id, 'adjustment' AS kind, NULL AS bill_id, CASE WHEN increases = 1 THEN amount ELSE -amount END AS amount_paise, business_day, at, reason AS note FROM credit_adjustments WHERE id = ?1")?, "credit_adjustments", &row.row_id),
            "customer_payments" => self.with_row(self.ledger(row, "SELECT id, customer_id, 'payment' AS kind, NULL AS bill_id, -amount AS amount_paise, business_day, received_at AS at, COALESCE(note, '') AS note FROM customer_payments WHERE id = ?1")?, "customer_payments", &row.row_id),
            "categories" => self.with_row(self.one(row, "SELECT id, name, sort_order, is_active FROM categories WHERE id = ?1", None)?, "categories", &row.row_id),
            "items" => self.with_row(self.one(row, "SELECT id, category_id, name, unit_price AS unit_price_paise, tax_rate_bp, short_code, is_available, sort_order FROM items WHERE id = ?1", None)?, "items", &row.row_id),
            "staff" => self.with_row(self.one(row, "SELECT id, role_id, name, code, phone, joined_on, status, designation, department, is_rider, employment_type, left_on, can_login_on_phone, pin_hash FROM staff WHERE id = ?1", None)?, "staff", &row.row_id),
            "roles" => self.role(row),
            _ => self.boxed(row),
        }
    }

    /// The expense's note is its description, with the note after it.
    fn expense_note(data: &mut Map<String, Value>) {
        let description = data
            .remove("description")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let note = data
            .get("note")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let joined = if note.is_empty() {
            description
        } else if description.is_empty() {
            note
        } else {
            format!("{description} — {note}")
        };
        data.insert("note".to_owned(), Value::from(joined));
    }

    /// One typed row by SQL. The SELECT names the cloud's columns; booleans become booleans.
    fn one(
        &self,
        row: &OutboxRow,
        sql: &str,
        fix: Option<fn(&mut Map<String, Value>)>,
    ) -> Result<Vec<WireRow>, DbError> {
        let Some(mut data) = self.select_one(sql, &row.row_id)? else {
            return Ok(Vec::new());
        };
        for flag in ["is_active", "is_available", "is_rider", "can_login_on_phone"] {
            if let Some(v) = data.get(flag).and_then(Value::as_i64) {
                data.insert(flag.to_owned(), Value::Bool(v != 0));
            }
        }
        if let Some(fix) = fix {
            fix(&mut data);
        }
        Ok(vec![WireRow {
            table: row.table_name.clone(),
            id: row.row_id.clone(),
            updated_at: row.created_at,
            deleted: false,
            data: Value::Object(data),
        }])
    }

    fn select_one(&self, sql: &str, key: &str) -> Result<Option<Map<String, Value>>, DbError> {
        let mut stmt = self.tx.prepare(sql)?;
        let names: Vec<String> = stmt.column_names().iter().map(|s| (*s).to_owned()).collect();
        let mut rows = stmt.query([key])?;
        let Some(r) = rows.next()? else {
            return Ok(None);
        };
        let mut data = Map::new();
        for (i, name) in names.iter().enumerate() {
            data.insert(name.clone(), sql_value(r.get_ref(i)?));
        }
        Ok(Some(data))
    }

    fn ledger(&self, row: &OutboxRow, sql: &str) -> Result<Vec<WireRow>, DbError> {
        let mut out = self.one(row, sql, None)?;
        for r in &mut out {
            "customer_ledger".clone_into(&mut r.table);
        }
        Ok(out)
    }

    fn customer(&self, _outlet: &str, id: &str, at: Timestamp) -> Result<Vec<WireRow>, DbError> {
        // One customer id is one customer, whichever outlet: the id is the key.
        let Some(mut data) = self.select_one(
            "SELECT id, name, phone, address, credit_limit AS credit_limit_paise, is_active FROM customers WHERE id = ?1",
            id,
        )?
        else {
            return Ok(Vec::new());
        };
        let balance = crate::repo::money::MoneyRepo::new(self.tx)
            .customer_balance(&mb_core::CustomerId::new(id))?;
        data.insert("balance_paise".to_owned(), Value::from(balance.paise()));
        if let Some(v) = data.get("is_active").and_then(Value::as_i64) {
            data.insert("is_active".to_owned(), Value::Bool(v != 0));
        }
        Ok(vec![WireRow {
            table: "customers".to_owned(),
            id: id.to_owned(),
            updated_at: at,
            deleted: false,
            data: Value::Object(data),
        }])
    }

    fn role(&self, row: &OutboxRow) -> Result<Vec<WireRow>, DbError> {
        let mut out = self.one(
            row,
            "SELECT id, name, is_builtin, max_discount_bp, max_discount_paise FROM roles WHERE id = ?1",
            None,
        )?;
        let Some(first) = out.first_mut() else {
            return Ok(out);
        };
        let mut stmt = self
            .tx
            .prepare_cached("SELECT permission_code FROM role_permissions WHERE role_id = ?1 ORDER BY 1")?;
        let codes = stmt
            .query_map([&row.row_id], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if let Value::Object(map) = &mut first.data {
            if let Some(v) = map.get("is_builtin").and_then(Value::as_i64) {
                map.insert("is_builtin".to_owned(), Value::Bool(v != 0));
            }
            map.insert("permissions".to_owned(), json!(codes));
        }
        Ok(out)
    }

    /// Any other table: the row as stored, column for column.
    fn boxed(&self, row: &OutboxRow) -> Result<Vec<WireRow>, DbError> {
        if !is_a_table_name(&row.table_name) {
            return Err(DbError::invariant(format!(
                "\"{}\" is not a table name",
                row.table_name
            )));
        }
        let sql = format!(
            "SELECT * FROM \"{}\" WHERE \"{}\" = ?1",
            row.table_name,
            key_column(&row.table_name)
        );
        let Some(data) = self.select_one(&sql, &row.row_id)? else {
            return Ok(Vec::new());
        };
        Ok(vec![WireRow {
            table: row.table_name.clone(),
            id: row.row_id.clone(),
            updated_at: row.created_at,
            deleted: false,
            data: Value::Object(data),
        }])
    }

    /// A settled or voided order, as a cloud bill. Anything else does not travel.
    fn order(&self, outlet: &str, id: &str) -> Result<Option<WireRow>, DbError> {
        let repos = crate::repo::Repos::new(self.tx);
        let Some(order) = repos.orders().find(&OrderId::new(id))? else {
            return Ok(None);
        };
        let (settled, void): (&SettledOrder, Option<(&str, Timestamp, &StaffId)>) = match &order {
            AnyOrder::Settled(s) => (s, None),
            AnyOrder::Voided(v) => {
                // A voided order carries the settled one inside it, field for field.
                return self.voided(outlet, v);
            }
            _ => return Ok(None),
        };
        Ok(Some(self.settled_row(outlet, settled, void)?))
    }

    fn voided(&self, outlet: &str, v: &mb_core::VoidedOrder) -> Result<Option<WireRow>, DbError> {
        let settled = SettledOrder {
            core: v.core.clone(),
            token: v.token.clone(),
            bill_number: v.bill_number.clone(),
            bill: v.bill.clone(),
            settlement: v.settlement.clone(),
            settled_at: v.settled_at,
            settled_by: v.settled_by.clone(),
        };
        Ok(Some(self.settled_row(
            outlet,
            &settled,
            Some((v.reason.as_str(), v.voided_at, &v.voided_by)),
        )?))
    }

    fn settled_row(
        &self,
        outlet: &str,
        s: &SettledOrder,
        void: Option<(&str, Timestamp, &StaffId)>,
    ) -> Result<WireRow, DbError> {
        let id = s.core.id.as_str();
        let repos = crate::repo::Repos::new(self.tx);

        // The parts of the header the core does not carry.
        let (terminal_id, customer_id): (String, Option<String>) = self.tx.query_row(
            "SELECT terminal_id, customer_id FROM orders WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let customer_name: Option<String> = self.tx.query_row(
            "SELECT customer_name FROM bills WHERE order_id = ?1",
            [id],
            |r| r.get(0),
        )?;

        let (placement, table_name) = match &s.core.placement {
            Placement::DineIn { table, seat } => {
                let label = repos
                    .floor()
                    .find_table(table)?
                    .map(|t| t.label)
                    .unwrap_or_else(|| table.as_str().to_owned());
                let seat = seat.as_ref().map(|x| x.as_str().to_owned()).unwrap_or_default();
                ("table", Some(format!("{label}{seat}")))
            }
            Placement::Parcel => ("parcel", None),
            Placement::SelfService => ("self_service", None),
            Placement::Delivery => ("delivery", None),
        };
        let staff_name = repos
            .people()
            .find_staff(outlet, s.settled_by.as_str())?
            .map(|p| p.name);

        let bill = &s.bill;
        let gst = bill.total_gst;
        let tax_paise = gst.central.paise() + gst.state.paise() + gst.integrated.paise()
            + bill.total_vat.into_money().paise();
        // As given, so the bill computes to the same rupee on the other side.
        let bill_discount = bill.bill_discount.clone();
        let charges: Vec<Charge> = bill
            .charges
            .iter()
            .map(|c| Charge {
                kind: c.kind.clone(),
                name: c.name.clone(),
                basis: c.basis,
                tax: c.tax,
            })
            .collect();

        let restore = json!({
            "placement": s.core.placement,
            "covers": s.core.covers,
            "note": s.core.note,
            "created_by": s.core.created_by,
            "token": s.token,
            "bill_number": s.bill_number,
            "bill_input": {
                "bill_discount": bill_discount,
                "charges": charges,
                "place_of_supply": bill.place_of_supply,
                "registration": bill.registration,
                "state_tax": bill.state_tax,
                "order_type": bill.order_type,
                "rounding": bill.rounding,
            },
            "settled_at": ms(s.settled_at),
            "settled_by": s.settled_by,
            "void": void.map(|(reason, at, by)| json!({ "reason": reason, "voided_at": ms(at), "voided_by": by })),
        });

        let data = json!({
            "terminal_id": terminal_id,
            "bill_number": s.bill_number.formatted,
            "token_number": s.token.value,
            "business_day": days(s.core.business_day),
            "created_at": ms(s.core.created_at),
            "settled_at": ms(s.settled_at),
            "order_type": encode::order_type_to_sql(bill.order_type),
            "placement": placement,
            "table_name": table_name,
            "customer_id": customer_id,
            "customer_name": customer_name,
            "staff_id": s.settled_by.as_str(),
            "staff_name": staff_name,
            "status": if void.is_some() { "voided" } else { "settled" },
            "subtotal_paise": bill.subtotal.paise(),
            "discount_paise": bill.total_discount.paise(),
            "tax_paise": tax_paise,
            "charges_paise": bill.total_charges.paise(),
            "round_off_paise": bill.round_off.paise(),
            "grand_total_paise": bill.grand_total.paise(),
            "payments": s.settlement,
            "lines": s.core.cart.lines(),
            "tax_rows": bill.summary,
            "void_reason": void.map(|(reason, _, _)| reason),
            "source": "counter",
            "restore": restore,
        });
        Ok(WireRow {
            table: "orders".to_owned(),
            id: id.to_owned(),
            updated_at: void.map_or(s.settled_at, |(_, at, _)| at),
            deleted: false,
            data,
        })
    }

    // ------------------------------------------------------------- totals

    fn day_of(key: &str) -> Result<BusinessDay, DbError> {
        let n: i64 = key.parse().map_err(|_| DbError::BadValue {
            column: "sync_outbox.row_id",
            value: key.to_owned(),
        })?;
        encode::business_day_from_sql(n, "sync_outbox.row_id")
    }

    /// The day's one row: bills, money, the split by payment mode, the expenses.
    fn day_totals(&self, outlet: &str, key: &str) -> Result<Option<WireRow>, DbError> {
        let day = Self::day_of(key)?;
        let repos = crate::repo::Repos::new(self.tx);
        let period = Period::one_day(day);
        let by_day = repos.reports().sales_by(outlet, period, SalesBy::Day)?;
        let corrections = repos.corrections().day_totals(outlet, day)?;
        let mut by_payment = Map::new();
        for bucket in repos.reports().sales_by(outlet, period, SalesBy::PaymentMode)? {
            by_payment.insert(bucket.key.clone(), Value::from(bucket.gross.paise()));
        }
        let day_sql = days(day);
        let charges: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(b.total_charges), 0) FROM bills b JOIN orders o ON o.id = b.order_id WHERE o.outlet_id = ?1 AND o.business_day = ?2 AND o.state = 'settled'",
            rusqlite::params![outlet, day_sql],
            |r| r.get(0),
        )?;
        let expenses: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM expenses WHERE outlet_id = ?1 AND business_day = ?2",
            rusqlite::params![outlet, day_sql],
            |r| r.get(0),
        )?;
        let credit_given: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(p.amount), 0) FROM payments p JOIN orders o ON o.id = p.order_id WHERE o.outlet_id = ?1 AND o.business_day = ?2 AND o.state = 'settled' AND p.mode = 'credit'",
            rusqlite::params![outlet, day_sql],
            |r| r.get(0),
        )?;
        let credit_collected: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM customer_payments WHERE outlet_id = ?1 AND business_day = ?2",
            rusqlite::params![outlet, day_sql],
            |r| r.get(0),
        )?;
        let is_day_closed = repos.money().find_day_close(outlet, day)?.is_some();
        let (gross, discount, tax) = by_day
            .first()
            .map_or((Money::ZERO, Money::ZERO, Money::ZERO), |b| {
                (b.gross, b.discount, b.tax)
            });
        let data = json!({
            "business_day": day_sql,
            "bills": corrections.bills,
            "voids": corrections.voided_bills,
            "gross_paise": gross.paise(),
            "discount_paise": discount.paise(),
            "tax_paise": tax.paise(),
            "charges_paise": charges,
            "net_paise": corrections.net.paise(),
            "by_payment": by_payment,
            "expenses_paise": expenses,
            "credit_given_paise": credit_given,
            "credit_collected_paise": credit_collected,
            "is_day_closed": is_day_closed,
        });
        Ok(Some(WireRow {
            table: "day_totals".to_owned(),
            id: key.to_owned(),
            updated_at: Timestamp::from_millis(0),
            deleted: false,
            data,
        }))
    }

    /// One row per item (or category) sold that day.
    fn day_group_totals(&self, outlet: &str, key: &str, by: SalesBy) -> Result<Vec<WireRow>, DbError> {
        let day = Self::day_of(key)?;
        let repos = crate::repo::Repos::new(self.tx);
        let buckets = repos.reports().sales_by(outlet, Period::one_day(day), by)?;
        let day_sql = days(day);
        let mut out = Vec::with_capacity(buckets.len());
        for b in buckets {
            let qty = b.qty.map_or(0, encode::qty_to_sql);
            let (table, data) = match by {
                SalesBy::Item => {
                    let category_id: Option<String> = self
                        .tx
                        .query_row("SELECT category_id FROM items WHERE id = ?1", [&b.key], |r| r.get(0))
                        .ok()
                        .flatten();
                    (
                        "day_item_totals",
                        json!({
                            "business_day": day_sql, "item_id": b.key, "item_name": b.label,
                            "category_id": category_id, "qty_thousandths": qty, "sales_paise": b.gross.paise(),
                        }),
                    )
                }
                _ => (
                    "day_category_totals",
                    json!({
                        "business_day": day_sql, "category_id": b.key, "category_name": b.label,
                        "qty_thousandths": qty, "sales_paise": b.gross.paise(),
                    }),
                ),
            };
            out.push(WireRow {
                table: table.to_owned(),
                id: format!("{key}|{}", b.key),
                updated_at: Timestamp::from_millis(0),
                deleted: false,
                data,
            });
        }
        Ok(out)
    }

    // ---------------------------------------------------------------- down

    /// A box row back into its table: only the columns this database has, and only when the
    /// table exists here. Unknown columns are the cloud's problem, not ours.
    pub fn write_boxed(&self, table: &str, payload: &Value) -> Result<bool, DbError> {
        if !is_a_table_name(table) {
            return Ok(false);
        }
        let Value::Object(map) = payload else {
            return Ok(false);
        };
        let mut stmt = self.tx.prepare(&format!("PRAGMA table_info(\"{table}\")"))?;
        let columns: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        if columns.is_empty() {
            return Ok(false);
        }
        let present: Vec<&String> = columns.iter().filter(|c| map.contains_key(*c)).collect();
        if present.is_empty() {
            return Ok(false);
        }
        let names = present
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let marks = (1..=present.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("INSERT OR REPLACE INTO \"{table}\" ({names}) VALUES ({marks})");
        let mut stmt = self.tx.prepare(&sql)?;
        let params: Vec<rusqlite::types::Value> = present
            .iter()
            .map(|c| to_sql(&map[*c]))
            .collect();
        stmt.execute(rusqlite::params_from_iter(params))?;
        Ok(true)
    }

    /// A cloud bill back into a real order, recomputed from its cart the same way it was the
    /// first time.
    pub fn write_order(&self, outlet: &str, id: &str, data: &Value) -> Result<bool, DbError> {
        let Some(restore) = data.get("restore") else {
            return Ok(false);
        };
        let parsed: Restore = serde_json::from_value(restore.clone())
            .map_err(|e| DbError::invariant(format!("bill {id} could not be read back: {e}")))?;
        let lines: Vec<CartLine> = serde_json::from_value(data.get("lines").cloned().unwrap_or(Value::Null))
            .map_err(|e| DbError::invariant(format!("bill {id} lines could not be read back: {e}")))?;
        let settlement: mb_core::Settlement =
            serde_json::from_value(data.get("payments").cloned().unwrap_or(Value::Null))
                .map_err(|e| DbError::invariant(format!("bill {id} payments could not be read back: {e}")))?;
        let business_day = encode::business_day_from_sql(
            data.get("business_day").and_then(Value::as_i64).unwrap_or(0),
            "bills.business_day",
        )?;
        let created_at = encode::timestamp_from_sql(data.get("created_at").and_then(Value::as_i64).unwrap_or(0));
        let terminal_id = data
            .get("terminal_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();

        let mut cart = Cart::new();
        for line in lines {
            cart.push(line)
                .map_err(|e| DbError::invariant(format!("bill {id} line could not be restored: {e}")))?;
        }
        let input = mb_core::BillInput {
            cart: &cart,
            bill_discount: parsed.bill_input.bill_discount,
            charges: &parsed.bill_input.charges,
            place_of_supply: parsed.bill_input.place_of_supply,
            registration: parsed.bill_input.registration,
            state_tax: parsed.bill_input.state_tax,
            order_type: parsed.bill_input.order_type,
            rounding: parsed.bill_input.rounding,
        };
        let bill = mb_core::compute_bill(input)
            .map_err(|e| DbError::invariant(format!("bill {id} could not be recomputed: {e}")))?;

        let core = OrderCore {
            id: OrderId::new(id),
            business_day,
            created_at,
            placement: parsed.placement,
            covers: parsed.covers,
            cart,
            created_by: parsed.created_by,
            note: parsed.note,
            kitchen: KitchenLedger::new(),
        };
        let settled = SettledOrder {
            core,
            token: parsed.token,
            bill_number: parsed.bill_number,
            bill,
            settlement,
            settled_at: encode::timestamp_from_sql(parsed.settled_at),
            settled_by: parsed.settled_by,
        };
        let order = match parsed.void {
            Some(v) => AnyOrder::Voided(
                settled
                    .void(&v.reason, v.voided_by, encode::timestamp_from_sql(v.voided_at))
                    .map_err(|e| DbError::invariant(format!("bill {id} could not be voided back: {e}")))?,
            ),
            None => AnyOrder::Settled(settled),
        };
        let repos = crate::repo::Repos::new(self.tx);
        repos.orders().save(outlet, &terminal_id, &order)?;
        if let Some(customer) = data.get("customer_id").and_then(Value::as_str) {
            self.tx.execute(
                "UPDATE orders SET customer_id = ?2 WHERE id = ?1",
                rusqlite::params![id, customer],
            )?;
        }
        if let Some(name) = data.get("customer_name").and_then(Value::as_str) {
            self.tx.execute(
                "UPDATE bills SET customer_name = ?2 WHERE order_id = ?1",
                rusqlite::params![id, name],
            )?;
        }
        Ok(true)
    }
}

fn to_sql(v: &Value) -> rusqlite::types::Value {
    use rusqlite::types::Value as S;
    match v {
        Value::Null => S::Null,
        Value::Bool(b) => S::Integer(i64::from(*b)),
        Value::Number(n) => n
            .as_i64()
            .map(S::Integer)
            .or_else(|| n.as_f64().map(S::Real))
            .unwrap_or(S::Null),
        Value::String(s) => S::Text(s.clone()),
        Value::Object(map) => match map.get(BLOB_KEY).and_then(Value::as_str) {
            Some(b64) => base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_or(S::Null, S::Blob),
            None => S::Text(v.to_string()),
        },
        Value::Array(_) => S::Text(v.to_string()),
    }
}

/// The part of a cloud bill that rebuilds the order.
#[derive(Debug, Deserialize)]
struct Restore {
    placement: Placement,
    covers: Option<u32>,
    note: Option<String>,
    created_by: StaffId,
    token: mb_core::Claimed,
    bill_number: mb_core::Claimed,
    bill_input: RestoreInput,
    settled_at: i64,
    settled_by: StaffId,
    void: Option<RestoreVoid>,
}

#[derive(Debug, Deserialize)]
struct RestoreInput {
    bill_discount: Option<DiscountEntry>,
    charges: Vec<Charge>,
    place_of_supply: mb_core::PlaceOfSupply,
    registration: mb_core::Registration,
    state_tax: mb_core::StateTax,
    order_type: mb_core::OrderType,
    rounding: mb_core::RoundingMode,
}

#[derive(Debug, Deserialize)]
struct RestoreVoid {
    reason: String,
    voided_at: i64,
    voided_by: StaffId,
}

/// What a restore did with one cloud row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restored {
    Written,
    /// Nothing here to write it into — an item-wise total, or a row from a cloud that predates
    /// the whole-row carry.
    Skipped,
}

/// Columns that never leave as part of a whole row. The PIN hash travels on its own to
/// `staff_secrets`, which no phone can read; the whole row is readable by the owner's phone.
const NEVER_IN_A_ROW: &[&str] = &["pin_hash"];

impl<'a> WireRepo<'a> {
    // ------------------------------------------------------- the whole row

    /// Attach the counter's whole row to each typed wire row, so the cloud can hand it back.
    fn with_row(&self, mut out: Vec<WireRow>, table: &str, id: &str) -> Result<Vec<WireRow>, DbError> {
        let Some(full) = self.full_row(table, id)? else {
            return Ok(out);
        };
        for wire in &mut out {
            if let Value::Object(map) = &mut wire.data {
                map.insert(ROW_KEY.to_owned(), Value::Object(full.clone()));
            }
        }
        Ok(out)
    }

    /// The row as stored, column for column, with the table's name inside it.
    fn full_row(&self, table: &str, id: &str) -> Result<Option<Map<String, Value>>, DbError> {
        if !is_a_table_name(table) {
            return Ok(None);
        }
        let sql = format!(
            "SELECT * FROM \"{table}\" WHERE \"{}\" = ?1",
            key_column(table)
        );
        let Some(mut row) = self.select_one(&sql, id)? else {
            return Ok(None);
        };
        for secret in NEVER_IN_A_ROW {
            row.remove(*secret);
        }
        row.insert(ROW_TABLE_KEY.to_owned(), Value::from(table));
        Ok(Some(row))
    }

    // ------------------------------------------------------------- restore

    /// One cloud row back into this database. `table` is the cloud's name for it; `data` is the
    /// row as the counter sent it (days and instants already integers).
    pub fn restore_row(
        &self,
        outlet: &str,
        table: &str,
        id: &str,
        updated_at: Timestamp,
        data: &Value,
    ) -> Result<Restored, DbError> {
        match table {
            "bills" | "orders" => Ok(if self.write_order(outlet, id, data)? {
                Restored::Written
            } else {
                Restored::Skipped
            }),
            "roles" => {
                let role = cloud_role_from(id, data);
                crate::repo::people::PeopleRepo::new(self.tx).apply_role_from_cloud(outlet, &role)?;
                Ok(Restored::Written)
            }
            "staff" => {
                // The whole row first (address, emergency contact, id proof), then the typed
                // columns, which win when the phone edited them after the counter last pushed.
                let mut written = self.write_row_inside(data)?;
                let staff = cloud_staff_from(id, updated_at, data)?;
                let people = crate::repo::people::PeopleRepo::new(self.tx);
                written |= people.apply_staff_from_cloud(outlet, &staff)?;
                Ok(if written { Restored::Written } else { Restored::Skipped })
            }
            "day_totals" => {
                self.write_day_totals(outlet, updated_at, data)?;
                Ok(Restored::Written)
            }
            "day_item_totals" | "day_category_totals" => Ok(Restored::Skipped),
            _ => Ok(if self.write_row_inside(data)? {
                Restored::Written
            } else {
                Restored::Skipped
            }),
        }
    }

    /// The whole counter row a typed cloud row carries, back into its own table.
    fn write_row_inside(&self, data: &Value) -> Result<bool, DbError> {
        let Some(row @ Value::Object(map)) = data.get(ROW_KEY) else {
            return Ok(false);
        };
        let Some(table) = map.get(ROW_TABLE_KEY).and_then(Value::as_str) else {
            return Ok(false);
        };
        self.write_boxed(table, row)
    }

    /// A day whose bills are not here: one row the day-wise report can read.
    fn write_day_totals(&self, outlet: &str, updated_at: Timestamp, data: &Value) -> Result<(), DbError> {
        let int = |key: &str| data.get(key).and_then(Value::as_i64).unwrap_or(0);
        let day = int("business_day");
        if day <= 0 {
            return Err(DbError::BadValue {
                column: "cloud_day_totals.business_day",
                value: data.get("business_day").map(Value::to_string).unwrap_or_default(),
            });
        }
        let by_payment = data
            .get("by_payment")
            .filter(|v| v.is_object())
            .map_or_else(|| "{}".to_owned(), Value::to_string);
        self.tx.execute(
            "INSERT INTO cloud_day_totals (outlet_id, business_day, bills, voids, gross, discount, tax,
                                           charges, net, by_payment, expenses, credit_given,
                                           credit_collected, is_day_closed, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT (outlet_id, business_day) DO UPDATE SET
                 bills = excluded.bills, voids = excluded.voids, gross = excluded.gross,
                 discount = excluded.discount, tax = excluded.tax, charges = excluded.charges,
                 net = excluded.net, by_payment = excluded.by_payment, expenses = excluded.expenses,
                 credit_given = excluded.credit_given, credit_collected = excluded.credit_collected,
                 is_day_closed = excluded.is_day_closed, updated_at = excluded.updated_at",
            rusqlite::params![
                outlet,
                day,
                int("bills"),
                int("voids"),
                int("gross_paise"),
                int("discount_paise"),
                int("tax_paise"),
                int("charges_paise"),
                int("net_paise"),
                by_payment,
                int("expenses_paise"),
                int("credit_given_paise"),
                int("credit_collected_paise"),
                encode::bool_to_sql(data.get("is_day_closed").and_then(Value::as_bool).unwrap_or(false)),
                encode::timestamp_to_sql(updated_at),
            ],
        )?;
        Ok(())
    }

    /// Days the report can only know from the cloud: everything in the period that has a
    /// totals row here. The caller drops the days it has bills for.
    pub fn cloud_days(
        &self,
        outlet: &str,
        period: Period,
    ) -> Result<Vec<crate::repo::reports::Bucket>, DbError> {
        let mut stmt = self.tx.prepare_cached(
            "SELECT business_day, bills, gross, discount, tax
               FROM cloud_day_totals
              WHERE outlet_id = ?1 AND business_day BETWEEN ?2 AND ?3
              ORDER BY business_day",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![outlet, days(period.from), days(period.to)],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            },
        )?;
        let mut out = Vec::new();
        for row in rows {
            let (day, bills, gross, discount, tax) = row?;
            let label = encode::business_day_from_sql(day, "cloud_day_totals.business_day")
                .map_or_else(|_| day.to_string(), |d| d.to_string());
            out.push(crate::repo::reports::Bucket {
                key: day.to_string(),
                label,
                bills,
                gross: encode::money_from_sql(gross),
                discount: encode::money_from_sql(discount),
                tax: encode::money_from_sql(tax),
                qty: None,
            });
        }
        Ok(out)
    }
}

fn str_of(data: &Value, key: &str) -> Option<String> {
    data.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn day_of_field(data: &Value, key: &str, column: &'static str) -> Result<Option<BusinessDay>, DbError> {
    data.get(key)
        .and_then(Value::as_i64)
        .map(|n| encode::business_day_from_sql(n, column))
        .transpose()
}

/// A staff row as the cloud carries it, read the way `mb_pull_payload` writes it.
pub fn cloud_staff_from(
    id: &str,
    updated_at: Timestamp,
    data: &Value,
) -> Result<crate::repo::people::CloudStaff, DbError> {
    use crate::repo::people::{CloudStaff, StaffStatus};
    let status = StaffStatus::from_sql(data.get("status").and_then(Value::as_str).unwrap_or("active"))?;
    let employment_type = str_of(data, "employment_type").unwrap_or_else(|| "full_time".to_owned());
    if !["full_time", "part_time", "casual"].contains(&employment_type.as_str()) {
        return Err(DbError::BadValue {
            column: "staff.employment_type",
            value: employment_type,
        });
    }
    Ok(CloudStaff {
        id: id.to_owned(),
        role_id: str_of(data, "role_id"),
        name: str_of(data, "name").unwrap_or_else(|| "Unnamed".to_owned()),
        code: str_of(data, "code"),
        phone: str_of(data, "phone"),
        joined_on: day_of_field(data, "joined_on", "staff.joined_on")?,
        status,
        designation: str_of(data, "designation"),
        department: str_of(data, "department"),
        is_rider: data.get("is_rider").and_then(Value::as_bool).unwrap_or(false),
        employment_type,
        left_on: day_of_field(data, "left_on", "staff.left_on")?,
        can_login_on_phone: data
            .get("can_login_on_phone")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        updated_at,
    })
}

/// A role as the cloud carries it.
#[must_use]
pub fn cloud_role_from(id: &str, data: &Value) -> crate::repo::people::CloudRole {
    let permissions = data
        .get("permissions")
        .and_then(Value::as_array)
        .map(|codes| {
            codes
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    crate::repo::people::CloudRole {
        id: id.to_owned(),
        name: str_of(data, "name").unwrap_or_else(|| "Role".to_owned()),
        is_builtin: data.get("is_builtin").and_then(Value::as_bool).unwrap_or(false),
        max_discount_bp: data.get("max_discount_bp").and_then(Value::as_i64),
        max_discount_paise: data.get("max_discount_paise").and_then(Value::as_i64),
        permissions,
    }
}

// So the id types are the same ones the rest of the crate speaks.
#[allow(dead_code, reason = "named so a reader can see which id a table is keyed by")]
type _Table = TableId;
