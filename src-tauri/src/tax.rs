//! Settings › Tax — the one screen for tax.
//!
//! Three rungs decide what an item is taxed at: the item's own rate, else its category's rate,
//! else the shop's rate. The shop's rate is a typed percentage; a category or a ticked item
//! picks a slab. The Menu screen never asks about tax.

use mb_auth::audit::action;
use mb_auth::{AuditEntry, Permission};
use mb_core::{
    CategoryId, ItemId, PriceBasis, TaxBook, TaxClass, TaxClassId, TaxKind, TaxRate, TaxSpec,
};
use serde::Serialize;
use ts_rs::TS;

use crate::flows::{now, today};
use crate::guard;
use crate::ipc::MoneyView;
use crate::log_info;
use crate::settings::ipc::ChoiceView;
use crate::state::{App, OUTLET};
use crate::words::{self, UiError, UiResult};

/// One slab, as the Tax page offers it.
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
    pub kind: TaxKind,
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
    /// `shop`, `inclusive` or `exclusive` — the item's own say.
    pub basis: String,
    /// "5% · added on top" — what this item is actually taxed at today.
    pub words: String,
    /// `shop`, `category` or `item` — which rung the rate comes from.
    pub from: String,
    pub is_available: bool,
}

/// A category and its items.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct TaxCategoryView {
    /// None for the items in no category.
    pub id: Option<String>,
    pub name: String,
    /// The category's own rate, when it has one.
    pub own_slab_id: Option<String>,
    /// "Shop rate" or "18%".
    pub rate_words: String,
    pub items: Vec<TaxItemView>,
}

/// The whole page, in one read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct TaxPageView {
    /// `unregistered`, `composition` or `regular`.
    pub registration: String,
    pub gstin: String,
    pub state_code: String,
    pub states: Vec<ChoiceView>,
    /// `inclusive` or `exclusive` — the shop's own default.
    pub shop_basis: String,
    /// The shop's rate as typed: "5".
    pub shop_rate: String,
    pub shop_slab_id: String,
    /// Whether bills carry GST at all.
    pub charges_gst: bool,
    /// The saved GST number judged; none when the box is empty.
    pub gstin_check: Option<GstinCheckView>,
    /// The live slabs, for picking.
    pub slabs: Vec<TaxSlabView>,
    pub categories: Vec<TaxCategoryView>,
}

/// The mark beside the GST number box: right, or what is off about it. Advice only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, export_to = "../../ui/src/ipc/generated/")]
#[serde(rename_all = "camelCase")]
pub struct GstinCheckView {
    pub fine: bool,
    pub says: String,
}

/// The number as typed, judged against its shape and the state. Empty is no mark at all.
#[must_use]
pub fn judge_gstin(gstin: &str, state_code: &str) -> Option<GstinCheckView> {
    if gstin.trim().is_empty() {
        return None;
    }
    Some(
        match crate::settings::catalog::judge_gstin(gstin, state_code) {
            Ok(()) => GstinCheckView {
                fine: true,
                says: "Looks right.".to_owned(),
            },
            Err(wrong) => GstinCheckView {
                fine: false,
                says: wrong.message,
            },
        },
    )
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
        TaxKind::Exempt => "Exempt".to_owned(),
        TaxKind::Untaxed => "No tax".to_owned(),
        TaxKind::OutsideGst => format!("VAT {} · {priced}", tax.rate.label()),
        TaxKind::Gst => format!("{} · {priced}", tax.rate.label()),
    }
}

/// The same words for an item, allowing for a shop that may not charge GST at all.
#[must_use]
pub fn item_words(
    book: &TaxBook,
    registration: mb_core::Registration,
    slab: &TaxClassId,
    basis: Option<PriceBasis>,
) -> String {
    match book.spec_for(slab, basis) {
        Err(_) => "No tax slab".to_owned(),
        Ok(spec) if spec.kind == TaxKind::Gst && !registration.charges_gst() => "No GST".to_owned(),
        Ok(spec) => tax_words(spec),
    }
}

