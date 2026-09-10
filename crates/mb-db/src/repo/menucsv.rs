//! The menu, in and out of a spreadsheet.

use std::collections::HashMap;

use mb_core::{CategoryId, ItemId, Money, PriceBasis, Timestamp};
use rusqlite::Transaction;

use crate::error::DbError;
use crate::export::{parse_csv, write_row};
use crate::repo::menu::{Category, MenuItem, MenuRepo};

/// The columns, in order, as the export writes them.
const COLUMNS: &[&str] = &[
    "id",
    "name",
    "category",
    "price_paise",
    "tax_class",
    "price_basis",
    "hsn",
    "short_code",
    "cost_paise",
    "available",
];

/// What an import would do, before it does anything.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportPlan {
    /// Categories the file names that the shop does not have yet. They are created, not
    /// refused — but they are listed here so the screen can say so first.
    pub new_categories: Vec<Category>,
    pub new_items: Vec<MenuItem>,
    pub updated_items: Vec<MenuItem>,
    /// `(line number, why)`.
    pub refused: Vec<(usize, String)>,
}

impl ImportPlan {
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.refused.is_empty()
    }

    /// What the screen shows before anybody commits to anything.
    #[must_use]
    pub fn summary(&self) -> String {
        let new = self.new_items.len();
        let updated = self.updated_items.len();
        let refused = self.refused.len();
        if refused > 0 {
            return format!(
                "{refused} row(s) cannot be read, so nothing will be imported until \
                 they are fixed. {new} would be new and {updated} would change."
            );
        }
        let mut said = match (new, updated) {
            (0, 0) => "Nothing to import — the file has no rows.".to_owned(),
            (n, 0) => format!("{n} new item(s), nothing changed."),
            (0, u) => format!("{u} item(s) would change, nothing new."),
            (n, u) => format!("{n} new item(s) and {u} change(s)."),
        };
        match self.new_categories.len() {
            0 => {}
            1 => said.push_str(" One new category will be added."),
            c => said.push_str(&format!(" {c} new categories will be added.")),
        }
        said
    }
}

#[derive(Debug)]
pub struct MenuCsvRepo<'a> {
    tx: &'a Transaction<'a>,
}

impl<'a> MenuCsvRepo<'a> {
    #[must_use]
    pub(crate) fn new(tx: &'a Transaction<'a>) -> Self {
        MenuCsvRepo { tx }
    }

    /// The whole menu as a spreadsheet.
    pub fn export(&self, outlet: &str) -> Result<String, DbError> {
        let repo = MenuRepo::new(self.tx);
        let categories = repo.list_categories(outlet)?;

        let mut out = String::new();
        write_row(&mut out, COLUMNS.iter().map(|c| Some(*c)));

        for item in repo.list_items(outlet, false)? {
            let category = item.category_id.as_ref().and_then(|id| {
                categories
                    .iter()
                    .find(|c| c.id == *id)
                    .map(|c| c.name.clone())
            });
            let price = item.unit_price.paise().to_string();
            let cost = item.cost_price.map(|c| c.paise().to_string());
            let available = if item.is_available { "yes" } else { "no" };

            write_row(
                &mut out,
                [
                    Some(item.id.as_str()),
                    Some(item.name.as_str()),
                    category.as_deref(),
                    Some(price.as_str()),
                    Some(item.tax_class_id.as_str()),
                    Some(basis_word(item.price_basis)),
                    item.hsn.as_deref(),
                    item.short_code.as_deref(),
                    cost.as_deref(),
                    Some(available),
                ]
                .into_iter(),
            );
        }
        Ok(out)
    }

