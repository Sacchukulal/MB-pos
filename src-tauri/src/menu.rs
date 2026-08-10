//! **The menu, as an owner edits it** — scope 2.1, 2.3, 2.4, 4.1, 6.1–6.3.
//!
//! > *"v1's menu was: category, name, price. That is all… so it could not bill
//! > a bar, an AC/non-AC outlet or anyone selling packaged goods, and it could
//! > never compute a real margin."*
//!
//! Bodies over `&App` (D46), and every one of them is gated on `menu.manage`
//! by `guard::require` — which is the control, not the rail item.
//!
//! # Cost price does not leave this process without permission
//!
//! Scope 4.1 stores it so P18 can show a margin. It is the owner's business and
//! not the counter's, so `reports.view` decides whether it crosses the IPC
//! boundary at all. **Hiding a column in React would send it anyway** — that is
//! P11's lesson about courtesies, and it applies to data as much as to buttons.

use mb_auth::audit::action;
use mb_auth::{AuditEntry, Permission};
use mb_core::{CategoryId, ItemId, Money, TaxClassId, TaxRate, TaxTreatment};
use mb_db::repo::menu::{Category, MenuItem};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::flows::{now, today};
use crate::guard;
use crate::ipc::MoneyView;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};
use crate::log_info;

// ---------------------------------------------------------------------------
// What the menu screen sees.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct TaxClassView {
    pub id: String,
    pub name: String,
    /// Preformatted — "5%", "12.5%". R8: TypeScript divides nothing, ever.
    pub rate: String,
    /// "Added on top", "Included in the price", "Exempt", "Outside GST".
    pub treatment: String,
    pub is_active: bool,
    /// How many items would move if this class changed. The screen says it out
    /// loud before an owner edits a rate.
    pub items_using: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct CategoryView {
    pub id: String,
    pub name: String,
    pub sort_order: i64,
    pub is_active: bool,
    pub item_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct MenuRowView {
    pub id: String,
    pub name: String,
    pub category_id: Option<String>,
    pub price: MoneyView,
    pub tax_class_id: Option<String>,
    /// What the item is actually charged at today — "5%", and the treatment in
    /// words, so a screen never has to work it out.
    pub rate: String,
    pub hsn: Option<String>,
    pub short_code: Option<String>,
    /// **Absent without `reports.view`.** Scope 4.1 is the owner's margin, not
    /// the counter's business — and this is refused in Rust rather than hidden.
    pub cost: Option<MoneyView>,
    /// Only when the cost is known and visible. Preformatted (R8).
    pub margin: Option<String>,
    pub is_open_price: bool,
    pub is_available: bool,
    /// Scope 3.5 — which course this dish belongs to, for the kitchen screen.
    /// Blank means no course, and a menu where every dish is blank fires the
    /// whole order at once (P24).
    #[serde(default)]
    pub course: Option<String>,
    /// Scope 3.6 — how many minutes the kitchen is expected to take. Blank
    /// means no target, and a ticket with no target never turns late.
    #[serde(default)]
    pub prep_minutes: Option<String>,
    pub variants: i64,
}

/// What the screen sends back when something is edited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct MenuEdit {
    pub id: String,
    pub name: String,
    pub category_id: Option<String>,
    /// Typed by a person: "120", "120.50". Parsed by Rust (D39).
    pub price: String,
    pub tax_class_id: Option<String>,
    pub hsn: Option<String>,
    pub short_code: Option<String>,
    pub cost: Option<String>,
    pub is_open_price: bool,
    pub is_available: bool,
    /// Scope 3.5 — the course. Blank means no course.
    #[serde(default)]
    pub course: Option<String>,
    /// Scope 3.6 — minutes, typed by a person and parsed in Rust (D39).
    #[serde(default)]
    pub prep_minutes: Option<String>,
}

// ---------------------------------------------------------------------------
// Reading.
// ---------------------------------------------------------------------------

pub fn tax_classes_on(app: &App) -> UiResult<Vec<TaxClassView>> {
    guard::require(app, Permission::MenuManage)?;
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let mut out = Vec::new();
                for class in repos.tax_classes().list(OUTLET)? {
                    let items_using = repos.tax_classes().items_using(&class.id)?;
                    out.push(TaxClassView {
                        id: class.id.as_str().to_owned(),
                        name: class.name.clone(),
                        rate: class.rate.label(),
                        treatment: treatment_words(class.treatment).to_owned(),
                        is_active: class.is_active,
                        items_using,
                    });
                }
                Ok(out)
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// The treatment, in words a shopkeeper uses — UI_GUIDELINES §6.
const fn treatment_words(treatment: TaxTreatment) -> &'static str {
    match treatment {
        TaxTreatment::Exclusive => "Tax added on top",
        TaxTreatment::Inclusive => "Tax included in the price",
        TaxTreatment::Exempt => "Exempt",
        TaxTreatment::NonGst => "Outside GST",
    }
}

