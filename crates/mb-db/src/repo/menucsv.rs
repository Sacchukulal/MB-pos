//! The menu, in and out of a spreadsheet.

use mb_core::{CategoryId, ItemId, Money, PriceBasis, Timestamp};
use rusqlite::Transaction;

use crate::error::DbError;
use crate::export::{parse_csv, write_row};
use crate::repo::menu::{MenuItem, MenuRepo};

/// The columns, in order.
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
        match (new, updated) {
            (0, 0) => "Nothing to import — the file has no rows.".to_owned(),
            (n, 0) => format!("{n} new item(s), nothing changed."),
            (0, u) => format!("{u} item(s) would change, nothing new."),
            (n, u) => format!("{n} new item(s) and {u} change(s)."),
        }
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
        let categories = repo.list_categories(outlet)?;
        let classes = crate::repo::taxclass::TaxClassRepo::new(self.tx).list(outlet)?;

        let rows = parse_csv(csv);
        let mut plan = ImportPlan::default();

        let Some(header) = rows.first() else {
            return Ok(plan);
        };
        // The header is checked once, by name, so a file with the columns in a different order
        // still works.
        let index_of = |wanted: &str| {
            header
                .iter()
                .position(|cell| cell.as_deref().map(str::trim) == Some(wanted))
        };
        let Some(name_at) = index_of("name") else {
            plan.refused.push((
                1,
                "the first line must name the columns, and one of them must be \
                 \"name\". Export the menu first to see the shape."
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
            let id = cell(index_of("id"));
            let found = match &id {
                Some(id) => existing.iter().find(|i| i.id.as_str() == id),
                None => existing.iter().find(|i| i.name.eq_ignore_ascii_case(&name)),
            };

            let price = match cell(index_of("price_paise")) {
                Some(text) => match text.parse::<i64>() {
                    Ok(paise) if paise >= 0 => Money::from_paise(paise),
                    _ => {
                        plan.refused.push((
                            line,
                            format!("\"{text}\" is not a price in paise. ₹120 is 12000."),
                        ));
                        continue;
                    }
                },
                None => match found {
                    Some(item) => item.unit_price,
                    None => {
                        plan.refused.push((line, format!("{name} has no price")));
                        continue;
                    }
                },
            };

            let cost = match cell(index_of("cost_paise")) {
                Some(text) => match text.parse::<i64>() {
                    Ok(paise) if paise >= 0 => Some(Money::from_paise(paise)),
                    _ => {
                        plan.refused
                            .push((line, format!("\"{text}\" is not a cost in paise")));
                        continue;
                    }
                },
                None => found.and_then(|i| i.cost_price),
            };

            // A category by name, and an unknown one is refused rather than silently dropped —
            // an item that lands in "no category" is an item nobody finds again.
            let category = match cell(index_of("category")) {
                Some(text) => {
                    match categories
                        .iter()
                        .find(|c| c.name.eq_ignore_ascii_case(&text))
                    {
                        Some(category) => Some(category),
                        None => {
                            plan.refused
                                .push((line, format!("there is no category called \"{text}\"")));
                            continue;
                        }
                    }
                }
                None => found
                    .and_then(|i| i.category_id.as_ref())
                    .and_then(|id| categories.iter().find(|c| &c.id == id)),
            };

            // A tax slab by id or by name. The file wins; failing that the item keeps the slab
            // it has; a NEW item with no slab takes its category's; failing that it is refused —
            // an item with no slab cannot be billed.
            let wanted_class = cell(index_of("tax_class"));
            let class = match &wanted_class {
                Some(text) => {
                    let found_class = classes.iter().find(|c| {
                        c.is_active
                            && (c.id.as_str() == text || c.name.eq_ignore_ascii_case(text))
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
                    .or_else(|| category.and_then(|c| c.default_tax_class_id.clone()))
                {
                    Some(id) => id,
                    None => {
                        plan.refused.push((
                            line,
                            format!("{name} needs a tax slab — add a tax_class column"),
                        ));
                        continue;
                    }
                },
            };

            // The item's own say on its price, if the file has one.
            let price_basis = match cell(index_of("price_basis")) {
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
            let category = category.map(|c| c.id.clone());

            let available = match cell(index_of("available")) {
                Some(text) => matches!(
                    text.to_ascii_lowercase().as_str(),
                    "yes" | "y" | "true" | "1"
                ),
                None => found.is_none_or(|i| i.is_available),
            };

            let item = MenuItem {
                id: found.map_or_else(
                    || ItemId::new(id.clone().unwrap_or_else(|| slug(&name, line))),
                    |i| i.id.clone(),
                ),
                category_id: category.map(|c| CategoryId::new(c.as_str().to_owned())),
                name,
                unit_price: price,
                tax_class_id: class,
                price_basis,
                hsn: cell(index_of("hsn")).or_else(|| found.and_then(|i| i.hsn.clone())),
                cost_price: cost,
                short_code: cell(index_of("short_code"))
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

    /// Do exactly what the plan said.
    pub fn apply(&self, outlet: &str, plan: &ImportPlan, at: Timestamp) -> Result<usize, DbError> {
        if !plan.is_clean() {
            return Err(DbError::invariant(format!(
                "{} row(s) in that file cannot be read, so nothing was imported",
                plan.refused.len()
            )));
        }
        let repo = MenuRepo::new(self.tx);
        for item in plan.new_items.iter().chain(&plan.updated_items) {
            repo.save_item(outlet, item, at)?;
        }
        Ok(plan.new_items.len() + plan.updated_items.len())
    }
}

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

/// An id for an item the file did not name one for.
fn slug(name: &str, line: usize) -> String {
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
        format!("itm_line_{line}")
    } else {
        format!("itm_{cleaned}")
    }
}
