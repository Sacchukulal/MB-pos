//! **The physical stock count** — P26, scope 4.8. Bodies over `&App` (D46).
//!
//! # The two decisions this screen exists to obey
//!
//! **D127 — the count freezes the book and posts a DELTA.** Somebody counts the
//! store at 11 pm on Sunday and the owner approves it at 9 am on Monday, after
//! Monday's rice has arrived. Approving posts `counted − the book as it was when
//! that line was counted`, so the delivery survives. A screen that said "set the
//! balance to what I counted" would erase it, and nobody would notice for a
//! month.
//!
//! **D128 — the printed count sheet has no book quantity on it.** A person
//! holding a clipboard that already says "12.5 kg" writes down 12.5 kg. Every
//! variance goes to zero, and the one thing the shop bought this module for
//! stops working — quietly, and in a way no test can detect. That is one line in
//! `sheet()` and it is the most valuable line in the feature.

use mb_auth::audit::action;
use mb_auth::{AuditEntry, Permission};
use mb_core::{MaterialId, Money, Qty};
use mb_db::repo::counts::CountState;
use mb_db::repo::stock::Material;
use mb_license::Feature;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::flows::{now, today};
use crate::guard;
use crate::ipc::{MoneyView, count as as_count};
use crate::log_info;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

// ---------------------------------------------------------------------------
// What the screen sees.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CountLineView {
    pub material_id: String,
    pub material: String,
    /// What was written on the sheet — "10 kg".
    pub counted: String,
    /// **The book AS IT WAS when this line was counted** (D127), so the screen
    /// can show what the software thought at that moment.
    pub book: String,
    /// "2 kg short", "500 g over", or "matches".
    pub variance: String,
    pub variance_value: MoneyView,
    pub is_short: bool,
    pub is_over: bool,
    /// True when the gap is big enough that the shop wants a reason.
    pub needs_reason: bool,
    pub reason_id: Option<String>,
    pub note: Option<String>,
}

/// A material still to be walked past.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ToCountView {
    pub material_id: String,
    pub material: String,
    pub base_unit: String,
    /// The units this may be written down in — a person counts bags, not grams.
    pub units: Vec<String>,
    pub default_unit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct StockCountView {
    pub id: Option<String>,
    pub location: String,
    /// "Being counted", "Approved", "Given up".
    pub state: String,
    pub state_tag: String,
    pub date: String,
    pub opened_by: String,
    pub lines: Vec<CountLineView>,
    pub remaining: Vec<ToCountView>,
    /// **What approving will do, said before anybody presses it** (D127):
    /// "This will add 2.4 kg and take away 800 g across 14 materials."
    pub effect: String,
    pub short_value: MoneyView,
    pub over_value: MoneyView,
    pub net_value: MoneyView,
    /// **The three figures as one sentence** (§6, and it fixes a double
    /// negative found by looking): "Short −600.00 Over 0.00" put a minus sign
    /// next to the word "short", which says the same thing twice and in two
    /// notations. This says it once.
    pub totals_says: String,
    /// The reasons a shop may pick from (`reasons.kind = 'count'`).
    pub reasons: Vec<crate::corrections::ReasonView>,
    /// Approved and abandoned counts, newest first.
    pub history: Vec<CountSummaryView>,
    pub may_approve: bool,
    /// The threshold above which the screen asks for a reason.
    pub reason_above: MoneyView,
    /// "Nobody has ever counted this store." — D115's honesty, on the screen
    /// that fixes it.
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CountSummaryView {
    pub id: String,
    pub date: String,
    pub location: String,
    pub state: String,
    pub materials: u32,
    pub value: MoneyView,
    pub who: String,
    pub ended_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CountEdit {
    pub count_id: String,
    pub material_id: String,
    /// What the person wrote, in `unit`.
    pub counted: String,
    pub unit: String,
}

// ---------------------------------------------------------------------------
// Reading.
// ---------------------------------------------------------------------------

pub fn stock_count_on(app: &App, id: Option<String>) -> UiResult<StockCountView> {
    guard::require(app, Permission::InventoryView)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let who = app.sessions().current();
    let may_approve = who
        .as_ref()
        .is_some_and(|s| s.actor.must(Permission::StockAdjust).is_ok());
    let day = today(now());
    let from = mb_core::BusinessDay::from_days_since_epoch(day.days_since_epoch() - 365);
    let threshold = app.shop_config().stock.count_reason_above;

    app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let counts = repos.counts();
                let open = match &id {
                    Some(id) => counts.count(OUTLET, id)?,
                    None => counts.open_at(OUTLET, mb_db::repo::stock::DEFAULT_LOCATION)?,
                };
                let materials = repos.stock().materials(OUTLET, false)?;
                let reasons = repos
                    .corrections()
                    .reasons(OUTLET, "count")?
                    .into_iter()
                    .map(|r| crate::corrections::ReasonView { id: r.id, text: r.text })
                    .collect();
                let history = counts
                    .counts(OUTLET, from, day)?
                    .into_iter()
                    .filter(|c| c.state != CountState::Draft)
                    .take(20)
                    .map(|c| CountSummaryView {
                        materials: as_count(c.lines.len() as i64),
                        value: MoneyView::from(c.variance_value()),
                        id: c.id,
                        date: c.business_day.to_string(),
                        location: c.location,
                        state: c.state.label().to_owned(),
                        who: String::new(),
                        ended_reason: c.ended_reason,
                    })
                    .collect::<Vec<_>>();
                let ever = history.iter().any(|h| h.state == CountState::Approved.label());

                Ok(view_of(open, &materials, reasons, history, ever, may_approve, threshold))
            })
            .map_err(|e| words::from_db(&e))
    })
}

