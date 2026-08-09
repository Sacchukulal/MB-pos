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
//! | [`raster`] | the dot sink — crown jewel 17 (P07) |
//! | [`font`] | the face those dots are drawn from, and what P23 inherits |
//! | [`image`] | the one-bit logo format (P07, D37) |
//! | [`settings`] | every receipt toggle v1 had (audit Part 3) |
//! | [`template`] | the only place that knows what a bill looks like |
//! | [`testprint`] | the test print, and the offset made adjustable (7.11) |
//!
//! And, added at P07, the half that talks to hardware:
//!
//! | module | owns |
//! |---|---|
//! | [`escpos`] | the typed command layer — every command named from the spec |
//! | [`printer`] | what a printer is: paper, offset, engine, role, capabilities |
//! | [`transport`] | spooler, network, serial, file (scopes 7.1–7.3) |
//! | [`queue`] | the durable, parallel, retrying print queue (audit D3 and D4) |
//! | [`drawer`] | when the cash drawer may open, and when it may not (7.4) |
//! | [`routing`] | which printer gets which items (scopes 3.1 and 1.8) |
//!
//! # What is deliberately not here
//!
//! * **No shaping.** [`layout`] wraps by counting characters, which is right for
//!   Latin and wrong for Kannada. [`font`] says exactly what P23 will need and
//!   names the one seam it will have to open.
//! * **No screens.** The queue's indicator, the printer settings and the preview
//!   are P08 and P17; this crate ships the events and the data they need.
//! * **No image decoding, no QR encoder, no async runtime.** See [`image`]
//!   (D37), [`escpos::EscPos::qr`] (D36) and [`queue`] respectively.
//! * **The database — almost.** The layout, the templates and the sinks still
//!   take structs and return bytes, and always will. Exactly one module,
//!   [`queue::sqlite`], writes a row: a print queue that cannot survive a power
//!   cut is not a queue, and that is audit D4 (decision D32).
//! * **`unsafe`.** This crate forbids it. The two transports that need the
//!   operating system go through `mb-winprint`, which is the only crate in the
//!   workspace allowed to say the word (decision D34).
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
//!   egress budget. The bitmap [`raster`] produces lives between the layout and
//!   the printer and nowhere else — not in the outbox, not in a backup, not in
//!   a log. Nor does a print job (D16): [`queue::sqlite`] is the one repository
//!   in the product that deliberately raises no outbox row.

#![deny(missing_debug_implementations)]

pub mod doc;
pub mod drawer;
pub mod error;
pub mod escpos;
pub mod font;
pub mod image;
pub mod layout;
pub mod paper;
pub mod pdf;
pub mod printer;
pub mod queue;
pub mod raster;
pub mod render;
pub mod routing;
pub mod settings;
pub mod template;
pub mod testprint;
pub mod text;
pub mod transport;

pub use doc::{Align, Block, Column, Document, Pattern, Style, Width};
pub use drawer::{DrawerConfig, DrawerPin, should_kick};
pub use error::PrintError;
pub use escpos::{EscPos, JobOptions, encode_raster, encode_text};
pub use font::Font;
pub use image::Monochrome;
pub use layout::{Laid, LaidContent, LaidLine, Note, layout};
pub use paper::{Offset, Paper, PaperKind};
pub use pdf::to_pdf;
pub use printer::{Capabilities, Engine, PrinterConfig, Role, Target, nudge};
pub use queue::{Job, JobKind, JobState, JobStatus, Queue, QueueConfig, QueueEvent};
pub use raster::{Raster, to_raster};
pub use render::{Call, Recorder, Sink, render};
pub use routing::{PrinterMode, RoutingTable, TicketStyle, route};
pub use settings::{KitchenSettings, ReceiptSettings};
pub use template::{BillContext, Copy, KitchenContext, bill_document, kitchen_document};
pub use testprint::test_document;
pub use text::to_text;
pub use transport::{Transport, TransportError};