    /// The dry run. Reads the file, decides everything, writes nothing.
    pub fn plan(&self, outlet: &str, csv: &str) -> Result<ImportPlan, DbError> {
        let repo = MenuRepo::new(self.tx);
        let existing = repo.list_items(outlet, false)?;
        let classes = crate::repo::taxclass::TaxClassRepo::new(self.tx).list(outlet)?;
        // The last rung of the tax ladder: item, then category, then the shop's own rate.
        let shop_slab = crate::repo::taxclass::TaxClassRepo::new(self.tx)
            .shop_slab(outlet)
            .ok();
        // The working list grows as the file names categories the shop does not have, so the
        // second row of a new category finds the one the first row made.
        let mut categories = repo.list_categories(outlet)?;

        let rows = parse_csv(csv);
        let mut plan = ImportPlan::default();

        let Some(first) = rows.first() else {
            return Ok(plan);
        };
        let header = Header::read(first);
        let Some(name_at) = header.of(NAME) else {
            plan.refused.push((
                1,
                "the first line must name the columns, and one of them must be \
                 \"name\". The shortest file that works is name,price."
                    .to_owned(),
            ));
            return Ok(plan);
        };

        for (offset, row) in rows.iter().skip(1).enumerate() {
            let line = offset + 2;
            if row
                .iter()
                .all(|cell| cell.as_deref().unwrap_or("").trim().is_empty())
            {
                continue; // a blank line at the end of a spreadsheet is normal
            }

            let cell = |at: Option<usize>| -> Option<String> {
                at.and_then(|i| row.get(i))
                    .and_then(|c| c.as_deref())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToOwned::to_owned)
            };

            let Some(name) = cell(Some(name_at)) else {
                plan.refused.push((line, "this row has no name".to_owned()));
                continue;
            };

            // An id matches an existing item; without one, the NAME does, so an owner can send
            // a plain two-column list and have it update rather than duplicate.
            let id = cell(header.of(ID));
            let found = match &id {
                Some(id) => existing.iter().find(|i| i.id.as_str() == id),
                None => existing.iter().find(|i| i.name.eq_ignore_ascii_case(&name)),
            };

            let price = match money(
                cell(header.of(PRICE)),
                cell(header.of(PRICE_PAISE)),
                "price",
            ) {
                Ok(Some(price)) => price,
                Ok(None) => match found {
                    Some(item) => item.unit_price,
                    None => {
                        plan.refused.push((line, format!("{name} has no price")));
                        continue;
                    }
                },
                Err(why) => {
                    plan.refused.push((line, why));
                    continue;
                }
            };

            let cost = match money(cell(header.of(COST)), cell(header.of(COST_PAISE)), "cost") {
                Ok(Some(cost)) => Some(cost),
                Ok(None) => found.and_then(|i| i.cost_price),
                Err(why) => {
                    plan.refused.push((line, why));
                    continue;
                }
            };

            // A category by name. One the shop does not have yet is CREATED rather than
            // refused — importing a menu into a fresh shop is the whole reason to import one.
            // The plan names every category it would add, so nothing appears unannounced.
            let category: Option<Category> = match cell(header.of(CATEGORY)) {
                Some(text) => {
                    let already = categories
                        .iter()
                        .find(|c| c.name.eq_ignore_ascii_case(&text))
                        .cloned();
                    match already {
                        Some(category) => Some(category),
                        None => {
                            let sort_order =
                                categories.iter().map(|c| c.sort_order).max().unwrap_or(-1) + 1;
                            let made = Category {
                                id: CategoryId::new(unique(slug("cat", &text, line), |id| {
                                    categories.iter().any(|c| c.id.as_str() == id)
                                })),
                                name: text,
                                sort_order,
                                is_active: true,
                                station: None,
                                default_tax_class_id: None,
                            };
                            categories.push(made.clone());
                            plan.new_categories.push(made.clone());
                            Some(made)
                        }
                    }
                }
                None => found
                    .and_then(|i| i.category_id.as_ref())
                    .and_then(|id| categories.iter().find(|c| &c.id == id))
                    .cloned(),
            };

            // A tax slab by id or by name. The file wins; failing that the item keeps the slab
            // it has; failing that it takes its category's, then the shop's. Only a shop with
            // no slab at all can refuse a row here.
            let wanted_class = cell(header.of(TAX_CLASS));
            let class = match &wanted_class {
                Some(text) => {
                    let found_class = classes.iter().find(|c| {
                        c.is_active && (c.id.as_str() == text || c.name.eq_ignore_ascii_case(text))
                    });
                    match found_class {
                        Some(class) => class.id.clone(),
                        None => {
                            plan.refused.push((
                                line,
                                format!("\"{text}\" is not one of this shop's tax slabs"),
                            ));
                            continue;
                        }
                    }
                }
                None => match found
                    .map(|i| i.tax_class_id.clone())
                    .or_else(|| {
                        category
                            .as_ref()
                            .and_then(|c| c.default_tax_class_id.clone())
                    })
                    .or_else(|| shop_slab.clone())
                {
                    Some(id) => id,
                    None => {
                        plan.refused.push((
                            line,
                            format!(
                                "{name} needs a tax slab, and this shop has none — \
                                 set the shop's rate on the Tax screen first"
                            ),
                        ));
                        continue;
                    }
                },
            };

            // The item's own say on its price, if the file has one.
            let price_basis = match cell(header.of(PRICE_BASIS)) {
                Some(text) => match basis_from_word(&text) {
                    Some(basis) => basis,
                    None => {
                        plan.refused.push((
                            line,
                            format!(
                                "\"{text}\" is not a price basis — shop, inclusive or exclusive"
                            ),
                        ));
                        continue;
                    }
                },
                None => found.and_then(|i| i.price_basis),
            };

            let available = match cell(header.of(AVAILABLE)) {
                Some(text) => matches!(
                    text.to_ascii_lowercase().as_str(),
                    "yes" | "y" | "true" | "1"
                ),
                None => found.is_none_or(|i| i.is_available),
            };

            // A minted id must not land on an item that already exists under another name —
            // saving is an upsert by id, so a collision would quietly overwrite that item.
            let item_id = match found {
                Some(item) => item.id.clone(),
                None => ItemId::new(match id {
                    Some(id) => id,
                    None => unique(slug("itm", &name, line), |id| {
                        existing.iter().any(|i| i.id.as_str() == id)
                            || plan.new_items.iter().any(|i| i.id.as_str() == id)
                    }),
                }),
            };

            let item = MenuItem {
                id: item_id,
                category_id: category.map(|c| c.id),
                name,
                unit_price: price,
                tax_class_id: class,
                price_basis,
                hsn: cell(header.of(HSN)).or_else(|| found.and_then(|i| i.hsn.clone())),
                cost_price: cost,
                short_code: cell(header.of(SHORT_CODE))
                    .or_else(|| found.and_then(|i| i.short_code.clone())),
                prep_minutes: found.and_then(|i| i.prep_minutes),
                // Kept from the existing item, not taken from the CSV.
                course: found.and_then(|i| i.course.clone()),
                is_open_price: found.is_some_and(|i| i.is_open_price),
                is_available: available,
                sort_order: found.map_or(0, |i| i.sort_order),
            };

            if found.is_some() {
                plan.updated_items.push(item);
            } else {
                plan.new_items.push(item);
            }
        }