// ---------------------------------------------------------------------------
// Writing.
// ---------------------------------------------------------------------------

pub fn open_stock_count_on(app: &App, location: String) -> UiResult<StockCountView> {
    let who = guard::require(app, Permission::StockCount)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let at = now();
    let day = today(at);
    let location = if location.trim().is_empty() {
        mb_db::repo::stock::DEFAULT_LOCATION.to_owned()
    } else {
        location.trim().to_owned()
    };
    let id = crate::newid::fresh_at("cnt", at);

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.counts().open(
                    OUTLET,
                    &id,
                    &location,
                    day,
                    at,
                    Some(&who.staff_id),
                )?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::COUNT_OPENED,
                        "stock_count",
                    )
                    .about(id.clone())
                    .with_after(serde_json::json!({ "location": location })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;
    stock_count_on(app, Some(id))
}

/// **Write down what is on the shelf.** The book is frozen against it here and
/// never read again (D127).
pub fn record_count_line_on(app: &App, edit: CountEdit) -> UiResult<StockCountView> {
    guard::require(app, Permission::StockCount)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let at = now();

    let materials = app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| mb_db::Repos::new(tx).stock().materials(OUTLET, true))
            .map_err(|e| words::from_db(&e))
    })?;
    let Some(material) = materials.iter().find(|m| m.id.as_str() == edit.material_id.trim())
    else {
        return Err(UiError::new("count.material", "That material is not on file."));
    };

    let units = material.units();
    let unit = if edit.unit.trim().is_empty() {
        material.dimension.base_unit().to_owned()
    } else {
        edit.unit.trim().to_owned()
    };
    let typed = Qty::parse(edit.counted.trim()).map_err(|e| {
        UiError::new(
            "count.qty",
            format!("`{}` is not an amount of {}.", edit.counted, material.name),
        )
        .with_detail(e.to_string())
    })?;
    let base = units.to_base(typed, &unit).map_err(|e| {
        UiError::new("count.unit", format!("`{unit}` is not a unit of {}.", material.name))
            .with_detail(e.to_string())
    })?;

    let id = edit.count_id.clone();
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx).counts().record_line(
                    OUTLET,
                    &id,
                    &material.id,
                    // **D109's pair travels together** — the base-unit truth
                    // and what the person actually wrote on the sheet.
                    &mb_db::repo::counts::Written { base, typed, unit: unit.clone() },
                    at,
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;
    stock_count_on(app, Some(edit.count_id))
}

pub fn explain_count_line_on(
    app: &App,
    count_id: String,
    material_id: String,
    reason_id: Option<String>,
    note: String,
) -> UiResult<StockCountView> {
    guard::require(app, Permission::StockCount)?;
    let id = count_id.clone();
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx).counts().explain_line(
                    OUTLET,
                    &id,
                    &MaterialId::new(material_id.clone()),
                    reason_id.as_deref(),
                    if note.trim().is_empty() { None } else { Some(note.trim()) },
                )
            })
            .map_err(|e| words::from_db(&e))
    })?;
    stock_count_on(app, Some(count_id))
}

pub fn remove_count_line_on(
    app: &App,
    count_id: String,
    material_id: String,
) -> UiResult<StockCountView> {
    guard::require(app, Permission::StockCount)?;
    let id = count_id.clone();
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                mb_db::Repos::new(tx).counts().remove_line(
                    OUTLET,
                    &id,
                    &MaterialId::new(material_id.clone()),
                )
            })
            .map_err(|e| words::from_db(&e))
    })?;
    stock_count_on(app, Some(count_id))
}

