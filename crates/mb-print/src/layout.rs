//! Turning a [`Document`] into positioned lines.
//!
//! Four rules live here, and each one is a decision somebody would otherwise
//! make differently in each sink.
//!
//! **Rule one — wrap, never truncate.** A name that is too wide goes onto the
//! next line and no character is lost. A shop selling "Paneer Butter Masala
//! (Half) - Extra Spicy, No Onion" is not unusual.
//!
//! **Rule two — the size a shop chose is the size that prints.** This used to
//! say *"cap the font, do not fail"*, and capping is what made five of the ten
//! sizes on the settings screen print identically: the cap landed on
//! `room / longest-name-on-this-bill`, which is the same number for every size
//! above it and a **different** number on the next bill. The owner found it on
//! real paper and ruled:
//!
//! > *"of item name, it may be in one line or 2 line, i dont want to damage the
//! > legth width ratio… u can take 2 lines if item name is tooo long, then
//! > only, otherwise in the same line… so dont damage the design, font, styles
//! > etc."*
//!
//! So a name that does not fit **wraps**. The size is reduced only when the
//! block cannot be drawn at all — a table whose fixed columns leave no room for
//! a single character of name, a heading with one word wider than the paper, an
//! amount wider than the line. That is crown jewel 18 honoured where it is
//! actually about overflow, and nowhere else. Every reduction is reported
//! ([`Note::ScaleCapped`]) and the preview says so in words.
//!
//! **Rule three — when a row will not fit, the money wins.** See [`fit_row`].
//! This one is not a layout preference; it is a money rule.
//!
//! **Rule four — everything is measured, nothing is assumed.** How many
//! characters fit on a line comes from [`crate::metrics::Metrics`], which knows
//! the face and the size. It used to be `usable × 24 / height`, which assumed
//! every character is half as wide as it is tall — true of the printer's own
//! font and of nothing else, and the reason the paper and the preview could
//! never agree.
//!
//! And the print offset (scope 7.11) is applied **once, here**, so every sink
//! inherits it and none of them can disagree.

// The workspace denies integer division because of D7 — nothing in the MONEY
// path may be silently lossy. This module divides paper widths into columns and
// dots into characters, and a receipt is 44 characters wide whether or not 44
// divides evenly. Losing a remainder here is a dot of padding; the remainder is
// then given back explicitly in `fit_columns`, so the table adds up exactly. No
// amount is computed anywhere in this file — the templates hand
// `Money::to_plain_string` in as text, and `t9` proves every number that
// reaches paper round-trips.
#![allow(
    clippy::integer_division,
    reason = "columns and dots, not money — see the note above"
)]

use serde::{Deserialize, Serialize};

use crate::doc::{Align, BandLine, Block, Column, Document, Pattern, Style, Width};
use crate::error::PrintError;
use crate::image::Monochrome;
use crate::metrics::Metrics;
use crate::paper::Paper;

/// **How thick a drawn rule is, in dots, for each pattern** — P32.
///
/// A separator used to be the pattern's character repeated across the paper,
/// and measured on real paper a `-` is **five dots of ink inside a twelve-dot
/// cell**: the printed rule came out as a row of widely spaced ticks while the
/// preview drew a near-solid line. Three dots of ink in Times New Roman.
///
/// A rule on a bill is a rule. These are the numbers the sinks draw it with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    /// Dots of ink, top to bottom, in each stroke.
    pub thickness: u32,
    /// Dots of ink before a gap. `None` is continuous.
    pub dash: Option<(u32, u32)>,
    /// How many strokes, and the gap between them.
    pub strokes: u32,
    pub stroke_gap: u32,
}

impl Rule {
    /// The rule this pattern draws.
    #[must_use]
    pub const fn of(pattern: Pattern) -> Rule {
        match pattern {
            Pattern::Solid => Rule {
                thickness: 1,
                dash: None,
                strokes: 1,
                stroke_gap: 0,
            },
            // Six on, four off — long enough to read as a dashed line at arm's
            // length and short enough that the ends always land on ink.
            Pattern::Dashed => Rule {
                thickness: 1,
                dash: Some((6, 4)),
                strokes: 1,
                stroke_gap: 0,
            },
            Pattern::Dotted => Rule {
                thickness: 1,
                dash: Some((2, 4)),
                strokes: 1,
                stroke_gap: 0,
            },
            Pattern::Bold => Rule {
                thickness: 3,
                dash: None,
                strokes: 1,
                stroke_gap: 0,
            },
            Pattern::Double => Rule {
                thickness: 1,
                dash: None,
                strokes: 2,
                stroke_gap: 3,
            },
        }
    }

