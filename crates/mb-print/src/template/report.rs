//! A report on paper — the same figures the screen shows, on a roll or on A4.

use crate::doc::{Align, Block, Column, Document, Pattern, Style};
use crate::paper::Paper;

use super::bill::Store;

/// One column of the table, as the screen describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportColumn {
    pub header: String,
    /// Right-aligned, and on a narrow roll the figure that stands beside the name.
    pub numeric: bool,
}

/// Everything a printed report carries. Every cell is already formatted: this crate does no
/// arithmetic.
#[derive(Debug, Clone)]
pub struct ReportContext<'a> {
    pub store: &'a Store,
    pub title: &'a str,
    /// The period, in words.
    pub subtitle: &'a str,
    pub columns: &'a [ReportColumn],
    pub rows: &'a [Vec<String>],
    pub totals: Option<&'a [String]>,
    /// Anything the report has to admit.
    pub notes: &'a [String],
    /// "Printed 13 Sep 2026, 9:41 am" — the caller owns the shop's clock.
    pub printed: Option<&'a str>,
}

/// Two spaces between one column and the next.
const GUTTER: usize = 2;

/// The narrowest a name may be squeezed before the table is the wrong shape for this paper.
const LEAST_NAME: usize = 12;

/// Lay out the report.
#[must_use]
pub fn report_document(paper: Paper, ctx: &ReportContext<'_>) -> Document {
    let mut doc = Document::new(paper);
    head(&mut doc, ctx);

    if ctx.columns.is_empty() || ctx.rows.is_empty() {
        doc.spacer(1)
            .text("Nothing in this period", Style::NORMAL, Align::Centre);
    } else if fits_a_table(paper, ctx) {
        table(&mut doc, ctx);
    } else {
        slips(&mut doc, paper, ctx);
    }

    for note in ctx.notes {
        doc.spacer(1).line(note.clone());
    }
    if let Some(printed) = ctx.printed {
        doc.spacer(1).text(printed, Style::NORMAL, Align::Centre);
    }
    doc.spacer(1);
    doc
}

/// Whose figures these are, what they are, and what they cover.
fn head(doc: &mut Document, ctx: &ReportContext<'_>) {
    doc.text(ctx.store.name.clone(), Style::new(2, true), Align::Centre);
    if !ctx.store.address.is_empty() {
        doc.text(ctx.store.address.clone(), Style::NORMAL, Align::Centre);
    }
    doc.spacer(1)
        .text(ctx.title, Style::new(1, true), Align::Centre)
        .text(ctx.subtitle, Style::NORMAL, Align::Centre)
        .separator(Pattern::Double);
}

/// The widest cell in a column, the header counted.
fn widest(ctx: &ReportContext<'_>, index: usize) -> usize {
    ctx.rows
        .iter()
        .map(Vec::as_slice)
        .chain(ctx.totals)
        .filter_map(|row| row.get(index))
        .map(|cell| cell.chars().count())
        .chain(std::iter::once(ctx.columns[index].header.chars().count()))
        .max()
        .unwrap_or(1)
}

/// Does the whole table fit across this paper? A roll that cannot hold six columns gets the
/// figures stacked instead of a table shrunk until nobody can read it.
fn fits_a_table(paper: Paper, ctx: &ReportContext<'_>) -> bool {
    let name_at = name_column(ctx);
    let wanted: usize = (0..ctx.columns.len())
        .map(|index| {
            if index == name_at {
                widest(ctx, index).min(LEAST_NAME)
            } else {
                widest(ctx, index)
            }
            .saturating_add(GUTTER)
            .saturating_add(if gap_before(ctx, index) { GUTTER } else { 0 })
        })
        .sum();
    wanted <= paper.columns()
}

/// The column that names a row — the first that is not a figure.
fn name_column(ctx: &ReportContext<'_>) -> usize {
    ctx.columns.iter().position(|c| !c.numeric).unwrap_or(0)
}

/// A figure sits at the right of its column and a word at the left, so a word straight after
/// a figure would touch it — "2Cash". Those two get a blank column between them.
fn gap_before(ctx: &ReportContext<'_>, index: usize) -> bool {
    index > 0 && !ctx.columns[index].numeric && ctx.columns[index - 1].numeric
}

