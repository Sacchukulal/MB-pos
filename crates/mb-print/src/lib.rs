//! Magic Bill — printing.
//!
//! **One description of a bill. One traversal. Several sinks.**
//!
//! This crate exists because of the single worst piece of design in v1
//! (audit D1):
//!
//! > *"The same bill is drawn three separate times, by hand, in three places:
//! > the on-screen preview, the 'graphics' printer path, and the 'text' printer
//! > path. The code even carries a note saying 'keep any layout change here in
//! > sync'. Every design change is triple work, and the three **will** drift
//! > apart. This is the single biggest source of 'the preview does not match
//! > the paper'."*
//!
//! The obvious fix — one description, three renderers — is not enough, and v1
//! is the proof: it already had a shared bill object and drifted anyway,
//! because the *drawing* was written three times. So here there is exactly one
//! function that walks a laid-out document ([`render::render`]) and every
//! renderer is a [`render::Sink`] it calls. A sink cannot forget a block; it is
//! handed everything, in order.
//!
//! # The layers
//!
//! | module | owns |
//! |---|---|
//! | [`paper`] | how wide the paper is, and the print offset (scope 7.11) |
//! | [`doc`] | the description. Knows nothing about bills |
//! | [`layout`] | wrapping, column fitting, the font cap, and "the money wins" |
//! | [`render`] | the one traversal, and the `Sink` trait |
//! | [`text`] | the plain-text sink — the printer's own font path |
//! | [`pdf`] | the A4 / PDF sink (scope 7.10), with no crate |
//! | [`settings`] | every receipt toggle v1 had (audit Part 3) |
//! | [`template`] | the only place that knows what a bill looks like |
//!
//! # What is deliberately not here
//!
//! * **The raster sink is P07's.** It needs a rasterisable font, there is none
//!   in this repository, and embedding one is a dependency, a licence decision
//!   and several hundred kilobytes — and the right size and threshold depend on
//!   the printer's dots-per-mm and its raster command, which are P07's to
//!   decide. Adding it should change nothing outside its own file; see
//!   [`render`] for what it inherits.
//! * **Drivers, the queue, the cash drawer, ESC/POS bytes** — all P07. This
//!   crate produces bytes and sends nothing anywhere.
//! * **The database.** Settings arrive as a struct.
//!
//! # The rules this crate obeys
//!
//! * **R2 / D2 — amounts on paper are exactly `Money::to_plain_string`.** No
//!   re-formatting, nowhere. A renderer that formats a number has just become a
//!   second money path.
//! * **R3 — nothing silently truncates.** This is the one crate where that rule
//!   meets a hard physical limit, so [`layout`] says precisely what happens
//!   instead, and it is never "the amount disappears".
//! * **Crown jewel 18 — font sizes are capped so text cannot overflow.**
//! * **Scope 17.11 — a receipt raster never goes over the wire.** A 576 × 1800
//!   bitmap is about 130 KB; thirteen bills would be the whole 10 MB monthly
//!   egress budget. When P07 builds the raster sink, the bitmap lives between
//!   the layout and the printer and nowhere else.

#![deny(missing_debug_implementations)]

pub mod doc;
pub mod error;
pub mod layout;
pub mod paper;
pub mod pdf;
pub mod render;
pub mod settings;
pub mod template;
pub mod text;

pub use doc::{Align, Block, Column, Document, FontFamily, Pattern, Style, Width};
pub use error::PrintError;
pub use layout::{Laid, LaidContent, LaidLine, Note, layout};
pub use paper::{Offset, Paper, PaperKind};
pub use pdf::to_pdf;
pub use render::{Call, Recorder, Sink, render};
pub use settings::{KitchenSettings, ReceiptSettings};
pub use template::{BillContext, Copy, KitchenContext, bill_document, kitchen_document};
pub use text::to_text;