    /// Air above and below a rule, in dots. A rule that touches the line above
    /// it reads as an underline.
    pub const AIR: u32 = 3;

    /// The whole row a rule occupies.
    #[must_use]
    pub const fn row(&self) -> u32 {
        let ink = self.thickness * self.strokes + self.stroke_gap * (self.strokes - 1);
        ink + Rule::AIR * 2
    }
}

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
        /// **Dots** wide, so a sink draws exactly this and works nothing out.
        width: u32,
    },
    Image {
        data: Vec<u8>,
        width_pct: u8,
        align: Align,
    },
    /// **A logo and the shop's name side by side** — P32, [`Block::Band`].
    ///
    /// Every position is already decided, in dots from the left edge of the
    /// paper and from the top of the band. A sink blits and draws; it works
    /// nothing out, exactly as everywhere else.
    Band {
        image: Vec<u8>,
        image_left: u32,
        image_top: u32,
        image_width: u32,
        image_height: u32,
        lines: Vec<BandText>,
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

/// One line of text inside a [`LaidContent::Band`], already placed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BandText {
    pub text: String,
    pub style: Style,
    /// Dots from the left edge of the paper to the box this run sits in.
    pub left: u32,
    /// Dots from the top of the band to the top of this row.
    pub top: u32,
    /// The box, in dots. The run is aligned inside it.
    pub width: u32,
    pub align: Align,
}

/// **Where one aligned run sits on a line**, measured in characters of that
/// line's own size.
///
/// # Why a padded string is not enough
///
/// Alignment used to be spaces: `align_to` padded a cell to its width and the
/// sinks drew the result. That is exact while every character is the same
/// width — and a shop can choose Times New Roman, where a space is about a
/// third of a digit. Padded to the same COUNT, an amount lands a long way left
/// of the right edge, and a bill's figures stop lining up: the one thing a
/// shopkeeper checks a bill for.
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
    /// **Dots** from the left edge of the paper, offset already applied. A sink
    /// positions from this and never recomputes it.
    pub indent_dots: u32,
    /// **Dots** of paper this line spends, top to bottom. Measured from the
    /// face's real ink plus leading — not from a nominal height nothing drew.
    pub row_dots: u32,
    /// The aligned runs on this line — see [`Segment`].
    ///
    /// Empty means "one run, left, the whole line", which is what a plain line
    /// of text is.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<Segment>,
}

