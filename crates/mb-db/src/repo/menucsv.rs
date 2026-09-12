//! The menu, in and out of a spreadsheet.
//!
//! One shape both ways. The file a restaurant already has is `category,name,price`, so that is
//! what the export leads with; the rarer columns follow, and the item's id comes last so a file
//! that went out can come back with a renamed item and still find it.
//!
//! An import is one of two things, and the owner says which before anything is written:
//! UPDATE adds what is new and changes what the file names; REPLACE makes the file the menu,
//! and whatever the file does not name goes.

use std::collections::{HashMap, HashSet};

use mb_core::{CategoryId, ItemId, Money, PriceBasis, TaxClass, Timestamp};
use rusqlite::Transaction;

use crate::error::DbError;
use crate::export::{parse_sheet, write_row};
use crate::repo::menu::{Category, MenuItem, MenuRepo};

/// The columns, in order, as the export writes them — and the words the import reads back.
const COLUMNS: &[&str] = &[
    CATEGORY,
    NAME,
    PRICE,
    TAX_CLASS,
    PRICE_BASIS,
    HSN,
    SHORT_CODE,
    COST,
    AVAILABLE,
    ID,
];

/// What the owner asked the file to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImportMode {
    /// Add what is new, change what the file names, leave the rest alone.
    #[default]
    Update,
    /// The file becomes the menu. Items it does not name are deleted, or taken off the menu
    /// when a bill, size, combo or recipe still needs them; categories left empty are retired.
    Replace,
}

/// What an import would do, before it does anything.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ImportPlan {
    pub mode: ImportMode,
    /// Categories the file names that the shop does not have yet. They are created, not
    /// refused — but they are listed here so the screen can say so first.
    pub new_categories: Vec<Category>,
    pub new_items: Vec<MenuItem>,
    /// Only items the file actually changes. A row that says what the shop already has is
    /// counted in `unchanged` and never written.
    pub updated_items: Vec<MenuItem>,
    pub unchanged: usize,
    /// Every row that landed on an item the menu already has — changed or not — as
    /// "Tea (TEA COFFEE)". The owner reads this list before agreeing: the file may be about
    /// to overwrite prices they set by hand.
    pub already: Vec<String>,
    /// REPLACE only: items the file does not name, deleted outright.
    pub removed: Vec<MenuItem>,
    /// REPLACE only: items the file does not name that something still points at — taken
    /// off the menu, kept for the bills that remember them.
    pub taken_off: Vec<MenuItem>,
    /// REPLACE only: categories that end up with nothing in them.
    pub retired_categories: Vec<Category>,
    /// `(line number, why)`.
    pub refused: Vec<(usize, String)>,
}