pub fn categories_on(app: &App) -> UiResult<Vec<CategoryView>> {
    guard::require(app, Permission::MenuManage)?;
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let items = repos.menu().list_items(OUTLET, false)?;
                Ok(repos
                    .menu()
                    .list_categories(OUTLET)?
                    .into_iter()
                    .map(|c| CategoryView {
                        item_count: items
                            .iter()
                            .filter(|i| i.category_id.as_ref() == Some(&c.id))
                            .count()
                            .try_into()
                            .unwrap_or(i64::MAX),
                        id: c.id.as_str().to_owned(),
                        name: c.name,
                        sort_order: c.sort_order,
                        is_active: c.is_active,
                    })
                    .collect())
            })
            .map_err(|e| words::from_db(&e))
    })
}

pub fn menu_rows_on(app: &App) -> UiResult<Vec<MenuRowView>> {
    let who = guard::require(app, Permission::MenuManage)?;
    // **The margin is a separate permission from the menu.** A manager who may
    // change a price is not automatically somebody who may see what the shop
    // pays for it.
    let may_see_cost = who.can(Permission::ReportsView);

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let mut out = Vec::new();
                for item in repos.menu().list_items(OUTLET, false)? {
                    let variants = repos
                        .composition()
                        .variants_of(&item.id)?
                        .len()
                        .try_into()
                        .unwrap_or(0);
                    let cost = if may_see_cost { item.cost_price } else { None };
                    out.push(MenuRowView {
                        id: item.id.as_str().to_owned(),
                        name: item.name.clone(),
                        category_id: item.category_id.as_ref().map(|c| c.as_str().to_owned()),
                        price: MoneyView::from(item.unit_price),
                        tax_class_id: item.tax_class_id.as_ref().map(|c| c.as_str().to_owned()),
                        rate: format!(
                            "{} · {}",
                            item.tax_rate.label(),
                            treatment_words(item.tax_treatment)
                        ),
                        hsn: item.hsn.clone(),
                        short_code: item.short_code.clone(),
                        margin: cost.and_then(|cost| margin_label(item.unit_price, cost)),
                        cost: cost.map(MoneyView::from),
                        is_open_price: item.is_open_price,
                        is_available: item.is_available,
                        // P24 — what the kitchen screen needs to know about
                        // this dish. Formatted here, so the menu screen shows
                        // "12 min" without doing arithmetic (R8).
                        course: item.course.clone(),
                        prep_minutes: item.prep_minutes.map(|m| m.to_string()),
                        variants,
                    });
                }
                Ok(out)
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// "58%" — the gross margin, formatted in Rust.
///
/// `None` when the item is free, because a margin on nothing is a division by
/// zero dressed as a number.
#[allow(
    clippy::integer_division,
    reason = "a percentage for a screen, not money — the money is price and cost"
)]
fn margin_label(price: Money, cost: Money) -> Option<String> {
    if !price.is_positive() {
        return None;
    }
    let margin = price.sub(cost).ok()?;
    let percent = i128::from(margin.paise()) * 100 / i128::from(price.paise());
    Some(format!("{percent}%"))
}

// ---------------------------------------------------------------------------
// Writing.
// ---------------------------------------------------------------------------

/// Save one item.
///
/// **The tax class decides the rate**, not the screen: the class is resolved
/// here and its rate written onto the item, so an item and its class can never
/// disagree (D56).
pub fn save_item_on(app: &App, edit: MenuEdit) -> UiResult<Vec<MenuRowView>> {
    let who = guard::require(app, Permission::MenuManage)?;
    let at = now();
    let day = today(at);

    if edit.name.trim().is_empty() {
        return Err(UiError::new("menu.name", "An item needs a name."));
    }
    let price = parse_money(&edit.price, "price")?;
    let cost = match edit.cost.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        Some(text) => Some(parse_money(text, "cost")?),
        None => None,
    };

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let before = repos.menu().find_item(&ItemId::new(edit.id.clone()))?;

                // The class, resolved. An item with no class keeps whatever it
                // had — an imported one-off is a real thing and must not be
                // silently moved to 5%.
                let (rate, treatment) = match &edit.tax_class_id {
                    Some(id) => {
                        let class = repos
                            .tax_classes()
                            .find(OUTLET, &TaxClassId::new(id.clone()))?
                            .ok_or_else(|| {
                                mb_db::DbError::invariant(
                                    "that tax class is not one this shop has",
                                )
                            })?;
                        (class.rate, class.treatment)
                    }
                    None => before.as_ref().map_or(
                        (TaxRate::ZERO, TaxTreatment::Exclusive),
                        |b| (b.tax_rate, b.tax_treatment),
                    ),
                };

                let item = MenuItem {
                    id: ItemId::new(edit.id.clone()),
                    category_id: edit.category_id.clone().map(CategoryId::new),
                    name: edit.name.trim().to_owned(),
                    unit_price: price,
                    tax_class_id: edit.tax_class_id.clone().map(TaxClassId::new),
                    tax_rate: rate,
                    tax_treatment: treatment,
                    hsn: edit.hsn.clone().filter(|h| !h.trim().is_empty()),
                    cost_price: cost,
                    short_code: edit.short_code.clone().filter(|s| !s.trim().is_empty()),
                    // Scope 3.6 — the kitchen screen's target. Typed as text
                    // and parsed HERE, because D39 says a number a person
                    // types is parsed in Rust and never in the browser.
                    // Unreadable text keeps whatever was there rather than
                    // silently clearing a target the kitchen depends on.
                    prep_minutes: edit
                        .prep_minutes
                        .as_ref()
                        .map(|text| text.trim())
                        .filter(|text| !text.is_empty())
                        .map_or_else(
                            || before.as_ref().and_then(|b| b.prep_minutes),
                            |text| {
                                text.parse::<i64>()
                                    .ok()
                                    .or_else(|| before.as_ref().and_then(|b| b.prep_minutes))
                            },
                        ),
                    // Scope 3.5 — the course. Blank means no course, and a
                    // blank menu fires the whole order at once.
                    course: edit
                        .course
                        .clone()
                        .map(|c| c.trim().to_owned())
                        .filter(|c| !c.is_empty()),
                    is_open_price: edit.is_open_price,
                    is_available: edit.is_available,
                    sort_order: before.as_ref().map_or(0, |b| b.sort_order),
                };
                repos.menu().save_item(OUTLET, &item, at)?;

                // **R11 / audit C4: "who changed a price".** In the same
                // transaction as the change.
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::PRICE_CHANGED,
                        "menu_item",
                    )
                    .about(edit.id.clone())
                    .changed(
                        before.as_ref().map_or(serde_json::Value::Null, item_json),
                        item_json(&item),
                    ),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    log_info!("{} changed the menu item {}", who.name, edit.name);
    menu_rows_on(app)
}

