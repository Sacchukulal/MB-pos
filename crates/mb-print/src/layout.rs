//! Turning a [`Document`] into positioned lines.
//!
//! Three rules live here, and each one is a decision somebody would otherwise
//! make differently in each sink.
//!
//! **Rule one — wrap, never truncate.** A name that is too wide goes onto the
//! next line and no character is lost. A shop selling "Paneer Butter Masala
//! (Half) - Extra Spicy, No Onion" is not unusual.
//!
//! **Rule two — cap the font, do not fail.** A 3× heading that will not fit
//! prints at 2×. Crown jewel 18: *"font sizes are capped automatically so text
//! can never overflow the paper width."* The cap is recorded so P17 can say
//! "your heading is too big for 58 mm paper".
//!
//! **Rule three — when a row will not fit, the money wins.** See
//! [`fit_row`]. This one is not a layout preference; it is a money rule.
//!
//! And the print offset (scope 7.11) is applied **once, here**, so every sink
//! inherits it and none of them can disagree.

// The workspace denies integer division because of D7 — nothing in the MONEY
// path may be silently lossy. This module divides paper widths into columns and
// columns into scaled cells, and a receipt is 32 characters wide whether or not
// 32 divides by 3. Losing a remainder here is a character of padding; the
// remainder is then given back explicitly in `fit_columns`, so the table adds up
// exactly. No amount is computed anywhere in this file — the templates hand
// `Money::to_plain_string` in as text, and `t9` proves every number that
// reaches paper round-trips.
#![allow(
    clippy::integer_division,
    reason = "columns and cells, not money — see the note above"
)]

use serde::{Deserialize, Serialize};

use crate::doc::{Align, Block, Column, Document, Pattern, Style, Width};
use crate::error::PrintError;
use crate::paper::Paper;

/// What one laid-out line is.
/// **A struct variant, not a newtype, and that is D20 again.**
///
/// `Text(String)` looks tidier and serde cannot write it under an internal tag
/// — *"cannot serialize tagged newtype variant containing a string"* — so the
/// document would cross IPC in one direction only. Found by `t14`, which is the
/// same way D20 itself was found at P03: a round-trip test, not reasoning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LaidContent {
    /// Already wrapped, already padded to its alignment, already offset.
    Text { text: String },
    Separator {
        pattern: Pattern,
        /// Columns wide, so a sink does not have to work it out.
        width: usize,
    },
    Image {
        data: Vec<u8>,
        width_pct: u8,
        align: Align,
    },
    QrCode {
        payload: String,
        width_pct: u8,
        align: Align,
    },
    /// P29 — a scannable code, and the digits under it.
    Barcode {
        payload: String,
        human_readable: bool,
        align: Align,
    },
    Blank,
}

/// **Where one aligned run sits on a line**, measured in characters of that
/// line's own size.
///
/// # Why a padded string is not enough any more
///
/// Alignment used to be spaces: `align_to` padded a cell to its width and the
/// sinks drew the result. That is exact while every character is the same
/// width — and from 2026-08-17 a shop can choose Times New Roman, where a space
/// is about a third of a digit. Padded to the same COUNT, an amount lands a
/// long way left of the right edge, and a bill's figures stop lining up: the
/// one thing a shopkeeper checks a bill for.
///
/// So the layout says where each run's box is and how the run sits in it. The
/// text is still padded — the ESC/POS text sink prints with the printer's own
/// fixed font and wants exactly that — and a sink that can measure uses the
/// boxes instead. **Both describe the same line**, which is what keeps them
/// from drifting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    /// Characters from the start of the line's text.
    pub start: usize,
    /// How many characters wide the box is.
    pub width: usize,
    pub align: Align,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaidLine {
    pub content: LaidContent,
    pub style: Style,
    /// Columns from the left edge of the paper, **offset already applied**. A
    /// sink positions from this and never recomputes it.
    pub indent: usize,
    /// The aligned runs on this line — see [`Segment`].
    ///
    /// Empty means "one run, left, the whole line", which is what a plain line
    /// of text is and what every line was before proportional faces existed.
    /// `serde(default)` so a document written by an older build still reads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<Segment>,
}

