use std::collections::BTreeMap;

use mb_auth::audit::action;
use mb_auth::{AuditEntry, Permission};
use mb_core::purchase::{Entry, Invoice};
use mb_core::{
    BusinessDay, MaterialId, Money, Qty, RoundingMode, StaffId, Timestamp, UnitCost, Units,
};
use mb_db::repo::buying::{
    self, Attachment, OrderState, PurchaseKind, Supplier, SupplierAdjustment, SupplierPayment,
};
use mb_db::repo::stock::Material;
use mb_license::Feature;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::credit::{AgeingView, MovementView};
use crate::flows::{now, today};
use crate::guard;
use crate::ipc::{MoneyView, count};
use crate::log_info;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

// What the screen sees.

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SupplierView {
    pub id: String,
    pub name: String,
    pub phone: Option<String>,
    pub gstin: Option<String>,
    pub address: Option<String>,
    pub terms_days: u32,
    /// "Cash and carry" or "15 days" — the words, not the number.
    pub terms: String,
    pub balance: MoneyView,
    /// True when the shop owes them money.
    pub owes: bool,
    /// "42 days overdue", "due in 3 days", or empty.
    pub when: String,
    pub is_overdue: bool,
    pub is_active: bool,
}

/// A pack a purchase line may be entered in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PackView {
    pub name: String,
    /// "25 kg" — what one of them is.
    pub size: String,
}