/// **Approve: post the deltas and seal it** (D127, D129).
///
/// Needs `stock.adjust` and not a permission of its own, because approving a
/// count IS adjusting stock by hand, at scale.
pub fn approve_stock_count_on(app: &App, id: String) -> UiResult<StockCountView> {
    let who = guard::require(app, Permission::StockAdjust)?;
    crate::licensing::gate(app, Feature::Inventory)?;
    let at = now();
    let day = today(at);

    // **D75 — the refusal is a VALUE.** A `DbError` here would be rewritten by
    // `words::from_db` into "The shop's data could not be read", and D77's door
    // would never be mentioned to the person standing in front of it.
    let moved = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| -> Result<Result<usize, UiError>, mb_db::DbError> {
                let repos = mb_db::Repos::new(tx);
                let Some(count) = repos.counts().count(OUTLET, &id)? else {
                    return Ok(Err(UiError::new(
                        "count.missing",
                        "That count is not on file. Refresh and try again.",
                    )));
                };
                // **D77 again.** A count that would move stock inside a day
                // somebody has closed and locked is refused, and the way past is
                // to reopen the day rather than to be handed a permission.
                if repos.corrections().day_is_locked(OUTLET, day)? {
                    return Ok(Err(UiError::new(
                        "count.day_locked",
                        format!(
                            "{day} has been closed and locked, so nothing may move in it. \
                             Reopen the day (Reports → Day close), then approve this count."
                        ),
                    )));
                }
                let moved =
                    repos.counts().approve(OUTLET, &id, at, day, Some(&who.staff_id))?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::COUNT_APPROVED,
                        "stock_count",
                    )
                    .about(id.clone())
                    .with_after(serde_json::json!({
                        "location": count.location,
                        "materials_moved": moved,
                        "variance_paise": count.variance_value().paise(),
                    })),
                )?;
                Ok(Ok(moved))
            })
            .map_err(|e| words::from_db(&e))
    })??;
    log_info!("stock count approved: {moved} materials moved");
    stock_count_on(app, Some(id))
}

pub fn abandon_stock_count_on(app: &App, id: String, reason: String) -> UiResult<StockCountView> {
    let who = guard::require(app, Permission::StockCount)?;
    let at = now();
    if reason.trim().is_empty() {
        return Err(UiError::new("count.reason", "Say why the count is being given up."));
    }
    let count_id = id.clone();
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.counts().abandon(OUTLET, &count_id, reason.trim(), at)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        today(at),
                        Some(who.staff_id.clone()),
                        action::COUNT_ABANDONED,
                        "stock_count",
                    )
                    .about(count_id.clone())
                    .with_after(serde_json::json!({ "reason": reason.trim() })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;
    stock_count_on(app, None)
}

/// **The count sheet — D128, and the book quantity is deliberately not on it.**
///
/// Returns the document as text the screen can print through the shop's own
/// printer or copy. A wide blank column, the unit already beside it, grouped by
/// where the material is kept.
pub fn count_sheet_on(app: &App, location: String) -> UiResult<String> {
    guard::require(app, Permission::StockCount)?;
    let day = today(now());
    let location = if location.trim().is_empty() {
        mb_db::repo::stock::DEFAULT_LOCATION.to_owned()
    } else {
        location.trim().to_owned()
    };

    let materials = app.with_shop(|shop| {
        shop.db
            .read_transaction(|tx| mb_db::Repos::new(tx).stock().materials(OUTLET, false))
            .map_err(|e| words::from_db(&e))
    })?;
    Ok(sheet(&materials, &location, &day.to_string()))
}

/// The sheet itself, as a pure function so **D128 has a test**.
///
/// The book quantity is not a parameter. It cannot be printed by accident,
/// because it is not here to print.
pub fn sheet(materials: &[Material], location: &str, date: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("STOCK COUNT SHEET — {location}\n{date}\n\n"));
    let mut any = false;
    for material in materials.iter().filter(|m| m.location == location) {
        any = true;
        // Name, the unit to count in, and a blank. **Nothing else.**
        out.push_str(&format!(
            "{:<28} {:>6}  ______________\n",
            truncate(&material.name, 28),
            material.default_purchase_unit()
        ));
    }
    if !any {
        out.push_str("Nothing is kept here yet.\n");
    }
    out.push_str("\nCounted by ____________________   Checked by ____________________\n");
    out
}