/// Something the layout had to do to make the document fit, that a person might
/// want to know about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "note", rename_all = "snake_case")]
pub enum Note {
    /// Crown jewel 18. P17 shows this as "too big for this paper".
    ///
    /// **In dots, since 2026-08-17.** It carried the ESC/POS multiplier, which
    /// meant a heading could only ever be reported as coming down a whole cell
    /// — and, worse, could only ever be *capped* by a whole cell. A shop that
    /// set 46 got 24.
    ScaleCapped { asked: u16, used: u16 },
    /// Scope 7.11. P07's test print shows this as "clamped to +N".
    OffsetClamped { asked_mm: i32, used_columns: i32 },
    /// A row's label wrapped so its amount could stay whole. Rule three.
    LabelWrapped { label: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Laid {
    pub paper: Paper,
    pub lines: Vec<LaidLine>,
    pub notes: Vec<Note>,
}

impl Laid {
    #[must_use]
    pub fn was_capped(&self) -> bool {
        self.notes
            .iter()
            .any(|n| matches!(n, Note::ScaleCapped { .. }))
    }

    #[must_use]
    pub fn was_clamped(&self) -> bool {
        self.notes
            .iter()
            .any(|n| matches!(n, Note::OffsetClamped { .. }))
    }

    /// Every line's text, for tests and for the golden files.
    #[must_use]
    pub fn text_lines(&self) -> Vec<String> {
        self.lines
            .iter()
            .filter_map(|l| match &l.content {
                LaidContent::Text { text } => Some(format!("{}{text}", " ".repeat(l.indent))),
                _ => None,
            })
            .collect()
    }
}

/// **What a character is measured in, which depends on who is printing.**
///
/// # The one thing the two engines cannot share
///
/// A receipt is laid out once and rendered by every sink, and that is D29 —
/// which is what stops the preview and the paper disagreeing. It works because
/// every sink can draw what the layout describes.
///
/// From 2026-08-17 one of them cannot. The **graphics** engine rasterises the
/// receipt itself, so it draws text of any height. The **text** engine sends
/// characters to the printer's own font, which has one size and three
/// multipliers — 24, 48 and 72 dots and nothing between. Lay a bill out with
/// 32-dot text and 36 characters to the line, and the graphics engine prints it
/// perfectly while the printer's own font runs 4 characters off the edge of the
/// roll. `t3_the_wire_is_what_it_was` caught exactly that.
///
/// So the layout is told which grid it is for. Nothing else about it changes:
/// both engines still get one document, one set of wrapping decisions, and one
/// answer about where a line breaks — for the grid they can each actually draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Grid {
    /// Any height, in dots. The graphics engine, and the preview beside it.
    #[default]
    Dots,
    /// Whole cells of the printer's own font. The ESC/POS text engine.
    Cells,
}

/// Lay a document out for its paper, for the graphics engine.
///
/// The only fallible case is rule three's last clause: an amount that is wider
/// than the paper at scale 1. Nothing sensible can be printed and pretending
/// otherwise is worse than saying so.
pub fn layout(doc: &Document) -> Result<Laid, PrintError> {
    layout_for(doc, Grid::Dots)
}

/// Lay a document out for a particular engine's grid — see [`Grid`].
pub fn layout_for(doc: &Document, grid: Grid) -> Result<Laid, PrintError> {
    // **Snapped once, at the door.** Every size becomes a whole cell before
    // anything measures anything, so the wrapping, the column widths, the caps
    // and the rendered output all agree — rather than each rounding in its own
    // way, which is how two sinks come to disagree.
    let snapped;
    let doc = match grid {
        Grid::Dots => doc,
        Grid::Cells => {
            snapped = doc.snapped_to_cells();
            &snapped
        }
    };
    let paper = doc.paper;
    let columns = paper.columns();
    let mut notes = Vec::new();
    let mut lines = Vec::new();

    // Scope 7.11, applied once. Whole columns, because half a character is not
    // a thing a text sink can do — and if the sinks rounded differently the
    // offset would have re-created the drift this crate exists to prevent.
    let asked_mm = paper.offset.x_mm;
    let wanted = paper.kind.columns_for_mm(asked_mm);
    let indent = clamp_offset(wanted, columns, &mut notes, asked_mm);

    // A vertical offset is whole blank lines at the top.
    let top = paper.kind.columns_for_mm(paper.offset.y_mm).max(0);
    for _ in 0..top {
        lines.push(LaidLine {
            content: LaidContent::Blank,
            style: Style::NORMAL,
            indent: 0,
            segments: Vec::new(),
        });
    }

    // The usable width is what is left after the offset, so a shifted document
    // still fits rather than falling off the right edge.
    let usable = columns.saturating_sub(indent).max(1);

    let table_needs = table_needs(doc);

    for (index, block) in doc.blocks.iter().enumerate() {
        lay_block(
            block,
            usable,
            indent,
            grid,
            table_needs.get(index).copied().unwrap_or(0),
            &mut lines,
            &mut notes,
        )?;
    }

    Ok(Laid {
        paper,
        lines,
        notes,
    })
}

fn clamp_offset(wanted: i32, columns: usize, notes: &mut Vec<Note>, asked_mm: i32) -> usize {
    // Never let the offset push content off the paper. A quarter of the width
    // is already an absurd correction — the real ones are one or two columns —
    // and leaving room for at least three quarters keeps a bill readable even
    // when somebody has fat-fingered the setting.
    let limit = i32::try_from(columns / 4).unwrap_or(0);
    let used = wanted.clamp(0, limit);
    if used != wanted {
        notes.push(Note::OffsetClamped {
            asked_mm,
            used_columns: used,
        });
    }
    usize::try_from(used).unwrap_or(0)
}

/// **How much room each table would like, shared across the blocks that make
/// up one table.**
///
/// # The item table is two blocks, and they must not be two sizes
///
/// A bill's item table is pushed as a `Columns` block for the headings and a
/// second one for the rows, so a separator can go between them. Capped
/// independently they cap differently — the headings are four short words and
/// the rows carry "Paneer Butter Masala" — and the paper comes out with a
/// heading row visibly bigger than the rows underneath it. The owner's printed
/// bill on 2026-08-17 had exactly that.
///
/// So every `Columns` block with the same shape and the same style is one
/// table, and they all get the largest need among them. One cap, one size.
fn table_needs(doc: &Document) -> Vec<usize> {
    let own: Vec<usize> = doc
        .blocks
        .iter()
        .map(|block| match block {
            Block::Columns { columns, rows, .. } => columns_need(columns, rows),
            _ => 0,
        })
        .collect();

    doc.blocks
        .iter()
        .enumerate()
        .map(|(index, block)| {
            let Block::Columns { columns, style, .. } = block else {
                return 0;
            };
            doc.blocks
                .iter()
                .enumerate()
                .filter(|(_, other)| match other {
                    Block::Columns {
                        columns: theirs,
                        style: their_style,
                        ..
                    } => their_style == style && theirs == columns,
                    _ => false,
                })
                .map(|(other, _)| own.get(other).copied().unwrap_or(0))
                .max()
                .unwrap_or_else(|| own.get(index).copied().unwrap_or(0))
        })
        .collect()
}

fn lay_block(
    block: &Block,
    usable: usize,
    indent: usize,
    grid: Grid,
    table_need: usize,
    lines: &mut Vec<LaidLine>,
    notes: &mut Vec<Note>,
) -> Result<(), PrintError> {
    match block {
        Block::Spacer { lines: n } => {
            for _ in 0..*n {
                lines.push(LaidLine {
                    content: LaidContent::Blank,
                    style: Style::NORMAL,
                    indent,
            segments: Vec::new(),
                });
            }
        }

        Block::Separator { pattern } => {
            for _ in 0..pattern.lines() {
                lines.push(LaidLine {
                    content: LaidContent::Separator {
                        pattern: *pattern,
                        width: usable,
                    },
                    style: Style::NORMAL,
                    indent,
                    segments: Vec::new(),
                });
            }
        }

        Block::Image {
            data,
            width_pct,
            align,
        } => lines.push(LaidLine {
            content: LaidContent::Image {
                data: data.clone(),
                width_pct: *width_pct,
                align: *align,
            },
            style: Style::NORMAL,
            indent,
            segments: Vec::new(),
        }),

        Block::QrCode {
            payload,
            width_pct,
            align,
        } => lines.push(LaidLine {
            content: LaidContent::QrCode {
                payload: payload.clone(),
                width_pct: *width_pct,
                align: *align,
            },
            style: Style::NORMAL,
            indent,
            segments: Vec::new(),
        }),

        Block::Barcode {
            payload,
            human_readable,
            align,
        } => lines.push(LaidLine {
            content: LaidContent::Barcode {
                payload: payload.clone(),
                human_readable: *human_readable,
                align: *align,
            },
            style: Style::NORMAL,
            indent,
            segments: Vec::new(),
        }),

        Block::Text {
            content,
            style,
            align,
        } => {
            let style = cap_scale(*style, longest_word(content), usable, grid, notes);
            let width = chars_across(usable, style);
            for line in wrap(content, width.max(1)) {
                lines.push(LaidLine {
                    content: LaidContent::Text { text: align_to(&line, width.max(1), *align) },
                    style,
                    indent,
                    // One box, the whole line. A centred heading in Times New
                    // Roman is centred by MEASURE, not by counting the spaces
                    // `align_to` padded it with.
                    segments: vec![Segment {
                        start: 0,
                        width: width.max(1),
                        align: *align,
                    }],
                });
            }
        }

        Block::Row { left, right, style } => {
            let needed = left.chars().count() + right.chars().count() + 1;
            let style = cap_scale(*style, needed, usable, grid, notes);
            let width = chars_across(usable, style);
            for (line, segments) in fit_row(left, right, width, notes)? {
                lines.push(LaidLine {
                    content: LaidContent::Text { text: line },
                    style,
                    indent,
                    segments,
                });
            }
        }

        Block::Columns {
            columns,
            rows,
            style,
        } => {
            // **What a table needs is its columns' WIDTHS, not how many of them
            // there are**, and asking the wrong one of those was a bug that
            // printed one letter per line.
            //
            // The old code passed `columns.len()`. On 48-column paper an item
            // table is four columns, so `4 * 2 = 8` fitted comfortably inside
            // 48 and nothing was ever capped — while the real arithmetic was
            // `48 / 2 = 24` usable, 23 of them already spoken for by qty, rate
            // and amount, and **one** column left for the item's name. A shop
            // that set "Item list size: Large" got
            //
            //     P   2   240.00    478.80
            //     a
            //     n
            //     e
            //     e
            //     r
            //
            // and a roll of paper per bill. The owner found it on a real
            // install; no test caught it because every test used the default
            // size.
            // A table cannot go below its fixed columns plus a readable fill;
            // if it has to come down, it comes down far enough for its names.
            let style = cap_to(
                *style,
                columns_need(columns, &[]),
                // The whole table's need, not this block's — see `table_needs`.
                table_need.max(columns_need(columns, rows)),
                usable,
                grid,
                notes,
            );
            let width = chars_across(usable, style);
            let widths = fit_columns(columns, width);
            for row in rows {
                for (line, segments) in lay_row(columns, &widths, row) {
                    lines.push(LaidLine {
                        content: LaidContent::Text { text: line },
                        style,
                        indent,
                        segments,
                    });
                }
            }
        }
    }
    Ok(())
}

/// **The narrowest a fill column may be squeezed to before the scale gives
/// way instead.**
///
/// Ten characters, because that is about where a dish stops being recognisable
/// — "Paneer But" can be guessed at, "P" cannot. It is deliberately not
/// generous: a kitchen ticket's food is 2× by default and a shop is entitled to
/// keep it that way, so the number has to be small enough that a ticket
/// (5 fixed columns) stays big and a bill (23 fixed columns) comes down.
const MIN_FILL: usize = 10;

/// How many scale-1 columns a table honestly needs.
///
/// Fixed columns need exactly what they asked for. A fill column needs enough
/// to be read — see [`MIN_FILL`] — because "fill" means *take what is left*,
/// and what is left can be one character.
///
/// # It looks at what is actually IN the column, since 2026-08-17
///
/// `MIN_FILL` alone is the bare minimum, and capping to the bare minimum is
/// how "Paneer Butter Masala" came out as
///
/// ```text
/// Paneer
/// Butter
/// Masala
/// ```
///
/// at a size that technically fitted. Before sizes were continuous the cap
/// could only land on a whole multiple of the printer's cell, and it happened
/// to land somewhere the name fitted on one line — the guarantee this
/// function's own test was written for, after an owner found every dish
/// printing vertically.
///
/// So a fill column asks for its longest CELL. The table then prints as large
/// as it can with its names intact, and a size that cannot manage that comes
/// down until it can. **The reduction is reported** (`Note::ScaleCapped`) and
/// the preview shows the same thing the paper will, so a shop that would
/// rather have big text than whole names can see the trade and pick.
fn columns_need(columns: &[Column], rows: &[Vec<String>]) -> usize {
    columns
        .iter()
        .enumerate()
        .map(|(index, column)| match column.width {
            Width::Fixed(n) => n,
            Width::Fill => rows
                .iter()
                .filter_map(|row| row.get(index))
                .map(|cell| cell.chars().count())
                .max()
                .unwrap_or(MIN_FILL)
                .max(MIN_FILL),
        })
        .sum()
}


/// Rule two, crown jewel 18: reduce the size until the content fits.
///
/// Reduce, never refuse. A shop whose heading is too big for 58 mm paper gets a
/// slightly smaller heading, not an error in the middle of service.
///
/// # It comes down by dots now, not by whole cells — 2026-08-17
///
/// This reduced the ESC/POS MULTIPLIER: 3× to 2× to 1×. That was the only
/// vocabulary a size had, so it was also the only way to cap one. Now that a
/// shop can ask for any height, capping by multiplier means **a size of 46
/// dots that does not fit comes back as 24** — half of what was asked, when
/// 44 would have fitted.
///
/// The owner found it on a printed bill: they had set the item list to 46 and
/// the paper came out at the same size as everything else. *"the printed real
/// page is completely different then the setting i set."*
///
/// So the largest size that fits is worked out directly. `needed` characters
/// fit in `usable` columns when `needed × size ≤ usable × CELL` — the same
/// arithmetic `chars_across` does, rearranged — and the answer is used as-is
/// rather than rounded to anything.
fn cap_scale(style: Style, needed: usize, usable: usize, grid: Grid, notes: &mut Vec<Note>) -> Style {
    cap_to(style, needed, needed, usable, grid, notes)
}

/// **How small AUTO-capping may make text: never below the ordinary body
/// size.**
///
/// This is a floor on the layout's own decision, not on the shop's. A shop that
/// chooses a small size still gets it — `cap_to` only ever reduces, so a size
/// already below this passes through untouched.
///
/// It is one whole cell for both grids, and the reason is the same on each. The
/// printer's own font has three sizes and 1× is the smallest, so "reduce below
/// 1×" is not an instruction it can take. The graphics engine COULD go smaller,
/// and letting it did something nobody asked for: a 58 mm tax summary that used
/// to print at the normal size came out at 21 dots because the layout decided
/// four columns would fit better if it shrank them. `t3_the_wire_is_what_it_was`
/// caught it — a shop's bill quietly getting smaller is exactly the kind of
/// change a golden file exists to make visible.
const fn floor_for(_grid: Grid) -> u16 {
    BASE_CELL_PX
}

/// **Whether to cap, and how far, are two different questions.**
///
/// `least` is what the block cannot go below — a table's fixed columns plus a
/// readable minimum for its fill. If that fits, nothing is capped and the shop
/// gets the size it asked for, even if a long name wraps: a kitchen ticket's
/// food stays big and "Paneer Butter Masala" spilling onto a second line is
/// exactly right there.
///
/// `comfortable` is what the block would like — the longest thing actually in
/// the fill column. It is used **only once a cap is already needed**, to decide
/// how far down to go, because stopping at `least` is what left the bill's item
/// names in six-letter chunks.
///
/// Two numbers rather than one because a single one cannot express both: raise
/// it and the kitchen ticket shrinks for no reason; lower it and the bill's
/// names fragment. Each test in this module holds one of those halves down.
fn cap_to(
    style: Style,
    least: usize,
    comfortable: usize,
    usable: usize,
    grid: Grid,
    notes: &mut Vec<Note>,
) -> Style {
    let asked = style.height(u32::from(BASE_CELL_PX));
    if least == 0 {
        return style;
    }
    let room = usable * usize::from(BASE_CELL_PX);
    // Does it fit at all at the size that was asked for?
    if least * (asked as usize) <= room {
        return style;
    }
    let fits = (room / comfortable.max(least)).max(usize::from(floor_for(grid)));
    let used = u32::try_from(fits).unwrap_or(u32::MAX).min(asked);
    if used >= asked {
        // **Nothing was capped, so nothing is rewritten.** Returning a rebuilt
        // style here rounded every in-between size away — see the note on
        // `Note::ScaleCapped` for what that cost.
        return style;
    }
    notes.push(Note::ScaleCapped {
        asked: u16::try_from(asked).unwrap_or(u16::MAX),
        used: u16::try_from(used).unwrap_or(u16::MAX),
    });
    Style {
        size: u16::try_from(used).unwrap_or(u16::MAX),
        bold: style.bold,
    }
}

/// What one multiplier step is worth, in dots.
///
/// The printer's own cell is 12 × 24, so a `scale` of 1 is 24 dots tall. This
/// is the number `Style::px` and `Style::at_scale` convert against, and it is
/// the printer's fact rather than the paper's — every roll this product prints
/// on uses the same 24-dot Font A.
const BASE_CELL_PX: u16 = 24;

/// **How many characters of this size fit across `usable` scale-1 columns.**
///
/// This was `usable / style.scale()`, which is why there were only ever three
/// sizes: the multiplier was the only thing that could divide the paper.
///
/// A character is half as wide as it is tall (`Cell::for_column`), so a line
/// `height` dots tall advances `height / 2` dots per character against a column
/// of `BASE / 2` — which is `usable × BASE / height`. At 24 dots that is
/// `usable` exactly, at 48 it is `usable / 2`, at 72 `usable / 3`: **the three
/// old answers, unchanged**, and an answer for every size between them.
///
/// The raster sink computes its cell from the same ratio, which is what keeps
/// the two agreeing about where a line breaks.
fn chars_across(usable: usize, style: Style) -> usize {
    let height = style.height(u32::from(BASE_CELL_PX)).max(1) as usize;
    (usable * usize::from(BASE_CELL_PX) / height).max(1)
}

/// Rule three. **The money wins.**
///
/// A row is `label ............ amount`, and on 32 columns the two often cannot
/// both fit. Something has to give, and if the choice is left to whoever writes
/// a sink then one day it will be the amount.
///
/// 1. The right side is **never** shortened, wrapped or ellipsised. It is the
///    amount, and a bill whose lines do not add up because a digit was dropped
///    is the worst output this product can produce — requirement 7 of the ten
///    says the printed lines always sum to the printed total.
/// 2. The left side wraps onto continuation lines, with the amount on the
///    first.
/// 3. If the amount alone is wider than the paper, that is an **error**, not a
///    truncation. Nothing sensible can be printed.
fn fit_row(
    left: &str,
    right: &str,
    width: usize,
    notes: &mut Vec<Note>,
) -> Result<Vec<(String, Vec<Segment>)>, PrintError> {
    let right_len = right.chars().count();
    if right_len > width {
        return Err(PrintError::AmountTooWide {
            amount: right.to_owned(),
            columns: width,
        });
    }

    // At least one space between the label and the amount, always: a label that
    // touches its amount reads as one number.
    let room = width.saturating_sub(right_len).saturating_sub(1);
    let left_lines = wrap(left, room.max(1));

    let mut out = Vec::with_capacity(left_lines.len());
    let first = left_lines.first().map_or("", String::as_str);
    let pad = width
        .saturating_sub(first.chars().count())
        .saturating_sub(right_len);
    // **The boxes, said by the thing that built the line.**
    //
    // Where the amount begins is decided right here, and a sink that has to
    // measure text should be told rather than left to work it out from the
    // padding — the first attempt looked for the last run of spaces, which is
    // right for this line and wrong for every continuation line below it,
    // where there is no amount at all.
    let label_box = width.saturating_sub(right_len);
    out.push((
        format!("{first}{}{right}", " ".repeat(pad)),
        vec![
            Segment { start: 0, width: label_box, align: Align::Left },
            Segment { start: label_box, width: right_len, align: Align::Right },
        ],
    ));

    if left_lines.len() > 1 {
        notes.push(Note::LabelWrapped {
            label: left.to_owned(),
        });
        for line in &left_lines[1..] {
            // Indented by two, so a continuation reads as part of the line
            // above rather than as a new item.
            out.push((
                format!("  {line}"),
                // All label, no amount — see above.
                vec![Segment { start: 0, width, align: Align::Left }],
            ));
        }
    }
    Ok(out)
}

/// Rule: the columns always add up to the paper width, exactly.
///
/// Not less. A ragged right edge on a receipt reads as a fault, and the item
/// table is the block a customer looks at hardest.
fn fit_columns(columns: &[Column], width: usize) -> Vec<usize> {
    let fixed: usize = columns
        .iter()
        .map(|c| match c.width {
            Width::Fixed(n) => n,
            Width::Fill => 0,
        })
        .sum();
    let fills = columns
        .iter()
        .filter(|c| matches!(c.width, Width::Fill))
        .count();

    let mut widths: Vec<usize> = columns
        .iter()
        .map(|c| match c.width {
            Width::Fixed(n) => n,
            Width::Fill => 0,
        })
        .collect();

    if fills > 0 {
        let spare = width.saturating_sub(fixed);
        let each = spare / fills;
        let mut left_over = spare % fills;
        for (w, c) in widths.iter_mut().zip(columns) {
            if matches!(c.width, Width::Fill) {
                *w = each.max(1);
                if left_over > 0 {
                    *w += 1;
                    left_over -= 1;
                }
            }
        }
    }

    // Whatever is still missing or spare lands on the widest column, so the sum
    // is exact rather than nearly right.
    let total: usize = widths.iter().sum();
    if total != width && !widths.is_empty() {
        let widest = widths
            .iter()
            .enumerate()
            .max_by_key(|(_, w)| **w)
            .map_or(0, |(i, _)| i);
        if total < width {
            widths[widest] += width - total;
        } else {
            let excess = total - width;
            widths[widest] = widths[widest].saturating_sub(excess).max(1);
        }
    }
    widths
}

fn lay_row(
    columns: &[Column],
    widths: &[usize],
    cells: &[String],
) -> Vec<(String, Vec<Segment>)> {
    // Wrap every cell first, then emit as many lines as the tallest one needs.
    let wrapped: Vec<Vec<String>> = cells
        .iter()
        .enumerate()
        .map(|(i, cell)| wrap(cell, widths.get(i).copied().unwrap_or(1).max(1)))
        .collect();
    let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);