/// **Never the cost price.** The audit trail is read by anybody with
/// `audit.view`, and what the shop pays for a dosa is a narrower secret than
/// what it charges.
fn item_json(item: &MenuItem) -> serde_json::Value {
    serde_json::json!({
        "name": item.name,
        "price_paise": item.unit_price.paise(),
        "rate": item.tax_rate.label(),
        "hsn": item.hsn,
        "is_available": item.is_available,
    })
}

/// "86 it" — scope 3.5. **Stops new orders and disturbs no open one** (T10).
pub fn set_available_on(app: &App, item_id: String, available: bool) -> UiResult<Vec<MenuRowView>> {
    let who = guard::require(app, Permission::MenuManage)?;
    let at = now();
    let day = today(at);

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos
                    .menu()
                    .set_available(OUTLET, &ItemId::new(item_id.clone()), available, at)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::PRICE_CHANGED,
                        "menu_item",
                    )
                    .about(item_id.clone())
                    .with_after(serde_json::json!({ "is_available": available })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    menu_rows_on(app)
}

/// Save a category. **A category with items cannot be retired** — P13 item 6,
/// and the message says what to do instead rather than letting a foreign key
/// produce "the shop's data rejected that".
pub fn save_category_on(
    app: &App,
    id: String,
    name: String,
    is_active: bool,
    // P24 — which kitchen screen this category's food goes to. `None` keeps
    // what is already set; blank text means the shop's one screen.
    station: Option<String>,
) -> UiResult<Vec<CategoryView>> {
    guard::require(app, Permission::MenuManage)?;
    let at = now();

    if name.trim().is_empty() {
        return Err(UiError::new("menu.category_name", "A category needs a name."));
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let category = CategoryId::new(id.clone());
                if !is_active {
                    let in_it = repos
                        .menu()
                        .list_items(OUTLET, false)?
                        .into_iter()
                        .filter(|i| i.category_id.as_ref() == Some(&category))
                        .count();
                    if in_it > 0 {
                        return Err(mb_db::DbError::invariant(format!(
                            "{name} still has {in_it} item(s). Move them to another \
                             category first, or make those items unavailable."
                        )));
                    }
                }
                let existing = repos
                    .menu()
                    .list_categories(OUTLET)?
                    .into_iter()
                    .find(|c| c.id == category);
                repos.menu().save_category(
                    OUTLET,
                    &Category {
                        id: category,
                        name: name.trim().to_owned(),
                        sort_order: existing.as_ref().map_or(0, |c| c.sort_order),
                        is_active,
                        // **This is how a shop makes a second kitchen screen**
                        // (P24). Blank means the one screen; typing "Tandoor"
                        // here sends this category's food to a screen of that
                        // name. Adding, renaming and removing a section is
                        // therefore editing the categories that belong to it —
                        // there is no separate station screen to learn.
                        station: station
                            .map(|s| s.trim().to_owned())
                            .filter(|s| !s.is_empty())
                            .or_else(|| existing.as_ref().and_then(|c| c.station.clone())),
                    },
                    at,
                )
            })
            .map_err(|e| words::from_db(&e))
    })?;

    categories_on(app)
}