        Ok(plan)
    }

    /// Do exactly what the plan said. Answers with the number of ITEMS written; the categories
    /// it made are the plan's to report.
    pub fn apply(&self, outlet: &str, plan: &ImportPlan, at: Timestamp) -> Result<usize, DbError> {
        if !plan.is_clean() {
            return Err(DbError::invariant(format!(
                "{} row(s) in that file cannot be read, so nothing was imported",
                plan.refused.len()
            )));
        }
        let repo = MenuRepo::new(self.tx);
        // Categories first — an item points at one.
        for category in &plan.new_categories {
            repo.save_category(outlet, category, at)?;
        }
        for item in plan.new_items.iter().chain(&plan.updated_items) {
            repo.save_item(outlet, item, at)?;
        }
        Ok(plan.new_items.len() + plan.updated_items.len())
    }
}

// The header.

const ID: &str = "id";
const NAME: &str = "name";
const CATEGORY: &str = "category";
const PRICE: &str = "price";
const PRICE_PAISE: &str = "price paise";
const COST: &str = "cost";
const COST_PAISE: &str = "cost paise";
const TAX_CLASS: &str = "tax class";
const PRICE_BASIS: &str = "price basis";
const HSN: &str = "hsn";
const SHORT_CODE: &str = "short code";
const AVAILABLE: &str = "available";

/// The everyday words a spreadsheet arrives with, against the column each one means. A file
/// typed by hand says "Item" and "Rate"; the export says "name" and "price_paise". Both work.
const ALIASES: &[(&str, &str)] = &[
    ("item", NAME),
    ("item name", NAME),
    ("product", NAME),
    ("dish", NAME),
    ("group", CATEGORY),
    ("section", CATEGORY),
    ("category name", CATEGORY),
    ("rate", PRICE),
    ("amount", PRICE),
    ("mrp", PRICE),
    ("selling price", PRICE),
    ("cost price", COST),
    ("tax", TAX_CLASS),
    ("tax slab", TAX_CLASS),
    ("gst", TAX_CLASS),
    ("basis", PRICE_BASIS),
    ("code", SHORT_CODE),
    ("hsn code", HSN),
    ("in stock", AVAILABLE),
    ("active", AVAILABLE),
];

/// Which column sits where. Read once, by name rather than by position, so a file with the
/// columns in a different order — or in different case — still works.
#[derive(Debug, Default)]
struct Header {
    at: HashMap<&'static str, usize>,
}

impl Header {
    fn read(row: &[Option<String>]) -> Self {
        let mut at = HashMap::new();
        for (index, cell) in row.iter().enumerate() {
            let word = normalise(cell.as_deref().unwrap_or(""));
            if word.is_empty() {
                continue;
            }
            let column = [
                ID,
                NAME,
                CATEGORY,
                PRICE,
                PRICE_PAISE,
                COST,
                COST_PAISE,
                TAX_CLASS,
                PRICE_BASIS,
                HSN,
                SHORT_CODE,
                AVAILABLE,
            ]
            .into_iter()
            .find(|known| *known == word)
            .or_else(|| {
                ALIASES
                    .iter()
                    .find(|(alias, _)| *alias == word)
                    .map(|(_, column)| *column)
            });
            // The first column of a given name wins, so a duplicate heading is ignored rather
            // than quietly overriding the one before it.
            if let Some(column) = column {
                at.entry(column).or_insert(index);
            }
        }
        Header { at }
    }