    let mut out = Vec::with_capacity(height);
    for row in 0..height {
        let mut line = String::new();
        // **The item table's boxes, which this function has always known.**
        // Qty, Rate and Amount are right-aligned columns; padded with spaces
        // they line up only while every character is the same width, and a
        // shop can choose Times New Roman since 2026-08-17. Saying where each
        // column is means a measuring sink lands them on the same edges the
        // printer's own font does.
        let mut segments = Vec::with_capacity(wrapped.len());
        let mut at = 0;
        for (i, cell_lines) in wrapped.iter().enumerate() {
            let w = widths.get(i).copied().unwrap_or(1).max(1);
            let align = columns.get(i).map_or(Align::Left, |c| c.align);
            let text = cell_lines.get(row).map_or("", String::as_str);
            line.push_str(&align_to(text, w, align));
            segments.push(Segment { start: at, width: w, align });
            at += w;
        }
        // The trim is what the text sink wants — trailing spaces are paper. The
        // boxes describe the line before it, which is what a measuring sink
        // wants, and neither loses anything the other needs.
        out.push((line.trim_end().to_owned(), segments));
    }
    out
}

/// Wrap on spaces, and break a word that is longer than the whole line rather
/// than letting it overflow. Nothing is ever dropped (rule one).
///
/// **Leading and repeated spaces ARE dropped**, and that is what wrapping on
/// spaces means. It is written down here because P17 tried to put a gutter
/// between two columns by starting a cell with a space, watched it vanish, and
/// had to be told why: a gap between columns is a **column**, not a character
/// somebody typed into a cell.
fn wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_owned()];
    }
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            if word.is_empty() {
                continue;
            }
            let word_len = word.chars().count();
            let line_len = line.chars().count();

            if line_len > 0 && line_len + 1 + word_len <= width {
                line.push(' ');
                line.push_str(word);
                continue;
            }
            if line_len > 0 {
                out.push(std::mem::take(&mut line));
            }
            if word_len <= width {
                line.push_str(word);
            } else {
                // A single word longer than the line. Break it — losing it
                // would be a truncation, and R3 says no.
                let mut chunk = String::new();
                for ch in word.chars() {
                    if chunk.chars().count() == width {
                        out.push(std::mem::take(&mut chunk));
                    }
                    chunk.push(ch);
                }
                line = chunk;
            }
        }
        out.push(line);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn align_to(text: &str, width: usize, align: Align) -> String {
    let len = text.chars().count();
    if len >= width {
        return text.to_owned();
    }
    let pad = width - len;
    match align {
        Align::Left => format!("{text}{}", " ".repeat(pad)),
        Align::Right => format!("{}{text}", " ".repeat(pad)),
        Align::Centre => {
            let before = pad / 2;
            format!("{}{text}{}", " ".repeat(before), " ".repeat(pad - before))
        }
    }
}

