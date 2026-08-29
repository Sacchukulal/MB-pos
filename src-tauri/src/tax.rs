//! Settings › Tax — the one screen for tax.
//!
//! The slabs live here, the charges' slabs are catalogue settings in the same group, and this
//! is where an owner ticks items (or a whole category) and puts them on a slab. The Menu screen
//! only picks a slab for one item at a time; it does not define any.

use mb_auth::audit::action;
use mb_auth::{AuditEntry, Permission};
use mb_core::{CategoryId, ItemId, PriceBasis, TaxClass, TaxClassId, TaxRate, TaxSpec};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::flows::{now, today};
use crate::guard;
use crate::ipc::MoneyView;
use crate::log_info;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

/// One slab, as the Tax page lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct TaxSlabView {
    pub id: String,
    pub name: String,
    /// "5%", "2.5%" — the rate as words.
    pub rate: String,
    pub rate_bp: u32,
    #[ts(type = "\"gst\" | \"exempt\" | \"outside_gst\" | \"untaxed\"")]
    pub kind: mb_core::TaxKind,
    /// `shop`, `inclusive` or `exclusive` — the slab's own say on pricing.
    pub basis: String,
    /// "Added on top" / "In the price" / "Shop default (added on top)".
    pub price_words: String,
    pub is_active: bool,
    pub items_using: u32,
}

/// One item in the tick list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct TaxItemView {
    pub id: String,
    pub name: String,
    pub price: MoneyView,
    pub slab_id: String,
    pub slab_name: String,
    /// `shop`, `inclusive` or `exclusive` — the item's own say.
    pub basis: String,
    /// "5% · added on top" — what this item is actually taxed at today.
    pub words: String,
    pub is_available: bool,
}

/// A category and its items, for ticking a whole group at once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct TaxCategoryView {
    /// None for the items in no category.
    pub id: Option<String>,
    pub name: String,
    /// The slab a new item in this category starts on.
    pub default_slab_id: Option<String>,
    pub items: Vec<TaxItemView>,
}

/// The whole page, in one read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct TaxPageView {
    /// `inclusive` or `exclusive` — the shop's own default.
    pub shop_basis: String,
    /// Why bills are not what the registration box says — no GST number, no state.
    pub registration_note: Option<String>,
    pub slabs: Vec<TaxSlabView>,
    pub categories: Vec<TaxCategoryView>,
}

/// The item's or slab's say on pricing, in the word the screen sends.
#[must_use]
pub const fn basis_word(basis: Option<PriceBasis>) -> &'static str {
    match basis {
        None => "shop",
        Some(PriceBasis::Inclusive) => "inclusive",
        Some(PriceBasis::Exclusive) => "exclusive",
    }
}

/// The same word, read back. Anything else is refused.
pub fn basis_from_word(text: &str) -> UiResult<Option<PriceBasis>> {
    match text.trim() {
        "" | "shop" => Ok(None),
        "inclusive" => Ok(Some(PriceBasis::Inclusive)),
        "exclusive" => Ok(Some(PriceBasis::Exclusive)),
        other => Err(UiError::new(
            "tax.basis",
            "Pick how this is priced: shop default, tax in the price, or tax added on top.",
        )
        .with_detail(format!("price basis {other}"))),
    }
}

/// What a resolved tax is called, in shop words: "5% · added on top".
#[must_use]
pub fn tax_words(tax: TaxSpec) -> String {
    let priced = match tax.basis {
        PriceBasis::Exclusive => "added on top",
        PriceBasis::Inclusive => "in the price",
    };
    match tax.kind {
        mb_core::TaxKind::Exempt => "Exempt".to_owned(),
        mb_core::TaxKind::Untaxed => "No tax".to_owned(),
        mb_core::TaxKind::OutsideGst => format!("VAT {} · {priced}", tax.rate.label()),
        mb_core::TaxKind::Gst => format!("{} · {priced}", tax.rate.label()),
    }
}