/// Something the layout had to do to make the document fit, that a person might
/// want to know about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "note", rename_all = "snake_case")]
pub enum Note {
    /// Crown jewel 18, and **only** where it is genuinely about overflow: a
    /// block that cannot be drawn at all at the size asked for. P17 shows this
    /// as "too big for this paper".
    ScaleCapped { asked: u16, used: u16 },
    /// Scope 7.11. P07's test print shows this as "clamped to +N".
    OffsetClamped { asked_mm: i32, used_dots: i32 },
    /// A row's label wrapped so its amount could stay whole. Rule three.
    LabelWrapped { label: String },
    /// **A logo that could not be read**, so the letterhead printed as text
    /// across the whole paper. D37: a shop with a corrupt logo still gets its
    /// bill.
    LogoUnreadable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Laid {
    pub paper: Paper,
    pub lines: Vec<LaidLine>,
    pub notes: Vec<Note>,
    /// **One character of the body size, in dots.** What an indent is a whole
    /// number of, and what a sink divides by to get back to columns.
    pub base_advance: u32,
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

    /// **How much roll this costs, in dots.** The preview says it in
    /// millimetres, because the owner's complaint began with paper being eaten.
    #[must_use]
    pub fn total_dots(&self) -> u32 {
        self.lines.iter().map(|l| l.row_dots).sum()
    }

    /// The same, in millimetres. A thermal head is 8 dots to the millimetre on
    /// every roll this product prints on.
    #[must_use]
    pub fn total_mm(&self) -> u32 {
        self.total_dots().div_ceil(crate::paper::DOTS_PER_MM)
    }

    /// **Dots back into whole characters of the body size.** What the ESC/POS
    /// text sink pads with, and what a golden file records.
    #[must_use]
    pub const fn columns_of(&self, dots: u32) -> usize {
        if self.base_advance == 0 {
            return 0;
        }
        (dots / self.base_advance) as usize
    }

    /// This line's indent, in those characters.
    #[must_use]
    pub const fn indent_columns(&self, line: &LaidLine) -> usize {
        self.columns_of(line.indent_dots)
    }

    /// Every line's text, for tests and for the golden files.
    #[must_use]
    pub fn text_lines(&self) -> Vec<String> {
        self.lines
            .iter()
            .filter_map(|l| match &l.content {
                LaidContent::Text { text } => {
                    Some(format!("{}{text}", " ".repeat(self.indent_columns(l))))
                }
                _ => None,
            })
            .collect()
    }
}

/// Lay a document out for a set of metrics — see [`crate::metrics`].
///
/// The only fallible case is rule three's last clause: an amount that is wider
/// than the paper even at the smallest size this product offers. Nothing
/// sensible can be printed and pretending otherwise is worse than saying so.
pub fn layout_for(doc: &Document, metrics: &Metrics) -> Result<Laid, PrintError> {
    let paper = doc.paper;
    let dots = metrics.dots();
    let base_advance = metrics.body().advance.max(1);
    let mut notes = Vec::new();
    let mut lines = Vec::new();

    // Scope 7.11, applied once. Whole characters of the body size, because
    // half a character is not a thing a text sink can do — and if the sinks
    // rounded differently the offset would have re-created the drift this
    // crate exists to prevent.
    let asked_mm = paper.offset.x_mm;
    let indent_dots = clamp_offset(asked_mm, dots, base_advance, &mut notes);

    // A vertical offset is dots of blank at the top — exact, where it used to
    // be rounded to whole lines.
    let top = paper.offset.y_mm.max(0);
    let top_dots =
        u32::try_from(top).unwrap_or(0) * crate::paper::DOTS_PER_MM;
    if top_dots > 0 {
        lines.push(LaidLine {
            content: LaidContent::Blank,
            style: Style::NORMAL,
            indent_dots: 0,
            row_dots: top_dots,
            segments: Vec::new(),
        });
    }

    // The usable width is what is left after the offset, so a shifted document
    // still fits rather than falling off the right edge.
    let usable = dots.saturating_sub(indent_dots).max(base_advance);

    for block in &doc.blocks {
        lay_block(
            block,
            usable,
            indent_dots,
            metrics,
            &mut lines,
            &mut notes,
        )?;
    }

    Ok(Laid {
        paper,
        lines,
        notes,
        base_advance,
    })
}

/// Lay a document out with the built-in face, for a caller that has none —
/// tests, and the samples the settings screen draws before a printer is chosen.
pub fn layout(doc: &Document) -> Result<Laid, PrintError> {
    let font = std::sync::Arc::new(crate::font::Font::builtin()?);
    layout_for(doc, &Metrics::face(doc.paper, font))
}

fn clamp_offset(asked_mm: i32, dots: u32, advance: u32, notes: &mut Vec<Note>) -> u32 {
    if asked_mm <= 0 {
        return 0;
    }
    let asked_dots = asked_mm.saturating_mul(
        i32::try_from(crate::paper::DOTS_PER_MM).unwrap_or(8),
    );
    // On the body grid, so the text sink can pad with whole characters.
    let advance = i32::try_from(advance).unwrap_or(12).max(1);
    let snapped = ((asked_dots + advance / 2) / advance) * advance;
    // Never let the offset push content off the paper. A quarter of the width
    // is already an absurd correction — the real ones are one or two
    // millimetres — and leaving three quarters keeps a bill readable even when
    // somebody has fat-fingered the setting.
    let limit = (i32::try_from(dots).unwrap_or(i32::MAX) / 4 / advance) * advance;
    let used = snapped.clamp(0, limit);
    if used != snapped {
        notes.push(Note::OffsetClamped {
            asked_mm,
            used_dots: used,
        });
    }
    u32::try_from(used).unwrap_or(0)
}

fn lay_block(
    block: &Block,
    usable: u32,
    indent_dots: u32,
    metrics: &Metrics,
    lines: &mut Vec<LaidLine>,
    notes: &mut Vec<Note>,
) -> Result<(), PrintError> {
    match block {
        Block::Spacer { lines: n } => {
            let row = metrics.body().row;
            for _ in 0..*n {
                lines.push(LaidLine {
                    content: LaidContent::Blank,
                    style: Style::NORMAL,
                    indent_dots,
                    row_dots: row,
                    segments: Vec::new(),
                });
            }
        }

        // **The ruler is as wide as the paper because the LAYOUT draws it.**
        // See `Block::Ruler`.
        Block::Ruler { marks } => {
            let size = metrics.body();
            let width = size.chars_across(usable);
            let text = if *marks {
                crate::testprint::ruler_marks(width)
            } else {
                crate::testprint::ruler_numbers(width)
            };
            lines.push(LaidLine {
                content: LaidContent::Text { text },
                style: Style::NORMAL,
                indent_dots,
                row_dots: size.row,
                segments: vec![Segment {
                    start: 0,
                    width,
                    align: Align::Left,
                }],
            });
        }

        Block::Separator { pattern } => {
            // **One line, whatever the pattern.** `Pattern::Double` used to
            // emit two rows of characters; it is one rule with two strokes now,
            // which is what it always looked like and half the paper.
            lines.push(LaidLine {
                content: LaidContent::Separator {
                    pattern: *pattern,
                    width: usable,
                },
                style: Style::NORMAL,
                indent_dots,
                row_dots: Rule::of(*pattern).row(),
                segments: Vec::new(),
            });
        }

        Block::Image {
            data,
            width_pct,
            align,
        } => {
            let width = usable * u32::from((*width_pct).clamp(1, 100)) / 100;
            let height = match Monochrome::size(data) {
                Some((w, h)) if w > 0 => h * width.max(1) / w,
                _ => 0,
            };
            lines.push(LaidLine {
                content: LaidContent::Image {
                    data: data.clone(),
                    width_pct: *width_pct,
                    align: *align,
                },
                style: Style::NORMAL,
                indent_dots,
                row_dots: height,
                segments: Vec::new(),
            });
        }

        Block::Band {
            image,
            image_side,
            image_pct,
            text,
        } => lay_band(
            image,
            *image_side,
            *image_pct,
            text,
            usable,
            indent_dots,
            metrics,
            lines,
            notes,
        )?,

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
            indent_dots,
            // The printer draws the square itself and the sink cannot know how
            // tall it will be. A QR is about a third of the paper across and
            // square, which is the honest estimate for "how long is this bill".
            row_dots: usable * u32::from((*width_pct).clamp(1, 100)) / 100,
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
            indent_dots,
            // `GS h` is set to 60 dots by `escpos`, plus the characters under it.
            row_dots: 60 + if *human_readable { metrics.body().row } else { 0 },
            segments: Vec::new(),
        }),