/// A material, as the purchase-line editor needs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BuyMaterialView {
    pub id: String,
    pub name: String,
    pub base_unit: String,
    pub packs: Vec<PackView>,
    /// The pack the shop buys in — rice is BOUGHT in bags and COOKED in grams.
    pub purchase_unit: String,
    /// "₹1,000.00 a bag, last on 12 Aug" — what this supplier last charged, which is
    /// information and never a cost.
    pub last_rate: String,
    pub last_rate_paise: i64,
    /// "₹42.30 a kg" — what the shelf says it costs now.
    pub cost: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PurchaseLineView {
    pub seq: u32,
    pub material_id: String,
    pub material: String,
    /// "2 bag", and the free quantity beside it.
    pub qty: String,
    pub free: String,
    pub rate: MoneyView,
    pub discount: MoneyView,
    /// "5%", or empty.
    pub tax: String,
    pub tax_amount: MoneyView,
    pub value: MoneyView,
    /// "₹42.30 a kg" — what this line actually made the food cost, all in.
    pub landed: String,
    pub returnable: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PurchaseView {
    pub id: String,
    pub supplier_id: String,
    pub supplier: String,
    /// "Delivery" or "Sent back".
    pub kind: String,
    pub is_return: bool,
    pub parent_id: Option<String>,
    pub invoice_no: Option<String>,
    /// "12 Aug 2026".
    pub date: String,
    pub due: String,
    pub lines: Vec<PurchaseLineView>,
    pub goods: MoneyView,
    pub discount: MoneyView,
    pub charges: MoneyView,
    pub tax: MoneyView,
    pub total: MoneyView,
    /// What may actually be claimed back, and the sentence that says why it is nothing for a
    /// 5%-scheme shop.
    pub creditable: MoneyView,
    pub paid: MoneyView,
    pub outstanding: MoneyView,
    /// "Cancelled on 12 Aug — entered twice", or empty.
    pub cancelled: String,
    pub has_photo: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PoLineView {
    pub seq: u32,
    pub material_id: String,
    pub material: String,
    pub qty: String,
    pub rate: MoneyView,
    pub value: MoneyView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PurchaseOrderView {
    pub id: String,
    pub number: String,
    pub supplier_id: String,
    pub supplier: String,
    pub state: String,
    pub state_tag: String,
    pub expected: String,
    pub lines: Vec<PoLineView>,
    pub value: MoneyView,
    pub note: Option<String>,
}

/// One supplier's account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SupplierAccountView {
    pub supplier: SupplierView,
    pub movements: Vec<MovementView>,
    pub ageing: AgeingView,
    /// The account as one sentence, composed in Rust.
    pub says: String,
}

/// The whole screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct BuyingView {
    pub suppliers: Vec<SupplierView>,
    pub purchases: Vec<PurchaseView>,
    pub orders: Vec<PurchaseOrderView>,
    pub materials: Vec<BuyMaterialView>,
    /// Everything owed, right now.
    pub owed: MoneyView,
    pub overdue: MoneyView,
    /// What was bought in the last thirty days.
    pub bought: MoneyView,
    /// False when the shop bills under the 5% scheme, and the screen says the tax is a cost
    /// rather than showing an empty credit column.
    pub claims_input_tax: bool,
    pub tax_note: String,
    /// What needs somebody: overdue suppliers, and a shop that has never counted its store.
    pub attention: Vec<String>,
    pub may_manage_suppliers: bool,
    pub may_enter_purchases: bool,
}

// What the screen sends.

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct SupplierEdit {
    pub id: String,
    pub name: String,
    pub phone: String,
    pub gstin: String,
    pub address: String,
    pub terms_days: String,
    pub note: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PurchaseLineEdit {
    pub material_id: String,
    /// What the person typed, in `unit`.
    pub qty: String,
    pub unit: String,
    pub free: String,
    /// Paise per whole `unit`, as a string.
    pub rate: String,
    pub discount: String,
    pub tax_percent: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PurchaseEdit {
    pub id: String,
    pub supplier_id: String,
    pub invoice_no: String,
    pub lines: Vec<PurchaseLineEdit>,
    pub invoice_discount: String,
    pub charges: String,
    /// What the paper's own total says, when somebody typed it.
    pub stated_total: String,
    /// Money handed over at the door.
    pub paid_now: String,
    pub paid_mode: String,
    pub attachment_id: String,
    pub po_id: String,
    pub note: String,
    /// Set on a goods return: the delivery it goes back on.
    pub returns_purchase_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PoEdit {
    pub id: String,
    pub supplier_id: String,
    pub number: String,
    pub expected: String,
    pub note: String,
    pub lines: Vec<PurchaseLineEdit>,
}

/// A photograph, once it is on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct PhotoView {
    pub id: String,
    /// "182 KB", so a person can see it was downscaled.
    pub size: String,
    /// A `data:` URL, only when the screen asked to see it.
    pub data_url: Option<String>,
}

/// The buying screen.
pub fn buying_on(app: &App, supplier: Option<String>) -> UiResult<BuyingView> {
    let who = guard::require(app, Permission::PurchasesManage)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let at = now();
    let day = today(at);
    let from = BusinessDay::from_days_since_epoch(day.days_since_epoch() - 30);

    // Only a regular taxpayer can take the credit back.
    let registration = app.shop_config().store.registration();
    let claims = registration.charges_gst();

    app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let buying = repos.buying();
                let outstanding = buying.outstanding(OUTLET, day)?;
                let by_supplier: BTreeMap<String, (Money, mb_core::credit::Ageing)> = outstanding
                    .iter()
                    .map(|o| (o.supplier.id.clone(), (o.balance, o.ageing)))
                    .collect();

                let suppliers: Vec<SupplierView> = buying
                    .suppliers(OUTLET, true)?
                    .into_iter()
                    .map(|s| {
                        let (balance, ageing) = by_supplier
                            .get(&s.id)
                            .copied()
                            .unwrap_or((Money::ZERO, mb_core::credit::Ageing::default()));
                        supplier_view(s, balance, ageing.oldest_days)
                    })
                    .collect();

                let purchases = buying.purchases(OUTLET, from, day, supplier.as_deref())?;
                let paid = buying.paid_by_purchase(OUTLET)?;
                let purchase_views: Vec<PurchaseView> = purchases
                    .iter()
                    .map(|p| {
                        purchase_view(p, paid.get(&p.id).copied().unwrap_or(Money::ZERO), None)
                    })
                    .collect();

                let orders: Vec<PurchaseOrderView> = buying
                    .orders(OUTLET, true)?
                    .iter()
                    .map(order_view)
                    .collect();

                let materials = buying_materials(&repos, supplier.as_deref())?;

                let owed =
                    Money::try_sum(outstanding.iter().map(|o| o.balance)).unwrap_or(Money::ZERO);
                let overdue = Money::try_sum(
                    outstanding
                        .iter()
                        .filter(|o| o.ageing.oldest_days.is_some_and(|d| d > 0))
                        .map(|o| o.balance),
                )
                .unwrap_or(Money::ZERO);
                let bought =
                    Money::try_sum(purchases.iter().filter(|p| p.cancelled.is_none()).map(|p| {
                        match p.kind {
                            PurchaseKind::Purchase => p.total,
                            PurchaseKind::Return => p.total.neg(),
                        }
                    }))
                    .unwrap_or(Money::ZERO);

                let mut attention = Vec::new();
                let late = outstanding
                    .iter()
                    .filter(|o| o.ageing.oldest_days.is_some_and(|d| d > 0))
                    .count();
                if late > 0 {
                    attention.push(format!(
                        "{} overdue, {} in all.",
                        words::count(late as i64, "supplier is", "suppliers are"),
                        overdue.to_plain_string()
                    ));
                }
                let counted: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM stock_counts WHERE outlet_id = ?1 AND state = 'approved'",
                    [OUTLET],
                    |row| row.get(0),
                )?;
                if counted == 0 && !materials.is_empty() {
                    attention.push(
                        "Nobody has counted the store yet, so every stock figure is what the \
                         software worked out and not what is on the shelf. Stock → Count."
                            .to_owned(),
                    );
                }

                Ok(BuyingView {
                    suppliers,
                    purchases: purchase_views,
                    orders,
                    materials,
                    owed: MoneyView::from(owed),
                    overdue: MoneyView::from(overdue),
                    bought: MoneyView::from(bought),
                    claims_input_tax: claims,
                    tax_note: match registration {
                        mb_core::Registration::Regular => {
                            "GST on what you buy can be claimed back, so it is not part of \
                             your food cost."
                                .to_owned()
                        }
                        mb_core::Registration::Composition => {
                            "You bill under the 5% scheme, so purchase GST is a cost and not \
                             a credit. It is already inside your food cost."
                                .to_owned()
                        }
                        mb_core::Registration::Unregistered => {
                            "You are not registered for GST, so purchase GST is a cost and \
                             not a credit. It is already inside your food cost."
                                .to_owned()
                        }
                    },
                    attention,
                    may_manage_suppliers: who.must(Permission::SuppliersManage).is_ok(),
                    may_enter_purchases: true,
                })
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// One supplier's ledger.
pub fn supplier_account_on(app: &App, id: String) -> UiResult<SupplierAccountView> {
    guard::require(app, Permission::PurchasesManage)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let day = today(now());

    app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let buying = repos.buying();
                let Some(supplier) = buying.supplier(OUTLET, &id)? else {
                    return Err(mb_db::DbError::invariant("that supplier is not on file"));
                };
                let movements = buying.supplier_ledger(&id)?;
                let balance = mb_core::credit::balance(&movements).unwrap_or(Money::ZERO);
                let ageing = mb_core::credit::ageing(&movements, day).unwrap_or_default();

                let mut running = Money::ZERO;
                let mut rows = Vec::with_capacity(movements.len());
                for movement in &movements {
                    let adds = movement.kind.adds();
                    running = if adds {
                        running.add(movement.amount).unwrap_or(running)
                    } else {
                        running.sub(movement.amount).unwrap_or(running)
                    };
                    rows.push(MovementView {
                        date: movement.day.to_string(),
                        kind: match movement.kind {
                            mb_core::credit::MovementKind::Sale => "Delivery".to_owned(),
                            mb_core::credit::MovementKind::Repayment => "Paid".to_owned(),
                            mb_core::credit::MovementKind::Opening => "Opening balance".to_owned(),
                            mb_core::credit::MovementKind::Adjustment { .. } => {
                                "Adjustment".to_owned()
                            }
                        },
                        note: movement.note.clone(),
                        amount: MoneyView::from(movement.amount),
                        adds,
                        running: MoneyView::from(running),
                    });
                }
                rows.reverse();

                let says = account_sentence(&supplier, balance, ageing.oldest_days);
                Ok(SupplierAccountView {
                    supplier: supplier_view(supplier, balance, ageing.oldest_days),
                    movements: rows,
                    ageing: AgeingView {
                        current: MoneyView::from(ageing.current),
                        days_30: MoneyView::from(ageing.days_30),
                        days_60: MoneyView::from(ageing.days_60),
                        days_90: MoneyView::from(ageing.days_90),
                        oldest: overdue_words(ageing.oldest_days),
                    },
                    says,
                })
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// One document, for looking at, returning against, or copying.
pub fn purchase_on(app: &App, id: String) -> UiResult<PurchaseView> {
    guard::require(app, Permission::PurchasesManage)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let Some(purchase) = repos.buying().purchase(OUTLET, &id)? else {
                    return Err(mb_db::DbError::invariant("that delivery is not on file"));
                };
                let paid = repos
                    .buying()
                    .paid_by_purchase(OUTLET)?
                    .get(&id)
                    .copied()
                    .unwrap_or(Money::ZERO);
                // What is still on the shelf to send back, per line.
                let mut returnable = BTreeMap::new();
                for line in &purchase.lines {
                    returnable.insert(line.seq, repos.buying().returnable(&id, line.seq)?);
                }
                Ok(purchase_view(&purchase, paid, Some(&returnable)))
            })
            .map_err(|e| words::from_db(&e))
    })
}