impl ImportPlan {
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.refused.is_empty()
    }

    /// True when applying the plan would write nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.new_items.is_empty()
            && self.updated_items.is_empty()
            && self.removed.is_empty()
            && self.taken_off.is_empty()
    }

    /// What the screen shows before anybody commits to anything.
    #[must_use]
    pub fn summary(&self) -> String {
        let new = self.new_items.len();
        let updated = self.updated_items.len();
        let same = self.unchanged;
        let refused = self.refused.len();
        if refused > 0 {
            return format!(
                "{refused} row(s) cannot be read, so nothing will be imported until \
                 they are fixed. {new} would be new and {updated} would change."
            );
        }
        let gone = self.removed.len() + self.taken_off.len();
        let mut said = match (new, updated, same, gone) {
            (0, 0, 0, 0) => "Nothing to import — the file has no rows.".to_owned(),
            (0, 0, s, 0) => {
                format!("Nothing would change — the menu already has all {s} of these.")
            }
            (n, 0, 0, 0) => format!("{n} new item(s), nothing changed."),
            (0, u, 0, 0) => format!("{u} item(s) would change, nothing new."),
            (n, u, 0, 0) => format!("{n} new item(s) and {u} change(s)."),
            (n, u, s, 0) => format!("{n} new item(s), {u} change(s), {s} already the same."),
            (n, u, s, g) => format!(
                "{n} new item(s), {u} change(s), {s} already the same, and {g} item(s) not in \
                 the file would go."
            ),
        };
        match self.new_categories.len() {
            0 => {}
            1 => said.push_str(" One new category will be added."),
            c => said.push_str(&format!(" {c} new categories will be added.")),
        }
        match self.retired_categories.len() {
            0 => {}
            1 => said.push_str(" One category would be left empty and removed."),
            c => said.push_str(&format!(" {c} categories would be left empty and removed.")),
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

    /// The whole menu as a spreadsheet, in the same words the import reads. Prices are rupees
    /// and the tax is the slab's name, because a person opens this in Excel. `with_cost` is the
    /// cost-price permission: without it the column is there but empty, so a file made by
    /// someone who may not see margins does not carry them out of the shop.
    pub fn export(&self, outlet: &str, with_cost: bool) -> Result<String, DbError> {
        let repo = MenuRepo::new(self.tx);
        let categories = repo.list_categories(outlet)?;
        let classes = crate::repo::taxclass::TaxClassRepo::new(self.tx).list(outlet)?;

        let mut out = String::new();
        write_row(&mut out, COLUMNS.iter().map(|c| Some(*c)));

        for item in repo.list_items(outlet, false)? {
            let category = item.category_id.as_ref().and_then(|id| {
                categories
                    .iter()
                    .find(|c| c.id == *id)
                    .map(|c| c.name.clone())
            });
            let price = item.unit_price.to_plain_string();
            let tax = classes
                .iter()
                .find(|c| c.id == item.tax_class_id)
                .map_or_else(|| item.tax_class_id.as_str().to_owned(), |c| c.name.clone());
            let cost = if with_cost {
                item.cost_price.map(Money::to_plain_string)
            } else {
                None
            };
            let available = if item.is_available { "yes" } else { "no" };

            write_row(
                &mut out,
                [
                    category.as_deref(),
                    Some(item.name.as_str()),
                    Some(price.as_str()),
                    Some(tax.as_str()),
                    Some(basis_word(item.price_basis)),
                    item.hsn.as_deref(),
                    item.short_code.as_deref(),
                    cost.as_deref(),
                    Some(available),
                    Some(item.id.as_str()),
                ]
                .into_iter(),
            );
        }
        Ok(out)
    }

    /// The dry run. Reads the file, decides everything, writes nothing.
    #[allow(
        clippy::too_many_lines,
        reason = "one pass over the file, one decision per column, in the order the row is read"
    )]
    pub fn plan(&self, outlet: &str, csv: &str, mode: ImportMode) -> Result<ImportPlan, DbError> {
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

        let rows = parse_sheet(csv);
        let mut plan = ImportPlan {
            mode,
            ..ImportPlan::default()
        };

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
        let has_category_column = header.of(CATEGORY).is_some();

        // Which existing items the file has already spoken for, and where. Two rows for one
        // item would fight over it, so the second is refused and says which line it repeats.
        let mut claimed: HashMap<String, usize> = HashMap::new();
        // The same for rows that make NEW items: one name in one category, once.
        let mut minted: HashMap<(String, Option<CategoryId>), usize> = HashMap::new();

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

            // A category by name. One the shop does not have yet is CREATED rather than
            // refused — importing a menu into a fresh shop is the whole reason to import one.
            // The plan names every category it would add, so nothing appears unannounced.
            let named_category: Option<Category> = cell(header.of(CATEGORY)).map(|text| {
                let already = categories
                    .iter()
                    .find(|c| c.name.eq_ignore_ascii_case(&text))
                    .cloned();
                already.unwrap_or_else(|| {
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
                    made
                })
            });

            // Which item this row is about, if the shop already has it.
            let id = cell(header.of(ID));
            let found = match find_existing(
                &existing,
                id.as_deref(),
                &name,
                has_category_column.then(|| named_category.as_ref().map(|c| &c.id)),
                &claimed,
            ) {
                Ok(found) => found,
                Err(why) => {
                    plan.refused.push((line, why));
                    continue;
                }
            };
            if let Some(item) = found {
                if let Some(first_at) = claimed.get(item.id.as_str()) {
                    plan.refused.push((
                        line,
                        format!("this row is about the same item as line {first_at}"),
                    ));
                    continue;
                }
                claimed.insert(item.id.as_str().to_owned(), line);
            }

            let category: Option<Category> = named_category.or_else(|| {
                found
                    .and_then(|i| i.category_id.as_ref())
                    .and_then(|id| categories.iter().find(|c| &c.id == id))
                    .cloned()
            });

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

            // A tax slab by id, by name, or by rate. The file wins; failing that the item keeps
            // the slab it has; failing that it takes its category's, then the shop's. Only a
            // shop with no slab at all can refuse a row here.
            let class = match cell(header.of(TAX_CLASS)) {
                Some(text) => match slab_named(&classes, &text) {
                    Ok(class) => class.id.clone(),
                    Err(why) => {
                        plan.refused.push((line, why));
                        continue;
                    }
                },
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
                None => {
                    let key = (name.to_lowercase(), category.as_ref().map(|c| c.id.clone()));
                    if let Some(first_at) = minted.get(&key) {
                        plan.refused.push((
                            line,
                            format!("{name} is already on line {first_at} in the same category"),
                        ));
                        continue;
                    }
                    minted.insert(key, line);
                    ItemId::new(match id {
                        Some(id) => id,
                        None => unique(slug("itm", &name, line), |id| {
                            existing.iter().any(|i| i.id.as_str() == id)
                                || plan.new_items.iter().any(|i| i.id.as_str() == id)
                        }),
                    })
                }
            };

            let item = MenuItem {
                id: item_id,
                category_id: category.as_ref().map(|c| c.id.clone()),
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

            match found {
                Some(was) => {
                    plan.already.push(match &category {
                        Some(c) => format!("{} ({})", item.name, c.name),
                        None => item.name.clone(),
                    });
                    if *was == item {
                        plan.unchanged += 1;
                    } else {
                        plan.updated_items.push(item);
                    }
                }
                None => plan.new_items.push(item),
            }
        }

        if mode == ImportMode::Replace && plan.is_clean() {
            // Whatever the file did not speak for goes. Deleted when nothing points at it;
            // taken off the menu when a bill, a size, a combo or a recipe still does.
            for item in &existing {
                if claimed.contains_key(item.id.as_str()) {
                    continue;
                }
                if repo.is_in_use(&item.id)? {
                    if item.is_available {
                        plan.taken_off.push(item.clone());
                    }
                } else {
                    plan.removed.push(item.clone());
                }
            }
            // A category with nothing left in it. What is left: the new items, the changed
            // ones where the file put them, and every existing item that neither goes nor
            // moves — an item taken off the menu still sits in its category.
            let gone: HashSet<&str> = plan.removed.iter().map(|i| i.id.as_str()).collect();
            let moved: HashSet<&str> = plan.updated_items.iter().map(|i| i.id.as_str()).collect();
            let still_used: HashSet<&CategoryId> = plan
                .new_items
                .iter()
                .chain(&plan.updated_items)
                .filter_map(|i| i.category_id.as_ref())
                .chain(
                    existing
                        .iter()
                        .filter(|i| !gone.contains(i.id.as_str()) && !moved.contains(i.id.as_str()))
                        .filter_map(|i| i.category_id.as_ref()),
                )
                .collect();
            plan.retired_categories = categories
                .iter()
                .filter(|c| c.is_active && !still_used.contains(&c.id))
                .filter(|c| !plan.new_categories.iter().any(|n| n.id == c.id))
                .cloned()
                .collect();
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
        // Categories first — an item points at one. Only the ones an item still needs: a
        // category named only by rows that turned out unchanged is not added for nothing.
        let needed: HashSet<&str> = plan
            .new_items
            .iter()
            .chain(&plan.updated_items)
            .filter_map(|i| i.category_id.as_ref().map(CategoryId::as_str))
            .collect();
        for category in &plan.new_categories {
            if needed.contains(category.id.as_str()) {
                repo.save_category(outlet, category, at)?;
            }
        }
        for item in plan.new_items.iter().chain(&plan.updated_items) {
            repo.save_item(outlet, item, at)?;
        }
        // REPLACE: the same doors the menu page uses — delete, else take off the menu.
        for item in &plan.removed {
            repo.delete_item(outlet, &item.id, at)?;
        }
        for item in &plan.taken_off {
            repo.set_available(outlet, &item.id, false, at)?;
        }
        for category in &plan.retired_categories {
            let mut retired = category.clone();
            retired.is_active = false;
            retired.default_tax_class_id = None;
            repo.save_category(outlet, &retired, at)?;
        }
        Ok(plan.new_items.len() + plan.updated_items.len())
    }
}