/// A rate's name on the page: "5%", "Exempt", "Liquor — state VAT".
#[must_use]
fn rate_name(class: &TaxClass) -> String {
    match class.kind {
        TaxKind::Gst => class.rate.label(),
        TaxKind::Exempt => "Exempt".to_owned(),
        TaxKind::Untaxed => "No tax".to_owned(),
        TaxKind::OutsideGst => class.name.clone(),
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
        name: rate_name(class),
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

#[cfg(test)]
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
    let shop_slab = repos.tax_classes().shop_slab(OUTLET)?;
    let slabs: Vec<TaxSlabView> = slabs_in(repos)?
        .into_iter()
        .filter(|s| s.is_active)
        .collect();
    let items = repos.menu().list_items(OUTLET, false)?;
    let categories = repos.menu().list_categories(OUTLET)?;
    let store = repos
        .settings()
        .store_profile(OUTLET)?
        .map(|p| crate::settings::Store::from_profile(&p))
        .unwrap_or_default();
    // The rate the owner chose; a missing GST number is said in the note, not on every row.
    let registration = crate::settings::registration_from(&store.registration);

    let item_view =
        |item: &mb_db::repo::menu::MenuItem, follows: &TaxClassId, category_has_own: bool| {
            let from = if item.tax_class_id != *follows {
                "item"
            } else if category_has_own {
                "category"
            } else {
                "shop"
            };
            TaxItemView {
                id: item.id.as_str().to_owned(),
                name: item.name.clone(),
                price: MoneyView::from(item.unit_price),
                slab_id: item.tax_class_id.as_str().to_owned(),
                basis: basis_word(item.price_basis).to_owned(),
                words: item_words(&book, registration, &item.tax_class_id, item.price_basis),
                from: from.to_owned(),
                is_available: item.is_available,
            }
        };

    let mut out: Vec<TaxCategoryView> = Vec::new();
    for c in categories.iter().filter(|c| c.is_active) {
        let follows = repos.tax_classes().rate_for(OUTLET, Some(&c.id))?;
        let own = c
            .default_tax_class_id
            .as_ref()
            .filter(|id| book.find(id).is_some_and(|s| s.is_active));
        let rate_words = own
            .and_then(|id| book.find(id))
            .map_or_else(|| "Shop rate".to_owned(), rate_name);
        out.push(TaxCategoryView {
            id: Some(c.id.as_str().to_owned()),
            name: c.name.clone(),
            own_slab_id: own.map(|id| id.as_str().to_owned()),
            rate_words,
            items: items
                .iter()
                .filter(|i| i.category_id.as_ref() == Some(&c.id))
                .map(|i| item_view(i, &follows, own.is_some()))
                .collect(),
        });
    }
    let loose: Vec<TaxItemView> = items
        .iter()
        .filter(|i| {
            i.category_id
                .as_ref()
                .is_none_or(|id| !categories.iter().any(|c| &c.id == id && c.is_active))
        })
        .map(|i| item_view(i, &shop_slab, false))
        .collect();
    if !loose.is_empty() {
        out.push(TaxCategoryView {
            id: None,
            name: "No category".to_owned(),
            own_slab_id: None,
            rate_words: "Shop rate".to_owned(),
            items: loose,
        });
    }
    let shop_rate = book
        .find(&shop_slab)
        .map(|c| c.rate.label().trim_end_matches('%').to_owned())
        .unwrap_or_default();
    Ok(TaxPageView {
        registration: store.registration.clone(),
        gstin: store.gstin.clone(),
        state_code: store.state_code.clone(),
        states: crate::settings::catalog::STATES
            .iter()
            .map(|c| ChoiceView {
                value: c.value.to_owned(),
                label: c.label.to_owned(),
            })
            .collect(),
        shop_basis: crate::settings::price_basis_to(book.shop_basis).to_owned(),
        shop_rate,
        shop_slab_id: shop_slab.as_str().to_owned(),
        charges_gst: store.registration().charges_gst(),
        gstin_check: judge_gstin(&store.gstin, &store.state_code),
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

/// A GST rate typed as a percentage, as a live slab: the one the shop has at that rate, else a
/// new one named after it.
fn gst_slab_for(
    repos: &mb_db::Repos<'_>,
    rate: TaxRate,
    at: mb_core::Timestamp,
) -> Result<TaxClassId, mb_db::DbError> {
    let classes = repos.tax_classes().list(OUTLET)?;
    if let Some(live) = classes
        .iter()
        .find(|c| c.kind == TaxKind::Gst && c.rate == rate && c.is_active)
    {
        return Ok(live.id.clone());
    }
    let id = TaxClassId::new(format!("tax_gst_{}", rate.basis_points()));
    let class = match classes.iter().find(|c| c.id == id) {
        Some(retired) => TaxClass {
            is_active: true,
            ..retired.clone()
        },
        None => TaxClass::new(
            id.clone(),
            format!("GST {}", rate.label()),
            TaxKind::Gst,
            rate,
        ),
    };
    repos.tax_classes().save(OUTLET, &class, at)?;
    Ok(id)
}

/// A percentage typed by a person, as a rate.
fn parse_rate(percent: &str) -> UiResult<TaxRate> {
    let bp = mb_auth::RoleShape::parse_percent(percent)
        .map_err(|e| UiError::new("tax.rate", e.to_string()))?
        .ok_or_else(|| UiError::new("tax.rate", "Type the GST rate — 5, or 18."))?;
    TaxRate::from_basis_points(bp)
        .ok_or_else(|| UiError::new("tax.rate", "A tax rate is between 0% and 100%."))
}

/// The shop's rate. Every item on the old shop rate moves with it.
pub fn set_shop_rate_on(app: &App, percent: String) -> UiResult<TaxPageView> {
    let who = guard::require(app, Permission::SettingsTax)?;
    let at = now();
    let day = today(at);
    let rate = parse_rate(&percent)?;
    let (page, moved) = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let before = repos.tax_classes().shop_slab(OUTLET)?;
                let slab = gst_slab_for(&repos, rate, at)?;
                let moved = repos.tax_classes().set_shop_slab(OUTLET, &slab, at)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::SETTING_CHANGED,
                        "shop_tax_rate",
                    )
                    .changed(
                        serde_json::json!({ "slab": before.as_str() }),
                        serde_json::json!({ "slab": slab.as_str(), "items_moved": moved }),
                    ),
                )?;
                Ok((page_in(&repos)?, moved))
            })
            .map_err(|e| words::from_db(&e))
    })?;
    app.reload_shop_config();
    log_info!(
        "{} set the shop's GST rate to {} ({moved} item(s) followed)",
        who.name,
        rate.label()
    );
    Ok(page)
}