/// Save a tax class, and say how many items moved with it.
pub fn save_tax_class_on(
    app: &App,
    id: String,
    name: String,
    rate: String,
    treatment: String,
) -> UiResult<String> {
    let who = guard::require(app, Permission::SettingsTax)?;
    let at = now();
    let day = today(at);

    let bp = mb_auth::RoleShape::parse_percent(&rate)
        .map_err(|e| UiError::new("menu.rate", e.to_string()))?
        .unwrap_or(0);
    let rate = TaxRate::from_basis_points(bp)
        .ok_or_else(|| UiError::new("menu.rate", "A tax rate is between 0% and 100%."))?;
    let treatment = match treatment.as_str() {
        "exclusive" => TaxTreatment::Exclusive,
        "inclusive" => TaxTreatment::Inclusive,
        "exempt" => TaxTreatment::Exempt,
        "non_gst" => TaxTreatment::NonGst,
        other => {
            return Err(UiError::new(
                "menu.treatment",
                format!("\"{other}\" is not a way tax can work."),
            ));
        }
    };

    let moved = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let class_id = TaxClassId::new(id.clone());
                let before = repos.tax_classes().find(OUTLET, &class_id)?;
                let mut class = before.clone().unwrap_or_else(|| {
                    mb_core::TaxClass::new(class_id.clone(), name.clone(), rate, treatment)
                });
                class.name = name.trim().to_owned();
                class.rate = rate;
                class.treatment = treatment;

                let moved = repos.tax_classes().save(OUTLET, &class, at)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::SETTING_CHANGED,
                        "tax_class",
                    )
                    .about(id.clone())
                    .changed(
                        before.as_ref().map_or(serde_json::Value::Null, |b| {
                            serde_json::json!({ "name": b.name, "rate": b.rate.label() })
                        }),
                        serde_json::json!({ "name": class.name, "rate": class.rate.label() }),
                    ),
                )?;
                Ok(moved)
            })
            .map_err(|e| words::from_db(&e))
    })?;

    Ok(match moved {
        0 => format!("{name} saved."),
        1 => format!("{name} saved. One item now charges {}.", rate.label()),
        n => format!("{name} saved. {n} items now charge {}.", rate.label()),
    })
}

/// **T8** — a percentage across a category, exact to the paisa on every item.
pub fn change_prices_on(
    app: &App,
    category_id: Option<String>,
    percent: String,
) -> UiResult<String> {
    let who = guard::require(app, Permission::MenuManage)?;
    let at = now();
    let day = today(at);

    let bp = mb_auth::RoleShape::parse_percent(percent.trim_start_matches(['+', '-']))
        .map_err(|e| UiError::new("menu.percent", e.to_string()))?
        .ok_or_else(|| UiError::new("menu.percent", "Type a percentage — 10, or 12.5."))?;
    let down = percent.trim().starts_with('-');

    let changed = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let wanted = category_id.clone().map(CategoryId::new);
                let mut changed = 0_usize;
                for mut item in repos.menu().list_items(OUTLET, false)? {
                    if wanted.is_some() && item.category_id != wanted {
                        continue;
                    }
                    // **One rounding rule.** `mul_ratio` is the only place a
                    // money value is rounded in this product (P00), so a bulk
                    // rise cannot disagree with a discount.
                    let delta = item
                        .unit_price
                        .mul_ratio(i64::from(bp), 10_000)
                        .map_err(|e| mb_db::DbError::invariant(e.to_string()))?;
                    let before = item.unit_price;
                    item.unit_price = if down {
                        before.sub(delta)
                    } else {
                        before.add(delta)
                    }
                    .map_err(|e| mb_db::DbError::invariant(e.to_string()))?;

                    repos.menu().save_item(OUTLET, &item, at)?;
                    repos.audit().append(
                        OUTLET,
                        &AuditEntry::new(
                            at,
                            day,
                            Some(who.staff_id.clone()),
                            action::PRICE_CHANGED,
                            "menu_item",
                        )
                        .about(item.id.as_str().to_owned())
                        .changed(
                            serde_json::json!({ "price_paise": before.paise() }),
                            serde_json::json!({ "price_paise": item.unit_price.paise() }),
                        ),
                    )?;
                    changed += 1;
                }
                Ok(changed)
            })
            .map_err(|e| words::from_db(&e))
    })?;

    let direction = if down { "down" } else { "up" };
    Ok(format!("{changed} item(s) went {direction}."))
}

/// A price or a cost, as a person typed it.
/// The same parser, for the sessions that came after P13 and would otherwise
/// write a second one. Money is parsed in Rust, in one place (D39).
pub fn parse_money_public(text: &str) -> UiResult<Money> {
    parse_money(text, "amount")
}

fn parse_money(text: &str, what: &'static str) -> UiResult<Money> {
    Money::parse(text.trim()).map_err(|e| {
        UiError::new(
            "menu.money",
            format!("\"{text}\" is not a {what}. Try 120, or 120.50."),
        )
        .with_detail(e.to_string())
    })
}

// ---------------------------------------------------------------------------
// The command seats (D46).
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn menu_tax_classes(app: tauri::State<'_, App>) -> UiResult<Vec<TaxClassView>> {
    tax_classes_on(&app)
}

#[tauri::command]
pub fn menu_categories(app: tauri::State<'_, App>) -> UiResult<Vec<CategoryView>> {
    categories_on(&app)
}

#[tauri::command]
pub fn menu_rows(app: tauri::State<'_, App>) -> UiResult<Vec<MenuRowView>> {
    menu_rows_on(&app)
}

#[tauri::command]
pub fn save_menu_item(app: tauri::State<'_, App>, edit: MenuEdit) -> UiResult<Vec<MenuRowView>> {
    save_item_on(&app, edit)
}

#[tauri::command]
pub fn set_item_available(
    app: tauri::State<'_, App>,
    item_id: String,
    available: bool,
) -> UiResult<Vec<MenuRowView>> {
    set_available_on(&app, item_id, available)
}