// Which item a row means.

/// The item a row is about, or `None` for a new one. An id is exact. Without one, the name
/// finds the item in the SAME category — a restaurant really does sell "Boiled rice" in two
/// sections at two prices, and the same name in another category is a second item, not a
/// match. A file with no category column at all matches the one item of that name anywhere,
/// so a plain `name,price` list updates rather than duplicates. Items an earlier row already
/// claimed are not offered again.
fn find_existing<'i>(
    existing: &'i [MenuItem],
    id: Option<&str>,
    name: &str,
    category: Option<Option<&CategoryId>>,
    claimed: &HashMap<String, usize>,
) -> Result<Option<&'i MenuItem>, String> {
    if let Some(id) = id {
        return Ok(existing.iter().find(|i| i.id.as_str() == id));
    }
    let same_name = |i: &&MenuItem| i.name.trim().eq_ignore_ascii_case(name);
    if let Some(category) = category {
        // The file says where this row belongs; only an item there is the same item.
        return Ok(existing
            .iter()
            .find(|i| same_name(i) && i.category_id.as_ref() == category));
    }
    let same: Vec<&MenuItem> = existing.iter().filter(same_name).collect();
    let mut free = same
        .iter()
        .copied()
        .filter(|i| !claimed.contains_key(i.id.as_str()));
    match (free.next(), free.next()) {
        (Some(one), None) => Ok(Some(one)),
        (None, _) => match same.iter().find_map(|i| claimed.get(i.id.as_str())) {
            // Every item of this name is already spoken for: the same row typed twice.
            Some(first_at) => Err(format!("this row repeats line {first_at}")),
            None => Ok(None),
        },
        (Some(_), Some(_)) => Err(format!(
            "the menu has more than one item called {name} — add a category column to say \
             which one this row means"
        )),
    }
}