pub fn save_supplier_on(app: &App, edit: SupplierEdit) -> UiResult<BuyingView> {
    let who = guard::require(app, Permission::SuppliersManage)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let at = now();

    if edit.name.trim().is_empty() {
        return Err(UiError::new("supplier.name", "A supplier needs a name."));
    }
    let gstin = match edit.gstin.trim() {
        "" => None,
        text => {
            crate::settings::value::check_gstin(text)
                .map_err(|e| UiError::new("supplier.gstin", e.message))?;
            Some(text.to_uppercase())
        }
    };
    let terms_days = match edit.terms_days.trim() {
        "" => 0,
        text => text.parse::<u32>().map_err(|_| {
            UiError::new(
                "supplier.terms",
                "Payment terms are a whole number of days. Type 0 for cash and carry.",
            )
        })?,
    };
    if terms_days > 365 {
        return Err(UiError::new(
            "supplier.terms",
            "A year of credit is not payment terms. Type the days they actually give you.",
        ));
    }

    // The same rule as a customer's — one `mb_core::Phone`, one shape on disk.
    let phone = mb_core::Phone::parse_optional(&edit.phone)
        .map_err(|e| UiError::new("supplier.phone", e.to_string()))?;

    let supplier = Supplier {
        id: edit.id.clone(),
        name: edit.name.trim().to_owned(),
        phone: phone.map(|p| p.as_str().to_owned()),
        gstin,
        address: trimmed(&edit.address),
        terms_days,
        note: trimmed(&edit.note),
        is_active: edit.is_active,
    };

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.buying().save_supplier(OUTLET, &supplier, at)?;
                trail(
                    &repos,
                    at,
                    &who.staff_id,
                    action::SUPPLIER_SAVED,
                    "supplier",
                    supplier.id.clone(),
                    serde_json::json!({
                        "name": supplier.name,
                        "terms_days": supplier.terms_days,
                        "active": supplier.is_active,
                    }),
                )
            })
            .map_err(|e| words::from_db(&e))
    })?;
    log_info!("supplier saved");
    buying_on(app, None)
}