fn truncate(text: &str, at: usize) -> String {
    if text.chars().count() <= at {
        return text.to_owned();
    }
    text.chars().take(at.saturating_sub(1)).collect::<String>() + "…"
}

// ---------------------------------------------------------------------------
// Shapes.
// ---------------------------------------------------------------------------

fn view_of(
    open: Option<mb_db::repo::counts::StockCount>,
    materials: &[Material],
    reasons: Vec<crate::corrections::ReasonView>,
    history: Vec<CountSummaryView>,
    ever_counted: bool,
    may_approve: bool,
    threshold: Money,
) -> StockCountView {
    let note = if ever_counted {
        String::new()
    } else {
        "Nobody has counted this store yet, so every stock figure is what the software \
         worked out from your recipes and not what is on the shelf."
            .to_owned()
    };

    let Some(count) = open else {
        return StockCountView {
            id: None,
            location: mb_db::repo::stock::DEFAULT_LOCATION.to_owned(),
            state: "Not started".to_owned(),
            state_tag: "none".to_owned(),
            date: String::new(),
            opened_by: String::new(),
            lines: Vec::new(),
            remaining: to_count(materials, &[]),
            effect: String::new(),
            short_value: MoneyView::from(Money::ZERO),
            over_value: MoneyView::from(Money::ZERO),
            net_value: MoneyView::from(Money::ZERO),
            totals_says: String::new(),
            reasons,
            history,
            may_approve,
            reason_above: MoneyView::from(threshold),
            note,
        };
    };

    let by_id: std::collections::BTreeMap<&str, &Material> =
        materials.iter().map(|m| (m.id.as_str(), m)).collect();
    let mut short = Money::ZERO;
    let mut over = Money::ZERO;
    let lines: Vec<CountLineView> = count
        .lines
        .iter()
        .map(|line| {
            let material = by_id.get(line.material_id.as_str());
            let units = material.map(|m| m.units());
            let say = |qty: Qty| match &units {
                Some(units) => units.say(qty),
                None => qty.to_string(),
            };
            if line.variance_value.is_negative() {
                short = short.add(line.variance_value).unwrap_or(short);
            } else {
                over = over.add(line.variance_value).unwrap_or(over);
            }
            CountLineView {
                material_id: line.material_id.to_string(),
                material: line.material_name.clone(),
                counted: say(line.counted_qty),
                book: say(line.book_qty),
                variance: if line.variance_qty.is_zero() {
                    "matches".to_owned()
                } else if line.variance_qty.is_negative() {
                    format!("{} short", say(line.variance_qty.abs()))
                } else {
                    format!("{} over", say(line.variance_qty))
                },
                variance_value: MoneyView::from(line.variance_value),
                is_short: line.variance_qty.is_negative(),
                is_over: line.variance_qty.is_positive(),
                needs_reason: line.variance_value.abs() >= threshold
                    && line.reason_id.is_none()
                    && line.note.is_none(),
                reason_id: line.reason_id.clone(),
                note: line.note.clone(),
            }
        })
        .collect();

    let counted: Vec<String> =
        count.lines.iter().map(|l| l.material_id.to_string()).collect();
    let (up, down) = count.effect();
    let effect = if up == 0 && down == 0 {
        "Nothing has changed — the shelf and the book agree.".to_owned()
    } else {
        format!(
            "This will add to {} and take away from {}.",
            words::count(up as i64, "material", "materials"),
            words::count(down as i64, "material", "materials")
        )
    };

    StockCountView {
        id: Some(count.id.clone()),
        location: count.location.clone(),
        state: count.state.label().to_owned(),
        state_tag: count.state.tag().to_owned(),
        date: count.business_day.to_string(),
        opened_by: count.opened_by.as_ref().map(ToString::to_string).unwrap_or_default(),
        lines,
        remaining: to_count(materials, &counted),
        effect,
        short_value: MoneyView::from(short.abs()),
        over_value: MoneyView::from(over),
        net_value: MoneyView::from(count.variance_value()),
        totals_says: totals_sentence(short, over, count.variance_value()),
        reasons,
        history,
        may_approve: may_approve && count.state == CountState::Draft,
        reason_above: MoneyView::from(threshold),
        note,
    }
}

