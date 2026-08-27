use mb_auth::audit::action;
use mb_auth::{AuditEntry, Permission};
use mb_core::{CategoryId, ItemId, Money, TaxClassId, TaxRate};
use mb_db::repo::menu::{Category, MenuItem};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::flows::{now, today};
use crate::guard;
use crate::ipc::MoneyView;
use crate::log_info;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

// What the menu screen sees.

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct TaxClassView {
    pub id: String,
    pub name: String,
    /// Preformatted — "5%", "12.5%".
    pub rate: String,
    /// The same rate in basis points, so the editor can send it back unchanged.
    pub rate_bp: u32,
    /// The machine values. The editor sends these back; it never reads the words, so rewording
    /// a label cannot change what a class taxes.
    #[ts(type = "\"gst\" | \"exempt\" | \"outside_gst\" | \"untaxed\"")]
    pub kind: mb_core::TaxKind,
    #[ts(type = "\"exclusive\" | \"inclusive\"")]
    pub basis: mb_core::PriceBasis,
    /// For a person to read in the list.
    pub treatment: String,
    pub is_active: bool,
    /// How many items would move if this class changed.
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
    /// What the item is actually charged at today — "5%", and the treatment in words, so a
    /// screen never has to work it out.
    pub rate: String,
    pub hsn: Option<String>,
    pub short_code: Option<String>,
    pub cost: Option<MoneyView>,
    /// Only when the cost is known and visible.
    pub margin: Option<String>,
    pub is_open_price: bool,
    pub is_available: bool,
    /// Which course this dish belongs to, for the kitchen screen.
    #[serde(default)]
    pub course: Option<String>,
    /// How many minutes the kitchen is expected to take.
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
    /// Typed by a person: "120", "120.50".
    pub price: String,
    pub tax_class_id: Option<String>,
    pub hsn: Option<String>,
    pub short_code: Option<String>,
    pub cost: Option<String>,
    pub is_open_price: bool,
    pub is_available: bool,
    /// The course.
    #[serde(default)]
    pub course: Option<String>,
    /// Minutes, typed by a person and parsed in Rust.
    #[serde(default)]
    pub prep_minutes: Option<String>,
}

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
                        rate: class.tax.rate.label(),
                        rate_bp: class.tax.rate.basis_points(),
                        kind: class.tax.kind,
                        basis: class.tax.basis,
                        treatment: tax_words(class.tax).to_owned(),
                        is_active: class.is_active,
                        items_using,
                    });
                }
                Ok(out)
            })
            .map_err(|e| words::from_db(&e))
    })
}