#[tauri::command]
pub fn save_menu_category(
    app: tauri::State<'_, App>,
    id: String,
    name: String,
    is_active: bool,
    station: Option<String>,
) -> UiResult<Vec<CategoryView>> {
    save_category_on(&app, id, name, is_active, station)
}

#[tauri::command]
pub fn save_tax_class(
    app: tauri::State<'_, App>,
    id: String,
    name: String,
    rate: String,
    treatment: String,
) -> UiResult<String> {
    save_tax_class_on(&app, id, name, rate, treatment)
}

#[tauri::command]
pub fn change_menu_prices(
    app: tauri::State<'_, App>,
    category_id: Option<String>,
    percent: String,
) -> UiResult<String> {
    change_prices_on(&app, category_id, percent)
}

// ---------------------------------------------------------------------------
// The spreadsheet — P13 item 7.
//
// "A shop with 400 items will not type them in, and a setup nobody finishes is
// a sale nobody keeps."
// ---------------------------------------------------------------------------

/// What an import would do, before it does anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ImportPlanView {
    /// The whole sentence, written in Rust: "312 new item(s) and 88 change(s)."
    pub summary: String,
    pub new_items: i64,
    pub updated_items: i64,
    /// "Line 4: there is no category called \"Snaks\"" — the line number is the
    /// one in the owner's spreadsheet, counting the header as line 1.
    pub refused: Vec<String>,
    /// Nothing may be imported until this is true.
    pub is_clean: bool,
}

/// Read a file and say what would happen. **Writes nothing.**
pub fn plan_import_on(app: &App, csv: String) -> UiResult<ImportPlanView> {
    guard::require(app, Permission::MenuManage)?;
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let plan = mb_db::Repos::new(tx).menu_csv().plan(OUTLET, &csv)?;
                Ok(ImportPlanView {
                    summary: plan.summary(),
                    new_items: plan.new_items.len().try_into().unwrap_or(i64::MAX),
                    updated_items: plan.updated_items.len().try_into().unwrap_or(i64::MAX),
                    refused: plan
                        .refused
                        .iter()
                        .map(|(line, why)| format!("Line {line}: {why}"))
                        .collect(),
                    is_clean: plan.is_clean(),
                })
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// Do it — **planning again inside the same transaction**, so what is written
/// is what the file says now rather than what it said when the owner looked.
///
/// The alternative is passing the plan across the IPC boundary and back, which
/// would let a screen edit it. A plan is a decision about a file, and the file
/// is the thing worth trusting.
pub fn run_import_on(app: &App, csv: String) -> UiResult<String> {
    let who = guard::require(app, Permission::MenuManage)?;
    let at = now();
    let day = today(at);

    let written = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let plan = repos.menu_csv().plan(OUTLET, &csv)?;
                let written = repos.menu_csv().apply(OUTLET, &plan, at)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::PRICE_CHANGED,
                        "menu",
                    )
                    .with_after(serde_json::json!({
                        "imported": written,
                        "new": plan.new_items.len(),
                        "changed": plan.updated_items.len(),
                    })),
                )?;
                Ok(written)
            })
            .map_err(|e| words::from_db(&e))
    })?;

    log_info!("{} imported {written} menu item(s)", who.name);
    Ok(match written {
        0 => "Nothing was imported — the file had no rows.".to_owned(),
        1 => "One item imported.".to_owned(),
        n => format!("{n} items imported."),
    })
}

/// The whole menu as a spreadsheet.
pub fn export_menu_on(app: &App) -> UiResult<String> {
    guard::require(app, Permission::MenuManage)?;
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).menu_csv().export(OUTLET))
            .map_err(|e| words::from_db(&e))
    })
}

#[tauri::command]
pub fn plan_menu_import(app: tauri::State<'_, App>, csv: String) -> UiResult<ImportPlanView> {
    plan_import_on(&app, csv)
}

#[tauri::command]
pub fn run_menu_import(app: tauri::State<'_, App>, csv: String) -> UiResult<String> {
    run_import_on(&app, csv)
}

#[tauri::command]
pub fn export_menu(app: tauri::State<'_, App>) -> UiResult<String> {
    export_menu_on(&app)
}