        Block::Text {
            content,
            style,
            align,
        } => {
            // The last resort, and only that: a single word wider than the
            // paper cannot be drawn at this size at all. A long name that
            // merely needs two lines gets two lines.
            let style = fit_at_least(*style, longest_word(content), usable, metrics, notes);
            let size = metrics.size(style);
            let width = size.chars_across(usable);
            for line in wrap(content, width) {
                lines.push(LaidLine {
                    content: LaidContent::Text {
                        text: align_to(&line, width, *align),
                    },
                    style,
                    indent_dots,
                    row_dots: size.row,
                    // One box, the whole line. A centred heading in Times New
                    // Roman is centred by MEASURE, not by counting the spaces
                    // `align_to` padded it with.
                    segments: vec![Segment {
                        start: 0,
                        width,
                        align: *align,
                    }],
                });
            }
        }

        Block::Row { left, right, style } => {
            // The amount has to fit; nothing else about a row does.
            let style = fit_at_least(*style, right.chars().count() + 1, usable, metrics, notes);
            let size = metrics.size(style);
            let width = size.chars_across(usable);
            for (line, segments) in fit_row(left, right, width, notes)? {
                lines.push(LaidLine {
                    content: LaidContent::Text { text: line },
                    style,
                    indent_dots,
                    row_dots: size.row,
                    segments,
                });
            }
        }