/// The paper is wide enough: one row per row, the way the screen has it.
fn table(doc: &mut Document, ctx: &ReportContext<'_>) {
    let name_at = name_column(ctx);
    // The laid columns, and which of the report's each one carries — `None` is a gap.
    let mut columns: Vec<Column> = Vec::new();
    let mut carries: Vec<Option<usize>> = Vec::new();
    for (index, spec) in ctx.columns.iter().enumerate() {
        if gap_before(ctx, index) {
            columns.push(Column::fixed(GUTTER, Align::Left));
            carries.push(None);
        }
        columns.push(if index == name_at {
            Column::fill(Align::Left)
        } else if spec.numeric {
            Column::fixed(widest(ctx, index) + GUTTER, Align::Right)
        } else {
            Column::fixed(widest(ctx, index) + GUTTER, Align::Left)
        });
        carries.push(Some(index));
    }
    let cells = |row: &[String]| -> Vec<String> {
        carries
            .iter()
            .map(|from| from.and_then(|i| row.get(i).cloned()).unwrap_or_default())
            .collect()
    };
    let headers: Vec<String> = ctx.columns.iter().map(|c| c.header.clone()).collect();

    doc.push(Block::Columns {
        columns: columns.clone(),
        rows: vec![cells(&headers)],
        style: Style::new(1, true),
        measured_as: None,
    })
    .separator(Pattern::Solid)
    .push(Block::Columns {
        columns: columns.clone(),
        rows: ctx.rows.iter().map(|row| cells(row)).collect(),
        style: Style::NORMAL,
        measured_as: None,
    });

    if let Some(totals) = ctx.totals {
        doc.separator(Pattern::Solid).push(Block::Columns {
            columns,
            rows: vec![cells(totals)],
            style: Style::new(1, true),
            measured_as: None,
        });
    }
}

/// A till roll: the name and the figure that matters on one row, the rest underneath in words.
/// Six columns on 80 mm paper is four characters a column, and that is not a report.
fn slips(doc: &mut Document, paper: Paper, ctx: &ReportContext<'_>) {
    let name_at = name_column(ctx);
    let last = ctx.columns.len() - 1;
    // The figure beside the name is the last column — the total, in every report that has one.
    let figure_at = (last != name_at).then_some(last);
    let across = pairs_across(paper, ctx, name_at, figure_at);

    for (n, row) in ctx.rows.iter().enumerate() {
        if n > 0 {
            doc.air(1);
        }
        slip(doc, ctx, row, name_at, figure_at, across, Style::NORMAL);
    }

    if let Some(totals) = ctx.totals {
        doc.separator(Pattern::Solid);
        slip(doc, ctx, totals, name_at, figure_at, across, Style::BOLD);
    }
}

/// How many labelled figures go on one line: two on a roll, one where two would wrap.
fn pairs_across(
    paper: Paper,
    ctx: &ReportContext<'_>,
    name_at: usize,
    figure_at: Option<usize>,
) -> usize {
    let longest = (0..ctx.columns.len())
        .filter(|index| *index != name_at && Some(*index) != figure_at)
        // The label, a space, and the widest figure under it.
        .map(|index| ctx.columns[index].header.chars().count() + 1 + widest(ctx, index))
        .max()
        .unwrap_or(0);
    if longest * 2 + GUTTER <= paper.columns() {
        2
    } else {
        1
    }
}

/// One row of a till-roll report: the head line, then the columns it could not carry.
fn slip(
    doc: &mut Document,
    ctx: &ReportContext<'_>,
    row: &[String],
    name_at: usize,
    figure_at: Option<usize>,
    across: usize,
    style: Style,
) {
    let cell = |index: usize| row.get(index).cloned().unwrap_or_default();
    let name = cell(name_at);
    match figure_at {
        Some(index) => {
            doc.row(name, cell(index), style);
        }
        None => {
            doc.text(name, style, Align::Left);
        }
    }

    // "Bills 3" and not a bare "3": the header is the label, because a column of figures under
    // a name says nothing once the name has been read.
    let rest: Vec<String> = (0..ctx.columns.len())
        .filter(|index| *index != name_at && Some(*index) != figure_at)
        .filter_map(|index| {
            let value = cell(index);
            (!value.trim().is_empty()).then(|| format!("{} {value}", ctx.columns[index].header))
        })
        .collect();
    if rest.is_empty() {
        return;
    }
    // In a table, so a pair is never broken across a wrap — which is what one long line of
    // them did on 80 mm paper.
    let columns = vec![Column::fill(Align::Left); across];
    doc.push(Block::Columns {
        rows: rest
            .chunks(across)
            .map(|line| {
                (0..across)
                    .map(|at| line.get(at).cloned().unwrap_or_default())
                    .collect()
            })
            .collect(),
        columns,
        style: Style::NORMAL,
        measured_as: None,
    });
}
