//! **The menu, in and out of a spreadsheet** — P13 item 7.
//!
//! > *"A shop with 400 items will not type them in, and a setup nobody
//! > finishes is a sale nobody keeps."*
//!
//! This is the difference between an owner who goes live on Tuesday and one who
//! gives up on Thursday, which makes it the most commercially important twenty
//! minutes of work in the session.
//!
//! # The dry run is the feature
//!
//! [`plan`] reads the file and reports exactly what *would* happen — *"312 new,
//! 88 updated, 4 refused"* — and writes nothing at all. [`apply`] then does
//! precisely that and nothing else, from the same plan.
//!
//! # An import that half succeeds is worse than one that refuses
//!
//! One transaction: **every row or none**. A shop that imports 400 items and
//! gets 396 has no way to know which four are missing, and will find out from a
//! customer. So a single bad row refuses the whole file, **by line number**, and
//! the owner fixes one cell and tries again.
//!
//! # It does not write its own CSV
//!
//! `export.rs` already has a correct writer and parser — quoting, doubled
//! quotes, CRLF, and NULL kept distinct from the empty string, which is audit
//! G7's *"an item name containing a comma will break the columns of that row"*.
//! A second one here would be a second set of those four bugs.

use mb_core::{CategoryId, ItemId, Money, TaxClassId, TaxRate, TaxTreatment, Timestamp};
use rusqlite::Transaction;

use crate::error::DbError;
use crate::export::{parse_csv, write_row};
use crate::repo::menu::{MenuItem, MenuRepo};

/// The columns, in order. **The header is the contract**, so an owner can open
/// an export, add rows, and import it back.
const COLUMNS: &[&str] = &[
    "id",
    "name",
    "category",
    "price_paise",
    "tax_class",
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
    /// `(line number, why)` — the line number is the one in the owner's
    /// spreadsheet, counting the header as line 1.
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
    ///
    /// **Money is integer paise, and the header says so.** A spreadsheet will
    /// reinterpret `123.45` as a float and hand it back rounded, which is D2
    /// undone by Excel — `export.rs` made that rule and this obeys it.
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
                    item.tax_class_id.as_ref().map(TaxClassId::as_str),
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

    /// **The dry run.** Reads the file, decides everything, writes nothing.
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
        // The header is checked once, by name, so a file with the columns in a
        // different order still works — an owner WILL move a column.
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
            // The owner's line number: the header is line 1.
            let line = offset + 2;
            if row.iter().all(|cell| cell.as_deref().unwrap_or("").trim().is_empty()) {
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

            // An id matches an existing item; without one, the NAME does, so an
            // owner can send a plain two-column list and have it update rather
            // than duplicate.
            let id = cell(index_of("id"));
            let found = match &id {
                Some(id) => existing.iter().find(|i| i.id.as_str() == id),
                None => existing
                    .iter()
                    .find(|i| i.name.eq_ignore_ascii_case(&name)),
            };

            let price = match cell(index_of("price_paise")) {
                Some(text) => match text.parse::<i64>() {
                    Ok(paise) if paise >= 0 => Money::from_paise(paise),
                    _ => {
                        plan.refused.push((
                            line,
                            format!(
                                "\"{text}\" is not a price in paise. ₹120 is 12000."
                            ),
                        ));
                        continue;
                    }
                },
                None => match found {
                    Some(item) => item.unit_price,
                    None => {
                        plan.refused
                            .push((line, format!("{name} has no price")));
                        continue;
                    }
                },
            };

            let cost = match cell(index_of("cost_paise")) {
                Some(text) => match text.parse::<i64>() {
                    Ok(paise) if paise >= 0 => Some(Money::from_paise(paise)),
                    _ => {
                        plan.refused.push((
                            line,
                            format!("\"{text}\" is not a cost in paise"),
                        ));
                        continue;
                    }
                },
                None => found.and_then(|i| i.cost_price),
            };

            // A tax class by id or by name — an owner types "Restaurant food 5%",
            // not "tax_food_5".
            let wanted_class = cell(index_of("tax_class"));
            let class = match &wanted_class {
                Some(text) => {
                    let found_class = classes
                        .iter()
                        .find(|c| c.id.as_str() == text || c.name.eq_ignore_ascii_case(text));
                    match found_class {
                        Some(class) => Some(class),
                        None => {
                            plan.refused.push((
                                line,
                                format!(
                                    "\"{text}\" is not one of this shop's tax classes"
                                ),
                            ));
                            continue;
                        }
                    }
                }
                None => None,
            };

            let (rate, treatment) = match class {
                Some(class) => (class.rate, class.treatment),
                None => found.map_or((TaxRate::ZERO, TaxTreatment::Exclusive), |i| {
                    (i.tax_rate, i.tax_treatment)
                }),
            };

            // A category by name, and an unknown one is refused rather than
            // silently dropped — an item that lands in "no category" is an item
            // nobody finds again.
            let category = match cell(index_of("category")) {
                Some(text) => {
                    match categories.iter().find(|c| c.name.eq_ignore_ascii_case(&text)) {
                        Some(category) => Some(category.id.clone()),
                        None => {
                            plan.refused.push((
                                line,
                                format!("there is no category called \"{text}\""),
                            ));
                            continue;
                        }
                    }
                }
                None => found.and_then(|i| i.category_id.clone()),
            };

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
                tax_class_id: class.map(|c| c.id.clone()),
                tax_rate: rate,
                tax_treatment: treatment,
                hsn: cell(index_of("hsn")).or_else(|| found.and_then(|i| i.hsn.clone())),
                cost_price: cost,
                short_code: cell(index_of("short_code"))
                    .or_else(|| found.and_then(|i| i.short_code.clone())),
                prep_minutes: found.and_then(|i| i.prep_minutes),
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

    /// **Do exactly what the plan said.**
    ///
    /// Refuses outright if the plan has any refusal — the caller has already
    /// been shown them, and an import that half succeeds is worse than one that
    /// refuses. The whole thing is one transaction because the caller owns it
    /// (`repo/mod.rs`), so a failure part way leaves nothing behind.
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

/// An id for an item the file did not name one for.
///
/// Derived from the name and the line so two imports of the same file update
/// rather than duplicate — the property an owner assumes and nobody tells them
/// about.
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
