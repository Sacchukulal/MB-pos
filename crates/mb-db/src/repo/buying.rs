use std::collections::BTreeMap;

use mb_core::credit::{self, Ageing};
use mb_core::purchase::Invoice;
use mb_core::{BusinessDay, MaterialId, Money, Qty, StaffId, Timestamp, UnitCost};
use rusqlite::{Transaction, params};

use crate::encode;
use crate::error::DbError;
use crate::repo::outbox::{Op, OutboxRepo};
use crate::repo::stock::{Movement, MovementKind, StockRepo};

/// Who you buy from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Supplier {
    pub id: String,
    pub name: String,
    pub phone: Option<String>,
    pub gstin: Option<String>,
    pub address: Option<String>,
    /// Days. Zero means cash-and-carry: due the day it arrives.
    pub terms_days: u32,
    pub note: Option<String>,
    pub is_active: bool,
}

impl Supplier {
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Supplier {
            id: id.into(),
            name: name.into(),
            phone: None,
            gstin: None,
            address: None,
            terms_days: 0,
            note: None,
            is_active: true,
        }
    }

    /// When this invoice falls due.
    #[must_use]
    pub fn due_day(&self, received: BusinessDay) -> BusinessDay {
        BusinessDay::from_days_since_epoch(
            received
                .days_since_epoch()
                .saturating_add(i32::try_from(self.terms_days).unwrap_or(0)),
        )
    }
}

/// What a supplier sells and what they last charged for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupplierMaterial {
    pub material_id: MaterialId,
    pub material_name: String,
    /// The pack the rate is quoted in, by name.
    pub pack: String,
    pub last_rate: Money,
    pub last_bought_day: Option<BusinessDay>,
}

/// A delivery, or goods going back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurchaseKind {
    Purchase,
    /// Its own document pointing at its parent, never a negative line edited onto the original,
    /// because the original is a photograph of something that happened.
    Return,
}

impl PurchaseKind {
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            PurchaseKind::Purchase => "purchase",
            PurchaseKind::Return => "return",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            PurchaseKind::Purchase => "Delivery",
            PurchaseKind::Return => "Sent back",
        }
    }

    pub fn from_tag(tag: &str) -> Result<Self, DbError> {
        match tag {
            "purchase" => Ok(PurchaseKind::Purchase),
            "return" => Ok(PurchaseKind::Return),
            other => Err(DbError::invariant(format!(
                "purchases.kind holds an unknown value `{other}`"
            ))),
        }
    }
}

/// One line of the paper, with the landed cost that came out of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseLine {
    pub seq: i64,
    pub material_id: MaterialId,
    /// Filled in when read back, for every screen and document.
    pub material_name: String,
    /// Both numbers.
    pub typed_qty: Qty,
    pub typed_unit: String,
    pub base_qty: Qty,
    pub free_typed_qty: Qty,
    pub free_base_qty: Qty,
    /// Paise per whole typed unit.
    pub rate: Money,
    pub line_value: Money,
    pub discount: Money,
    pub tax_rate_bp: u32,
    pub tax_amount: Money,
    pub charge_share: Money,
    pub discount_share: Money,
    pub landed_value: Money,
    pub landed_unit_cost: UnitCost,
    /// The ledger row this line wrote.
    pub movement_id: Option<String>,
    /// On a return line: which line of the parent invoice is going back.
    pub returns_seq: Option<i64>,
}

impl PurchaseLine {
    /// Everything that came off the vehicle — charged plus free.
    pub fn received(&self) -> Qty {
        self.base_qty
            .add(self.free_base_qty)
            .unwrap_or(self.base_qty)
    }
}

/// The paper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Purchase {
    pub id: String,
    pub supplier_id: String,
    /// Filled in when read back.
    pub supplier_name: String,
    pub kind: PurchaseKind,
    pub parent_id: Option<String>,
    pub invoice_no: Option<String>,
    pub business_day: BusinessDay,
    pub received_at: Timestamp,
    pub due_day: BusinessDay,
    pub lines_value: Money,
    pub line_discounts: Money,
    pub invoice_discount: Money,
    pub charges: Money,
    pub tax_total: Money,
    /// How much of that tax the shop may actually claim back.
    pub tax_creditable: Money,
    pub round_off: Money,
    pub total: Money,
    /// What the paper's own total line said, when somebody typed it.
    pub stated_total: Option<Money>,
    pub po_id: Option<String>,
    pub attachment_id: Option<String>,
    pub note: Option<String>,
    pub created_by: Option<StaffId>,
    /// The only correction path a purchase has.
    pub cancelled: Option<Cancellation>,
    pub lines: Vec<PurchaseLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cancellation {
    pub at: Timestamp,
    pub by: Option<StaffId>,
    pub reason: String,
}

impl Purchase {
    /// A blank delivery, for a caller that is about to fill the lines in.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        supplier_id: impl Into<String>,
        business_day: BusinessDay,
        received_at: Timestamp,
    ) -> Self {
        Purchase {
            id: id.into(),
            supplier_id: supplier_id.into(),
            supplier_name: String::new(),
            kind: PurchaseKind::Purchase,
            parent_id: None,
            invoice_no: None,
            business_day,
            received_at,
            due_day: business_day,
            lines_value: Money::ZERO,
            line_discounts: Money::ZERO,
            invoice_discount: Money::ZERO,
            charges: Money::ZERO,
            tax_total: Money::ZERO,
            tax_creditable: Money::ZERO,
            round_off: Money::ZERO,
            total: Money::ZERO,
            stated_total: None,
            po_id: None,
            attachment_id: None,
            note: None,
            created_by: None,
            cancelled: None,
            lines: Vec::new(),
        }
    }

    /// The sign a return carries into every ledger.
    #[must_use]
    pub const fn direction(&self) -> i64 {
        match self.kind {
            PurchaseKind::Purchase => 1,
            PurchaseKind::Return => -1,
        }
    }
}

