//! Rows as the cloud expects them, and back again.
//!
//! The outbox names a table and a row; this reads the row at send time and shapes it the way
//! `MB-backend/docs/SYNC_PROTOCOL.md` says. Typed tables get the cloud's column names; every
//! other table goes into the cloud's box exactly as it is stored here, column for column.
//! The same shapes make up a day file (`crate::archive`), and the same module brings a shop
//! back down onto a new computer — from the cloud's tables and from the day files alike.

use base64::Engine as _;
use mb_core::{
    AnyOrder, Bill, BusinessDay, Cart, CartLine, Charge, Claimed, DiscountEntry, KitchenLedger,
    Money, OrderCore, OrderId, Placement, Settlement, SettledOrder, StaffId, TableId, Timestamp,
};
use rusqlite::Transaction;
use rusqlite::types::ValueRef;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::encode;
use crate::error::DbError;
use crate::repo::outbox::{Op, OutboxRow, key_column};
use crate::repo::reports::{Period, SalesBy};

/// The three per-day rows a settled bill changes. Queued by day, computed when sent.
pub const TOTALS_TABLES: &[&str] = &["day_totals", "day_item_totals", "day_category_totals"];

/// The cloud's name for a counter table, where the two differ. ONE map: the sender names a
/// wire row with it, the restore reads either spelling, and the cloud accepts both.
pub const CLOUD_NAME: &[(&str, &str)] = &[
    ("orders", "bills"),
    ("categories", "menu_categories"),
    ("items", "menu_items"),
    ("credit_adjustments", "customer_ledger"),
    ("customer_payments", "customer_ledger"),
];

/// What the cloud calls this counter table.
#[must_use]
pub fn cloud_name(counter_table: &str) -> &str {
    CLOUD_NAME
        .iter()
        .find(|(ours, _)| *ours == counter_table)
        .map_or(counter_table, |(_, theirs)| theirs)
}

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

    /// A wire row read back from its JSON — a day file's line, or a test's.
    pub fn from_json(v: &Value) -> Result<WireRow, DbError> {
        let bad = |what: &str| DbError::invariant(format!("a wire row needs {what}"));
        Ok(WireRow {
            table: v.get("table").and_then(Value::as_str).ok_or_else(|| bad("a table"))?.to_owned(),
            id: v.get("id").and_then(Value::as_str).ok_or_else(|| bad("an id"))?.to_owned(),
            updated_at: Timestamp::from_millis(
                v.get("updated_at").and_then(Value::as_i64).ok_or_else(|| bad("updated_at"))?,
            ),
            deleted: v.get("deleted").and_then(Value::as_bool).unwrap_or(false),
            data: v.get("data").cloned().unwrap_or_else(|| json!({})),
        })
    }
}

/// A blob crosses as `{"$b64": "…"}`, because JSON has no bytes.
const BLOB_KEY: &str = "$b64";

/// A typed row also carries the whole counter row under this key, so a restore puts back every
/// column and not only the ones the cloud has names for. `$table` inside it names the table.
pub const ROW_KEY: &str = "row";
pub const ROW_TABLE_KEY: &str = "$table";