/// A slab by id, by name, or by rate: `tax_food_5`, `Restaurant food 5%`, `5%`, `gst 5`. A rate
/// only works when exactly one active slab has it.
fn slab_named<'c>(classes: &'c [TaxClass], text: &str) -> Result<&'c TaxClass, String> {
    let active = classes.iter().filter(|c| c.is_active);
    if let Some(class) = active
        .clone()
        .find(|c| c.id.as_str() == text || c.name.eq_ignore_ascii_case(text))
    {
        return Ok(class);
    }
    if let Some(bp) = percent_to_basis_points(text) {
        let mut at_rate = active.filter(|c| c.rate.basis_points() == bp);
        let one = at_rate.next();
        let another = at_rate.next();
        return match (one, another) {
            (Some(class), None) => Ok(class),
            (Some(a), Some(b)) => Err(format!(
                "more than one slab is {text} — say which: {} or {}",
                a.name, b.name
            )),
            (None, _) => Err(format!("no slab of this shop is {text}")),
        };
    }
    Err(format!("\"{text}\" is not one of this shop's tax slabs"))
}

/// `5`, `5%`, `GST 5%`, `12.5 %` — in basis points. Anything else is a name, not a rate.
fn percent_to_basis_points(text: &str) -> Option<u32> {
    let cleaned: String = text
        .to_ascii_lowercase()
        .replace("gst", "")
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '%')
        .collect();
    if cleaned.is_empty() || !cleaned.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let (whole, fraction) = cleaned.split_once('.').unwrap_or((cleaned.as_str(), ""));
    if fraction.len() > 2 || (whole.is_empty() && fraction.is_empty()) {
        return None;
    }
    let whole: u32 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let hundredths: u32 = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<u32>().ok()? * 10,
        _ => fraction.parse().ok()?,
    };
    whole.checked_mul(100)?.checked_add(hundredths)
}