/// The tax, in words a shopkeeper reads.
const fn tax_words(tax: mb_core::TaxSpec) -> &'static str {
    match tax.kind {
        mb_core::TaxKind::Exempt => "Exempt",
        mb_core::TaxKind::OutsideGst => "Outside GST",
        mb_core::TaxKind::Untaxed => "No tax",
        mb_core::TaxKind::Gst => match tax.basis {
            mb_core::PriceBasis::Exclusive => "Tax added on top",
            mb_core::PriceBasis::Inclusive => "Tax included in the price",
        },
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
    // The margin is a separate permission from the menu.
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
                        rate: format!("{} · {}", item.tax.rate.label(), tax_words(item.tax)),
                        hsn: item.hsn.clone(),
                        short_code: item.short_code.clone(),
                        margin: cost.and_then(|cost| margin_label(item.unit_price, cost)),
                        cost: cost.map(MoneyView::from),
                        is_open_price: item.is_open_price,
                        is_available: item.is_available,
                        // What the kitchen screen needs to know about this dish.
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

/// Save one item.
pub fn save_item_on(app: &App, edit: MenuEdit) -> UiResult<Vec<MenuRowView>> {
    let who = guard::require(app, Permission::MenuManage)?;
    let at = now();
    let day = today(at);

    if edit.name.trim().is_empty() {
        return Err(UiError::new("menu.name", "An item needs a name."));
    }
    check_hsn(edit.hsn.as_deref())?;
    let price = parse_money(&edit.price, "price")?;
    let cost = match edit
        .cost
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        Some(text) => Some(parse_money(text, "cost")?),
        None => None,
    };

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let before = repos.menu().find_item(&ItemId::new(edit.id.clone()))?;

                // The class, resolved. An item with no class keeps whatever it had — an
                // imported one-off is a real thing and must not be silently moved to 5%.
                let tax = match &edit.tax_class_id {
                    Some(id) => {
                        let class = repos
                            .tax_classes()
                            .find(OUTLET, &TaxClassId::new(id.clone()))?
                            .ok_or_else(|| {
                                mb_db::DbError::invariant("that tax class is not one this shop has")
                            })?;
                        class.tax
                    }
                    // No class chosen: keep what the item already had, or start it as ordinary
                    // GST at nil rate.
                    None => before
                        .as_ref()
                        .map_or(mb_core::TaxSpec::gst(TaxRate::ZERO), |b| b.tax),
                };

                let item = MenuItem {
                    id: ItemId::new(edit.id.clone()),
                    category_id: edit.category_id.clone().map(CategoryId::new),
                    name: edit.name.trim().to_owned(),
                    unit_price: price,
                    tax_class_id: edit.tax_class_id.clone().map(TaxClassId::new),
                    tax,
                    hsn: edit.hsn.clone().filter(|h| !h.trim().is_empty()),
                    cost_price: cost,
                    short_code: edit.short_code.clone().filter(|s| !s.trim().is_empty()),
                    // The kitchen screen's target.
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
                    // The course.
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

/// An HSN or SAC code is 2, 4, 6 or 8 digits — or nothing at all.
fn check_hsn(hsn: Option<&str>) -> UiResult<()> {
    let Some(code) = hsn.map(str::trim).filter(|h| !h.is_empty()) else {
        return Ok(());
    };
    if matches!(code.len(), 2 | 4 | 6 | 8) && code.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(());
    }
    Err(UiError::new(
        "menu.hsn",
        "An HSN or SAC code is 2, 4, 6 or 8 digits, or blank.",
    ))
}

/// Never the cost price.
fn item_json(item: &MenuItem) -> serde_json::Value {
    serde_json::json!({
        "name": item.name,
        "price_paise": item.unit_price.paise(),
        "rate": item.tax.rate.label(),
        "hsn": item.hsn,
        "is_available": item.is_available,
    })
}

/// "86 it".
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

/// Save a category. A category with items cannot be retired.
pub fn save_category_on(
    app: &App,
    id: String,
    name: String,
    is_active: bool,
    // Which kitchen screen this category's food goes to.
    station: Option<String>,
) -> UiResult<Vec<CategoryView>> {
    guard::require(app, Permission::MenuManage)?;
    let at = now();

    if name.trim().is_empty() {
        return Err(UiError::new(
            "menu.category_name",
            "A category needs a name.",
        ));
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
                        // This is how a shop makes a second kitchen screen.
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
    kind: mb_core::TaxKind,
    basis: mb_core::PriceBasis,
) -> UiResult<String> {
    let who = guard::require(app, Permission::SettingsTax)?;
    let at = now();
    let day = today(at);

    let bp = mb_auth::RoleShape::parse_percent(&rate)
        .map_err(|e| UiError::new("menu.rate", e.to_string()))?
        .unwrap_or(0);
    let rate = TaxRate::from_basis_points(bp)
        .ok_or_else(|| UiError::new("menu.rate", "A tax rate is between 0% and 100%."))?;
    // Outside GST carries a STATE VAT rate, and the rate typed on this screen is that rate.
    let tax = mb_core::TaxSpec { kind, rate, basis };

    // The rate is free entry, so "exempt at 5%" is typeable and refused here.
    if !tax.is_coherent() {
        return Err(UiError::new(
            "menu.rate",
            "Exempt and no-tax classes have no rate. Set it to 0, or change the kind.",
        ));
    }

    let moved = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let class_id = TaxClassId::new(id.clone());
                let before = repos.tax_classes().find(OUTLET, &class_id)?;
                let mut class = before
                    .clone()
                    .unwrap_or_else(|| mb_core::TaxClass::new(class_id.clone(), name.clone(), tax));
                class.name = name.trim().to_owned();
                class.tax = tax;

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
                        before.as_ref().map_or(
                            serde_json::Value::Null,
                            |b| serde_json::json!({ "name": b.name, "rate": b.tax.rate.label() }),
                        ),
                        serde_json::json!({ "name": class.name, "rate": class.tax.rate.label() }),
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

/// A percentage across a category, exact to the paisa on every item.
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
                    // One rounding rule. `mul_ratio` is the only place a money value is rounded
                    // in this product, so a bulk rise cannot disagree with a discount.
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

// The command seats.

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
    kind: mb_core::TaxKind,
    basis: mb_core::PriceBasis,
) -> UiResult<String> {
    save_tax_class_on(&app, id, name, rate, kind, basis)
}

#[tauri::command]
pub fn change_menu_prices(
    app: tauri::State<'_, App>,
    category_id: Option<String>,
    percent: String,
) -> UiResult<String> {
    change_prices_on(&app, category_id, percent)
}

// The spreadsheet.

/// What an import would do, before it does anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ImportPlanView {
    /// The whole sentence, written in Rust: "312 new item(s) and 88 change(s).".
    pub summary: String,
    pub new_items: i64,
    pub updated_items: i64,
    /// "Line 4: there is no category called \"Snaks\"".
    pub refused: Vec<String>,
    /// Nothing may be imported until this is true.
    pub is_clean: bool,
}

/// Read a file and say what would happen.
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

/// Do it.
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

// What one item is made of.

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
    /// Preformatted, and it may be negative — "No onion, −10.00" is a real line on a real menu.
    pub price_delta: MoneyView,
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ModifierGroupView {
    pub id: String,
    pub name: String,
    /// `u32`, not `i64`, and the reason is the wire.
    pub min_select: u32,
    pub max_select: Option<u32>,
    /// The rule in words — "Choose one", "Any number".
    pub rule: String,
    pub modifiers: Vec<ModifierView>,
    /// Whether THIS item offers it.
    pub attached: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ItemComposition {
    pub item_id: String,
    pub item_name: String,
    pub variants: Vec<VariantView>,
    /// Every group the shop has, each flagged with whether this item offers it — so attaching
    /// one is a tick rather than a retyping.
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

/// A stored count, made sendable.
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

/// Add or edit a size.
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
    /// See `ModifierGroupView`.
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
    /// Typed by a person, and it may lead with a minus.
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

/// A modifier's price difference, which may be negative.
fn parse_delta(text: &str) -> UiResult<Money> {
    let text = text.trim();
    let (negative, rest) = match text
        .strip_prefix('-')
        .or_else(|| text.strip_prefix('\u{2212}'))
    {
        Some(rest) => (true, rest.trim()),
        None => (false, text),
    };
    if rest.is_empty() {
        return Ok(Money::ZERO);
    }
    let amount = parse_money(rest, "price")?;
    Ok(if negative { amount.neg() } else { amount })
}

/// Offer a group on an item, or stop offering it.
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
                    repos
                        .composition()
                        .attach_group(OUTLET, &id, &group_id, 0, at)
                } else {
                    repos.composition().detach_group(OUTLET, &id, &group_id, at)
                }
            })
            .map_err(|e| words::from_db(&e))
    })?;

    item_composition_on(app, item_id)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ComboView {
    pub id: String,
    pub name: String,
    pub price: MoneyView,
    pub is_active: bool,
    pub parts: Vec<ComboPartView>,
    /// What the parts cost bought separately.
    pub separately: MoneyView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct ComboPartView {
    pub item_id: String,
    pub item_name: String,
    pub qty: String,
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
                    // The shares are recomputed from today's prices rather than read back from
                    // `share_bp`.
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
                                        .map_or_else(|| "—".to_owned(), |i| i.tax.rate.label()),
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
            // Filled in by `save_combo`, which is the only thing allowed to decide a share.
            share_bp: 0,
        });
    }

    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                // The standalone prices the shares are worked out from, read now so the stored
                // proportions match today's menu.
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

// The seats.

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
