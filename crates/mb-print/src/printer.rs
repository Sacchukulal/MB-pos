//! What a printer *is*, from this crate's point of view.
//!
//! Everything the queue needs to turn a document into bytes and put them
//! somewhere, in one struct, with no database anywhere near it. P17 edits these
//! on a screen, mb-db stores them, and `queue::sqlite` is the only module that
//! knows both.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::drawer::DrawerConfig;
use crate::paper::{Offset, Paper, PaperKind};

/// Where the bytes go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum Target {
    /// The Windows spooler, RAW datatype. Scope 7.1 — what almost every shop
    /// runs today.
    Spooler { name: String },
    /// Raw TCP, port 9100. Scope 7.2, and the biggest single hardware gap in
    /// v1: a kitchen printer on the shop's own switch, with no PC in front of
    /// it.
    Network { host: String, port: u16 },
    /// A COM port. Scope 7.3.
    Serial { port: String, baud: u32 },
    /// A file. Testing, "print to file", and the only transport every test in
    /// this crate uses — which is how P07 stays provable with no hardware.
    File { path: PathBuf },
    /// Accepts everything and prints nothing.
    ///
    /// **Not a placeholder.** A shop whose printer has not arrived yet must
    /// still be able to bill (requirement 3 of the ten), and a queue with
    /// nowhere to send is a queue that has to grow a special case.
    None,
}

/// Which sink a job is drawn with — v1's *"Print engine: Graphics or Text"*
/// (audit Part 3), kept because a shop that has chosen one should not have it
/// chosen again for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    /// Crown jewel 17. What you see is what you get, and the path Kannada will
    /// one day take.
    #[default]
    Raster,
    /// The printer's own font. Faster, smaller, and the only thing that works
    /// on a printer with no raster support.
    Text,
}

/// What this printer is allowed to receive.
///
/// Routing that ignores this is how a customer's bill ends up in the tandoor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Bill,
    Kitchen,
    #[default]
    Both,
}

impl Role {
    #[must_use]
    pub const fn accepts_bill(self) -> bool {
        matches!(self, Role::Bill | Role::Both)
    }

    #[must_use]
    pub const fn accepts_kitchen(self) -> bool {
        matches!(self, Role::Kitchen | Role::Both)
    }
}

/// What the hardware can do.
///
/// **These belong to the printer, not to the transport.** A TCP socket to port
/// 9100 knows nothing about whether there is a blade or a drawer on the other
/// end of it, and two printers reached the same way disagree about both. They
/// are defaulted from the target and then the shop corrects them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub cut: bool,
    pub drawer: bool,
    /// `GS v 0`. A printer without it can only be driven in [`Engine::Text`].
    pub raster: bool,
    /// `GS ( k` — the printer's own QR encoder (D36).
    pub native_qr: bool,
    /// P29. `GS k` — the printer's own barcode encoder, which is what makes a
    /// bill scannable back into the till. Same argument as the QR: the
    /// printer draws it far better than a raster of ours would.
    #[serde(default = "yes")]
    pub native_barcode: bool,
}

/// Serde default for a capability added after shops had saved printers.
const fn yes() -> bool {
    true
}

impl Default for Capabilities {
    fn default() -> Self {
        // What a thermal receipt printer sold in the last decade does. A shop
        // whose printer is older turns one off and the queue stops sending it.
        Capabilities {
            cut: true,
            drawer: true,
            raster: true,
            native_qr: true,
            native_barcode: true,
        }
    }
}

impl Capabilities {
    /// Sensible defaults for a target, before the shop has said anything.
    #[must_use]
    pub fn for_target(target: &Target) -> Capabilities {
        match target {
            // A file has no blade and no solenoid, and a test that "cut the
            // paper" would only be writing bytes nobody reads.
            Target::File { .. } | Target::None => Capabilities {
                cut: false,
                drawer: false,
                raster: true,
                native_qr: false,
                native_barcode: false,
            },
            Target::Spooler { .. } | Target::Network { .. } | Target::Serial { .. } => {
                Capabilities::default()
            }
        }
    }
}

/// One configured printer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrinterConfig {
    pub id: String,
    pub name: String,
    pub target: Target,
    /// **The paper and the offset both live here** (scope 7.11), so one shop
    /// can run two printers with two different corrections and one document.
    /// The queue overrides the document's paper from this before it lays out.
    pub paper: Paper,
    pub engine: Engine,
    pub role: Role,
    pub caps: Capabilities,
    pub drawer: DrawerConfig,
    /// v1's "Bold & Dark" print option: emphasis and double-strike on
    /// everything, for a printer whose head or paper has gone pale.
    pub bold_dark: bool,
    /// Copies of every job. Some shops want two bill copies, one for the file.
    pub copies: u8,
    pub is_default: bool,
}

