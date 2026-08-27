//! Magic Bill — printing.

#![deny(missing_debug_implementations)]

pub mod doc;
pub mod drawer;
pub mod error;
pub mod escpos;
pub mod font;
pub mod image;
pub mod layout;
pub mod metrics;
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
pub use metrics::{Metrics, SizeMetrics};
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