/// Turn what somebody typed into the rows that will be written — the one place the paper
/// becomes a document, used by the command and by the tests.
pub fn draft(
    id: impl Into<String>,
    supplier: &Supplier,
    day: BusinessDay,
    at: Timestamp,
    materials: &[(MaterialId, String)],
    invoice: &Invoice,
) -> Result<Purchase, DbError> {
    if materials.len() != invoice.lines.len() {
        return Err(DbError::invariant("every line needs a material against it"));
    }
    let costed =
        mb_core::purchase::cost_invoice(invoice).map_err(|e| DbError::invariant(e.to_string()))?;

    let mut purchase = Purchase::new(id, supplier.id.clone(), day, at);
    purchase.supplier_name = supplier.name.clone();
    purchase.due_day = supplier.due_day(day);
    purchase.lines_value = costed.lines_value;
    purchase.line_discounts = costed.line_discounts;
    purchase.invoice_discount = costed.invoice_discount;
    purchase.charges = costed.charges;
    purchase.tax_total = costed.tax_total;
    purchase.tax_creditable = costed.tax_creditable;
    purchase.round_off = costed.round_off;
    purchase.total = costed.total;

    for (index, line) in costed.lines.iter().enumerate() {
        let (material, unit) = &materials[index];
        let entry = &invoice.lines[index];
        purchase.lines.push(PurchaseLine {
            seq: i64::try_from(index).unwrap_or(0) + 1,
            material_id: material.clone(),
            material_name: String::new(),
            typed_qty: entry.typed_qty,
            typed_unit: unit.clone(),
            base_qty: line.base_qty,
            free_typed_qty: entry.free_typed_qty,
            free_base_qty: line.free_base_qty,
            rate: entry.rate,
            line_value: line.line_value,
            discount: entry.discount,
            tax_rate_bp: entry.tax_rate_bp,
            tax_amount: line.tax_amount,
            charge_share: line.charge_share,
            discount_share: line.discount_share,
            landed_value: line.landed_value,
            landed_unit_cost: line.landed_unit_cost,
            movement_id: None,
            returns_seq: None,
        });
    }
    Ok(purchase)
}

/// Money handed to a supplier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupplierPayment {
    pub id: String,
    pub supplier_id: String,
    pub amount: Money,
    /// `cash`, `bank`, `upi` or `card`.
    pub mode: String,
    pub reference: Option<String>,
    /// Set when the money went over with the delivery.
    pub purchase_id: Option<String>,
    pub paid_at: Timestamp,
    pub business_day: BusinessDay,
    pub paid_by: Option<StaffId>,
    pub note: Option<String>,
}

/// An opening balance, a write-off, or a correction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupplierAdjustment {
    pub id: String,
    pub supplier_id: String,
    /// Always positive; `increases` is the direction.
    pub amount: Money,
    pub increases: bool,
    pub reason: String,
    pub at: Timestamp,
    pub business_day: BusinessDay,
    pub made_by: Option<StaffId>,
}

/// One line of "who am I overdue with".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outstanding {
    pub supplier: Supplier,
    pub balance: Money,
    /// Customer ageing fed the DUE date, so "current" means not due yet and the 90-day bucket
    /// means ninety days overdue.
    pub ageing: Ageing,
    pub last_movement: BusinessDay,
}

/// Where a purchase order has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderState {
    Draft,
    Sent,
    Received,
    Closed,
    Cancelled,
}

impl OrderState {
    pub const ALL: &'static [OrderState] = &[
        OrderState::Draft,
        OrderState::Sent,
        OrderState::Received,
        OrderState::Closed,
        OrderState::Cancelled,
    ];

    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            OrderState::Draft => "draft",
            OrderState::Sent => "sent",
            OrderState::Received => "received",
            OrderState::Closed => "closed",
            OrderState::Cancelled => "cancelled",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            OrderState::Draft => "Being written",
            OrderState::Sent => "Sent to the supplier",
            OrderState::Received => "Partly received",
            OrderState::Closed => "Finished",
            OrderState::Cancelled => "Cancelled",
        }
    }

    pub fn from_tag(tag: &str) -> Result<Self, DbError> {
        OrderState::ALL
            .iter()
            .copied()
            .find(|s| s.tag() == tag)
            .ok_or_else(|| {
                DbError::invariant(format!(
                    "purchase_orders.state holds an unknown value `{tag}`"
                ))
            })
    }
}

/// A purchase order is optional, and the proof is that nothing reads one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseOrder {
    pub id: String,
    pub supplier_id: String,
    pub supplier_name: String,
    pub number: String,
    pub state: OrderState,
    pub expected_day: Option<BusinessDay>,
    pub note: Option<String>,
    pub created_at: Timestamp,
    pub created_by: Option<StaffId>,
    pub sent_at: Option<Timestamp>,
    pub closed_at: Option<Timestamp>,
    pub lines: Vec<OrderLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderLine {
    pub seq: i64,
    pub material_id: MaterialId,
    pub material_name: String,
    pub typed_qty: Qty,
    pub typed_unit: String,
    pub base_qty: Qty,
    pub rate: Money,
}

/// One row of a buying report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuyingRow {
    /// The stable key â a material id, so the screen can say the quantity in the unit a
    /// person uses.
    pub key: String,
    pub label: String,
    /// How many invoices are behind this row.
    pub count: i64,
    /// Only for the material report; `None` elsewhere, because a quantity of "Metro" is not a
    /// thing.
    pub qty: Option<Qty>,
    pub unit: String,
    pub value: Money,
    pub tax: Money,
}

/// What one material has been costing — and the rise is the finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceTrendRow {
    pub material: MaterialId,
    pub name: String,
    pub unit: String,
    pub deliveries: i64,
    pub cheapest: UnitCost,
    pub dearest: UnitCost,
    pub average: UnitCost,
    pub latest: UnitCost,
}

impl PriceTrendRow {
    /// How far the last delivery is above or below the period's average, in basis points.
    #[must_use]
    #[allow(
        clippy::integer_division,
        reason = "a percentage in basis points IS a division; the guard above \
                  is the only case that could lose anything"
    )]
    pub fn change_bp(&self) -> Option<i64> {
        if self.average.is_zero() || self.deliveries < 2 {
            return None;
        }
        let gap = i128::from(self.latest.paise_per_thousand())
            - i128::from(self.average.paise_per_thousand());
        i64::try_from(gap * 10_000 / i128::from(self.average.paise_per_thousand())).ok()
    }
}

/// The photograph's metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub id: String,
    pub kind: String,
    pub subject_id: Option<String>,
    /// `<sha256>.jpg`. Stored rather than derived, so a future format change cannot orphan
    /// today's files.
    pub filename: String,
    pub byte_count: i64,
    pub sha256: String,
    pub created_at: Timestamp,
    pub created_by: Option<StaffId>,
}

#[derive(Debug)]
pub struct BuyingRepo<'a> {
    tx: &'a Transaction<'a>,
}

const SUPPLIER_COLUMNS: &str = "id, name, phone, gstin, address, terms_days, note, is_active";

const PURCHASE_COLUMNS: &str = "p.id, p.supplier_id, s.name, p.kind, p.parent_id, p.invoice_no, \
     p.business_day, p.received_at, p.due_day, p.lines_value, p.line_discounts, \
     p.invoice_discount, p.charges, p.tax_total, p.tax_creditable, p.round_off, p.total, \
     p.stated_total, p.po_id, p.attachment_id, p.note, p.created_by, p.is_cancelled, \
     p.cancelled_at, p.cancelled_by, p.cancel_reason";