        Block::Columns {
            columns,
            rows,
            style,
        } => {
            // **A table needs its fixed columns plus one character of fill.**
            //
            // Asking for more than that is what shrank the font: the old code
            // capped to `room / longest-name-in-this-table`, so sizes 6 to 10
            // all landed on the same number and the same setting printed
            // differently on a bill with a longer dish name. A name that does
            // not fit its column wraps now — rule two.
            let style = fit_at_least(*style, least_for(columns), usable, metrics, notes);
            let size = metrics.size(style);
            let width = size.chars_across(usable);
            let widths = fit_columns(columns, width);
            for row in rows {
                for (line, segments) in lay_row(columns, &widths, row) {
                    lines.push(LaidLine {
                        content: LaidContent::Text { text: line },
                        style,
                        indent_dots,
                        row_dots: size.row,
                        segments,
                    });
                }
            }
        }
    }
    Ok(())
}

/// **A logo and the shop's name in one band of rows** — P32.
///
/// The owner's ruling, exactly: the picture takes `image_pct` of the paper on
/// its side, the text takes the rest and is **centred inside its share**, the
/// picture is scaled up to fill its box, and whichever of the two is shorter is
/// centred against the taller.
#[expect(clippy::too_many_arguments, reason = "one band, and every one of these is a position it needs")]
fn lay_band(
    image: &[u8],
    side: Align,
    pct: u8,
    text: &[BandLine],
    usable: u32,
    indent_dots: u32,
    metrics: &Metrics,
    lines: &mut Vec<LaidLine>,
    notes: &mut Vec<Note>,
) -> Result<(), PrintError> {
    let image_width = (usable * u32::from(pct.clamp(10, 60)) / 100).max(1);
    let text_width = usable.saturating_sub(image_width).max(metrics.body().advance);

    // **The header is read, not the picture.** Four bytes of `MB1` say how big
    // it is, which is all a layout needs — decoding the dots is the sink's job
    // and D37's rule that this crate does not do it still stands.
    let Some((source_w, source_h)) = Monochrome::size(image).filter(|(w, _)| *w > 0) else {
        notes.push(Note::LogoUnreadable {
            reason: "the logo file could not be read".to_owned(),
        });
        // D37: a shop with a corrupt logo still gets its bill — with the
        // letterhead across the whole paper, which is what it would have been
        // without a logo at all.
        for line in text {
            lay_block(
                &Block::Text {
                    content: line.content.clone(),
                    style: line.style,
                    align: line.align,
                },
                usable,
                indent_dots,
                metrics,
                lines,
                notes,
            )?;
        }
        return Ok(());
    };
    let image_height = source_h * image_width / source_w;

    // The text, wrapped to its own share and stacked.
    let mut laid: Vec<(String, Style, Align, u32)> = Vec::new();
    let mut text_height = 0;
    for line in text {
        let style = fit_at_least(
            line.style,
            longest_word(&line.content),
            text_width,
            metrics,
            notes,
        );
        let size = metrics.size(style);
        let across = size.chars_across(text_width);
        for wrapped in wrap(&line.content, across) {
            laid.push((wrapped, style, line.align, size.row));
            text_height += size.row;
        }
    }

    let band = image_height.max(text_height);
    let image_top = (band - image_height) / 2;
    let mut pen = (band - text_height) / 2;

    let left_is_image = !matches!(side, Align::Right);
    let image_left = if left_is_image {
        indent_dots
    } else {
        indent_dots + text_width
    };
    let text_left = if left_is_image {
        indent_dots + image_width
    } else {
        indent_dots
    };

    let mut placed = Vec::with_capacity(laid.len());
    for (text, style, align, row) in laid {
        placed.push(BandText {
            text,
            style,
            left: text_left,
            top: pen,
            width: text_width,
            align,
        });
        pen += row;
    }

    lines.push(LaidLine {
        content: LaidContent::Band {
            image: image.to_vec(),
            image_left,
            image_top,
            image_width,
            image_height,
            lines: placed,
        },
        style: Style::NORMAL,
        indent_dots,
        row_dots: band,
        segments: Vec::new(),
    });
    Ok(())
}