// ---------------------------------------------------------------------------
// What one item is made of — scope 6.1, 6.2, 6.3.
//
// Variants, modifier groups and combos each have their storage and their rules
// already (mb-db's `composition`, mb-core's `combo`). This is the way in: the
// screens that let an owner set them up, which is the last thing P13 owes.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct VariantView {
    pub id: String,
    pub name: String,
    pub price: MoneyView,
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ModifierView {
    pub id: String,
    pub name: String,
    /// Preformatted, and it may be negative — "No onion, −10.00" is a real line
    /// on a real menu. R8: TypeScript formats nothing.
    pub price_delta: MoneyView,
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ModifierGroupView {
    pub id: String,
    pub name: String,
    /// **`u32`, not `i64`, and the reason is the wire.** `ts-rs` renders an
    /// `i64` as a TypeScript `bigint`, and `JSON.stringify` — which is what
    /// Tauri's `invoke` uses — throws on one. A screen that honestly built a
    /// `1n` could not send it back. Caught by saving a group and reading
    /// *"Do not know how to serialize a BigInt"* (P13).
    ///
    /// A group with four billion choices is not a thing, so nothing is lost.
    pub min_select: u32,
    pub max_select: Option<u32>,
    /// The rule in words — "Choose one", "Any number". Worked out once, here,
    /// rather than in every screen that shows a group (UI_GUIDELINES §6).
    pub rule: String,
    pub modifiers: Vec<ModifierView>,
    /// Whether THIS item offers it. Only meaningful inside `item_composition`.
    pub attached: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ItemComposition {
    pub item_id: String,
    pub item_name: String,
    pub variants: Vec<VariantView>,
    /// **Every group the shop has**, each flagged with whether this item offers
    /// it — so attaching one is a tick rather than a retyping. A shop has
    /// "Spice level" once, not once per curry.
    pub groups: Vec<ModifierGroupView>,
}

/// The rule a group enforces, in words a shopkeeper reads.
fn group_rule(min: i64, max: Option<i64>) -> String {
    match (min, max) {
        (0, None) => "Any number".to_owned(),
        (0, Some(1)) => "One at most".to_owned(),
        (1, Some(1)) => "Choose one".to_owned(),
        (0, Some(n)) => format!("Up to {n}"),
        (n, None) => format!("At least {n}"),
        (a, Some(b)) if a == b => format!("Choose {a}"),
        (a, Some(b)) => format!("Choose {a} to {b}"),
    }
}

/// A stored count, made sendable. A number this could not hold is a number the
/// database should not have, so it clamps rather than failing a whole screen.
fn narrow(count: i64) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

fn modifier_view(m: &mb_db::repo::composition::Modifier) -> ModifierView {
    ModifierView {
        id: m.id.as_str().to_owned(),
        name: m.name.clone(),
        price_delta: MoneyView::from(m.price_delta),
        is_active: m.is_active,
    }
}

fn group_view(g: &mb_db::repo::composition::ModifierGroup, attached: bool) -> ModifierGroupView {
    ModifierGroupView {
        id: g.id.clone(),
        name: g.name.clone(),
        min_select: narrow(g.min_select),
        max_select: g.max_select.map(narrow),
        rule: group_rule(g.min_select, g.max_select),
        modifiers: g.modifiers.iter().map(modifier_view).collect(),
        attached,
    }
}

pub fn item_composition_on(app: &App, item_id: String) -> UiResult<ItemComposition> {
    guard::require(app, Permission::MenuManage)?;
    let id = ItemId::new(item_id);

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let item = repos.menu().find_item(&id)?.ok_or_else(|| {
                    mb_db::DbError::invariant("that item is not on the menu any more")
                })?;
                let (variants, attached) = repos.composition().for_item(OUTLET, &id)?;
                let all = repos.composition().groups(OUTLET)?;

                Ok(ItemComposition {
                    item_id: item.id.as_str().to_owned(),
                    item_name: item.name,
                    variants: variants
                        .iter()
                        .map(|v| VariantView {
                            id: v.id.as_str().to_owned(),
                            name: v.name.clone(),
                            price: MoneyView::from(v.unit_price),
                            is_active: v.is_active,
                        })
                        .collect(),
                    groups: all
                        .iter()
                        .map(|g| group_view(g, attached.iter().any(|a| a.id == g.id)))
                        .collect(),
                })
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// Add or edit a size — scope 6.1.
///
/// **A variant carries its own price, not a discount off the parent.** A half
/// plate is not a discounted full plate: it is a different thing to cook, at a
/// price the owner sets, and it lands on its own line of a rate summary.
pub fn save_variant_on(
    app: &App,
    item_id: String,
    variant_id: String,
    name: String,
    price: String,
    is_active: bool,
) -> UiResult<ItemComposition> {
    let who = guard::require(app, Permission::MenuManage)?;
    let at = now();
    let day = today(at);

    if name.trim().is_empty() {
        return Err(UiError::new(
            "menu.variant_name",
            "A size needs a name — Half, Full, 500g.",
        ));
    }
    let price = parse_money(&price, "price")?;

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.composition().save_variant(
                    OUTLET,
                    &mb_db::repo::composition::Variant {
                        id: ItemId::new(variant_id.clone()),
                        item_id: ItemId::new(item_id.clone()),
                        name: name.trim().to_owned(),
                        unit_price: price,
                        sort_order: 0,
                        is_active,
                    },
                    at,
                )?;
                // R11 / audit C4 — a variant IS a price, so it is a price change.
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::PRICE_CHANGED,
                        "item_variant",
                    )
                    .about(variant_id.clone())
                    .with_after(serde_json::json!({
                        "item": item_id,
                        "name": name.trim(),
                        "price_paise": price.paise(),
                        "is_available": is_active,
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    item_composition_on(app, item_id)
}

/// What the screen sends when a group is edited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct GroupEdit {
    pub id: String,
    pub name: String,
    /// See `ModifierGroupView` — `u32` so it can cross the wire at all.
    pub min_select: u32,
    /// `None` means "any number".
    pub max_select: Option<u32>,
    pub modifiers: Vec<ModifierEdit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ModifierEdit {
    pub id: String,
    pub name: String,
    /// Typed by a person, and it may lead with a minus. Parsed by Rust (D39).
    pub price_delta: String,
}

pub fn list_groups_on(app: &App) -> UiResult<Vec<ModifierGroupView>> {
    guard::require(app, Permission::MenuManage)?;
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| mb_db::Repos::new(tx).composition().groups(OUTLET))
            .map_err(|e| words::from_db(&e))
            .map(|groups| groups.iter().map(|g| group_view(g, false)).collect())
    })
}

/// Save a group and everything in it.
///
/// **An impossible group is refused as it is written**, not when a cashier
/// meets it mid-rush: mb-db's `ModifierGroup::check` is the same rule, and
/// "choose at least 3 of 2" fails here.
pub fn save_group_on(app: &App, group: GroupEdit) -> UiResult<Vec<ModifierGroupView>> {
    let who = guard::require(app, Permission::MenuManage)?;
    let at = now();
    let day = today(at);

    if group.name.trim().is_empty() {
        return Err(UiError::new(
            "menu.group_name",
            "A group needs a name — Spice level, Add-ons.",
        ));
    }

    let mut modifiers = Vec::new();
    for m in &group.modifiers {
        if m.name.trim().is_empty() {
            return Err(UiError::new("menu.modifier_name", "A choice needs a name."));
        }
        modifiers.push(mb_db::repo::composition::Modifier {
            id: mb_core::ModifierId::new(m.id.clone()),
            name: m.name.trim().to_owned(),
            price_delta: parse_delta(&m.price_delta)?,
            sort_order: 0,
            is_active: true,
        });
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                repos.composition().save_group(
                    OUTLET,
                    &mb_db::repo::composition::ModifierGroup {
                        id: group.id.clone(),
                        name: group.name.trim().to_owned(),
                        min_select: i64::from(group.min_select),
                        max_select: group.max_select.map(i64::from),
                        sort_order: 0,
                        modifiers: modifiers.clone(),
                    },
                    at,
                )?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::PRICE_CHANGED,
                        "modifier_group",
                    )
                    .about(group.id.clone())
                    .with_after(serde_json::json!({
                        "name": group.name.trim(),
                        "min": group.min_select,
                        "max": group.max_select,
                        "choices": modifiers.len(),
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    list_groups_on(app)
}

/// A modifier's price difference, **which may be negative**.
///
/// "No cheese, −10" takes money off, and stripping the minus would quietly
/// charge for it. Blank means free, because most choices are.
fn parse_delta(text: &str) -> UiResult<Money> {
    let text = text.trim();
    let (negative, rest) = match text.strip_prefix('-').or_else(|| text.strip_prefix('\u{2212}')) {
        Some(rest) => (true, rest.trim()),
        None => (false, text),
    };
    if rest.is_empty() {
        return Ok(Money::ZERO);
    }
    let amount = parse_money(rest, "price")?;
    Ok(if negative { amount.neg() } else { amount })
}

/// Offer a group on an item, or stop offering it — scope 6.2.
pub fn attach_group_on(
    app: &App,
    item_id: String,
    group_id: String,
    attach: bool,
) -> UiResult<ItemComposition> {
    guard::require(app, Permission::MenuManage)?;
    let at = now();
    let id = ItemId::new(item_id.clone());

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                if attach {
                    repos.composition().attach_group(OUTLET, &id, &group_id, 0, at)
                } else {
                    repos.composition().detach_group(OUTLET, &id, &group_id, at)
                }
            })
            .map_err(|e| words::from_db(&e))
    })?;

    item_composition_on(app, item_id)
}

// ---------------------------------------------------------------------------
// Combos — scope 6.3.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ComboView {
    pub id: String,
    pub name: String,
    pub price: MoneyView,
    pub is_active: bool,
    pub parts: Vec<ComboPartView>,
    /// What the parts cost bought separately — so an owner can see what the
    /// deal gives away without doing the arithmetic on paper.
    pub separately: MoneyView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ComboPartView {
    pub item_id: String,
    pub item_name: String,
    pub qty: String,
    /// This part's slice of the combo price by D14's rule — **the money**, not
    /// the stored proportion, so it is right today rather than on the day the
    /// combo was made.
    pub share: MoneyView,
    /// "5%", so a mixed-rate combo shows why it has to be apportioned at all.
    pub rate: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ComboEdit {
    pub id: String,
    pub name: String,
    pub price: String,
    pub is_active: bool,
    /// `(item id, quantity as typed)`.
    pub parts: Vec<(String, String)>,
}

pub fn list_combos_on(app: &App) -> UiResult<Vec<ComboView>> {
    guard::require(app, Permission::MenuManage)?;

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let items = repos.menu().list_items(OUTLET, false)?;
                let mut out = Vec::new();

                for combo in repos.composition().combos(OUTLET)? {
                    // **The shares are recomputed from today's prices** rather
                    // than read back from `share_bp` (D53). A component's price
                    // moves and a stored share would be wrong in a way nobody
                    // would ever notice.
                    let parts: Vec<mb_core::ComboComponent> = combo
                        .components
                        .iter()
                        .map(|p| mb_core::ComboComponent {
                            item_id: p.item_id.clone(),
                            qty: p.qty,
                            standalone: items
                                .iter()
                                .find(|i| i.id == p.item_id)
                                .map_or(Money::ZERO, |i| i.unit_price),
                        })
                        .collect();

                    let shares = mb_core::apportion(combo.unit_price, &parts).unwrap_or_default();
                    let separately = Money::try_sum(
                        parts.iter().filter_map(|p| p.qty.extend(p.standalone).ok()),
                    )
                    .unwrap_or(Money::ZERO);

                    out.push(ComboView {
                        id: combo.id.clone(),
                        name: combo.name.clone(),
                        price: MoneyView::from(combo.unit_price),
                        is_active: combo.is_active,
                        separately: MoneyView::from(separately),
                        parts: combo
                            .components
                            .iter()
                            .map(|p| {
                                let item = items.iter().find(|i| i.id == p.item_id);
                                ComboPartView {
                                    item_id: p.item_id.as_str().to_owned(),
                                    item_name: item.map_or_else(
                                        || "An item that is no longer on the menu".to_owned(),
                                        |i| i.name.clone(),
                                    ),
                                    qty: p.qty.to_string(),
                                    share: MoneyView::from(
                                        shares
                                            .iter()
                                            .find(|s| s.item_id == p.item_id)
                                            .map_or(Money::ZERO, |s| s.share),
                                    ),
                                    rate: item
                                        .map_or_else(|| "—".to_owned(), |i| i.tax_rate.label()),
                                }
                            })
                            .collect(),
                    });
                }
                Ok(out)
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// Save a combo. The apportionment is the repository's, by D14's rule.
pub fn save_combo_on(app: &App, combo: ComboEdit) -> UiResult<Vec<ComboView>> {
    let who = guard::require(app, Permission::MenuManage)?;
    let at = now();
    let day = today(at);

    if combo.name.trim().is_empty() {
        return Err(UiError::new("menu.combo_name", "A combo needs a name."));
    }
    if combo.parts.is_empty() {
        return Err(UiError::new(
            "menu.combo_empty",
            "A combo needs at least one thing in it.",
        ));
    }
    let price = parse_money(&combo.price, "price")?;

    let mut parts = Vec::new();
    for (item_id, qty) in &combo.parts {
        let parsed = mb_core::Qty::parse(qty.trim()).map_err(|e| {
            UiError::new(
                "menu.combo_qty",
                format!("\"{qty}\" is not a quantity. Try 1, 2 or 0.5."),
            )
            .with_detail(e.to_string())
        })?;
        parts.push(mb_db::repo::composition::ComboPart {
            item_id: ItemId::new(item_id.clone()),
            qty: parsed,
            // Filled in by `save_combo`, which is the only thing allowed to
            // decide a share.
            share_bp: 0,
        });
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                // The standalone prices the shares are worked out from, read
                // now so the stored proportions match today's menu.
                let standalone: Vec<(ItemId, Money)> = repos
                    .menu()
                    .list_items(OUTLET, false)?
                    .into_iter()
                    .map(|i| (i.id, i.unit_price))
                    .collect();

                repos.composition().save_combo(
                    OUTLET,
                    &mb_db::repo::composition::Combo {
                        id: combo.id.clone(),
                        name: combo.name.trim().to_owned(),
                        unit_price: price,
                        is_active: combo.is_active,
                        components: parts.clone(),
                    },
                    &standalone,
                    at,
                )?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::PRICE_CHANGED,
                        "combo",
                    )
                    .about(combo.id.clone())
                    .with_after(serde_json::json!({
                        "name": combo.name.trim(),
                        "price_paise": price.paise(),
                        "parts": parts.len(),
                        "is_available": combo.is_active,
                    })),
                )?;
                Ok(())
            })
            .map_err(|e| words::from_db(&e))
    })?;

    log_info!("{} saved the combo {}", who.name, combo.name);
    list_combos_on(app)
}