/// Save a delivery — the paper, the shelf and the ledger, in one transaction.
#[allow(
    clippy::too_many_lines,
    reason = "one delivery is one decision; splitting the validation away from \
              the write would put the refusals somewhere else from the thing \
              they refuse"
)]
pub fn save_purchase_on(app: &App, edit: PurchaseEdit) -> UiResult<BuyingView> {
    let who = guard::require(app, Permission::PurchasesManage)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let at = now();
    let day = today(at);
    // Only a regular taxpayer can take the credit back.
    let claims = app.shop_config().store.registration().charges_gst();

    if edit.supplier_id.trim().is_empty() {
        return Err(UiError::new("purchase.supplier", "Say who this came from."));
    }
    if edit.lines.is_empty() {
        return Err(UiError::new(
            "purchase.lines",
            "There is nothing on this invoice yet.",
        ));
    }

    let materials = app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| mb_db::Repos::new(tx).stock().materials(OUTLET, true))
            .map_err(|e| words::from_db(&e))
    })?;
    let by_id: BTreeMap<String, &Material> =
        materials.iter().map(|m| (m.id.to_string(), m)).collect();

    let mut entries = Vec::with_capacity(edit.lines.len());
    let mut names = Vec::with_capacity(edit.lines.len());
    for line in &edit.lines {
        let Some(material) = by_id.get(line.material_id.trim()) else {
            return Err(UiError::new(
                "purchase.material",
                "One of those lines is for a material that is not on file.",
            )
            .with_detail(line.material_id.clone()));
        };
        let units = material.units();
        let unit = if line.unit.trim().is_empty() {
            material.default_purchase_unit()
        } else {
            line.unit.trim().to_owned()
        };
        let Some(pack) = units.find(&unit).cloned() else {
            return Err(UiError::new(
                "purchase.unit",
                format!("`{unit}` is not a unit of {}.", material.name),
            ));
        };
        let typed = parse_qty(&line.qty, &material.name)?;
        if !typed.is_positive() {
            return Err(UiError::new(
                "purchase.qty",
                format!("How much {} came?", material.name),
            ));
        }
        let free = if line.free.trim().is_empty() {
            Qty::ZERO
        } else {
            parse_qty(&line.free, &material.name)?
        };
        let rate = crate::menu::parse_money_public(&line.rate)?;
        let discount = if line.discount.trim().is_empty() {
            Money::ZERO
        } else {
            crate::menu::parse_money_public(&line.discount)?
        };
        let tax_rate_bp = parse_percent(&line.tax_percent)?;

        entries.push(Entry {
            typed_qty: typed,
            free_typed_qty: free,
            pack,
            rate,
            discount,
            tax_rate_bp,
        });
        names.push((material.id.clone(), unit));
    }

    let invoice = Invoice {
        lines: entries,
        invoice_discount: parse_money_or_zero(&edit.invoice_discount)?,
        charges: parse_money_or_zero(&edit.charges)?,
        // A property of the SHOP.
        tax_is_creditable: claims,
        rounding: RoundingMode::NearestRupee,
    };

    let is_return = !edit.returns_purchase_id.trim().is_empty();
    let paid_now = parse_money_or_zero(&edit.paid_now)?;
    if paid_now.is_positive()
        && !matches!(edit.paid_mode.as_str(), "cash" | "bank" | "upi" | "card")
    {
        return Err(UiError::new(
            "purchase.paid",
            "Say how it was paid: cash, bank, UPI or card.",
        ));
    }

    // A refusal a person must act on is returned as a VALUE.
    let saved =
        app.with_shop(|shop| {
            shop.db
            .transaction(|tx| -> Result<Result<buying::Purchase, UiError>, mb_db::DbError> {
                let repos = mb_db::Repos::new(tx);
                let buying = repos.buying();
                let Some(supplier) = buying.supplier(OUTLET, edit.supplier_id.trim())? else {
                    return Ok(Err(UiError::new(
                        "purchase.supplier",
                        "That supplier is not on file. Add them on the Suppliers tab first.",
                    )));
                };
                let mut draft =
                    buying::draft(edit.id.clone(), &supplier, day, at, &names, &invoice)?;
                draft.invoice_no = trimmed(&edit.invoice_no);
                draft.note = trimmed(&edit.note);
                draft.po_id = trimmed(&edit.po_id);
                draft.attachment_id = trimmed(&edit.attachment_id);
                draft.created_by = Some(who.staff_id.clone());
                draft.stated_total = match edit.stated_total.trim() {
                    "" => None,
                    text => Some(mb_core::Money::parse(text).unwrap_or(Money::ZERO)),
                };

                if is_return {
                    // The goods leave at what they cost coming in.
                    let parent_id = edit.returns_purchase_id.trim().to_owned();
                    let Some(parent) = buying.purchase(OUTLET, &parent_id)? else {
                        return Ok(Err(UiError::new(
                            "purchase.parent",
                            "The delivery this goes back on is not on file.",
                        )));
                    };
                    draft.kind = PurchaseKind::Return;
                    draft.parent_id = Some(parent_id.clone());
                    for line in &mut draft.lines {
                        let Some(source) =
                            parent.lines.iter().find(|l| l.material_id == line.material_id)
                        else {
                            return Ok(Err(UiError::new(
                                "purchase.return",
                                format!(
                                    "{} was not on that delivery, so it cannot go back on it.",
                                    line.material_name
                                ),
                            )));
                        };
                        let left = buying.returnable(&parent_id, source.seq)?;
                        if line.received() > left {
                            return Ok(Err(UiError::new(
                                "purchase.return",
                                format!(
                                    "Only {left} of that is left to send back on this \
                                     delivery."
                                ),
                            )));
                        }
                        line.returns_seq = Some(source.seq);
                        line.landed_unit_cost = source.landed_unit_cost;
                    }
                }

                let written = buying.record_purchase(OUTLET, &draft)?;

                // The payment is its own row, written in the same transaction so the drawer and
                // the paper cannot disagree.
                if paid_now.is_positive() {
                    buying.record_payment(
                        OUTLET,
                        &SupplierPayment {
                            id: format!("spay_{}", written.id),
                            supplier_id: written.supplier_id.clone(),
                            amount: paid_now.min(written.total),
                            mode: edit.paid_mode.clone(),
                            reference: written.invoice_no.clone(),
                            purchase_id: Some(written.id.clone()),
                            paid_at: at,
                            business_day: day,
                            paid_by: Some(who.staff_id.clone()),
                            note: None,
                        },
                    )?;
                }

                // The photograph was taken before the lines were typed (a person shoots the
                // paper, then reads it), so this is where it learns what it is a picture of.
                if let Some(attachment) = &written.attachment_id {
                    buying.point_attachment_at(OUTLET, attachment, &written.id)?;
                }

                trail(
                    &repos,
                    at,
                    &who.staff_id,
                    if is_return { action::PURCHASE_RETURNED } else { action::PURCHASE_SAVED },
                    "purchase",
                    written.id.clone(),
                    serde_json::json!({
                        "supplier": supplier.name,
                        "invoice": written.invoice_no,
                        "total_paise": written.total.paise(),
                        "lines": written.lines.len(),
                    }),
                )?;
                Ok(Ok(written))
            })
            .map_err(|e| words::from_db(&e))
        })??;
    log_info!("delivery recorded");

    let mut view = buying_on(app, None)?;
    // The three sanity checks, as sentences and never refusals — the paper is the truth and the
    // software is not there to argue with it.
    view.attention.splice(0..0, sanity(&saved, &edit, &by_id));
    Ok(view)
}

/// The only correction path a purchase has.
pub fn cancel_purchase_on(app: &App, id: String, reason: String) -> UiResult<BuyingView> {
    let who = guard::require(app, Permission::PurchasesManage)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let at = now();
    if reason.trim().is_empty() {
        return Err(UiError::new(
            "purchase.reason",
            "Say why it is being cancelled.",
        ));
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| -> Result<Result<(), UiError>, mb_db::DbError> {
                let repos = mb_db::Repos::new(tx);
                let Some(purchase) = repos.buying().purchase(OUTLET, &id)? else {
                    return Ok(Err(UiError::new(
                        "purchase.missing",
                        "That delivery is not on file. Refresh and try again.",
                    )));
                };
                // The day lock has a door, not a back door.
                if repos
                    .corrections()
                    .day_is_locked(OUTLET, purchase.business_day)?
                {
                    return Ok(Err(UiError::new(
                        "purchase.day_locked",
                        format!(
                            "{} has been closed and locked. Reopen that day first \
                             (Reports → Day close), then cancel this.",
                            purchase.business_day
                        ),
                    )));
                }
                repos.buying().cancel_purchase(
                    OUTLET,
                    &id,
                    reason.trim(),
                    Some(&who.staff_id),
                    at,
                )?;
                trail(
                    &repos,
                    at,
                    &who.staff_id,
                    action::PURCHASE_CANCELLED,
                    "purchase",
                    id.clone(),
                    serde_json::json!({
                        "reason": reason.trim(),
                        "total_paise": purchase.total.paise(),
                    }),
                )?;
                Ok(Ok(()))
            })
            .map_err(|e| words::from_db(&e))
    })??;
    log_info!("delivery cancelled");
    buying_on(app, None)
}