impl<'a> BuyingRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        BuyingRepo { tx }
    }

    /// Add or change a supplier.
    pub fn save_supplier(
        &self,
        outlet: &str,
        supplier: &Supplier,
        at: Timestamp,
    ) -> Result<(), DbError> {
        if supplier.name.trim().is_empty() {
            return Err(DbError::invariant("a supplier needs a name"));
        }
        self.tx.execute(
            "INSERT INTO suppliers
                 (id, outlet_id, name, phone, gstin, address, terms_days, note, is_active,
                  created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT (id) DO UPDATE SET
                 name = excluded.name,
                 phone = excluded.phone,
                 gstin = excluded.gstin,
                 address = excluded.address,
                 terms_days = excluded.terms_days,
                 note = excluded.note,
                 is_active = excluded.is_active",
            params![
                supplier.id,
                outlet,
                supplier.name.trim(),
                supplier.phone,
                supplier.gstin,
                supplier.address,
                i64::from(supplier.terms_days),
                supplier.note,
                encode::bool_to_sql(supplier.is_active),
                encode::timestamp_to_sql(at),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "suppliers", &supplier.id, Op::Upsert, at)
    }

    pub fn suppliers(&self, outlet: &str, include_retired: bool) -> Result<Vec<Supplier>, DbError> {
        let sql = format!(
            "SELECT {SUPPLIER_COLUMNS} FROM suppliers
              WHERE outlet_id = ?1 AND (?2 = 1 OR is_active = 1)
              ORDER BY name"
        );
        let mut stmt = self.tx.prepare(&sql)?;
        let rows = stmt.query_map(params![outlet, i64::from(include_retired)], read_supplier)?;
        rows.collect::<Result<_, _>>().map_err(DbError::from)
    }

    pub fn supplier(&self, outlet: &str, id: &str) -> Result<Option<Supplier>, DbError> {
        let sql =
            format!("SELECT {SUPPLIER_COLUMNS} FROM suppliers WHERE outlet_id = ?1 AND id = ?2");
        let mut stmt = self.tx.prepare(&sql)?;
        let mut rows = stmt.query_map(params![outlet, id], read_supplier)?;
        rows.next().transpose().map_err(DbError::from)
    }

    /// What this supplier sells, with the rate they last charged.
    pub fn supplier_materials(&self, supplier: &str) -> Result<Vec<SupplierMaterial>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT sm.material_id, m.name, sm.pack, sm.last_rate, sm.last_bought_day
               FROM supplier_materials sm
               JOIN materials m ON m.id = sm.material_id
              WHERE sm.supplier_id = ?1
              ORDER BY m.name",
        )?;
        let mut cursor = stmt.query([supplier])?;
        let mut out = Vec::new();
        while let Some(row) = cursor.next()? {
            out.push(SupplierMaterial {
                material_id: MaterialId::new(row.get::<_, String>(0)?),
                material_name: row.get(1)?,
                pack: row.get(2)?,
                last_rate: encode::money_from_sql(row.get(3)?),
                last_bought_day: row
                    .get::<_, Option<i64>>(4)?
                    .map(|d| encode::business_day_from_sql(d, "supplier_materials.last_bought_day"))
                    .transpose()?,
            });
        }
        Ok(out)
    }

    /// Remember what a supplier charges for a material, so the next purchase line pre-fills.
    pub fn remember_price(
        &self,
        supplier: &str,
        material: &MaterialId,
        pack: &str,
        rate: Money,
        day: BusinessDay,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO supplier_materials
                 (supplier_id, material_id, pack, last_rate, last_bought_day)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (supplier_id, material_id) DO UPDATE SET
                 pack = excluded.pack,
                 last_rate = excluded.last_rate,
                 last_bought_day = excluded.last_bought_day",
            params![
                supplier,
                material.as_str(),
                pack,
                encode::money_to_sql(rate),
                encode::business_day_to_sql(day),
            ],
        )?;
        Ok(())
    }

    // THE PAPER.

    /// Write a delivery: the paper, the shelf and the ledger, in one transaction.
    pub fn record_purchase(&self, outlet: &str, purchase: &Purchase) -> Result<Purchase, DbError> {
        if purchase.lines.is_empty() {
            return Err(DbError::invariant(
                "a purchase with no lines is not a purchase",
            ));
        }
        if purchase.kind == PurchaseKind::Return && purchase.parent_id.is_none() {
            return Err(DbError::invariant(
                "a return has to say which delivery it goes back on",
            ));
        }

        let stock = StockRepo::new(self.tx);
        let direction = purchase.direction();
        let mut written = purchase.clone();

        self.tx.execute(
            "INSERT INTO purchases
                 (id, outlet_id, supplier_id, kind, parent_id, invoice_no, business_day,
                  received_at, due_day, lines_value, line_discounts, invoice_discount, charges,
                  tax_total, tax_creditable, round_off, total, stated_total, po_id, attachment_id,
                  note, created_by, is_cancelled, cancelled_at, cancelled_by, cancel_reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                     ?18, ?19, ?20, ?21, ?22, 0, NULL, NULL, NULL)",
            params![
                purchase.id,
                outlet,
                purchase.supplier_id,
                purchase.kind.tag(),
                purchase.parent_id,
                purchase.invoice_no,
                encode::business_day_to_sql(purchase.business_day),
                encode::timestamp_to_sql(purchase.received_at),
                encode::business_day_to_sql(purchase.due_day),
                encode::money_to_sql(purchase.lines_value),
                encode::money_to_sql(purchase.line_discounts),
                encode::money_to_sql(purchase.invoice_discount),
                encode::money_to_sql(purchase.charges),
                encode::money_to_sql(purchase.tax_total),
                encode::money_to_sql(purchase.tax_creditable),
                encode::money_to_sql(purchase.round_off),
                encode::money_to_sql(purchase.total),
                purchase.stated_total.map(encode::money_to_sql),
                purchase.po_id,
                purchase.attachment_id,
                purchase.note,
                purchase.created_by.as_ref().map(StaffId::as_str),
            ],
        )?;

        for line in &mut written.lines {
            let movement_id = format!("mov_{}_{}", purchase.id, line.seq);
            let signed =
                Qty::from_thousandths(line.received().thousandths().saturating_mul(direction));
            let movement = Movement {
                note: purchase.invoice_no.clone(),
                ..Movement::new(
                    movement_id.clone(),
                    line.material_id.clone(),
                    MovementKind::Purchase,
                    signed,
                    purchase.received_at,
                    purchase.business_day,
                )
            }
            .typed(
                Qty::from_thousandths(
                    line.typed_qty
                        .add(line.free_typed_qty)
                        .unwrap_or(line.typed_qty)
                        .thousandths()
                        .saturating_mul(direction),
                ),
                line.typed_unit.clone(),
            )
            .costing(line.landed_unit_cost);
            let movement = match purchase.created_by.clone() {
                Some(staff) => movement.by(staff),
                None => movement,
            };
            stock.record(outlet, &movement)?;
            line.movement_id = Some(movement_id.clone());

            self.tx.execute(
                "INSERT INTO purchase_lines
                     (purchase_id, seq, material_id, typed_qty, typed_unit, base_qty,
                      free_typed_qty, free_base_qty, rate, line_value, discount, tax_rate_bp,
                      tax_amount, charge_share, discount_share, landed_value, landed_unit_cost,
                      movement_id, returns_seq)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                         ?17, ?18, ?19)",
                params![
                    purchase.id,
                    line.seq,
                    line.material_id.as_str(),
                    encode::qty_to_sql(line.typed_qty),
                    line.typed_unit,
                    encode::qty_to_sql(line.base_qty),
                    encode::qty_to_sql(line.free_typed_qty),
                    encode::qty_to_sql(line.free_base_qty),
                    encode::money_to_sql(line.rate),
                    encode::money_to_sql(line.line_value),
                    encode::money_to_sql(line.discount),
                    i64::from(line.tax_rate_bp),
                    encode::money_to_sql(line.tax_amount),
                    encode::money_to_sql(line.charge_share),
                    encode::money_to_sql(line.discount_share),
                    encode::money_to_sql(line.landed_value),
                    line.landed_unit_cost.paise_per_thousand(),
                    movement_id,
                    line.returns_seq,
                ],
            )?;

            if purchase.kind == PurchaseKind::Purchase && line.rate.is_positive() {
                self.remember_price(
                    &purchase.supplier_id,
                    &line.material_id,
                    &line.typed_unit,
                    line.rate,
                    purchase.business_day,
                )?;
            }
        }

        OutboxRepo::new(self.tx).enqueue(
            outlet,
            "purchases",
            &purchase.id,
            Op::Upsert,
            purchase.received_at,
        )?;
        Ok(written)
    }

    /// The only correction path a purchase has.
    pub fn cancel_purchase(
        &self,
        outlet: &str,
        id: &str,
        reason: &str,
        by: Option<&StaffId>,
        at: Timestamp,
    ) -> Result<(), DbError> {
        if reason.trim().is_empty() {
            return Err(DbError::invariant("a cancellation needs a reason"));
        }
        let Some(purchase) = self.purchase(outlet, id)? else {
            return Err(DbError::invariant("that delivery is not on file"));
        };
        if purchase.cancelled.is_some() {
            return Err(DbError::invariant("that delivery was already cancelled"));
        }

        let stock = StockRepo::new(self.tx);
        for line in &purchase.lines {
            let Some(original) = &line.movement_id else {
                continue;
            };
            let mut reversal = Movement::new(
                format!("mov_cx_{}_{}", purchase.id, line.seq),
                line.material_id.clone(),
                MovementKind::Purchase,
                Qty::from_thousandths(
                    line.received()
                        .thousandths()
                        .saturating_mul(-purchase.direction()),
                ),
                at,
                purchase.business_day,
            )
            // At the cost it came in at, never today's average.
            .costing(line.landed_unit_cost);
            reversal.reverses_id = Some(original.clone());
            reversal.note = Some(format!("Cancelled: {}", reason.trim()));
            reversal.typed_unit = line.typed_unit.clone();
            reversal.typed_qty = Qty::from_thousandths(
                line.typed_qty
                    .add(line.free_typed_qty)
                    .unwrap_or(line.typed_qty)
                    .thousandths()
                    .saturating_mul(-purchase.direction()),
            );
            if let Some(staff) = by {
                reversal.staff = Some(staff.clone());
            }
            stock.record(outlet, &reversal)?;
        }

        self.tx.execute(
            "UPDATE purchases
                SET is_cancelled = 1, cancelled_at = ?2, cancelled_by = ?3, cancel_reason = ?4
              WHERE id = ?1",
            params![
                id,
                encode::timestamp_to_sql(at),
                by.map(StaffId::as_str),
                reason.trim(),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(outlet, "purchases", id, Op::Upsert, at)
    }

    /// One document, with its lines.
    pub fn purchase(&self, outlet: &str, id: &str) -> Result<Option<Purchase>, DbError> {
        let sql = format!(
            "SELECT {PURCHASE_COLUMNS} FROM purchases p
               JOIN suppliers s ON s.id = p.supplier_id
              WHERE p.outlet_id = ?1 AND p.id = ?2"
        );
        let mut stmt = self.tx.prepare(&sql)?;
        let mut rows = stmt.query_map(params![outlet, id], read_purchase)?;
        let Some(mut purchase) = rows.next().transpose()? else {
            return Ok(None);
        };
        drop(rows);
        purchase.lines = self.lines_of(&purchase.id)?;
        Ok(Some(purchase))
    }

    fn lines_of(&self, purchase: &str) -> Result<Vec<PurchaseLine>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT pl.seq, pl.material_id, m.name, pl.typed_qty, pl.typed_unit, pl.base_qty,
                    pl.free_typed_qty, pl.free_base_qty, pl.rate, pl.line_value, pl.discount,
                    pl.tax_rate_bp, pl.tax_amount, pl.charge_share, pl.discount_share,
                    pl.landed_value, pl.landed_unit_cost, pl.movement_id, pl.returns_seq
               FROM purchase_lines pl
               JOIN materials m ON m.id = pl.material_id
              WHERE pl.purchase_id = ?1
              ORDER BY pl.seq",
        )?;
        let rows = stmt.query_map([purchase], |row| {
            Ok(PurchaseLine {
                seq: row.get(0)?,
                material_id: MaterialId::new(row.get::<_, String>(1)?),
                material_name: row.get(2)?,
                typed_qty: encode::qty_from_sql(row.get(3)?),
                typed_unit: row.get(4)?,
                base_qty: encode::qty_from_sql(row.get(5)?),
                free_typed_qty: encode::qty_from_sql(row.get(6)?),
                free_base_qty: encode::qty_from_sql(row.get(7)?),
                rate: encode::money_from_sql(row.get(8)?),
                line_value: encode::money_from_sql(row.get(9)?),
                discount: encode::money_from_sql(row.get(10)?),
                tax_rate_bp: u32::try_from(row.get::<_, i64>(11)?).unwrap_or(0),
                tax_amount: encode::money_from_sql(row.get(12)?),
                charge_share: encode::money_from_sql(row.get(13)?),
                discount_share: encode::money_from_sql(row.get(14)?),
                landed_value: encode::money_from_sql(row.get(15)?),
                landed_unit_cost: UnitCost::from_paise_per_thousand(row.get(16)?),
                movement_id: row.get(17)?,
                returns_seq: row.get(18)?,
            })
        })?;
        rows.collect::<Result<_, _>>().map_err(DbError::from)
    }

    /// Every document in a period, newest first.
    pub fn purchases(
        &self,
        outlet: &str,
        from: BusinessDay,
        to: BusinessDay,
        supplier: Option<&str>,
    ) -> Result<Vec<Purchase>, DbError> {
        let sql = format!(
            "SELECT {PURCHASE_COLUMNS} FROM purchases p
               JOIN suppliers s ON s.id = p.supplier_id
              WHERE p.outlet_id = ?1 AND p.business_day BETWEEN ?2 AND ?3
                AND (?4 IS NULL OR p.supplier_id = ?4)
              ORDER BY p.business_day DESC, p.received_at DESC"
        );
        let mut stmt = self.tx.prepare(&sql)?;
        let rows = stmt.query_map(
            params![
                outlet,
                encode::business_day_to_sql(from),
                encode::business_day_to_sql(to),
                supplier,
            ],
            read_purchase,
        )?;
        let mut out: Vec<Purchase> = rows.collect::<Result<_, _>>()?;
        drop(stmt);
        for purchase in &mut out {
            purchase.lines = self.lines_of(&purchase.id)?;
        }
        Ok(out)
    }

    /// How much of one delivery's line is still on the shelf to send back.
    pub fn returnable(&self, purchase: &str, seq: i64) -> Result<Qty, DbError> {
        let bought: i64 = self
            .tx
            .query_row(
                "SELECT base_qty + free_base_qty FROM purchase_lines
                  WHERE purchase_id = ?1 AND seq = ?2",
                params![purchase, seq],
                |row| row.get(0),
            )
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(0),
                other => Err(other),
            })?;
        let returned: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(rl.base_qty + rl.free_base_qty), 0)
               FROM purchase_lines rl
               JOIN purchases r ON r.id = rl.purchase_id
              WHERE r.parent_id = ?1 AND r.kind = 'return' AND r.is_cancelled = 0
                AND rl.returns_seq = ?2",
            params![purchase, seq],
            |row| row.get(0),
        )?;
        Ok(encode::qty_from_sql(bought.saturating_sub(returned)))
    }

    // THE SUPPLIER LEDGER.

    pub fn record_payment(&self, outlet: &str, payment: &SupplierPayment) -> Result<(), DbError> {
        if !payment.amount.is_positive() {
            return Err(DbError::invariant("a payment of nothing is not a payment"));
        }
        self.tx.execute(
            "INSERT INTO supplier_payments
                 (id, outlet_id, supplier_id, amount, mode, reference, purchase_id, paid_at,
                  business_day, paid_by, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                payment.id,
                outlet,
                payment.supplier_id,
                encode::money_to_sql(payment.amount),
                payment.mode,
                payment.reference,
                payment.purchase_id,
                encode::timestamp_to_sql(payment.paid_at),
                encode::business_day_to_sql(payment.business_day),
                payment.paid_by.as_ref().map(StaffId::as_str),
                payment.note,
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(
            outlet,
            "supplier_payments",
            &payment.id,
            Op::Upsert,
            payment.paid_at,
        )
    }

    pub fn save_adjustment(
        &self,
        outlet: &str,
        adjustment: &SupplierAdjustment,
    ) -> Result<(), DbError> {
        if adjustment.reason.trim().is_empty() {
            return Err(DbError::invariant("an adjustment needs a reason"));
        }
        if !adjustment.amount.is_positive() {
            return Err(DbError::invariant(
                "an adjustment of nothing changes nothing",
            ));
        }
        self.tx.execute(
            "INSERT INTO supplier_adjustments
                 (id, outlet_id, supplier_id, amount, increases, reason, at, business_day, made_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                adjustment.id,
                outlet,
                adjustment.supplier_id,
                encode::money_to_sql(adjustment.amount),
                encode::bool_to_sql(adjustment.increases),
                adjustment.reason.trim(),
                encode::timestamp_to_sql(adjustment.at),
                encode::business_day_to_sql(adjustment.business_day),
                adjustment.made_by.as_ref().map(StaffId::as_str),
            ],
        )?;
        OutboxRepo::new(self.tx).enqueue(
            outlet,
            "supplier_adjustments",
            &adjustment.id,
            Op::Upsert,
            adjustment.at,
        )
    }

    /// The account, as movements — and it is `mb_core::credit::Movement`, the very type a
    /// customer's account uses.
    pub fn supplier_ledger(&self, supplier: &str) -> Result<Vec<credit::Movement>, DbError> {
        let mut out = Vec::new();

        let mut stmt = self.tx.prepare(
            "SELECT due_day, total, kind, invoice_no, id FROM purchases
              WHERE supplier_id = ?1 AND is_cancelled = 0
              ORDER BY due_day",
        )?;
        let mut cursor = stmt.query([supplier])?;
        while let Some(row) = cursor.next()? {
            let day = encode::business_day_from_sql(row.get(0)?, "purchases.due_day")?;
            let amount = encode::money_from_sql(row.get(1)?);
            let kind = PurchaseKind::from_tag(&row.get::<_, String>(2)?)?;
            let invoice: Option<String> = row.get(3)?;
            let id: String = row.get(4)?;
            let note = invoice.unwrap_or(id);
            out.push(match kind {
                // A delivery increases what the shop owes.
                PurchaseKind::Purchase => credit::Movement {
                    day,
                    kind: credit::MovementKind::Sale,
                    amount,
                    note,
                },
                // Goods going back reduce it, and they are an adjustment rather than a
                // repayment because no money moved.
                PurchaseKind::Return => credit::Movement {
                    day,
                    kind: credit::MovementKind::Adjustment { increases: false },
                    amount,
                    note: format!("Sent back — {note}"),
                },
            });
        }
        drop(cursor);
        drop(stmt);

        let mut stmt = self.tx.prepare(
            "SELECT business_day, amount, mode, reference FROM supplier_payments
              WHERE supplier_id = ?1 ORDER BY business_day",
        )?;
        let mut cursor = stmt.query([supplier])?;
        while let Some(row) = cursor.next()? {
            let reference: Option<String> = row.get(3)?;
            out.push(credit::Movement {
                day: encode::business_day_from_sql(row.get(0)?, "supplier_payments.business_day")?,
                kind: credit::MovementKind::Repayment,
                amount: encode::money_from_sql(row.get(1)?),
                note: match reference {
                    Some(reference) => format!("{} — {reference}", row.get::<_, String>(2)?),
                    None => row.get::<_, String>(2)?,
                },
            });
        }
        drop(cursor);
        drop(stmt);

        let mut stmt = self.tx.prepare(
            "SELECT business_day, amount, increases, reason FROM supplier_adjustments
              WHERE supplier_id = ?1 ORDER BY business_day",
        )?;
        let mut cursor = stmt.query([supplier])?;
        while let Some(row) = cursor.next()? {
            out.push(credit::Movement {
                day: encode::business_day_from_sql(
                    row.get(0)?,
                    "supplier_adjustments.business_day",
                )?,
                kind: credit::MovementKind::Adjustment {
                    increases: encode::bool_from_sql(
                        row.get(2)?,
                        "supplier_adjustments.increases",
                    )?,
                },
                amount: encode::money_from_sql(row.get(1)?),
                note: row.get(3)?,
            });
        }

        out.sort_by_key(|m| m.day.days_since_epoch());
        Ok(out)
    }

    /// A `SUM` every time.
    pub fn supplier_balance(&self, supplier: &str) -> Result<Money, DbError> {
        let movements = self.supplier_ledger(supplier)?;
        credit::balance(&movements)
            .map_err(|e| DbError::invariant(format!("that account cannot be totalled: {e}")))
    }

    /// "Who am I overdue with".
    pub fn outstanding(
        &self,
        outlet: &str,
        today: BusinessDay,
    ) -> Result<Vec<Outstanding>, DbError> {
        let mut out = Vec::new();
        for supplier in self.suppliers(outlet, true)? {
            let movements = self.supplier_ledger(&supplier.id)?;
            if movements.is_empty() {
                continue;
            }
            let balance = credit::balance(&movements)
                .map_err(|e| DbError::invariant(format!("that account cannot be totalled: {e}")))?;
            if balance.is_zero() {
                continue;
            }
            let ageing = credit::ageing(&movements, today)
                .map_err(|e| DbError::invariant(format!("that account cannot be aged: {e}")))?;
            let last_movement = movements
                .iter()
                .map(|m| m.day)
                .max_by_key(|d| d.days_since_epoch())
                .unwrap_or(today);
            out.push(Outstanding {
                supplier,
                balance,
                ageing,
                last_movement,
            });
        }
        // The oldest money first, which is the order somebody pays in.
        out.sort_by_key(|o| o.ageing.oldest_days.map(|d| -d).unwrap_or(0));
        Ok(out)
    }

    /// What the drawer paid out to suppliers.
    pub fn cash_paid_out(&self, outlet: &str, day: BusinessDay) -> Result<Money, DbError> {
        let paise: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM supplier_payments
              WHERE outlet_id = ?1 AND business_day = ?2 AND mode = 'cash'",
            params![outlet, encode::business_day_to_sql(day)],
            |row| row.get(0),
        )?;
        Ok(encode::money_from_sql(paise))
    }

    // PURCHASE ORDERS.

    pub fn save_order(&self, outlet: &str, order: &PurchaseOrder) -> Result<(), DbError> {
        if order.number.trim().is_empty() {
            return Err(DbError::invariant("a purchase order needs a number"));
        }
        self.tx.execute(
            "INSERT INTO purchase_orders
                 (id, outlet_id, supplier_id, number, state, expected_day, note, created_at,
                  created_by, sent_at, closed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT (id) DO UPDATE SET
                 supplier_id = excluded.supplier_id,
                 number = excluded.number,
                 state = excluded.state,
                 expected_day = excluded.expected_day,
                 note = excluded.note,
                 sent_at = excluded.sent_at,
                 closed_at = excluded.closed_at",
            params![
                order.id,
                outlet,
                order.supplier_id,
                order.number.trim(),
                order.state.tag(),
                order.expected_day.map(encode::business_day_to_sql),
                order.note,
                encode::timestamp_to_sql(order.created_at),
                order.created_by.as_ref().map(StaffId::as_str),
                order.sent_at.map(encode::timestamp_to_sql),
                order.closed_at.map(encode::timestamp_to_sql),
            ],
        )?;
        self.tx.execute(
            "DELETE FROM purchase_order_lines WHERE po_id = ?1",
            [&order.id],
        )?;
        for line in &order.lines {
            self.tx.execute(
                "INSERT INTO purchase_order_lines
                     (po_id, seq, material_id, typed_qty, typed_unit, base_qty, rate)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    order.id,
                    line.seq,
                    line.material_id.as_str(),
                    encode::qty_to_sql(line.typed_qty),
                    line.typed_unit,
                    encode::qty_to_sql(line.base_qty),
                    encode::money_to_sql(line.rate),
                ],
            )?;
        }
        OutboxRepo::new(self.tx).enqueue(
            outlet,
            "purchase_orders",
            &order.id,
            Op::Upsert,
            order.created_at,
        )
    }

    pub fn orders(&self, outlet: &str, open_only: bool) -> Result<Vec<PurchaseOrder>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT po.id, po.supplier_id, s.name, po.number, po.state, po.expected_day, po.note,
                    po.created_at, po.created_by, po.sent_at, po.closed_at
               FROM purchase_orders po
               JOIN suppliers s ON s.id = po.supplier_id
              WHERE po.outlet_id = ?1
                AND (?2 = 0 OR po.state IN ('draft', 'sent', 'received'))
              ORDER BY po.created_at DESC",
        )?;
        let rows = stmt.query_map(params![outlet, i64::from(open_only)], |row| {
            Ok(PurchaseOrder {
                id: row.get(0)?,
                supplier_id: row.get(1)?,
                supplier_name: row.get(2)?,
                number: row.get(3)?,
                state: OrderState::Draft,
                expected_day: None,
                note: row.get(6)?,
                created_at: encode::timestamp_from_sql(row.get(7)?),
                created_by: row.get::<_, Option<String>>(8)?.map(StaffId::new),
                sent_at: row
                    .get::<_, Option<i64>>(9)?
                    .map(encode::timestamp_from_sql),
                closed_at: row
                    .get::<_, Option<i64>>(10)?
                    .map(encode::timestamp_from_sql),
                lines: Vec::new(),
            })
            .map(|order| (order, row.get::<_, String>(4), row.get::<_, Option<i64>>(5)))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (mut order, state, expected) = row?;
            order.state = OrderState::from_tag(&state?)?;
            order.expected_day = expected?
                .map(|d| encode::business_day_from_sql(d, "purchase_orders.expected_day"))
                .transpose()?;
            out.push(order);
        }
        drop(stmt);
        for order in &mut out {
            order.lines = self.order_lines(&order.id)?;
        }
        Ok(out)
    }

    pub fn order(&self, outlet: &str, id: &str) -> Result<Option<PurchaseOrder>, DbError> {
        Ok(self.orders(outlet, false)?.into_iter().find(|o| o.id == id))
    }

    fn order_lines(&self, po: &str) -> Result<Vec<OrderLine>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT l.seq, l.material_id, m.name, l.typed_qty, l.typed_unit, l.base_qty, l.rate
               FROM purchase_order_lines l
               JOIN materials m ON m.id = l.material_id
              WHERE l.po_id = ?1 ORDER BY l.seq",
        )?;
        let rows = stmt.query_map([po], |row| {
            Ok(OrderLine {
                seq: row.get(0)?,
                material_id: MaterialId::new(row.get::<_, String>(1)?),
                material_name: row.get(2)?,
                typed_qty: encode::qty_from_sql(row.get(3)?),
                typed_unit: row.get(4)?,
                base_qty: encode::qty_from_sql(row.get(5)?),
                rate: encode::money_from_sql(row.get(6)?),
            })
        })?;
        rows.collect::<Result<_, _>>().map_err(DbError::from)
    }

    /// What went to each supplier in a period.
    pub fn by_supplier(
        &self,
        outlet: &str,
        from: BusinessDay,
        to: BusinessDay,
    ) -> Result<Vec<BuyingRow>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT s.name,
                    COUNT(*),
                    COALESCE(SUM(CASE WHEN p.kind = 'purchase' THEN p.total ELSE -p.total END), 0),
                    COALESCE(SUM(CASE WHEN p.kind = 'purchase' THEN p.tax_total
                                      ELSE -p.tax_total END), 0)
               FROM purchases p JOIN suppliers s ON s.id = p.supplier_id
              WHERE p.outlet_id = ?1 AND p.business_day BETWEEN ?2 AND ?3
                AND p.is_cancelled = 0
              GROUP BY s.id ORDER BY 3 DESC",
        )?;
        let rows = stmt.query_map(
            params![
                outlet,
                encode::business_day_to_sql(from),
                encode::business_day_to_sql(to)
            ],
            |row| {
                Ok(BuyingRow {
                    key: String::new(),
                    label: row.get(0)?,
                    count: row.get(1)?,
                    qty: None,
                    unit: String::new(),
                    value: encode::money_from_sql(row.get(2)?),
                    tax: encode::money_from_sql(row.get(3)?),
                })
            },
        )?;
        rows.collect::<Result<_, _>>().map_err(DbError::from)
    }

    /// What was bought of each material — the report that answers "where does ₹40,000 a month
    /// go".
    pub fn by_material(
        &self,
        outlet: &str,
        from: BusinessDay,
        to: BusinessDay,
    ) -> Result<Vec<BuyingRow>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT m.id, m.name, m.dimension,
                    COUNT(DISTINCT pl.purchase_id),
                    COALESCE(SUM(CASE WHEN p.kind = 'purchase' THEN pl.base_qty + pl.free_base_qty
                                      ELSE -(pl.base_qty + pl.free_base_qty) END), 0),
                    COALESCE(SUM(CASE WHEN p.kind = 'purchase' THEN pl.landed_value
                                      ELSE -pl.landed_value END), 0)
               FROM purchase_lines pl
               JOIN purchases p ON p.id = pl.purchase_id
               JOIN materials m ON m.id = pl.material_id
              WHERE p.outlet_id = ?1 AND p.business_day BETWEEN ?2 AND ?3
                AND p.is_cancelled = 0
              GROUP BY m.id ORDER BY 6 DESC",
        )?;
        let rows = stmt.query_map(
            params![
                outlet,
                encode::business_day_to_sql(from),
                encode::business_day_to_sql(to)
            ],
            |row| {
                let dimension: String = row.get(2)?;
                Ok(BuyingRow {
                    key: row.get(0)?,
                    label: row.get(1)?,
                    count: row.get(3)?,
                    qty: Some(encode::qty_from_sql(row.get(4)?)),
                    unit: mb_core::Dimension::from_tag(&dimension)
                        .map(|d| d.base_unit().to_owned())
                        .unwrap_or_default(),
                    value: encode::money_from_sql(row.get(5)?),
                    tax: Money::ZERO,
                })
            },
        )?;
        rows.collect::<Result<_, _>>().map_err(DbError::from)
    }

    /// The price trend, and the rise is the finding.
    pub fn price_trend(
        &self,
        outlet: &str,
        from: BusinessDay,
        to: BusinessDay,
    ) -> Result<Vec<PriceTrendRow>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT m.id, m.name, m.dimension,
                    COUNT(*),
                    MIN(pl.landed_unit_cost),
                    MAX(pl.landed_unit_cost),
                    AVG(pl.landed_unit_cost),
                    (SELECT pl2.landed_unit_cost
                       FROM purchase_lines pl2 JOIN purchases p2 ON p2.id = pl2.purchase_id
                      WHERE pl2.material_id = m.id AND p2.is_cancelled = 0
                        AND p2.kind = 'purchase'
                        AND p2.business_day BETWEEN ?2 AND ?3
                      ORDER BY p2.business_day DESC, p2.received_at DESC LIMIT 1)
               FROM purchase_lines pl
               JOIN purchases p ON p.id = pl.purchase_id
               JOIN materials m ON m.id = pl.material_id
              WHERE p.outlet_id = ?1 AND p.business_day BETWEEN ?2 AND ?3
                AND p.is_cancelled = 0 AND p.kind = 'purchase'
              GROUP BY m.id ORDER BY m.name",
        )?;
        let rows = stmt.query_map(
            params![
                outlet,
                encode::business_day_to_sql(from),
                encode::business_day_to_sql(to)
            ],
            |row| {
                let dimension: String = row.get(2)?;
                let average: f64 = row.get(6)?;
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "SQLite's AVG is a float; this is a display figure \
                              on a report and not a money value on a bill (D2 \
                              governs what is STORED, and nothing here is)"
                )]
                let average = UnitCost::from_paise_per_thousand(average.round() as i64);
                let latest = UnitCost::from_paise_per_thousand(row.get(7)?);
                Ok(PriceTrendRow {
                    material: MaterialId::new(row.get::<_, String>(0)?),
                    name: row.get(1)?,
                    unit: mb_core::Dimension::from_tag(&dimension)
                        .map(|d| d.base_unit().to_owned())
                        .unwrap_or_default(),
                    deliveries: row.get(3)?,
                    cheapest: UnitCost::from_paise_per_thousand(row.get(4)?),
                    dearest: UnitCost::from_paise_per_thousand(row.get(5)?),
                    average,
                    latest,
                })
            },
        )?;
        rows.collect::<Result<_, _>>().map_err(DbError::from)
    }

    /// Input credit, rate by rate.
    pub fn input_credit(
        &self,
        outlet: &str,
        from: BusinessDay,
        to: BusinessDay,
    ) -> Result<Vec<BuyingRow>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT pl.tax_rate_bp,
                    COUNT(DISTINCT pl.purchase_id),
                    COALESCE(SUM(CASE WHEN p.kind = 'purchase' THEN pl.line_value - pl.discount
                                                                    - pl.discount_share
                                      ELSE -(pl.line_value - pl.discount - pl.discount_share)
                                 END), 0),
                    COALESCE(SUM(CASE WHEN p.kind = 'purchase' THEN pl.tax_amount
                                      ELSE -pl.tax_amount END), 0)
               FROM purchase_lines pl JOIN purchases p ON p.id = pl.purchase_id
              WHERE p.outlet_id = ?1 AND p.business_day BETWEEN ?2 AND ?3
                AND p.is_cancelled = 0 AND pl.tax_rate_bp > 0
              GROUP BY pl.tax_rate_bp ORDER BY pl.tax_rate_bp",
        )?;
        let rows = stmt.query_map(
            params![
                outlet,
                encode::business_day_to_sql(from),
                encode::business_day_to_sql(to)
            ],
            |row| {
                let rate_bp: i64 = row.get(0)?;
                Ok(BuyingRow {
                    key: String::new(),
                    #[allow(
                        clippy::integer_division,
                        reason = "basis points to whole percent; GST has no \
                                  fractional-percent rate"
                    )]
                    label: format!("{}%", rate_bp / 100),
                    count: row.get(1)?,
                    qty: None,
                    unit: String::new(),
                    value: encode::money_from_sql(row.get(2)?),
                    tax: encode::money_from_sql(row.get(3)?),
                })
            },
        )?;
        rows.collect::<Result<_, _>>().map_err(DbError::from)
    }

    /// How much of the period's input tax the shop may actually claim.
    pub fn creditable_total(
        &self,
        outlet: &str,
        from: BusinessDay,
        to: BusinessDay,
    ) -> Result<Money, DbError> {
        let paise: i64 = self.tx.query_row(
            "SELECT COALESCE(SUM(CASE WHEN kind = 'purchase' THEN tax_creditable
                                      ELSE -tax_creditable END), 0)
               FROM purchases
              WHERE outlet_id = ?1 AND business_day BETWEEN ?2 AND ?3 AND is_cancelled = 0",
            params![
                outlet,
                encode::business_day_to_sql(from),
                encode::business_day_to_sql(to)
            ],
            |row| row.get(0),
        )?;
        Ok(encode::money_from_sql(paise))
    }

    pub fn save_attachment(&self, outlet: &str, attachment: &Attachment) -> Result<(), DbError> {
        self.tx.execute(
            "INSERT INTO attachments
                 (id, outlet_id, kind, subject_id, filename, byte_count, sha256, created_at,
                  created_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (id) DO UPDATE SET subject_id = excluded.subject_id",
            params![
                attachment.id,
                outlet,
                attachment.kind,
                attachment.subject_id,
                attachment.filename,
                attachment.byte_count,
                attachment.sha256,
                encode::timestamp_to_sql(attachment.created_at),
                attachment.created_by.as_ref().map(StaffId::as_str),
            ],
        )?;
        Ok(())
    }

    /// Tell a photograph what it is a picture of.
    pub fn point_attachment_at(
        &self,
        outlet: &str,
        attachment: &str,
        subject: &str,
    ) -> Result<(), DbError> {
        self.tx.execute(
            "UPDATE attachments SET subject_id = ?3 WHERE outlet_id = ?1 AND id = ?2",
            params![outlet, attachment, subject],
        )?;
        Ok(())
    }

    /// What has been paid against each delivery, for the screen's outstanding column.
    pub fn paid_by_purchase(&self, outlet: &str) -> Result<BTreeMap<String, Money>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT purchase_id, SUM(amount) FROM supplier_payments
              WHERE outlet_id = ?1 AND purchase_id IS NOT NULL GROUP BY purchase_id",
        )?;
        let mut cursor = stmt.query([outlet])?;
        let mut out = BTreeMap::new();
        while let Some(row) = cursor.next()? {
            out.insert(
                row.get::<_, String>(0)?,
                encode::money_from_sql(row.get(1)?),
            );
        }
        Ok(out)
    }

    pub fn attachment(&self, outlet: &str, id: &str) -> Result<Option<Attachment>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT id, kind, subject_id, filename, byte_count, sha256, created_at, created_by
               FROM attachments WHERE outlet_id = ?1 AND id = ?2",
        )?;
        let mut rows = stmt.query_map(params![outlet, id], read_attachment)?;
        rows.next().transpose().map_err(DbError::from)
    }

    /// Every photograph on file — what a backup has to carry, and what Health counts.
    pub fn attachments(&self, outlet: &str) -> Result<Vec<Attachment>, DbError> {
        let mut stmt = self.tx.prepare(
            "SELECT id, kind, subject_id, filename, byte_count, sha256, created_at, created_by
               FROM attachments WHERE outlet_id = ?1 ORDER BY created_at",
        )?;
        let rows = stmt.query_map([outlet], read_attachment)?;
        rows.collect::<Result<_, _>>().map_err(DbError::from)
    }
}