fn longest_word(text: &str) -> usize {
    text.split_whitespace()
        .map(|w| w.chars().count())
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paper::{Offset, PaperKind};

    #[test]
    fn wrapping_loses_nothing() {
        let name = "Paneer Butter Masala (Half) - Extra Spicy, No Onion";
        let lines = wrap(name, 20);
        let rejoined: String = lines.join(" ");
        for word in name.split_whitespace() {
            assert!(rejoined.contains(word), "{word} was lost");
        }
        assert!(lines.iter().all(|l| l.chars().count() <= 20));
    }

    #[test]
    fn a_word_longer_than_the_line_is_broken_not_dropped() {
        let lines = wrap("Supercalifragilisticexpialidocious", 10);
        let rejoined: String = lines.concat();
        assert_eq!(rejoined, "Supercalifragilisticexpialidocious");
    }

    #[test]
    fn the_money_wins_and_the_label_wraps() {
        let mut notes = Vec::new();
        let out = fit_row(
            "Paneer Butter Masala (Half) Extra Spicy",
            "1,240.00",
            32,
            &mut notes,
        )
        .expect("fits");
        assert!(out[0].0.ends_with("1,240.00"), "the amount is not intact");
        assert!(out.len() > 1, "the label should have wrapped");
        assert!(notes.iter().any(|n| matches!(n, Note::LabelWrapped { .. })));
        assert_eq!(out[0].0.chars().count(), 32);

        // **And the boxes say where the amount is**, so a proportional face
        // puts it against the same right edge the padding does.
        let amount = out[0].1.last().expect("a row has two boxes");
        assert_eq!(amount.align, Align::Right);
        assert_eq!(amount.start + amount.width, 32, "the amount box is not at the edge");
        // A continuation line is all label and no amount — the case a
        // "find the last run of spaces" guess got wrong.
        assert_eq!(out[1].1.len(), 1, "a continuation line has an amount box");
        assert_eq!(out[1].1[0].align, Align::Left);
    }

    #[test]
    fn an_amount_wider_than_the_paper_is_an_error_not_a_truncation() {
        let mut notes = Vec::new();
        let err = fit_row("x", "12,34,56,78,900.00", 10, &mut notes).expect_err("must refuse");
        assert!(matches!(err, PrintError::AmountTooWide { .. }));
    }

    #[test]
    fn columns_always_add_up_to_the_paper_width() {
        for kind in [
            PaperKind::Mm58,
            PaperKind::Mm80,
            PaperKind::Mm100,
            PaperKind::A4,
        ] {
            let width = kind.columns();
            for spec in [
                vec![
                    Column::fill(Align::Left),
                    Column::fixed(3, Align::Right),
                    Column::fixed(9, Align::Right),
                ],
                vec![
                    Column::fill(Align::Left),
                    Column::fixed(6, Align::Left),
                    Column::fixed(3, Align::Right),
                    Column::fixed(9, Align::Right),
                ],
            ] {
                let widths = fit_columns(&spec, width);
                assert_eq!(
                    widths.iter().sum::<usize>(),
                    width,
                    "{kind:?} with {} columns does not add up",
                    spec.len()
                );
            }
        }
    }

    #[test]
    fn the_offset_moves_everything_and_clamps() {
        let mut doc = Document::new(Paper::new(PaperKind::Mm80));
        doc.line("TOTAL");
        let plain = layout(&doc).expect("lays out");

        let mut shifted_doc = doc.clone();
        shifted_doc.paper = Paper::new(PaperKind::Mm80).with_offset(Offset::new(3, 0));
        let shifted = layout(&shifted_doc).expect("lays out");
        assert_eq!(plain.lines[0].indent, 0);
        assert_eq!(shifted.lines[0].indent, 2, "3 mm on 80 mm paper is 2 columns");
        assert!(!shifted.was_clamped());

        let mut silly = doc.clone();
        silly.paper = Paper::new(PaperKind::Mm58).with_offset(Offset::new(40, 0));
        let clamped = layout(&silly).expect("lays out");
        assert!(clamped.was_clamped(), "an absurd offset must be clamped");
        assert!(clamped.lines[0].indent <= PaperKind::Mm58.columns() / 4);
    }

    #[test]
    fn a_heading_too_big_for_the_paper_is_capped_not_refused() {
        let mut doc = Document::new(Paper::new(PaperKind::Mm58));
        doc.text(
            "ANNAPOORNESHWARI REFRESHMENTS",
            Style::new(3, true),
            Align::Centre,
        );
        let laid = layout(&doc).expect("lays out");
        assert!(laid.was_capped(), "crown jewel 18: the scale must be capped");
        let rendered: String = laid.text_lines().join(" ");
        for word in "ANNAPOORNESHWARI REFRESHMENTS".split(' ') {
            assert!(rendered.contains(word), "{word} was lost while capping");
        }
    }

    /// **The one-letter item column.** The owner found this on a real install:
    /// set "Item list size: Large" and every dish printed vertically, one
    /// character per line.
    ///
    /// The table is the shape `template::bill` builds — a fill for the name,
    /// then qty, rate and amount. At 2× on 48-column paper the three fixed
    /// columns take 23 of the 24 available and the name is left with one.
    #[test]
    fn a_table_whose_name_column_would_vanish_drops_the_scale_instead() {
        let columns = vec![
            Column::fill(Align::Left),
            Column::fixed(4, Align::Right),
            Column::fixed(9, Align::Right),
            Column::fixed(10, Align::Right),
        ];
        let mut doc = Document::new(Paper::new(PaperKind::Mm80));
        doc.push(Block::Columns {
            columns,
            rows: vec![vec![
                "Paneer Butter Masala".to_owned(),
                "2".to_owned(),
                "240.00".to_owned(),
                "478.80".to_owned(),
            ]],
            style: Style::new(2, false),
        });

        let laid = layout(&doc).expect("lays out");
        assert!(laid.was_capped(), "2x cannot fit this table and must be capped");

        // The whole name on one line, which is the point.
        let lines = laid.text_lines();
        assert_eq!(lines.len(), 1, "one row must not become twenty: {lines:?}");
        assert!(
            lines[0].starts_with("Paneer Butter Masala"),
            "the name column collapsed again: {:?}",
            lines[0]
        );
        assert!(lines[0].ends_with("478.80"), "the amount moved: {:?}", lines[0]);
    }

    /// The other direction, and it is the one that keeps the fix honest: a
    /// kitchen ticket has almost no fixed columns, so its food stays big.
    #[test]
    fn a_kitchen_ticket_keeps_its_big_food() {
        let columns = vec![
            Column::fixed(4, Align::Right),
            Column::fixed(1, Align::Left),
            Column::fill(Align::Left),
        ];
        let mut doc = Document::new(Paper::new(PaperKind::Mm80));
        doc.push(Block::Columns {
            columns,
            rows: vec![vec![
                "2".to_owned(),
                "x".to_owned(),
                "Paneer Butter Masala".to_owned(),
            ]],
            style: Style::new(2, true),
        });

        let laid = layout(&doc).expect("lays out");
        assert!(!laid.was_capped(), "a ticket at 2x fits and must not be shrunk");
        assert_eq!(laid.lines[0].style.scale(), 2);
    }
}