pub fn record_supplier_payment_on(
    app: &App,
    supplier_id: String,
    amount: String,
    mode: String,
    reference: String,
) -> UiResult<SupplierAccountView> {
    let who = guard::require(app, Permission::SuppliersManage)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let at = now();
    let day = today(at);

    let amount = crate::menu::parse_money_public(&amount)?;
    if !amount.is_positive() {
        return Err(UiError::new(
            "payment.amount",
            "A payment is more than nothing.",
        ));
    }
    if !matches!(mode.as_str(), "cash" | "bank" | "upi" | "card") {
        return Err(UiError::new(
            "payment.mode",
            "Pay in cash, by bank, UPI or card.",
        ));
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.buying().record_payment(
                    OUTLET,
                    &SupplierPayment {
                        id: crate::newid::fresh_at("spay", at),
                        supplier_id: supplier_id.clone(),
                        amount,
                        mode: mode.clone(),
                        reference: trimmed(&reference),
                        purchase_id: None,
                        paid_at: at,
                        business_day: day,
                        paid_by: Some(who.staff_id.clone()),
                        note: None,
                    },
                )?;
                trail(
                    &repos,
                    at,
                    &who.staff_id,
                    action::SUPPLIER_PAID,
                    "supplier",
                    supplier_id.clone(),
                    serde_json::json!({ "amount_paise": amount.paise(), "mode": mode }),
                )
            })
            .map_err(|e| words::from_db(&e))
    })?;
    log_info!("supplier paid");
    supplier_account_on(app, supplier_id)
}

pub fn save_supplier_adjustment_on(
    app: &App,
    supplier_id: String,
    amount: String,
    increases: bool,
    reason: String,
) -> UiResult<SupplierAccountView> {
    let who = guard::require(app, Permission::SuppliersManage)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let at = now();
    let day = today(at);

    let amount = crate::menu::parse_money_public(&amount)?;
    if !amount.is_positive() {
        return Err(UiError::new(
            "adjustment.amount",
            "An adjustment of nothing changes nothing.",
        ));
    }
    if reason.trim().is_empty() {
        return Err(UiError::new(
            "adjustment.reason",
            "Say why the account is being changed.",
        ));
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.buying().save_adjustment(
                    OUTLET,
                    &SupplierAdjustment {
                        id: crate::newid::fresh_at("sadj", at),
                        supplier_id: supplier_id.clone(),
                        amount,
                        increases,
                        reason: reason.trim().to_owned(),
                        at,
                        business_day: day,
                        made_by: Some(who.staff_id.clone()),
                    },
                )?;
                trail(
                    &repos,
                    at,
                    &who.staff_id,
                    action::SUPPLIER_ADJUSTED,
                    "supplier",
                    supplier_id.clone(),
                    serde_json::json!({
                        "amount_paise": amount.paise(),
                        "increases": increases,
                        "reason": reason.trim(),
                    }),
                )
            })
            .map_err(|e| words::from_db(&e))
    })?;
    supplier_account_on(app, supplier_id)
}

/// Optional, and nothing reads one.
pub fn save_purchase_order_on(app: &App, edit: PoEdit) -> UiResult<BuyingView> {
    let who = guard::require(app, Permission::PurchasesManage)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let at = now();

    if edit.supplier_id.trim().is_empty() {
        return Err(UiError::new(
            "po.supplier",
            "Say who the order is going to.",
        ));
    }
    if edit.lines.is_empty() {
        return Err(UiError::new(
            "po.lines",
            "An order with nothing on it is not an order.",
        ));
    }

    let materials = app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| mb_db::Repos::new(tx).stock().materials(OUTLET, true))
            .map_err(|e| words::from_db(&e))
    })?;
    let by_id: BTreeMap<String, &Material> =
        materials.iter().map(|m| (m.id.to_string(), m)).collect();

    let mut lines = Vec::with_capacity(edit.lines.len());
    for (index, line) in edit.lines.iter().enumerate() {
        let Some(material) = by_id.get(line.material_id.trim()) else {
            return Err(UiError::new("po.material", "That material is not on file."));
        };
        let units = material.units();
        let unit = if line.unit.trim().is_empty() {
            material.default_purchase_unit()
        } else {
            line.unit.trim().to_owned()
        };
        let typed = parse_qty(&line.qty, &material.name)?;
        let base = units.to_base(typed, &unit).map_err(|e| {
            UiError::new(
                "po.unit",
                format!("`{unit}` is not a unit of {}.", material.name),
            )
            .with_detail(e.to_string())
        })?;
        lines.push(buying::OrderLine {
            seq: i64::try_from(index).unwrap_or(0) + 1,
            material_id: material.id.clone(),
            material_name: material.name.clone(),
            typed_qty: typed,
            typed_unit: unit,
            base_qty: base,
            rate: parse_money_or_zero(&line.rate)?,
        });
    }

    let number = if edit.number.trim().is_empty() {
        // This was `at.millis() % 1_000_000`, which repeats every sixteen minutes.
        format!(
            "PO-{}-{}",
            today(at).days_since_epoch(),
            crate::newid::tail_only()
        )
    } else {
        edit.number.trim().to_owned()
    };
    let expected = match edit.expected.trim() {
        "" => None,
        text => text.parse::<BusinessDay>().ok(),
    };

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.buying().save_order(
                    OUTLET,
                    &buying::PurchaseOrder {
                        id: edit.id.clone(),
                        supplier_id: edit.supplier_id.trim().to_owned(),
                        supplier_name: String::new(),
                        number: number.clone(),
                        state: OrderState::Draft,
                        expected_day: expected,
                        note: trimmed(&edit.note),
                        created_at: at,
                        created_by: Some(who.staff_id.clone()),
                        sent_at: None,
                        closed_at: None,
                        lines: lines.clone(),
                    },
                )?;
                trail(
                    &repos,
                    at,
                    &who.staff_id,
                    action::ORDER_PLACED,
                    "purchase_order",
                    edit.id.clone(),
                    serde_json::json!({ "number": number, "lines": lines.len() }),
                )
            })
            .map_err(|e| words::from_db(&e))
    })?;
    buying_on(app, None)
}

pub fn set_order_state_on(app: &App, id: String, state: String) -> UiResult<BuyingView> {
    let who = guard::require(app, Permission::PurchasesManage)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let at = now();
    let state = OrderState::from_tag(&state)
        .map_err(|_| UiError::new("po.state", "That is not something an order can be."))?;

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let Some(mut order) = repos.buying().order(OUTLET, &id)? else {
                    return Err(mb_db::DbError::invariant("that order is not on file"));
                };
                order.state = state;
                match state {
                    OrderState::Sent => order.sent_at = Some(at),
                    OrderState::Closed | OrderState::Cancelled => order.closed_at = Some(at),
                    _ => {}
                }
                repos.buying().save_order(OUTLET, &order)?;
                trail(
                    &repos,
                    at,
                    &who.staff_id,
                    action::ORDER_PLACED,
                    "purchase_order",
                    id.clone(),
                    serde_json::json!({ "state": state.tag() }),
                )
            })
            .map_err(|e| words::from_db(&e))
    })?;
    buying_on(app, None)
}