/// **What the count is worth, said once.**
///
/// The first version showed three labelled figures — `Short −600.00`,
/// `Over 0.00`, `Net −600.00` — and the minus sign beside the word "short" says
/// the same thing twice, in two notations, one of which is a hyphen. §6: a
/// sentence is composed in Rust, not assembled out of fragments on a screen.
fn totals_sentence(short: Money, over: Money, net: Money) -> String {
    if short.is_zero() && over.is_zero() {
        return "Everything counted matches the book.".to_owned();
    }
    let mut parts = Vec::new();
    if !short.is_zero() {
        parts.push(format!("{} short", short.abs().to_plain_string()));
    }
    if !over.is_zero() {
        parts.push(format!("{} over", over.to_plain_string()));
    }
    let said = parts.join(", ");
    if short.is_zero() || over.is_zero() {
        return format!("{said}.");
    }
    // Both directions, so the net is worth saying — and which way it goes is a
    // word, never a sign.
    format!(
        "{said} — {} {} in all.",
        net.abs().to_plain_string(),
        if net.is_negative() { "short" } else { "over" }
    )
}

fn to_count(materials: &[Material], done: &[String]) -> Vec<ToCountView> {
    materials
        .iter()
        .filter(|m| !done.iter().any(|id| id == m.id.as_str()))
        .map(|material| {
            let units = material.units();
            ToCountView {
                material_id: material.id.to_string(),
                material: material.name.clone(),
                base_unit: material.dimension.base_unit().to_owned(),
                units: units.all().map(|p| p.name.clone()).collect(),
                default_unit: material.default_purchase_unit(),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The seats (D46).
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn stock_count(app: tauri::State<'_, App>, id: Option<String>) -> UiResult<StockCountView> {
    stock_count_on(&app, id)
}

#[tauri::command]
pub fn open_stock_count(
    app: tauri::State<'_, App>,
    location: String,
) -> UiResult<StockCountView> {
    open_stock_count_on(&app, location)
}

#[tauri::command]
pub fn record_count_line(
    app: tauri::State<'_, App>,
    edit: CountEdit,
) -> UiResult<StockCountView> {
    record_count_line_on(&app, edit)
}

#[tauri::command]
pub fn explain_count_line(
    app: tauri::State<'_, App>,
    count_id: String,
    material_id: String,
    reason_id: Option<String>,
    note: String,
) -> UiResult<StockCountView> {
    explain_count_line_on(&app, count_id, material_id, reason_id, note)
}

#[tauri::command]
pub fn remove_count_line(
    app: tauri::State<'_, App>,
    count_id: String,
    material_id: String,
) -> UiResult<StockCountView> {
    remove_count_line_on(&app, count_id, material_id)
}

#[tauri::command]
pub fn approve_stock_count(app: tauri::State<'_, App>, id: String) -> UiResult<StockCountView> {
    approve_stock_count_on(&app, id)
}

#[tauri::command]
pub fn abandon_stock_count(
    app: tauri::State<'_, App>,
    id: String,
    reason: String,
) -> UiResult<StockCountView> {
    abandon_stock_count_on(&app, id, reason)
}

#[tauri::command]
pub fn count_sheet(app: tauri::State<'_, App>, location: String) -> UiResult<String> {
    count_sheet_on(&app, location)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mb_core::Dimension;

    /// **T10 — the count sheet has no book quantity in it.**
    ///
    /// One line, against a decision a future session will otherwise "improve" by
    /// helpfully printing what the software thinks is there. A person holding a
    /// clipboard that already says 12.5 kg writes down 12.5 kg, every variance
    /// goes to zero, and the module quietly stops working.
    #[test]
    fn the_count_sheet_never_prints_the_book_quantity() {
        let mut rice = Material::new(MaterialId::new("mat_rice"), "Rice", Dimension::Weight);
        rice.packs = vec![("bag".to_owned(), Qty::from_whole(25_000).expect("in range"))];
        rice.purchase_unit = Some("bag".to_owned());

        let printed = sheet(&[rice], "Store", "12 Aug 2026");
        assert!(printed.contains("Rice"), "the sheet names the material");
        assert!(printed.contains("bag"), "and the unit to count it in");
        assert!(printed.contains("______"), "and leaves a blank to write in");
        // The book figure for this material is 50,000 g / 2 bags. Neither may
        // appear, in any form.
        for forbidden in ["50000", "50,000", "2 bag", "12.5"] {
            assert!(
                !printed.contains(forbidden),
                "the count sheet printed a book quantity ({forbidden}) — D128"
            );
        }
    }

    #[test]
    fn an_empty_location_says_so_rather_than_printing_a_blank_page() {
        let printed = sheet(&[], "Cold room", "12 Aug 2026");
        assert!(printed.contains("Nothing is kept here yet"));
    }
}