impl PrinterConfig {
    /// A printer with everything defaulted, for a target.
    #[must_use]
    pub fn new(id: impl Into<String>, name: impl Into<String>, target: Target) -> PrinterConfig {
        let caps = Capabilities::for_target(&target);
        PrinterConfig {
            id: id.into(),
            name: name.into(),
            target,
            paper: Paper::new(PaperKind::Mm80),
            engine: Engine::Raster,
            role: Role::Both,
            caps,
            drawer: DrawerConfig::default(),
            bold_dark: false,
            copies: 1,
            is_default: false,
        }
    }

    #[must_use]
    pub const fn with_paper(mut self, kind: PaperKind) -> PrinterConfig {
        self.paper = Paper {
            kind,
            offset: self.paper.offset,
        };
        self
    }

    #[must_use]
    pub const fn with_offset(mut self, offset: Offset) -> PrinterConfig {
        self.paper.offset = offset;
        self
    }

    #[must_use]
    pub const fn with_engine(mut self, engine: Engine) -> PrinterConfig {
        self.engine = engine;
        self
    }

    #[must_use]
    pub const fn with_role(mut self, role: Role) -> PrinterConfig {
        self.role = role;
        self
    }

    /// The engine this printer can **actually** use.
    ///
    /// A printer set to raster that cannot raster prints as text (item 5's
    /// fallback), and the job records that it happened — a shop whose receipt
    /// suddenly looks different gets an answer on the screen instead of making
    /// a support call.
    #[must_use]
    pub const fn effective_engine(&self) -> Engine {
        match self.engine {
            Engine::Raster if !self.caps.raster => Engine::Text,
            other => other,
        }
    }
}

/// Scope 7.11 — the nudge.
///
/// Print, look at the paper, nudge, print again. The value belongs to the
/// printer and is saved with it, because a printer that needs +2 mm today needs
/// +2 mm forever.
///
/// Clamped to ±20 mm here rather than by the layout: the layout clamps to what
/// fits on the paper, and this clamps to what could ever be a real correction.
/// Both exist so a slip of a finger is not a bill printed sideways.
pub fn nudge(printer: &mut PrinterConfig, dx_mm: i32, dy_mm: i32) {
    printer.paper.offset = Offset {
        x_mm: (printer.paper.offset.x_mm + dx_mm).clamp(-20, 20),
        y_mm: (printer.paper.offset.y_mm + dy_mm).clamp(-20, 20),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_target_has_no_blade() {
        let caps = Capabilities::for_target(&Target::File {
            path: PathBuf::from("out.bin"),
        });
        assert!(!caps.cut);
        assert!(!caps.drawer);
    }

    #[test]
    fn a_printer_that_cannot_raster_prints_as_text() {
        let mut printer = PrinterConfig::new("p1", "Counter", Target::None);
        printer.engine = Engine::Raster;
        printer.caps.raster = false;
        assert_eq!(printer.effective_engine(), Engine::Text);
    }

    #[test]
    fn a_printer_set_to_text_is_never_upgraded() {
        let mut printer = PrinterConfig::new("p1", "Counter", Target::None);
        printer.engine = Engine::Text;
        printer.caps.raster = true;
        assert_eq!(printer.effective_engine(), Engine::Text);
    }

    #[test]
    fn roles_decide_who_may_receive_what() {
        assert!(Role::Bill.accepts_bill());
        assert!(!Role::Bill.accepts_kitchen());
        assert!(Role::Kitchen.accepts_kitchen());
        assert!(!Role::Kitchen.accepts_bill());
        assert!(Role::Both.accepts_bill() && Role::Both.accepts_kitchen());
    }

    #[test]
    fn nudging_accumulates_and_clamps() {
        let mut printer = PrinterConfig::new("p1", "Counter", Target::None);
        nudge(&mut printer, 2, 0);
        nudge(&mut printer, 1, -1);
        assert_eq!(printer.paper.offset, Offset::new(3, -1));
        nudge(&mut printer, 100, 100);
        assert_eq!(printer.paper.offset, Offset::new(20, 20));
    }
}