fn slab_view(class: &TaxClass, shop: PriceBasis, items_using: i64) -> TaxSlabView {
    let price_words = match class.basis {
        Some(PriceBasis::Inclusive) => "In the price".to_owned(),
        Some(PriceBasis::Exclusive) => "Added on top".to_owned(),
        None => match shop {
            PriceBasis::Inclusive => "Shop default (in the price)".to_owned(),
            PriceBasis::Exclusive => "Shop default (added on top)".to_owned(),
        },
    };
    TaxSlabView {
        id: class.id.as_str().to_owned(),
        name: class.name.clone(),
        rate: class.rate.label(),
        rate_bp: class.rate.basis_points(),
        kind: class.kind,
        basis: basis_word(class.basis).to_owned(),
        price_words,
        is_active: class.is_active,
        items_using: u32::try_from(items_using).unwrap_or(u32::MAX),
    }
}

fn slabs_in(repos: &mb_db::Repos<'_>) -> Result<Vec<TaxSlabView>, mb_db::DbError> {
    let book = repos.tax_classes().book(OUTLET)?;
    let mut out = Vec::new();
    for class in &book.classes {
        let using = repos.tax_classes().items_using(&class.id)?;
        out.push(slab_view(class, book.shop_basis, using));
    }
    Ok(out)
}

pub fn slabs_on(app: &App) -> UiResult<Vec<TaxSlabView>> {
    guard::require(app, Permission::MenuManage)?;
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| slabs_in(&mb_db::Repos::new(tx)))
            .map_err(|e| words::from_db(&e))
    })
}

fn page_in(repos: &mb_db::Repos<'_>) -> Result<TaxPageView, mb_db::DbError> {
    let book = repos.tax_classes().book(OUTLET)?;
    let slabs = slabs_in(repos)?;
    let items = repos.menu().list_items(OUTLET, false)?;
    let categories = repos.menu().list_categories(OUTLET)?;

    let item_view = |item: &mb_db::repo::menu::MenuItem| TaxItemView {
        id: item.id.as_str().to_owned(),
        name: item.name.clone(),
        price: MoneyView::from(item.unit_price),
        slab_id: item.tax_class_id.as_str().to_owned(),
        slab_name: book
            .find(&item.tax_class_id)
            .map_or_else(|| "—".to_owned(), |c| c.name.clone()),
        basis: basis_word(item.price_basis).to_owned(),
        words: book
            .spec_for(&item.tax_class_id, item.price_basis)
            .map_or_else(|_| "No tax slab".to_owned(), tax_words),
        is_available: item.is_available,
    };

    let mut out: Vec<TaxCategoryView> = categories
        .iter()
        .filter(|c| c.is_active)
        .map(|c| TaxCategoryView {
            id: Some(c.id.as_str().to_owned()),
            name: c.name.clone(),
            default_slab_id: c.default_tax_class_id.as_ref().map(|s| s.as_str().to_owned()),
            items: items
                .iter()
                .filter(|i| i.category_id.as_ref() == Some(&c.id))
                .map(item_view)
                .collect(),
        })
        .collect();
    let loose: Vec<TaxItemView> = items
        .iter()
        .filter(|i| {
            i.category_id
                .as_ref()
                .is_none_or(|id| !categories.iter().any(|c| &c.id == id && c.is_active))
        })
        .map(item_view)
        .collect();
    if !loose.is_empty() {
        out.push(TaxCategoryView {
            id: None,
            name: "No category".to_owned(),
            default_slab_id: None,
            items: loose,
        });
    }
    let registration_note = repos
        .settings()
        .store_profile(OUTLET)?
        .and_then(|p| crate::settings::Store::from_profile(&p).registration_note());
    Ok(TaxPageView {
        shop_basis: crate::settings::price_basis_to(book.shop_basis).to_owned(),
        registration_note,
        slabs,
        categories: out,
    })
}

pub fn page_on(app: &App) -> UiResult<TaxPageView> {
    guard::require(app, Permission::MenuManage)?;
    app.with_shop(|shop| {
        shop.db
            .transaction(|tx| page_in(&mb_db::Repos::new(tx)))
            .map_err(|e| words::from_db(&e))
    })
}