/// The fixed columns plus one character of fill — the least a table can be
/// drawn in at all.
fn least_for(columns: &[Column]) -> usize {
    columns
        .iter()
        .map(|column| match column.width {
            Width::Fixed(n) => n,
            Width::Fill => 1,
        })
        .sum()
}

/// **Rule two's last resort, and nothing more.**
///
/// If `needed` characters fit at the size that was asked for, the size is
/// returned untouched — whatever that means for wrapping, because wrapping is
/// what the owner asked for. Only when they do not is the size stepped down the
/// ladder, one rung at a time, until they do; and the reduction is reported so
/// the preview can say so.
///
/// Stepping down the ladder rather than computing `room / needed` is the second
/// half of the fix: a computed cap lands on an arbitrary number that is not one
/// of the ten a shop can choose, so the setting screen and the paper described
/// different sizes.
fn fit_at_least(
    style: Style,
    needed: usize,
    usable: u32,
    metrics: &Metrics,
    notes: &mut Vec<Note>,
) -> Style {
    if needed == 0 || metrics.size(style).chars_across(usable) >= needed {
        return style;
    }
    let asked = style.size;
    for rung in Style::LADDER.iter().rev() {
        if *rung >= asked {
            continue;
        }
        let candidate = Style {
            size: *rung,
            bold: style.bold,
        };
        if metrics.size(candidate).chars_across(usable) >= needed {
            notes.push(Note::ScaleCapped {
                asked,
                used: *rung,
            });
            return candidate;
        }
    }
    // Even the smallest cannot hold it. Return the smallest and let `wrap`
    // break the word — rule one, and losing characters is never the answer.
    if asked > Style::SMALLEST {
        notes.push(Note::ScaleCapped {
            asked,
            used: Style::SMALLEST,
        });
    }
    Style {
        size: Style::SMALLEST,
        bold: style.bold,
    }
}

/// Rule three. **The money wins.**
///
/// A row is `label ............ amount`, and on a narrow roll the two often
/// cannot both fit. Something has to give, and if the choice is left to whoever
/// writes a sink then one day it will be the amount.
///
/// 1. The right side is **never** shortened, wrapped or ellipsised. It is the
///    amount, and a bill whose lines do not add up because a digit was dropped
///    is the worst output this product can produce — requirement 7 of the ten
///    says the printed lines always sum to the printed total.
/// 2. The left side wraps onto continuation lines, with the amount on the
///    first.
/// 3. If the amount alone is wider than the paper at the smallest size this
///    product offers, that is an **error**, not a truncation.
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
            Segment {
                start: 0,
                width: label_box,
                align: Align::Left,
            },
            Segment {
                start: label_box,
                width: right_len,
                align: Align::Right,
            },
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
                vec![Segment {
                    start: 0,
                    width,
                    align: Align::Left,
                }],
            ));
        }
    }
    Ok(out)
}