/// A boxed row whose children travel with it, as rows of their own: the child table and the
/// column that names the parent.
fn children_of(table: &str) -> &'static [(&'static str, &'static str)] {
    match table {
        "bill_reverts" => &[
            ("bill_revert_lines", "revert_id"),
            ("bill_revert_payments", "revert_id"),
        ],
        _ => &[],
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

/// The row's own moment where the table keeps one, else the outbox entry's. One rule for
/// every table, typed or boxed.
fn stamp_of(data: &Map<String, Value>, fallback: Timestamp) -> Timestamp {
    data.get("updated_at")
        .and_then(Value::as_i64)
        .filter(|ms| *ms > 0)
        .map_or(fallback, Timestamp::from_millis)
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
                table: cloud_name(&row.table_name).to_owned(),
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
            "orders" => Ok(self.order_row(outlet, &row.row_id)?.into_iter().collect()),
            "day_totals" => Ok(self.day_totals(outlet, &row.row_id, row.created_at)?.into_iter().collect()),
            "day_item_totals" => self.day_group_totals(outlet, &row.row_id, row.created_at, SalesBy::Item),
            "day_category_totals" => self.day_group_totals(outlet, &row.row_id, row.created_at, SalesBy::Category),
            "expenses" => self.with_row(self.one(row, "SELECT e.id, e.category_id, c.name AS category_name, e.amount AS amount_paise, e.description, e.note, e.business_day, e.paid_by AS paid_by_staff_id, e.paid_at AS created_at, e.mode, e.paid_to, e.reference, e.gst_rate_bp, e.gst_amount AS gst_paise FROM expenses e LEFT JOIN expense_categories c ON c.id = e.category_id WHERE e.id = ?1", Some(Self::expense_note))?, "expenses", &row.row_id),
            "expense_categories" => self.with_row(self.one(row, "SELECT id, name, sort_order FROM expense_categories WHERE id = ?1", None)?, "expense_categories", &row.row_id),
            "cash_movements" => self.with_row(self.one(row, "SELECT id, kind, amount AS amount_paise, business_day, reason AS note, moved_by AS staff_id, at AS created_at FROM cash_movements WHERE id = ?1", None)?, "cash_movements", &row.row_id),
            "customers" => self.with_row(self.customer(&row.row_id, row.created_at)?, "customers", &row.row_id),
            "credit_adjustments" => self.with_row(self.one(row, "SELECT id, customer_id, 'adjustment' AS kind, NULL AS bill_id, CASE WHEN increases = 1 THEN amount ELSE -amount END AS amount_paise, business_day, at, reason AS note FROM credit_adjustments WHERE id = ?1", None)?, "credit_adjustments", &row.row_id),
            "customer_payments" => self.with_row(self.one(row, "SELECT id, customer_id, 'payment' AS kind, NULL AS bill_id, -amount AS amount_paise, business_day, received_at AS at, COALESCE(note, '') AS note FROM customer_payments WHERE id = ?1", None)?, "customer_payments", &row.row_id),
            "categories" => self.with_row(self.one(row, "SELECT id, name, sort_order, is_active, updated_at FROM categories WHERE id = ?1", None)?, "categories", &row.row_id),
            "items" => self.with_row(self.one(row, "SELECT i.id, i.category_id, i.name, i.unit_price AS unit_price_paise, c.rate_bp AS tax_rate_bp, i.short_code, i.is_available, i.sort_order, i.updated_at FROM items i JOIN tax_classes c ON c.id = i.tax_class_id WHERE i.id = ?1", None)?, "items", &row.row_id),
            "staff" => self.with_row(self.one(row, "SELECT id, role_id, name, phone, joined_on, status, designation, department, is_rider, employment_type, left_on, pin_hash, updated_at FROM staff WHERE id = ?1", None)?, "staff", &row.row_id),
            "roles" => self.role(row),
            crate::numbering::TABLE => self.counter(outlet, row),
            _ => self.boxed(row),
        }
    }

    /// A counter row, whole, named by its terminal and kind: the shape of a series is part of
    /// the shop, and comes back with it.
    fn counter(&self, outlet: &str, row: &OutboxRow) -> Result<Vec<WireRow>, DbError> {
        let Some((terminal, kind)) = crate::numbering::parse_row_id(&row.row_id) else {
            return Ok(Vec::new());
        };
        let Some(data) = self.select_one(
            "SELECT * FROM counters WHERE outlet_id = ?1 AND terminal_id = ?2 AND kind = ?3",
            rusqlite::params![outlet, terminal, kind.as_sql()],
        )?
        else {
            return Ok(Vec::new());
        };
        Ok(vec![Self::wire_row(&row.table_name, &row.row_id, row.created_at, data)])
    }

    /// One wire row from a map of columns, stamped with the row's own moment where it has one.
    fn wire_row(table: &str, id: &str, at: Timestamp, data: Map<String, Value>) -> WireRow {
        WireRow {
            table: cloud_name(table).to_owned(),
            id: id.to_owned(),
            updated_at: stamp_of(&data, at),
            deleted: false,
            data: Value::Object(data),
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
        let Some(mut data) = self.select_one(sql, [&row.row_id])? else {
            return Ok(Vec::new());
        };
        Self::booleans(&mut data);
        if let Some(fix) = fix {
            fix(&mut data);
        }
        Ok(vec![Self::wire_row(&row.table_name, &row.row_id, row.created_at, data)])
    }

    /// The 0/1 columns the cloud types as booleans. ONE list, applied to every typed row.
    fn booleans(data: &mut Map<String, Value>) {
        for flag in ["is_active", "is_available", "is_rider", "is_builtin"] {
            if let Some(v) = data.get(flag).and_then(Value::as_i64) {
                data.insert(flag.to_owned(), Value::Bool(v != 0));
            }
        }
    }

    /// One row of any query, column for column, or `None`.
    fn select_one<P: rusqlite::Params>(
        &self,
        sql: &str,
        params: P,
    ) -> Result<Option<Map<String, Value>>, DbError> {
        Ok(self.select_all(sql, params)?.pop())
    }

    /// Every row of a query, column for column.
    fn select_all<P: rusqlite::Params>(
        &self,
        sql: &str,
        params: P,
    ) -> Result<Vec<Map<String, Value>>, DbError> {
        let mut stmt = self.tx.prepare(sql)?;
        let names: Vec<String> = stmt.column_names().iter().map(|s| (*s).to_owned()).collect();
        let mut rows = stmt.query(params)?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            let mut data = Map::new();
            for (i, name) in names.iter().enumerate() {
                data.insert(name.clone(), sql_value(r.get_ref(i)?));
            }
            out.push(data);
        }
        Ok(out)
    }

    fn customer(&self, id: &str, at: Timestamp) -> Result<Vec<WireRow>, DbError> {
        // One customer id is one customer, whichever outlet: the id is the key.
        let Some(mut data) = self.select_one(
            "SELECT id, name, phone, address, credit_limit AS credit_limit_paise, is_active, updated_at FROM customers WHERE id = ?1",
            [id],
        )?
        else {
            return Ok(Vec::new());
        };
        let balance = crate::repo::money::MoneyRepo::new(self.tx)
            .customer_balance(&mb_core::CustomerId::new(id))?;
        data.insert("balance_paise".to_owned(), Value::from(balance.paise()));
        Self::booleans(&mut data);
        Ok(vec![Self::wire_row("customers", id, at, data)])
    }

    fn role(&self, row: &OutboxRow) -> Result<Vec<WireRow>, DbError> {
        let mut out = self.one(
            row,
            "SELECT id, name, is_builtin, max_discount_bp, max_discount_paise, updated_at FROM roles WHERE id = ?1",
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
            map.insert("permissions".to_owned(), json!(codes));
        }
        Ok(out)
    }

    /// Any other table: the row as stored, column for column, with its children after it.
    fn boxed(&self, row: &OutboxRow) -> Result<Vec<WireRow>, DbError> {
        self.whole_rows(&row.table_name, &row.row_id, row.created_at)
    }

    /// One stored row, whole, as a wire row — and, for a table whose children travel with it,
    /// each child as a wire row of its own. This is the one boxed reader: the outbox, the day
    /// file and `with_row` all read through it.
    pub fn whole_rows(&self, table: &str, id: &str, at: Timestamp) -> Result<Vec<WireRow>, DbError> {
        let Some(data) = self.stored_row(table, id)? else {
            return Ok(Vec::new());
        };
        let mut out = vec![Self::wire_row(table, id, at, data)];
        for (child, parent_column) in children_of(table) {
            for row in self.select_all(
                &format!("SELECT * FROM \"{child}\" WHERE \"{parent_column}\" = ?1 ORDER BY \"{}\"", key_column(child)),
                [id],
            )? {
                let child_id = row.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
                out.push(Self::wire_row(child, &child_id, at, row));
            }
        }
        Ok(out)
    }

    /// A settled, voided or (once billed, then) cancelled order, as a cloud bill. An order
    /// that never had a bill number does not travel.
    pub(crate) fn order_row(&self, outlet: &str, id: &str) -> Result<Option<WireRow>, DbError> {
        let repos = crate::repo::Repos::new(self.tx);
        let Some(order) = repos.orders().find(&OrderId::new(id))? else {
            return Ok(None);
        };
        let parts = match &order {
            AnyOrder::Settled(s) => Billed {
                core: &s.core,
                token: &s.token,
                number: &s.bill_number,
                money: Some((&s.bill, &s.settlement)),
                staff: &s.settled_by,
                fate: Fate::Settled { at: s.settled_at },
            },
            AnyOrder::Voided(v) => Billed {
                core: &v.core,
                token: &v.token,
                number: &v.bill_number,
                money: Some((&v.bill, &v.settlement)),
                staff: &v.settled_by,
                fate: Fate::Voided {
                    settled_at: v.settled_at,
                    reason: &v.reason,
                    at: v.voided_at,
                    by: &v.voided_by,
                },
            },
            AnyOrder::Cancelled(c) => {
                let Some(number) = &c.bill_number else {
                    return Ok(None);
                };
                Billed {
                    core: &c.core,
                    token: &c.token,
                    number,
                    money: None,
                    staff: &c.cancelled_by,
                    fate: Fate::Cancelled {
                        reason: &c.reason,
                        at: c.cancelled_at,
                        by: &c.cancelled_by,
                    },
                }
            }
            _ => return Ok(None),
        };
        Ok(Some(self.bill_row(outlet, &parts)?))
    }

    /// The one builder of a cloud bill, whatever became of the order.
    fn bill_row(&self, outlet: &str, parts: &Billed<'_>) -> Result<WireRow, DbError> {
        let Billed { core, token, number, money, staff, fate } = *parts;
        let id = core.id.as_str();
        let repos = crate::repo::Repos::new(self.tx);

        // The parts of the header the core does not carry.
        let (terminal_id, customer_id): (String, Option<String>) = self.tx.query_row(
            "SELECT terminal_id, customer_id FROM orders WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let customer_name: Option<String> = self
            .tx
            .query_row("SELECT customer_name FROM bills WHERE order_id = ?1", [id], |r| r.get(0))
            .ok()
            .flatten();

        let (placement, table_name) = match &core.placement {
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
        let staff_name = repos.people().find_staff(outlet, staff.as_str())?.map(|p| p.name);

        // What rebuilds the order on another counter. Nothing here repeats a typed column:
        // `settled_at` and the settling person are read back from `settled_at` / `staff_id`.
        let mut restore = Map::new();
        restore.insert("placement".to_owned(), json!(core.placement));
        if let Some(covers) = core.covers {
            restore.insert("covers".to_owned(), json!(covers));
        }
        if let Some(note) = &core.note {
            restore.insert("note".to_owned(), json!(note));
        }
        restore.insert("created_by".to_owned(), json!(core.created_by));
        restore.insert("token".to_owned(), json!(token));
        restore.insert("bill_number".to_owned(), json!(number));
        if let Some((bill, _)) = money {
            // As given, so the bill computes to the same rupee on the other side.
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
            restore.insert(
                "bill_input".to_owned(),
                json!({
                    "bill_discount": bill.bill_discount,
                    "charges": charges,
                    "place_of_supply": bill.place_of_supply,
                    "registration": bill.registration,
                    "state_tax": bill.state_tax,
                    "order_type": bill.order_type,
                    "rounding": bill.rounding,
                }),
            );
        }
        let (status, settled_at, reason, updated_at) = match fate {
            Fate::Settled { at } => ("settled", Some(at), None, at),
            Fate::Voided { settled_at, reason, at, by } => {
                restore.insert(
                    "void".to_owned(),
                    json!({ "reason": reason, "voided_at": ms(at), "voided_by": by }),
                );
                ("voided", Some(settled_at), Some(reason), at)
            }
            Fate::Cancelled { reason, at, by } => {
                restore.insert(
                    "cancel".to_owned(),
                    json!({ "reason": reason, "cancelled_at": ms(at), "cancelled_by": by }),
                );
                ("cancelled", None, Some(reason), at)
            }
        };

        let paise = |m: Money| m.paise();
        let (subtotal, discount, tax, charges_total, round_off, grand_total) = money.map_or(
            (0, 0, 0, 0, 0, 0),
            |(bill, _)| {
                let gst = bill.total_gst;
                (
                    paise(bill.subtotal),
                    paise(bill.total_discount),
                    gst.central.paise() + gst.state.paise() + gst.integrated.paise()
                        + bill.total_vat.into_money().paise(),
                    paise(bill.total_charges),
                    paise(bill.round_off),
                    paise(bill.grand_total),
                )
            },
        );
        let data = json!({
            "terminal_id": terminal_id,
            "bill_number": number.formatted,
            "token_number": token.value,
            "business_day": days(core.business_day),
            "created_at": ms(core.created_at),
            "settled_at": settled_at.map(ms),
            "order_type": encode::order_type_to_sql(core.order_type()),
            "placement": placement,
            "table_name": table_name,
            "customer_id": customer_id,
            "customer_name": customer_name,
            "staff_id": staff.as_str(),
            "staff_name": staff_name,
            "status": status,
            "subtotal_paise": subtotal,
            "discount_paise": discount,
            "tax_paise": tax,
            "charges_paise": charges_total,
            "round_off_paise": round_off,
            "grand_total_paise": grand_total,
            "payments": money.map(|(_, s)| json!(s)).unwrap_or_else(|| json!(Settlement::new())),
            "lines": core.cart.lines(),
            "tax_rows": money.map(|(b, _)| json!(b.summary)).unwrap_or_else(|| json!(mb_core::TaxSummary::default())),
            "void_reason": reason,
            "source": "counter",
            "restore": Value::Object(restore),
        });
        Ok(WireRow {
            table: cloud_name("orders").to_owned(),
            id: id.to_owned(),
            updated_at,
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

    /// The day's one row: bills, money, the split by payment mode, the expenses. The money is
    /// `DaysRepo::figures` — the same figures the close freezes; gross, discount and tax are
    /// the day-wise report's.
    pub(crate) fn day_totals(&self, outlet: &str, key: &str, at: Timestamp) -> Result<Option<WireRow>, DbError> {
        let day = Self::day_of(key)?;
        let repos = crate::repo::Repos::new(self.tx);
        let figures = repos.days().figures(outlet, day)?;
        let voids = repos.corrections().day_totals(outlet, day)?.voided_bills;
        let by_day = repos.reports().sales_by(outlet, Period::one_day(day), SalesBy::Day)?;
        let mut by_payment = Map::new();
        for (mode, amount) in &figures.by_payment {
            by_payment.insert(mode.clone(), Value::from(amount.paise()));
        }
        let is_day_closed = repos.days().is_locked(outlet, day)?;
        let (gross, discount, tax) = by_day
            .first()
            .map_or((Money::ZERO, Money::ZERO, Money::ZERO), |b| {
                (b.gross, b.discount, b.tax)
            });
        let data = json!({
            "business_day": days(day),
            "bills": figures.bills,
            "voids": voids,
            "gross_paise": gross.paise(),
            "discount_paise": discount.paise(),
            "tax_paise": tax.paise(),
            "charges_paise": figures.charges.paise(),
            "net_paise": figures.net.paise(),
            "by_payment": by_payment,
            "expenses_paise": figures.expenses.paise(),
            "credit_given_paise": figures.credit_given.paise(),
            "credit_collected_paise": figures.credit_collected.paise(),
            "is_day_closed": is_day_closed,
        });
        Ok(Some(WireRow {
            table: "day_totals".to_owned(),
            id: key.to_owned(),
            // The outbox row's moment, NOT zero: the cloud keeps the newest row per key
            // (`where updated_at < excluded.updated_at`), so a totals row stamped 0 was
            // written once and then never updated again — the owner's phone showed the
            // first bill of the day forever.
            updated_at: at,
            deleted: false,
            data,
        }))
    }

    /// One row per item (or category) sold that day.
    pub(crate) fn day_group_totals(&self, outlet: &str, key: &str, at: Timestamp, by: SalesBy) -> Result<Vec<WireRow>, DbError> {
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
                updated_at: at,
                deleted: false,
                data,
            });
        }
        Ok(out)
    }

    // ---------------------------------------------------------------- down

    /// A box row back into its table: only the columns this database has, and only when the
    /// table exists here. Unknown columns are the cloud's problem, not ours; a column this
    /// database has and the row does not takes the table's default.
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

    /// Newest wins per bill: the order here already carries this moment or a later one. An
    /// order's moment is what became of it last — settled, voided or cancelled — the same stamp
    /// its wire row wears.
    fn order_here_is_as_new(&self, id: &str, incoming: Timestamp) -> Result<bool, DbError> {
        let repos = crate::repo::Repos::new(self.tx);
        let Some(order) = repos.orders().find(&OrderId::new(id))? else {
            return Ok(false);
        };
        let here = match &order {
            AnyOrder::Settled(s) => s.settled_at,
            AnyOrder::Voided(v) => v.voided_at,
            AnyOrder::Cancelled(c) => c.cancelled_at,
            AnyOrder::Draft(_) | AnyOrder::Open(_) => return Ok(false),
        };
        Ok(here.millis() >= incoming.millis())
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
        let number = match parsed.bill_number {
            Some(number) => number,
            None => Claimed {
                value: 0,
                formatted: data.get("bill_number").and_then(Value::as_str).unwrap_or("").to_owned(),
                business_day,
            },
        };

        let mut cart = Cart::new();
        for line in lines {
            cart.push(line)
                .map_err(|e| DbError::invariant(format!("bill {id} line could not be restored: {e}")))?;
        }
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

        let order = if let Some(cancel) = parsed.cancel {
            AnyOrder::Cancelled(mb_core::CancelledOrder {
                core,
                token: parsed.token,
                bill_number: Some(number),
                reason: cancel.reason,
                cancelled_at: encode::timestamp_from_sql(cancel.cancelled_at),
                cancelled_by: cancel.cancelled_by,
            })
        } else {
            let Some(input) = parsed.bill_input else {
                return Err(DbError::invariant(format!("bill {id} carries no bill input")));
            };
            let settlement: Settlement =
                serde_json::from_value(data.get("payments").cloned().unwrap_or(Value::Null))
                    .map_err(|e| DbError::invariant(format!("bill {id} payments could not be read back: {e}")))?;
            let bill = mb_core::compute_bill(mb_core::BillInput {
                cart: &core.cart,
                bill_discount: input.bill_discount,
                charges: &input.charges,
                place_of_supply: input.place_of_supply,
                registration: input.registration,
                state_tax: input.state_tax,
                order_type: input.order_type,
                rounding: input.rounding,
            })
            .map_err(|e| DbError::invariant(format!("bill {id} could not be recomputed: {e}")))?;
            // The settling moment and person: the typed columns, or the older restore block
            // that still carried a copy of them.
            let settled_at = parsed
                .settled_at
                .or_else(|| data.get("settled_at").and_then(Value::as_i64))
                .unwrap_or(0);
            let settled_by = parsed
                .settled_by
                .or_else(|| data.get("staff_id").and_then(Value::as_str).map(StaffId::new))
                .ok_or_else(|| DbError::invariant(format!("bill {id} names nobody who settled it")))?;
            let settled = SettledOrder {
                core,
                token: parsed.token,
                bill_number: number,
                bill,
                settlement,
                settled_at: encode::timestamp_from_sql(settled_at),
                settled_by,
            };
            match parsed.void {
                Some(v) => AnyOrder::Voided(
                    settled
                        .void(&v.reason, v.voided_by, encode::timestamp_from_sql(v.voided_at))
                        .map_err(|e| DbError::invariant(format!("bill {id} could not be voided back: {e}")))?,
                ),
                None => AnyOrder::Settled(settled),
            }
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

/// The parts of an order a cloud bill is built from, whatever became of it.
#[derive(Clone, Copy)]
struct Billed<'a> {
    core: &'a OrderCore,
    token: &'a Claimed,
    number: &'a Claimed,
    money: Option<(&'a Bill, &'a Settlement)>,
    staff: &'a StaffId,
    fate: Fate<'a>,
}

/// What became of the order a cloud bill stands for.
#[derive(Clone, Copy)]
enum Fate<'a> {
    Settled { at: Timestamp },
    Voided { settled_at: Timestamp, reason: &'a str, at: Timestamp, by: &'a StaffId },
    Cancelled { reason: &'a str, at: Timestamp, by: &'a StaffId },
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

/// The part of a cloud bill that rebuilds the order. Every optional field is one an older
/// sender wrote or a newer one leaves out; both shapes read.
#[derive(Debug, Deserialize)]
struct Restore {
    placement: Placement,
    #[serde(default)]
    covers: Option<u32>,
    #[serde(default)]
    note: Option<String>,
    created_by: StaffId,
    token: Claimed,
    #[serde(default)]
    bill_number: Option<Claimed>,
    #[serde(default)]
    bill_input: Option<RestoreInput>,
    #[serde(default)]
    settled_at: Option<i64>,
    #[serde(default)]
    settled_by: Option<StaffId>,
    #[serde(default)]
    void: Option<RestoreVoid>,
    #[serde(default)]
    cancel: Option<RestoreCancel>,
}

#[derive(Debug, Deserialize)]
struct RestoreInput {
    #[serde(default)]
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

#[derive(Debug, Deserialize)]
struct RestoreCancel {
    reason: String,
    cancelled_at: i64,
    cancelled_by: StaffId,
}

/// What a restore did with one cloud row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restored {
    Written,
    /// Nothing here to write it into — an item-wise total, or a row from a cloud that predates
    /// the whole-row carry.
    Skipped,
    /// Already here, as new or newer: the 7-day window and the day files overlap, and a bill
    /// that came down twice is one bill.
    Unchanged,
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
        Ok(self.stored_row(table, id)?.map(|mut row| {
            row.insert(ROW_TABLE_KEY.to_owned(), Value::from(table));
            row
        }))
    }

    /// The row as stored, column for column, minus what never leaves.
    fn stored_row(&self, table: &str, id: &str) -> Result<Option<Map<String, Value>>, DbError> {
        if !is_a_table_name(table) {
            return Ok(None);
        }
        let sql = format!(
            "SELECT * FROM \"{table}\" WHERE \"{}\" = ?1",
            key_column(table)
        );
        let Some(mut row) = self.select_one(&sql, [id])? else {
            return Ok(None);
        };
        for secret in NEVER_IN_A_ROW {
            row.remove(*secret);
        }
        Ok(Some(row))
    }

    // ------------------------------------------------------------- restore

    /// One cloud row back into this database, on its own: a row that fails leaves nothing
    /// half-written and the rows around it stand. `table` is the cloud's name for it or the
    /// counter's; `data` is the row as the counter sent it (days and instants already
    /// integers).
    pub fn restore_row(
        &self,
        outlet: &str,
        table: &str,
        id: &str,
        updated_at: Timestamp,
        data: &Value,
    ) -> Result<Restored, DbError> {
        self.tx.execute_batch("SAVEPOINT restore_row")?;
        let outcome = self.restore_row_inside(outlet, table, id, updated_at, data);
        match &outcome {
            Ok(_) => self.tx.execute_batch("RELEASE restore_row")?,
            Err(_) => self.tx.execute_batch("ROLLBACK TO restore_row; RELEASE restore_row")?,
        }
        outcome
    }

    fn restore_row_inside(
        &self,
        outlet: &str,
        table: &str,
        id: &str,
        updated_at: Timestamp,
        data: &Value,
    ) -> Result<Restored, DbError> {
        let written = match table {
            "bills" | "orders" => {
                if self.order_here_is_as_new(id, updated_at)? {
                    return Ok(Restored::Unchanged);
                }
                self.write_order(outlet, id, data)?
            }
            "roles" => {
                let role = cloud_role_from(id, updated_at, data);
                crate::repo::people::PeopleRepo::new(self.tx).apply_role_from_cloud(outlet, &role)?
            }
            "staff" => {
                // The whole row first (address, emergency contact, id proof), then the typed
                // columns, which win when the phone edited them after the counter last pushed.
                let mut written = self.write_row_inside(data)?;
                let staff = cloud_staff_from(id, updated_at, data)?;
                let people = crate::repo::people::PeopleRepo::new(self.tx);
                written |= people.apply_staff_from_cloud(outlet, &staff)?;
                written
            }
            "day_totals" => {
                self.write_day_totals(outlet, updated_at, data)?;
                true
            }
            "day_item_totals" | "day_category_totals" => false,
            // A typed row carries the counter's whole row inside it; a boxed row IS the
            // counter's row.
            _ => match data.get(ROW_KEY) {
                Some(_) => self.write_row_inside(data)?,
                None => self.write_boxed(table, data)?,
            },
        };
        Ok(if written { Restored::Written } else { Restored::Skipped })
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

/// A staff row as the cloud carries it, read the way `mb_pull_payload` writes it. A person
/// the phone "deleted" (`deleted_at` set) comes back as having left: the counter never deletes
/// a staff row, because bills name the people who made them.
pub fn cloud_staff_from(
    id: &str,
    updated_at: Timestamp,
    data: &Value,
) -> Result<crate::repo::people::CloudStaff, DbError> {
    use crate::repo::people::{CloudStaff, StaffStatus};
    let deleted = data.get("deleted_at").is_some_and(|v| !v.is_null())
        || data.get("deleted").and_then(Value::as_bool).unwrap_or(false);
    let status = if deleted {
        StaffStatus::Left
    } else {
        StaffStatus::from_sql(data.get("status").and_then(Value::as_str).unwrap_or("active"))?
    };
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
        phone: str_of(data, "phone"),
        joined_on: day_of_field(data, "joined_on", "staff.joined_on")?,
        status,
        designation: str_of(data, "designation"),
        department: str_of(data, "department"),
        is_rider: data.get("is_rider").and_then(Value::as_bool).unwrap_or(false),
        employment_type,
        left_on: day_of_field(data, "left_on", "staff.left_on")?,
        updated_at,
    })
}

/// A role as the cloud carries it.
#[must_use]
pub fn cloud_role_from(id: &str, updated_at: Timestamp, data: &Value) -> crate::repo::people::CloudRole {
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
        updated_at,
    }
}

// So the id types are the same ones the rest of the crate speaks.
#[allow(dead_code, reason = "named so a reader can see which id a table is keyed by")]
type _Table = TableId;

/// What a restore brought down, row by row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestoreReport {
    pub bills: u32,
    pub rows: u32,
    pub staff: u32,
    pub roles: u32,
    pub days: u32,
    pub skipped: u32,
    /// Rows already here as new or newer — a bill in both the window and its day file.
    pub unchanged: u32,
    /// Rows that would not write, as "table id: why", for the log. Never fatal.
    pub failed: Vec<String>,
}