/// Add or change a slab. Items on it need nothing done — they read it.
pub fn save_slab_on(
    app: &App,
    id: String,
    name: String,
    rate: String,
    kind: mb_core::TaxKind,
    basis: String,
) -> UiResult<Vec<TaxSlabView>> {
    let who = guard::require(app, Permission::SettingsTax)?;
    let at = now();
    let day = today(at);
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(UiError::new("tax.name", "A slab needs a name — GST 5%, Liquor."));
    }
    let bp = mb_auth::RoleShape::parse_percent(&rate)
        .map_err(|e| UiError::new("tax.rate", e.to_string()))?
        .unwrap_or(0);
    let rate = TaxRate::from_basis_points(bp)
        .ok_or_else(|| UiError::new("tax.rate", "A tax rate is between 0% and 100%."))?;
    let basis = basis_from_word(&basis)?;
    let class_id = TaxClassId::new(id.trim().to_owned());
    if class_id.as_str().is_empty() {
        return Err(UiError::new("tax.id", "This slab has no id. Reload and try again."));
    }
    let mut class = TaxClass::new(class_id.clone(), name.clone(), kind, rate);
    class.basis = basis;
    if !class.is_coherent() {
        return Err(UiError::new(
            "tax.rate",
            "Exempt and no-tax slabs have no rate. Set it to 0, or change the kind.",
        ));
    }
    let slabs = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let before = repos.tax_classes().find(OUTLET, &class_id)?;
                let mut fresh = class.clone();
                fresh.is_active = before.as_ref().is_none_or(|b| b.is_active);
                repos.tax_classes().save(OUTLET, &fresh, at)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::SETTING_CHANGED,
                        "tax_slab",
                    )
                    .about(class_id.as_str().to_owned())
                    .changed(
                        before.as_ref().map_or(serde_json::Value::Null, slab_json),
                        slab_json(&fresh),
                    ),
                )?;
                slabs_in(&repos)
            })
            .map_err(|e| words::from_db(&e))
    })?;
    app.reload_shop_config();
    log_info!("{} saved the tax slab {name} at {}", who.name, rate.label());
    Ok(slabs)
}

/// Take a slab away. Refused with the count while items or a charge still use it.
pub fn remove_slab_on(app: &App, id: String) -> UiResult<Vec<TaxSlabView>> {
    let who = guard::require(app, Permission::SettingsTax)?;
    let at = now();
    let day = today(at);
    let class_id = TaxClassId::new(id);
    let slabs = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let before = repos
                    .tax_classes()
                    .find(OUTLET, &class_id)?
                    .ok_or_else(|| mb_db::DbError::invariant("that tax slab is already gone"))?;
                repos.tax_classes().remove(OUTLET, &class_id, at)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::SETTING_CHANGED,
                        "tax_slab",
                    )
                    .about(class_id.as_str().to_owned())
                    .changed(slab_json(&before), serde_json::Value::Null),
                )?;
                slabs_in(&repos)
            })
            .map_err(|e| words::from_db(&e))
    })?;
    app.reload_shop_config();
    log_info!("{} removed the tax slab {}", who.name, class_id.as_str());
    Ok(slabs)
}

