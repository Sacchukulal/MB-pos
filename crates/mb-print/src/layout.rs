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
    Blank,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaidLine {
    pub content: LaidContent,
    pub style: Style,
    /// Columns from the left edge of the paper, **offset already applied**. A
    /// sink positions from this and never recomputes it.
    pub indent: usize,
}

/// Something the layout had to do to make the document fit, that a person might
/// want to know about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "note", rename_all = "snake_case")]
pub enum Note {
    /// Crown jewel 18. P17 shows this as "too big for this paper".
    ScaleCapped { asked: u8, used: u8 },
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

/// Lay a document out for its paper.
///
/// The only fallible case is rule three's last clause: an amount that is wider
/// than the paper at scale 1. Nothing sensible can be printed and pretending
/// otherwise is worse than saying so.
pub fn layout(doc: &Document) -> Result<Laid, PrintError> {
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
        });
    }

    // The usable width is what is left after the offset, so a shifted document
    // still fits rather than falling off the right edge.
    let usable = columns.saturating_sub(indent).max(1);

    for block in &doc.blocks {
        lay_block(block, usable, indent, &mut lines, &mut notes)?;
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

fn lay_block(
    block: &Block,
    usable: usize,
    indent: usize,
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
        }),

        Block::Text {
            content,
            style,
            align,
        } => {
            let style = cap_scale(*style, longest_word(content), usable, notes);
            let width = usable / usize::from(style.scale());
            for line in wrap(content, width.max(1)) {
                lines.push(LaidLine {
                    content: LaidContent::Text { text: align_to(&line, width.max(1), *align) },
                    style,
                    indent,
                });
            }
        }

        Block::Row { left, right, style } => {
            let needed = left.chars().count() + right.chars().count() + 1;
            let style = cap_scale(*style, needed, usable, notes);
            let width = (usable / usize::from(style.scale())).max(1);
            for line in fit_row(left, right, width, notes)? {
                lines.push(LaidLine {
                    content: LaidContent::Text { text: line },
                    style,
                    indent,
                });
            }
        }

        Block::Columns {
            columns,
            rows,
            style,
        } => {
            let style = cap_scale(*style, columns.len(), usable, notes);
            let width = (usable / usize::from(style.scale())).max(1);
            let widths = fit_columns(columns, width);
            for row in rows {
                for line in lay_row(columns, &widths, row) {
                    lines.push(LaidLine {
                        content: LaidContent::Text { text: line },
                        style,
                        indent,
                    });
                }
            }
        }
    }
    Ok(())
}

/// Rule two, crown jewel 18: reduce the scale until the content fits.
///
/// Reduce, never refuse. A shop whose heading is too big for 58 mm paper gets a
/// slightly smaller heading, not an error in the middle of service.
fn cap_scale(style: Style, needed: usize, usable: usize, notes: &mut Vec<Note>) -> Style {
    let asked = style.scale();
    let mut used = asked;
    while used > 1 && needed * usize::from(used) > usable {
        used -= 1;
    }
    if used != asked {
        notes.push(Note::ScaleCapped { asked, used });
    }
    Style {
        scale: used,
        bold: style.bold,
    }
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
) -> Result<Vec<String>, PrintError> {
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
    out.push(format!("{first}{}{right}", " ".repeat(pad)));

    if left_lines.len() > 1 {
        notes.push(Note::LabelWrapped {
            label: left.to_owned(),
        });
        for line in &left_lines[1..] {
            // Indented by two, so a continuation reads as part of the line
            // above rather than as a new item.
            out.push(format!("  {line}"));
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

fn lay_row(columns: &[Column], widths: &[usize], cells: &[String]) -> Vec<String> {
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
        for (i, cell_lines) in wrapped.iter().enumerate() {
            let w = widths.get(i).copied().unwrap_or(1).max(1);
            let align = columns.get(i).map_or(Align::Left, |c| c.align);
            let text = cell_lines.get(row).map_or("", String::as_str);
            line.push_str(&align_to(text, w, align));
        }
        out.push(line.trim_end().to_owned());
    }
    out
}

/// Wrap on spaces, and break a word that is longer than the whole line rather
/// than letting it overflow. Nothing is ever dropped (rule one).
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
        assert!(out[0].ends_with("1,240.00"), "the amount is not intact");
        assert!(out.len() > 1, "the label should have wrapped");
        assert!(notes.iter().any(|n| matches!(n, Note::LabelWrapped { .. })));
        assert_eq!(out[0].chars().count(), 32);
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
}