fn read_supplier(row: &rusqlite::Row<'_>) -> rusqlite::Result<Supplier> {
    Ok(Supplier {
        id: row.get(0)?,
        name: row.get(1)?,
        phone: row.get(2)?,
        gstin: row.get(3)?,
        address: row.get(4)?,
        terms_days: u32::try_from(row.get::<_, i64>(5)?).unwrap_or(0),
        note: row.get(6)?,
        is_active: row.get::<_, i64>(7)? != 0,
    })
}

fn read_attachment(row: &rusqlite::Row<'_>) -> rusqlite::Result<Attachment> {
    Ok(Attachment {
        id: row.get(0)?,
        kind: row.get(1)?,
        subject_id: row.get(2)?,
        filename: row.get(3)?,
        byte_count: row.get(4)?,
        sha256: row.get(5)?,
        created_at: encode::timestamp_from_sql(row.get(6)?),
        created_by: row.get::<_, Option<String>>(7)?.map(StaffId::new),
    })
}

fn read_purchase(row: &rusqlite::Row<'_>) -> rusqlite::Result<Purchase> {
    let cancelled_at: Option<i64> = row.get(23)?;
    Ok(Purchase {
        id: row.get(0)?,
        supplier_id: row.get(1)?,
        supplier_name: row.get(2)?,
        kind: match row.get::<_, String>(3)?.as_str() {
            "return" => PurchaseKind::Return,
            _ => PurchaseKind::Purchase,
        },
        parent_id: row.get(4)?,
        invoice_no: row.get(5)?,
        business_day: BusinessDay::from_days_since_epoch(
            i32::try_from(row.get::<_, i64>(6)?).unwrap_or(0),
        ),
        received_at: encode::timestamp_from_sql(row.get(7)?),
        due_day: BusinessDay::from_days_since_epoch(
            i32::try_from(row.get::<_, i64>(8)?).unwrap_or(0),
        ),
        lines_value: encode::money_from_sql(row.get(9)?),
        line_discounts: encode::money_from_sql(row.get(10)?),
        invoice_discount: encode::money_from_sql(row.get(11)?),
        charges: encode::money_from_sql(row.get(12)?),
        tax_total: encode::money_from_sql(row.get(13)?),
        tax_creditable: encode::money_from_sql(row.get(14)?),
        round_off: encode::money_from_sql(row.get(15)?),
        total: encode::money_from_sql(row.get(16)?),
        stated_total: row.get::<_, Option<i64>>(17)?.map(encode::money_from_sql),
        po_id: row.get(18)?,
        attachment_id: row.get(19)?,
        note: row.get(20)?,
        created_by: row.get::<_, Option<String>>(21)?.map(StaffId::new),
        cancelled: cancelled_at.map(|at| Cancellation {
            at: encode::timestamp_from_sql(at),
            by: row
                .get::<_, Option<String>>(24)
                .ok()
                .flatten()
                .map(StaffId::new),
            reason: row
                .get::<_, Option<String>>(25)
                .ok()
                .flatten()
                .unwrap_or_default(),
        }),
        lines: Vec::new(),
    })
}