// The photograph.

/// The most a single photograph may be, after the webview has downscaled it.
const MAX_PHOTO_BYTES: usize = 500 * 1024;

/// The whole folder, past which Health says something.
pub const PHOTO_FOLDER_WARN_BYTES: u64 = 200 * 1024 * 1024;

/// Attach a photograph of the paper.
pub fn attach_photo_on(app: &App, data_url: String) -> UiResult<PhotoView> {
    let who = guard::require(app, Permission::PurchasesManage)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let at = now();

    let encoded = data_url
        .split_once(",")
        .map_or(data_url.as_str(), |(_, rest)| rest);
    let bytes = base64_decode(encoded)
        .ok_or_else(|| UiError::new("photo.data", "That photograph could not be read."))?;
    if bytes.is_empty() {
        return Err(UiError::new("photo.data", "That photograph is empty."));
    }
    if bytes.len() > MAX_PHOTO_BYTES {
        return Err(UiError::new(
            "photo.size",
            format!(
                "That photograph is {}, and the limit is {}. Take it again — the camera's \
                 own picture is far bigger than a bill needs to be.",
                kilobytes(bytes.len() as u64),
                kilobytes(MAX_PHOTO_BYTES as u64)
            ),
        ));
    }

    let digest = mb_auth::audit::sha256(&bytes);
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let filename = format!("{hex}.jpg");

    let dir = app.with_shop(|shop| Ok(mb_db::backup::attachments_dir(shop.db.path())))?;
    std::fs::create_dir_all(&dir).map_err(|e| {
        UiError::new("photo.folder", "The photographs folder could not be made.")
            .with_detail(e.to_string())
    })?;
    std::fs::write(dir.join(&filename), &bytes).map_err(|e| {
        UiError::new("photo.write", "That photograph could not be saved.")
            .with_detail(e.to_string())
    })?;

    let id = format!("att_{hex}");
    let attachment = Attachment {
        id: id.clone(),
        kind: "purchase".to_owned(),
        subject_id: None,
        filename,
        byte_count: i64::try_from(bytes.len()).unwrap_or(0),
        sha256: hex,
        created_at: at,
        created_by: Some(who.staff_id.clone()),
    };
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx)
                    .buying()
                    .save_attachment(OUTLET, &attachment)
            })
            .map_err(|e| words::from_db(&e))
    })?;

    Ok(PhotoView {
        id,
        size: kilobytes(bytes.len() as u64),
        data_url: None,
    })
}

/// Read one back, for the screen to show.
pub fn purchase_photo_on(app: &App, id: String) -> UiResult<PhotoView> {
    guard::require(app, Permission::PurchasesManage)?;
    let (attachment, dir) = app.with_shop(|shop| {
        let dir = mb_db::backup::attachments_dir(shop.db.path());
        let attachment = shop
            .db
            .read_transaction(|tx| mb_db::Repos::new(tx).buying().attachment(OUTLET, &id))
            .map_err(|e| words::from_db(&e))?;
        Ok((attachment, dir))
    })?;
    let Some(attachment) = attachment else {
        return Err(UiError::new(
            "photo.missing",
            "There is no photograph on that delivery.",
        ));
    };
    // A row with no file is a detectable fact, not a mystery — which is the whole reason the
    // metadata is in the database.
    let bytes = std::fs::read(dir.join(&attachment.filename)).map_err(|e| {
        UiError::new(
            "photo.lost",
            "The photograph is recorded but its file is not there. Restore from a backup \
             that has it, or take the picture again.",
        )
        .with_detail(e.to_string())
    })?;
    Ok(PhotoView {
        id: attachment.id,
        size: kilobytes(u64::try_from(attachment.byte_count).unwrap_or(0)),
        data_url: Some(format!("data:image/jpeg;base64,{}", base64_encode(&bytes))),
    })
}

// Words and shapes.

fn trail(
    repos: &mb_db::Repos<'_>,
    at: Timestamp,
    who: &StaffId,
    what: mb_auth::audit::AuditAction,
    entity: &'static str,
    id: impl Into<String>,
    saying: serde_json::Value,
) -> Result<(), mb_db::DbError> {
    repos.audit().append(
        OUTLET,
        &AuditEntry::new(at, today(at), Some(who.clone()), what, entity)
            .about(id)
            .with_after(saying),
    )?;
    Ok(())
}

fn trimmed(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_owned())
    }
}

fn parse_money_or_zero(text: &str) -> UiResult<Money> {
    if text.trim().is_empty() {
        return Ok(Money::ZERO);
    }
    crate::menu::parse_money_public(text)
}

fn parse_qty(text: &str, what: &str) -> UiResult<Qty> {
    Qty::parse(text.trim()).map_err(|e| {
        UiError::new(
            "purchase.qty",
            format!("`{text}` is not an amount of {what}."),
        )
        .with_detail(e.to_string())
    })
}

fn parse_percent(text: &str) -> UiResult<u32> {
    if text.trim().is_empty() {
        return Ok(0);
    }
    let bp = mb_auth::RoleShape::parse_percent(text.trim())
        .map_err(|e| UiError::new("purchase.tax", e.to_string()))?
        .unwrap_or(0);
    Ok(bp)
}

#[allow(
    clippy::integer_division,
    reason = "a file size a person reads; the remainder of a kilobyte is noise"
)]
fn kilobytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} bytes");
    }
    format!("{} KB", bytes / 1024)
}

/// "42 days overdue", "due in 3 days", "due today".
fn overdue_words(days: Option<i32>) -> String {
    match days {
        None => "—".to_owned(),
        Some(0) => "due today".to_owned(),
        Some(n) if n < 0 => format!("due in {}", words::count(i64::from(-n), "day", "days")),
        Some(n) => format!("{} overdue", words::count(i64::from(n), "day", "days")),
    }
}

fn account_sentence(supplier: &Supplier, balance: Money, oldest: Option<i32>) -> String {
    if balance.is_zero() {
        return format!("Nothing is owed to {}.", supplier.name);
    }
    if balance.is_negative() {
        return format!(
            "{} is holding {} of yours in advance.",
            supplier.name,
            balance.abs().to_plain_string()
        );
    }
    match oldest {
        Some(n) if n > 0 => format!(
            "{} is owed {}, and the oldest of it is {} overdue.",
            supplier.name,
            balance.to_plain_string(),
            words::count(i64::from(n), "day", "days")
        ),
        _ => format!(
            "{} is owed {}, none of it overdue yet.",
            supplier.name,
            balance.to_plain_string()
        ),
    }
}