    fn of(&self, column: &str) -> Option<usize> {
        self.at.get(column).copied()
    }
}

/// A heading as we compare it: no case, and `_` reads as a space, so `price_paise`,
/// `Price Paise` and `PRICE  PAISE` are one column.
fn normalise(cell: &str) -> String {
    cell.trim()
        .to_ascii_lowercase()
        .split(|c: char| c == '_' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

// The money columns.

/// One amount, from whichever column the file used. Rupees are how a person writes a menu;
/// paise are how the export writes it. Both in one row is a guess we refuse to make — the
/// difference between them is a hundredfold, and this is money.
fn money(
    rupees: Option<String>,
    paise: Option<String>,
    what: &str,
) -> Result<Option<Money>, String> {
    match (rupees, paise) {
        (Some(_), Some(_)) => Err(format!(
            "this row gives both {what} and {what}_paise — keep one column, not both"
        )),
        (Some(text), None) => match rupees_to_paise(&text) {
            Some(paise) => Ok(Some(Money::from_paise(paise))),
            None => Err(format!("\"{text}\" is not a {what} in rupees")),
        },
        (None, Some(text)) => match text.parse::<i64>() {
            Ok(paise) if paise >= 0 => Ok(Some(Money::from_paise(paise))),
            _ => Err(format!(
                "\"{text}\" is not a {what} in paise. ₹120 is 12000."
            )),
        },
        (None, None) => Ok(None),
    }
}

/// A price the way a person writes it — `200`, `200.50`, `₹1,200`, `Rs 45` — in paise.
fn rupees_to_paise(text: &str) -> Option<i64> {
    let mut cleaned = text.trim().to_ascii_lowercase();
    for prefix in ["₹", "rs.", "rs", "inr"] {
        if let Some(rest) = cleaned.strip_prefix(prefix) {
            cleaned = rest.to_owned();
            break;
        }
    }
    let cleaned: String = cleaned
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ',')
        .collect();

    let (whole, fraction) = cleaned.split_once('.').unwrap_or((cleaned.as_str(), ""));
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    if fraction.len() > 2 {
        return None; // paise do not go finer than this, and a third digit means a typo
    }
    if !whole.chars().all(|c| c.is_ascii_digit()) || !fraction.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    let rupees: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let paise: i64 = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<i64>().ok()? * 10,
        _ => fraction.parse().ok()?,
    };
    rupees.checked_mul(100)?.checked_add(paise)
}

// Ids for things the file did not name one for.

/// The price-basis column, in the words the file uses.
const fn basis_word(basis: Option<PriceBasis>) -> &'static str {
    match basis {
        None => "shop",
        Some(PriceBasis::Inclusive) => "inclusive",
        Some(PriceBasis::Exclusive) => "exclusive",
    }
}

/// The same words, read back. `Some(None)` is "shop"; `None` is a word we do not know.
fn basis_from_word(text: &str) -> Option<Option<PriceBasis>> {
    match text.trim().to_ascii_lowercase().as_str() {
        "shop" | "default" | "" => Some(None),
        "inclusive" | "included" | "yes" => Some(Some(PriceBasis::Inclusive)),
        "exclusive" | "added" | "no" => Some(Some(PriceBasis::Exclusive)),
        _ => None,
    }
}

/// An id for a row the file did not name one for.
fn slug(prefix: &str, name: &str, line: usize) -> String {
    let cleaned: String = name
        .chars()
        .filter_map(|c| {
            if c.is_ascii_alphanumeric() {
                Some(c.to_ascii_lowercase())
            } else if c.is_whitespace() {
                Some('_')
            } else {
                None
            }
        })
        .take(32)
        .collect();
    if cleaned.is_empty() {
        format!("{prefix}_line_{line}")
    } else {
        format!("{prefix}_{cleaned}")
    }
}

/// The same id, moved along until nothing else is using it. Two names can slug to one id
/// ("Tea!" and "Tea?"), and saving is an upsert — a collision would overwrite a real row.
fn unique(base: String, taken: impl Fn(&str) -> bool) -> String {
    if !taken(&base) {
        return base;
    }
    for n in 2..1000 {
        let candidate = format!("{base}_{n}");
        if !taken(&candidate) {
            return candidate;
        }
    }
    base
}