// The header.

const ID: &str = "id";
const NAME: &str = "name";
const CATEGORY: &str = "category";
const PRICE: &str = "price";
const PRICE_PAISE: &str = "price paise";
const COST: &str = "cost";
const COST_PAISE: &str = "cost paise";
const TAX_CLASS: &str = "tax";
const PRICE_BASIS: &str = "price basis";
const HSN: &str = "hsn";
const SHORT_CODE: &str = "short code";
const AVAILABLE: &str = "available";

/// The everyday words a spreadsheet arrives with, against the column each one means. A file
/// typed by hand says "Item" and "Rate"; an export from before 1.6.17 says "price_paise" and
/// "tax_class". All of them work.
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
    ("tax class", TAX_CLASS),
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
/// paise are how the export wrote it before 1.6.17. Both in one row is a guess we refuse to
/// make — the difference between them is a hundredfold, and this is money.
fn money(
    rupees: Option<String>,
    paise: Option<String>,
    what: &str,
) -> Result<Option<Money>, String> {
    match (rupees, paise) {
        (Some(_), Some(_)) => Err(format!(
            "this row gives both {what} and {what} paise — keep one column, not both"
        )),
        (Some(text), None) => match rupees_typed(&text) {
            Some(money) => Ok(Some(money)),
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

/// A price the way a person writes it — `200`, `200.50`, `₹1,200`, `Rs 45` — read by the one
/// money parser the whole product uses. A menu price is never negative.
fn rupees_typed(text: &str) -> Option<Money> {
    let mut cleaned = text.trim().to_ascii_lowercase();
    for prefix in ["rs.", "rs", "inr"] {
        if let Some(rest) = cleaned.strip_prefix(prefix) {
            cleaned = rest.to_owned();
            break;
        }
    }
    let money = Money::parse(&cleaned).ok()?;
    if money.is_negative() {
        return None;
    }
    Some(money)
}

// The price basis, in words.

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

// Ids for things the file did not name one for.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_price_is_read_the_way_people_write_it() {
        for (typed, paise) in [
            ("200", 20_000),
            ("200.5", 20_050),
            ("80.01", 8_001),
            ("₹1,200", 120_000),
            ("Rs 45", 4_500),
            ("Rs. 45", 4_500),
            ("INR 10", 1_000),
            (" 15 ", 1_500),
        ] {
            assert_eq!(
                rupees_typed(typed).map(|m| m.paise()),
                Some(paise),
                "{typed}"
            );
        }
        for typed in ["-5", "free", "10.001", "", "1/2"] {
            assert_eq!(rupees_typed(typed), None, "{typed}");
        }
    }

    #[test]
    fn a_rate_finds_its_slab() {
        for (typed, bp) in [
            ("5", 500),
            ("5%", 500),
            ("GST 5%", 500),
            ("12.5 %", 1_250),
            ("0", 0),
        ] {
            assert_eq!(percent_to_basis_points(typed), Some(bp), "{typed}");
        }
        for typed in ["five", "5.005", "", "Restaurant food 5%"] {
            assert_eq!(percent_to_basis_points(typed), None, "{typed}");
        }
    }

    #[test]
    fn the_header_reads_old_exports_and_hand_typed_files() {
        let header = |line: &str| {
            Header::read(
                &line
                    .split(',')
                    .map(|c| Some(c.to_owned()))
                    .collect::<Vec<_>>(),
            )
        };
        let old = header("id,name,category,price_paise,tax_class,price_basis");
        assert_eq!(old.of(PRICE_PAISE), Some(3));
        assert_eq!(old.of(TAX_CLASS), Some(4));
        let typed = header("Item Name,Rate,GST,Section");
        assert_eq!(typed.of(NAME), Some(0));
        assert_eq!(typed.of(PRICE), Some(1));
        assert_eq!(typed.of(TAX_CLASS), Some(2));
        assert_eq!(typed.of(CATEGORY), Some(3));
        let ours = header(&COLUMNS.join(","));
        for (index, column) in COLUMNS.iter().enumerate() {
            assert_eq!(ours.of(column), Some(index), "{column}");
        }
    }
}