// --- the seats -------------------------------------------------------------

#[tauri::command]
pub fn item_composition(app: tauri::State<'_, App>, item_id: String) -> UiResult<ItemComposition> {
    item_composition_on(&app, item_id)
}

#[tauri::command]
pub fn save_item_variant(
    app: tauri::State<'_, App>,
    item_id: String,
    variant_id: String,
    name: String,
    price: String,
    is_active: bool,
) -> UiResult<ItemComposition> {
    save_variant_on(&app, item_id, variant_id, name, price, is_active)
}

#[tauri::command]
pub fn list_modifier_groups(app: tauri::State<'_, App>) -> UiResult<Vec<ModifierGroupView>> {
    list_groups_on(&app)
}

#[tauri::command]
pub fn save_modifier_group(
    app: tauri::State<'_, App>,
    group: GroupEdit,
) -> UiResult<Vec<ModifierGroupView>> {
    save_group_on(&app, group)
}

#[tauri::command]
pub fn attach_modifier_group(
    app: tauri::State<'_, App>,
    item_id: String,
    group_id: String,
    attach: bool,
) -> UiResult<ItemComposition> {
    attach_group_on(&app, item_id, group_id, attach)
}

#[tauri::command]
pub fn list_combos(app: tauri::State<'_, App>) -> UiResult<Vec<ComboView>> {
    list_combos_on(&app)
}

#[tauri::command]
pub fn save_combo(app: tauri::State<'_, App>, combo: ComboEdit) -> UiResult<Vec<ComboView>> {
    save_combo_on(&app, combo)
}