/// Add or change a slab. Items on it need nothing done — they read it.
#[cfg(test)]
pub fn save_slab_on(
    app: &App,
    id: String,
    name: String,
    rate: String,
    kind: TaxKind,
    basis: String,
) -> UiResult<Vec<TaxSlabView>> {
    let who = guard::require(app, Permission::SettingsTax)?;
    let at = now();
    let day = today(at);
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(UiError::new(
            "tax.name",
            "A slab needs a name — GST 5%, Liquor.",
        ));
    }
    let bp = mb_auth::RoleShape::parse_percent(&rate)
        .map_err(|e| UiError::new("tax.rate", e.to_string()))?
        .unwrap_or(0);
    let rate = TaxRate::from_basis_points(bp)
        .ok_or_else(|| UiError::new("tax.rate", "A tax rate is between 0% and 100%."))?;
    let basis = basis_from_word(&basis)?;
    let class_id = TaxClassId::new(id.trim().to_owned());
    if class_id.as_str().is_empty() {
        return Err(UiError::new(
            "tax.id",
            "This slab has no id. Reload and try again.",
        ));
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

/// Put the ticked items on a slab and/or give them a pricing say. An empty slab puts them back
/// on their category's or the shop's rate. Either half may be left alone by sending nothing.
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
    // `None` = leave the slab; `Some(None)` = follow the category or the shop; `Some(id)`.
    let slab: Option<Option<TaxClassId>> = slab_id.map(|s| {
        let s = s.trim();
        if s.is_empty() || s == "follow" {
            None
        } else {
            Some(TaxClassId::new(s.to_owned()))
        }
    });
    let basis = match basis.as_deref() {
        None => None,
        Some(word) => Some(basis_from_word(word)?),
    };
    if slab.is_none() && basis.is_none() {
        return Err(UiError::new(
            "tax.items",
            "Choose a rate or a price rule to apply.",
        ));
    }
    let items: Vec<ItemId> = item_ids.iter().map(|id| ItemId::new(id.clone())).collect();
    let page = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let mut changed = 0;
                match &slab {
                    Some(Some(class)) => {
                        changed += repos
                            .tax_classes()
                            .assign(OUTLET, &items, Some(class), basis, at)?;
                    }
                    Some(None) => {
                        // Each item goes back to its own category's rung.
                        let menu = repos.menu().list_items(OUTLET, false)?;
                        for item in &items {
                            let category = menu
                                .iter()
                                .find(|i| &i.id == item)
                                .and_then(|i| i.category_id.clone());
                            let follows =
                                repos.tax_classes().rate_for(OUTLET, category.as_ref())?;
                            changed += repos.tax_classes().assign(
                                OUTLET,
                                std::slice::from_ref(item),
                                Some(&follows),
                                basis,
                                at,
                            )?;
                        }
                    }
                    None => {
                        changed += repos
                            .tax_classes()
                            .assign(OUTLET, &items, None, basis, at)?;
                    }
                }
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
                        "slab": slab.as_ref().map(|s| s.as_ref().map_or("follow", TaxClassId::as_str)),
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

/// The category's own rate. Everything in it on the old rate follows; an empty slab puts the
/// category back on the shop's rate.
pub fn set_category_on(
    app: &App,
    category_id: String,
    slab_id: Option<String>,
) -> UiResult<TaxPageView> {
    let who = guard::require(app, Permission::SettingsTax)?;
    let at = now();
    let day = today(at);
    let category = CategoryId::new(category_id.clone());
    let slab = slab_id
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty() && s != "follow")
        .map(TaxClassId::new);
    let page = app.with_shop(|shop| {
        shop.db
            .transaction(|tx| {
                let repos = mb_db::Repos::new(tx);
                let moved =
                    repos
                        .tax_classes()
                        .set_category_slab(OUTLET, &category, slab.as_ref(), at)?;
                repos.audit().append(
                    OUTLET,
                    &AuditEntry::new(
                        at,
                        day,
                        Some(who.staff_id.clone()),
                        action::SETTING_CHANGED,
                        "category_tax_rate",
                    )
                    .about(category_id.clone())
                    .with_after(serde_json::json!({
                        "slab": slab.as_ref().map(TaxClassId::as_str),
                        "items_moved": moved,
                    })),
                )?;
                page_in(&repos)
            })
            .map_err(|e| words::from_db(&e))
    })?;
    app.reload_shop_config();
    log_info!("{} set a category's tax rate", who.name);
    Ok(page)
}

#[cfg(test)]
fn slab_json(class: &TaxClass) -> serde_json::Value {
    serde_json::json!({
        "name": class.name,
        "rate": class.rate.label(),
        "kind": class.kind,
        "basis": basis_word(class.basis),
        "is_active": class.is_active,
    })
}

#[tauri::command]
pub fn tax_page(app: tauri::State<'_, App>) -> UiResult<TaxPageView> {
    page_on(&app)
}

/// The mark beside the box as the number is typed. Pure judgement, no shop data.
#[tauri::command]
pub fn judge_gstin_typed(
    app: tauri::State<'_, App>,
    gstin: String,
    state_code: String,
) -> UiResult<Option<GstinCheckView>> {
    guard::require(&app, Permission::MenuManage)?;
    Ok(judge_gstin(&gstin, &state_code))
}

#[tauri::command]
pub fn set_shop_tax_rate(app: tauri::State<'_, App>, percent: String) -> UiResult<TaxPageView> {
    set_shop_rate_on(&app, percent)
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