fn supplier_view(supplier: Supplier, balance: Money, oldest: Option<i32>) -> SupplierView {
    SupplierView {
        terms: if supplier.terms_days == 0 {
            "Cash and carry".to_owned()
        } else {
            words::count(i64::from(supplier.terms_days), "day", "days")
        },
        balance: MoneyView::from(balance),
        owes: balance.is_positive(),
        when: if balance.is_zero() {
            String::new()
        } else {
            overdue_words(oldest)
        },
        is_overdue: oldest.is_some_and(|d| d > 0) && balance.is_positive(),
        id: supplier.id,
        name: supplier.name,
        phone: supplier.phone,
        gstin: supplier.gstin,
        address: supplier.address,
        terms_days: supplier.terms_days,
        is_active: supplier.is_active,
    }
}

fn purchase_view(
    purchase: &buying::Purchase,
    paid: Money,
    returnable: Option<&BTreeMap<i64, Qty>>,
) -> PurchaseView {
    let goods = purchase
        .lines_value
        .sub(purchase.line_discounts)
        .and_then(|v| v.sub(purchase.invoice_discount))
        .unwrap_or(purchase.lines_value);
    let discount = purchase
        .line_discounts
        .add(purchase.invoice_discount)
        .unwrap_or(purchase.line_discounts);

    PurchaseView {
        id: purchase.id.clone(),
        supplier_id: purchase.supplier_id.clone(),
        supplier: purchase.supplier_name.clone(),
        kind: purchase.kind.label().to_owned(),
        is_return: purchase.kind == PurchaseKind::Return,
        parent_id: purchase.parent_id.clone(),
        invoice_no: purchase.invoice_no.clone(),
        date: purchase.business_day.to_string(),
        due: purchase.due_day.to_string(),
        lines: purchase
            .lines
            .iter()
            .map(|line| PurchaseLineView {
                seq: count(line.seq),
                material_id: line.material_id.to_string(),
                material: line.material_name.clone(),
                qty: format!("{} {}", line.typed_qty, line.typed_unit),
                free: if line.free_typed_qty.is_zero() {
                    String::new()
                } else {
                    format!("{} {} free", line.free_typed_qty, line.typed_unit)
                },
                rate: MoneyView::from(line.rate),
                discount: MoneyView::from(line.discount),
                tax: if line.tax_rate_bp == 0 {
                    String::new()
                } else {
                    percent_words(line.tax_rate_bp)
                },
                tax_amount: MoneyView::from(line.tax_amount),
                value: MoneyView::from(line.landed_value),
                landed: landed_words(line),
                returnable: returnable
                    .and_then(|left| left.get(&line.seq))
                    .map(ToString::to_string)
                    .unwrap_or_default(),
            })
            .collect(),
        goods: MoneyView::from(goods),
        discount: MoneyView::from(discount),
        charges: MoneyView::from(purchase.charges),
        tax: MoneyView::from(purchase.tax_total),
        total: MoneyView::from(purchase.total),
        creditable: MoneyView::from(purchase.tax_creditable),
        paid: MoneyView::from(paid),
        outstanding: MoneyView::from(purchase.total.sub(paid).unwrap_or(Money::ZERO)),
        cancelled: purchase
            .cancelled
            .as_ref()
            .map_or_else(String::new, |c| format!("Cancelled — {}", c.reason)),
        has_photo: purchase.attachment_id.is_some(),
        note: purchase.note.clone(),
    }
}

/// "5%", "12.5%" â basis points said the way an invoice prints them, with no float anywhere
/// near it.
#[allow(
    clippy::integer_division,
    reason = "basis points to a percent AND its two decimals, done with a \n              remainder; a float here would be the one in the whole product"
)]
fn percent_words(rate_bp: u32) -> String {
    if rate_bp.is_multiple_of(100) {
        return format!("{}%", rate_bp / 100);
    }
    format!("{}.{:02}%", rate_bp / 100, rate_bp % 100)
}

/// What a line really made the goods cost, said in the unit it was bought in — "₹909.09 per
/// bag" beside a rate of ₹1,000.
fn landed_words(line: &buying::PurchaseLine) -> String {
    let typed = line
        .typed_qty
        .add(line.free_typed_qty)
        .unwrap_or(line.typed_qty);
    if !typed.is_positive() || line.landed_unit_cost.is_zero() {
        return String::new();
    }
    // From the STORED cost, not from the value divided by the quantity.
    let base_per_unit = line
        .received()
        .thousandths()
        .saturating_mul(1_000)
        .checked_div(typed.thousandths())
        .unwrap_or(0);
    match Money::from_paise(line.landed_unit_cost.paise_per_thousand())
        .mul_ratio(base_per_unit, 1_000_000)
    {
        Ok(each) => format!("{} per {}", each.to_indian_string(), line.typed_unit),
        Err(_) => String::new(),
    }
}

/// "₹42.30 per kg", never "₹0.04 per g".
fn cost_words(cost: UnitCost, units: &Units, unit: &str) -> String {
    if cost.is_zero() {
        return String::new();
    }
    let Some(pack) = units.find(unit) else {
        return String::new();
    };
    match cost.per_pack(pack) {
        Ok(money) => format!("{} per {}", money.to_indian_string(), pack.name),
        Err(_) => String::new(),
    }
}

/// The unit a cost is best said in: the shop's own purchase pack, or the dimension's everyday
/// standard (kg, litre, dozen) — never the base unit, because "per g" is the sentence nobody
/// reads.
fn saying_unit(material: &Material, units: &Units) -> String {
    if let Some(pack) = material.purchase_unit.as_deref()
        && units.find(pack).is_some()
    {
        return pack.to_owned();
    }
    let (_, pack) = units.readable(Qty::from_thousandths(1_000_000));
    pack.name.clone()
}