/// Put the ticked items on a slab and/or give them a pricing say. Either half may be left
/// alone by sending nothing for it.
pub fn set_items_on(
    app: &App,
    item_ids: Vec<String>,
    slab_id: Option<String>,
    basis: Option<String>,
) -> UiResult<TaxPageView> {
    let who = guard::require(app, Permission::SettingsTax)?;
    let at = now();
    let day = today(at);
    if item_ids.is_empty() {
        return Err(UiError::new("tax.items", "Tick at least one item first."));
    }
    let slab = slab_id
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .map(TaxClassId::new);
    let basis = match basis.as_deref() {
        None => None,
        Some(word) => Some(basis_from_word(word)?),
    };
    if slab.is_none() && basis.is_none() {
        return Err(UiError::new("tax.items", "Choose a slab or a price basis to apply."));
    }
    let items: Vec<ItemId> = item_ids.iter().map(|id| ItemId::new(id.clone())).collect();
    let page = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let changed = repos
                    .tax_classes()
                    .assign(OUTLET, &items, slab.as_ref(), basis, at)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::PRICE_CHANGED,
                        "menu_item",
                    )
                    .with_after(serde_json::json!({
                        "items": item_ids,
                        "changed": changed,
                        "slab": slab.as_ref().map(|s| s.as_str()),
                        "price_basis": basis.map(basis_word),
                    })),
                )?;
                page_in(&repos)
            })
            .map_err(|e| words::from_db(&e))
    })?;
    app.reload_shop_config();
    log_info!("{} moved {} item(s) on the tax page", who.name, items.len());
    Ok(page)
}

/// The slab a new item in this category starts on. Nothing already in it moves.
pub fn set_category_on(
    app: &App,
    category_id: String,
    slab_id: Option<String>,
) -> UiResult<TaxPageView> {
    let who = guard::require(app, Permission::SettingsTax)?;
    let at = now();
    let category = CategoryId::new(category_id);
    let slab = slab_id
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .map(TaxClassId::new);
    let page = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                if let Some(slab) = &slab {
                    let live = repos
                        .tax_classes()
                        .find(OUTLET, slab)?
                        .is_some_and(|c| c.is_active);
                    if !live {
                        return Err(mb_db::DbError::invariant(
                            "that tax slab is not one this shop has",
                        ));
                    }
                }
                let mut found = repos
                    .menu()
                    .list_categories(OUTLET)?
                    .into_iter()
                    .find(|c| c.id == category)
                    .ok_or_else(|| mb_db::DbError::invariant("that category is gone"))?;
                found.default_tax_class_id = slab.clone();
                repos.menu().save_category(OUTLET, &found, at)?;
                page_in(&repos)
            })
            .map_err(|e| words::from_db(&e))
    })?;
    log_info!("{} set a category's starting tax slab", who.name);
    Ok(page)
}

fn slab_json(class: &TaxClass) -> serde_json::Value {
    serde_json::json!({
        "name": class.name,
        "rate": class.rate.label(),
        "kind": class.kind,
        "basis": basis_word(class.basis),
        "is_active": class.is_active,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct TaxSlabEdit {
    pub id: String,
    pub name: String,
    /// Per cent, as typed: "5", "2.5".
    pub rate: String,
    #[ts(type = "\"gst\" | \"exempt\" | \"outside_gst\" | \"untaxed\"")]
    pub kind: mb_core::TaxKind,
    /// `shop`, `inclusive` or `exclusive`.
    pub basis: String,
}

#[tauri::command]
pub fn tax_slabs(app: tauri::State<'_, App>) -> UiResult<Vec<TaxSlabView>> {
    slabs_on(&app)
}

#[tauri::command]
pub fn tax_page(app: tauri::State<'_, App>) -> UiResult<TaxPageView> {
    page_on(&app)
}

#[tauri::command]
pub fn save_tax_slab(app: tauri::State<'_, App>, edit: TaxSlabEdit) -> UiResult<Vec<TaxSlabView>> {
    save_slab_on(&app, edit.id, edit.name, edit.rate, edit.kind, edit.basis)
}

#[tauri::command]
pub fn remove_tax_slab(app: tauri::State<'_, App>, id: String) -> UiResult<Vec<TaxSlabView>> {
    remove_slab_on(&app, id)
}

#[tauri::command]
pub fn set_items_tax(
    app: tauri::State<'_, App>,
    item_ids: Vec<String>,
    slab_id: Option<String>,
    basis: Option<String>,
) -> UiResult<TaxPageView> {
    set_items_on(&app, item_ids, slab_id, basis)
}

#[tauri::command]
pub fn set_category_tax(
    app: tauri::State<'_, App>,
    category_id: String,
    slab_id: Option<String>,
) -> UiResult<TaxPageView> {
    set_category_on(&app, category_id, slab_id)
}