impl RestoreReport {
    fn count(&mut self, table: &str, outcome: Restored) {
        let slot = match outcome {
            Restored::Skipped => &mut self.skipped,
            Restored::Unchanged => &mut self.unchanged,
            Restored::Written => match table {
                "bills" | "orders" => &mut self.bills,
                "staff" => &mut self.staff,
                "roles" => &mut self.roles,
                "day_totals" => &mut self.days,
                _ => &mut self.rows,
            },
        };
        *slot = slot.saturating_add(1);
    }
}

/// The order rows are written in: parents before the rows that name them. Everything not
/// listed is a boxed master row and goes first.
fn restore_rank(table: &str) -> u8 {
    match table {
        "roles" => 1,
        "staff" => 2,
        "menu_categories" | "categories" => 3,
        "menu_items" | "items" | "customers" | "expense_categories" => 4,
        "customer_ledger" | "credit_adjustments" | "customer_payments" | "expenses" | "cash_movements" => 5,
        "bills" | "orders" => 6,
        "refunds" | "bill_reverts" | "bill_revert_lines" | "bill_revert_payments" => 7,
        "day_totals" | "day_item_totals" | "day_category_totals" => 8,
        _ => 0,
    }
}

impl WireRepo<'_> {
    /// Every row the cloud handed back, into this database in the one order that works:
    /// the box, the people, the masters, the bills, the days. A tombstoned row is skipped —
    /// except a staff member, who comes back as having left, because bills name people.
    /// Masters are written with foreign keys deferred (they name each other in any order);
    /// from the bills on, each row stands or fails on its own.
    pub fn restore_rows(&self, outlet: &str, mut rows: Vec<WireRow>, report: &mut RestoreReport) -> Result<(), DbError> {
        rows.sort_by_key(|r| restore_rank(&r.table));
        self.tx.execute_batch("PRAGMA defer_foreign_keys = ON")?;
        let mut checking = false;
        for row in &rows {
            if !checking && restore_rank(&row.table) >= restore_rank("bills") {
                self.tx.execute_batch("PRAGMA defer_foreign_keys = OFF")?;
                checking = true;
            }
            if row.deleted && row.table != "staff" {
                report.count(&row.table, Restored::Skipped);
                continue;
            }
            match self.restore_row(outlet, &row.table, &row.id, row.updated_at, &row.data) {
                Ok(outcome) => report.count(&row.table, outcome),
                // One row that will not write must not lose the other nine thousand.
                Err(e) => {
                    report.skipped = report.skipped.saturating_add(1);
                    report.failed.push(format!("{} {}: {e}", row.table, row.id));
                }
            }
        }
        Ok(())
    }
}