/// Rule: the columns always add up to the line width, exactly.
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
        // shop can choose Times New Roman. Saying where each column is means a
        // measuring sink lands them on the same edges the printer's own font
        // does.
        let mut segments = Vec::with_capacity(wrapped.len());
        let mut at = 0;
        for (i, cell_lines) in wrapped.iter().enumerate() {
            let w = widths.get(i).copied().unwrap_or(1).max(1);
            let align = columns.get(i).map_or(Align::Left, |c| c.align);
            let text = cell_lines.get(row).map_or("", String::as_str);
            line.push_str(&align_to(text, w, align));
            segments.push(Segment {
                start: at,
                width: w,
                align,
            });
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
    use crate::doc::Block;
    use crate::paper::{Offset, PaperKind};

    fn metrics(kind: PaperKind) -> Metrics {
        let font = std::sync::Arc::new(crate::font::Font::builtin().expect("loads"));
        Metrics::face(Paper::new(kind), font)
    }

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
        assert_eq!(
            amount.start + amount.width,
            32,
            "the amount box is not at the edge"
        );
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
    fn columns_always_add_up_to_the_line_width() {
        for width in [22_usize, 26, 32, 44, 56, 64] {
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
                    "{width} with {} columns does not add up",
                    spec.len()
                );
            }
        }
    }

    #[test]
    fn the_offset_moves_everything_and_clamps() {
        let m = metrics(PaperKind::Mm80);
        let mut doc = Document::new(Paper::new(PaperKind::Mm80));
        doc.line("TOTAL");
        let plain = layout_for(&doc, &m).expect("lays out");

        let mut shifted_doc = doc.clone();
        shifted_doc.paper = Paper::new(PaperKind::Mm80).with_offset(Offset::new(3, 0));
        let shifted = layout_for(&shifted_doc, &m).expect("lays out");
        assert_eq!(plain.lines[0].indent_dots, 0);
        assert!(
            shifted.lines[0].indent_dots >= 24 && shifted.lines[0].indent_dots <= 26,
            "3 mm is 24 dots, snapped to whole characters: {}",
            shifted.lines[0].indent_dots
        );
        assert_eq!(shifted.lines[0].indent_dots % plain.base_advance, 0);
        assert!(!shifted.was_clamped());

        let narrow = metrics(PaperKind::Mm58);
        let mut silly = doc.clone();
        silly.paper = Paper::new(PaperKind::Mm58).with_offset(Offset::new(40, 0));
        let clamped = layout_for(&silly, &narrow).expect("lays out");
        assert!(clamped.was_clamped(), "an absurd offset must be clamped");
        assert!(clamped.lines[0].indent_dots <= narrow.dots() / 4);
    }

    /// **The owner's ruling, as a test.** A long dish name at a big size wraps
    /// onto a second line and the size is NOT touched.
    ///
    /// Before P32 this shrank the whole table, and the shrink landed on the
    /// same number for the top five sizes — so five entries on the settings
    /// dropdown printed identically.
    #[test]
    fn a_long_name_wraps_and_the_size_is_left_alone() {
        let m = metrics(PaperKind::Mm80);
        let columns = vec![
            Column::fill(Align::Left),
            Column::fixed(3, Align::Right),
            Column::fixed(8, Align::Right),
            Column::fixed(9, Align::Right),
        ];
        let mut doc = Document::new(Paper::new(PaperKind::Mm80));
        doc.push(Block::Columns {
            columns,
            rows: vec![vec![
                "Paneer Butter Masala Extra Spicy".to_owned(),
                "2".to_owned(),
                "240.00".to_owned(),
                "504.00".to_owned(),
            ]],
            style: Style {
                size: Style::LADDER[7],
                bold: false,
            },
        });

        let laid = layout_for(&doc, &m).expect("lays out");
        assert!(
            !laid.was_capped(),
            "the size was reduced; the name should have wrapped instead"
        );
        assert_eq!(
            laid.lines[0].style.size,
            Style::LADDER[7],
            "the size a shop chose is the size that prints"
        );
        let lines = laid.text_lines();
        assert!(lines.len() > 1, "the name should have wrapped: {lines:?}");
        assert!(lines[0].ends_with("504.00"), "the amount moved: {:?}", lines[0]);
        // Nothing lost — rule one.
        let all = lines.join(" ");
        for word in "Paneer Butter Masala Extra Spicy".split(' ') {
            assert!(all.contains(word), "{word} was lost");
        }
    }

    /// **Every size on the ladder produces a different line.** The test that
    /// would have caught five dead dropdown entries.
    ///
    /// A plain line of text, deliberately: a table has fixed columns, and on
    /// narrow paper the biggest sizes genuinely cannot draw one — that is rule
    /// two's last resort and it is tested separately. What must never happen
    /// again is two sizes producing the same paper for text that fits.
    #[test]
    fn every_size_changes_what_is_laid_out() {
        let m = metrics(PaperKind::Mm80);
        let mut seen = std::collections::BTreeSet::new();
        for cap in Style::LADDER {
            let mut doc = Document::new(Paper::new(PaperKind::Mm80));
            doc.text("Idli", Style { size: cap, bold: false }, Align::Left);
            let laid = layout_for(&doc, &m).expect("lays out");
            let line = laid.lines.first().expect("a row");
            assert!(!laid.was_capped(), "size {cap} was reduced for four letters");
            assert!(
                seen.insert((line.style.size, line.row_dots)),
                "size {cap} laid out exactly like another size"
            );
        }
        assert_eq!(seen.len(), Style::LADDER.len());
    }

    /// A size is only reduced when the block cannot be drawn at all — a table
    /// whose fixed columns already fill the paper.
    #[test]
    fn a_table_that_cannot_be_drawn_at_all_comes_down_a_rung() {
        let m = metrics(PaperKind::Mm58);
        let mut doc = Document::new(Paper::new(PaperKind::Mm58));
        doc.push(Block::Columns {
            columns: vec![
                Column::fill(Align::Left),
                Column::fixed(6, Align::Left),
                Column::fixed(3, Align::Right),
                Column::fixed(9, Align::Right),
                Column::fixed(10, Align::Right),
            ],
            rows: vec![vec![
                "Dosa".to_owned(),
                "2106".to_owned(),
                "2".to_owned(),
                "240.00".to_owned(),
                "504.00".to_owned(),
            ]],
            style: Style {
                size: Style::LADDER[8],
                bold: false,
            },
        });
        let laid = layout_for(&doc, &m).expect("lays out");
        assert!(laid.was_capped(), "29 fixed columns cannot fit 58 mm at that size");
        // And it came down to a rung a shop could have chosen, not to an
        // arbitrary number.
        assert!(Style::LADDER.contains(&laid.lines[0].style.size));
    }

    /// A separator is one row of a known height, whatever the pattern —
    /// including `Double`, which used to be two rows of characters.
    #[test]
    fn a_rule_is_one_row_and_a_small_one() {
        let m = metrics(PaperKind::Mm80);
        for pattern in [
            Pattern::Solid,
            Pattern::Dashed,
            Pattern::Dotted,
            Pattern::Bold,
            Pattern::Double,
        ] {
            let mut doc = Document::new(Paper::new(PaperKind::Mm80));
            doc.separator(pattern);
            let laid = layout_for(&doc, &m).expect("lays out");
            assert_eq!(laid.lines.len(), 1, "{pattern:?} is more than one row");
            assert!(
                laid.lines[0].row_dots < m.body().row,
                "{pattern:?} costs {} dots, more than a line of text",
                laid.lines[0].row_dots
            );
            let LaidContent::Separator { width, .. } = laid.lines[0].content else {
                panic!("not a separator");
            };
            assert_eq!(width, m.dots(), "a rule spans the paper");
        }
    }

    /// The logo band: 30 % picture, 70 % text, and the shorter of the two
    /// centred against the taller.
    #[test]
    fn a_band_puts_the_logo_beside_the_name() {
        let m = metrics(PaperKind::Mm80);
        let logo = Monochrome::blank(100, 50).encode();
        let mut doc = Document::new(Paper::new(PaperKind::Mm80));
        doc.push(Block::Band {
            image: logo,
            image_side: Align::Left,
            image_pct: 30,
            text: vec![
                BandLine::new("SADGURU BAKERY", Style::new(2, true), Align::Centre),
                BandLine::new("12 MG Road", Style::NORMAL, Align::Centre),
            ],
        });
        let laid = layout_for(&doc, &m).expect("lays out");
        assert_eq!(laid.lines.len(), 1, "a band is one row of the document");
        let LaidContent::Band {
            image_left,
            image_width,
            lines,
            ..
        } = &laid.lines[0].content
        else {
            panic!("not a band");
        };
        assert_eq!(*image_left, 0);
        assert_eq!(*image_width, m.dots() * 30 / 100);
        assert_eq!(lines.len(), 2);
        for line in lines {
            assert_eq!(line.left, *image_width, "the text does not start after the logo");
            assert_eq!(line.width, m.dots() - image_width);
        }
    }

    /// D37 — a logo that will not read is still a bill.
    #[test]
    fn a_band_with_an_unreadable_logo_still_prints_the_name() {
        let m = metrics(PaperKind::Mm80);
        let mut doc = Document::new(Paper::new(PaperKind::Mm80));
        doc.push(Block::Band {
            image: vec![9, 9, 9, 9],
            image_side: Align::Left,
            image_pct: 30,
            text: vec![BandLine::new("SADGURU", Style::new(2, true), Align::Centre)],
        });
        let laid = layout_for(&doc, &m).expect("lays out");
        assert!(laid.text_lines().iter().any(|l| l.contains("SADGURU")));
        assert!(
            laid.notes
                .iter()
                .any(|n| matches!(n, Note::LogoUnreadable { .. }))
        );
    }
}