fn order_view(order: &buying::PurchaseOrder) -> PurchaseOrderView {
    let value = Money::try_sum(
        order
            .lines
            .iter()
            .map(|l| l.typed_qty.extend(l.rate).unwrap_or(Money::ZERO)),
    )
    .unwrap_or(Money::ZERO);
    PurchaseOrderView {
        id: order.id.clone(),
        number: order.number.clone(),
        supplier_id: order.supplier_id.clone(),
        supplier: order.supplier_name.clone(),
        state: order.state.label().to_owned(),
        state_tag: order.state.tag().to_owned(),
        expected: order
            .expected_day
            .map(|d| d.to_string())
            .unwrap_or_default(),
        lines: order
            .lines
            .iter()
            .map(|line| PoLineView {
                seq: count(line.seq),
                material_id: line.material_id.to_string(),
                material: line.material_name.clone(),
                qty: format!("{} {}", line.typed_qty, line.typed_unit),
                rate: MoneyView::from(line.rate),
                value: MoneyView::from(line.typed_qty.extend(line.rate).unwrap_or(Money::ZERO)),
            })
            .collect(),
        value: MoneyView::from(value),
        note: order.note.clone(),
    }
}

fn buying_materials(
    repos: &mb_db::Repos<'_>,
    supplier: Option<&str>,
) -> Result<Vec<BuyMaterialView>, mb_db::DbError> {
    let materials = repos.stock().materials(OUTLET, false)?;
    let prices: BTreeMap<MaterialId, (Money, Option<BusinessDay>)> = match supplier {
        Some(id) => repos
            .buying()
            .supplier_materials(id)?
            .into_iter()
            .map(|m| (m.material_id, (m.last_rate, m.last_bought_day)))
            .collect(),
        None => BTreeMap::new(),
    };
    Ok(materials
        .iter()
        .map(|material| {
            let units: Units = material.units();
            let (rate, when) = prices
                .get(&material.id)
                .copied()
                .unwrap_or((Money::ZERO, None));
            BuyMaterialView {
                id: material.id.to_string(),
                name: material.name.clone(),
                base_unit: material.dimension.base_unit().to_owned(),
                packs: units
                    .all()
                    .map(|pack| PackView {
                        name: pack.name.clone(),
                        size: units.say(pack.base_per_unit),
                    })
                    .collect(),
                purchase_unit: material.default_purchase_unit(),
                last_rate: if rate.is_zero() {
                    String::new()
                } else {
                    match when {
                        Some(day) => format!("{} last on {day}", rate.to_plain_string()),
                        None => rate.to_plain_string(),
                    }
                },
                last_rate_paise: rate.paise(),
                cost: cost_words(material.avg_cost, &units, &saying_unit(material, &units)),
            }
        })
        .collect())
}

/// The three sanity checks, as sentences and never refusals.
fn sanity(
    saved: &buying::Purchase,
    edit: &PurchaseEdit,
    materials: &BTreeMap<String, &Material>,
) -> Vec<String> {
    let mut out = Vec::new();

    if let Some(stated) = saved.stated_total
        && stated != saved.total
    {
        out.push(format!(
            "The invoice says {} and the lines make {}. One of them needs a look.",
            stated.to_plain_string(),
            saved.total.to_plain_string()
        ));
    }

    for line in &saved.lines {
        let Some(material) = materials.get(line.material_id.as_str()) else {
            continue;
        };
        // A rate more than a fifth above what the shelf has been paying is the finding an owner
        // acts on fastest — and it arrives while somebody can still phone the supplier.
        if !material.avg_cost.is_zero() && !line.landed_unit_cost.is_zero() {
            let before = material.avg_cost.paise_per_thousand();
            let now = line.landed_unit_cost.paise_per_thousand();
            if now > before.saturating_mul(6).saturating_div(5) {
                out.push(format!(
                    "{} came in at {} against {} you were paying — {}% more.",
                    material.name,
                    Money::from_paise(now).to_plain_string(),
                    Money::from_paise(before).to_plain_string(),
                    (now - before)
                        .saturating_mul(100)
                        .saturating_div(before.max(1))
                ));
            }
        }
    }

    if !edit.po_id.trim().is_empty() {
        out.push("This was received against an order — check the quantities match it.".to_owned());
    }
    out
}

// Base64, because the photograph arrives as a data URL.

pub(crate) fn base64_decode(text: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .ok()
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

// The seats.

#[tauri::command]
pub fn buying(app: tauri::State<'_, App>, supplier: Option<String>) -> UiResult<BuyingView> {
    buying_on(&app, supplier)
}

#[tauri::command]
pub fn supplier_account(app: tauri::State<'_, App>, id: String) -> UiResult<SupplierAccountView> {
    supplier_account_on(&app, id)
}

#[tauri::command]
pub fn purchase(app: tauri::State<'_, App>, id: String) -> UiResult<PurchaseView> {
    purchase_on(&app, id)
}

#[tauri::command]
pub fn save_supplier(app: tauri::State<'_, App>, edit: SupplierEdit) -> UiResult<BuyingView> {
    save_supplier_on(&app, edit)
}

#[tauri::command]
pub fn save_purchase(app: tauri::State<'_, App>, edit: PurchaseEdit) -> UiResult<BuyingView> {
    save_purchase_on(&app, edit)
}

#[tauri::command]
pub fn cancel_purchase(
    app: tauri::State<'_, App>,
    id: String,
    reason: String,
) -> UiResult<BuyingView> {
    cancel_purchase_on(&app, id, reason)
}

#[tauri::command]
pub fn record_supplier_payment(
    app: tauri::State<'_, App>,
    supplier_id: String,
    amount: String,
    mode: String,
    reference: String,
) -> UiResult<SupplierAccountView> {
    record_supplier_payment_on(&app, supplier_id, amount, mode, reference)
}

#[tauri::command]
pub fn save_supplier_adjustment(
    app: tauri::State<'_, App>,
    supplier_id: String,
    amount: String,
    increases: bool,
    reason: String,
) -> UiResult<SupplierAccountView> {
    save_supplier_adjustment_on(&app, supplier_id, amount, increases, reason)
}

#[tauri::command]
pub fn save_purchase_order(app: tauri::State<'_, App>, edit: PoEdit) -> UiResult<BuyingView> {
    save_purchase_order_on(&app, edit)
}

#[tauri::command]
pub fn set_order_state(
    app: tauri::State<'_, App>,
    id: String,
    state: String,
) -> UiResult<BuyingView> {
    set_order_state_on(&app, id, state)
}

#[tauri::command]
pub fn attach_photo(app: tauri::State<'_, App>, data_url: String) -> UiResult<PhotoView> {
    attach_photo_on(&app, data_url)
}

#[tauri::command]
pub fn purchase_photo(app: tauri::State<'_, App>, id: String) -> UiResult<PhotoView> {
    purchase_photo_on(&app, id)
}
