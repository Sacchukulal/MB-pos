//! What a printer is, from this crate's point of view.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::drawer::DrawerConfig;
use crate::paper::{Offset, Paper, PaperKind};

/// Where the bytes go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum Target {
    /// The Windows spooler, RAW datatype.
    Spooler {
        name: String,
    },
    Network {
        host: String,
        port: u16,
    },
    Serial {
        port: String,
        baud: u32,
    },
    /// A file. Testing, "print to file", and the only transport every test in this crate uses.
    File {
        path: PathBuf,
    },
    /// Accepts everything and prints nothing.
    None,
}

/// Which sink a job is drawn with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    /// What you see is what you get, and the path Kannada will one day take.
    #[default]
    Raster,
    /// The printer's own font.
    Text,
}

impl Engine {
    /// The word the screen and the log use — `raster` or `text`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Engine::Raster => "raster",
            Engine::Text => "text",
        }
    }
}

/// What this printer is allowed to receive.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub cut: bool,
    pub drawer: bool,
    /// `GS v 0`. A printer without it can only be driven in `Engine::Text`.
    pub raster: bool,
    /// `GS ( k` — the printer's own QR encoder.
    pub native_qr: bool,
    /// `GS k` — the printer's own barcode encoder, which is what makes a bill scannable back
    /// into the till.
    #[serde(default = "yes")]
    pub native_barcode: bool,
}

/// Serde default for a capability added after shops had saved printers.
const fn yes() -> bool {
    true
}

impl Default for Capabilities {
    fn default() -> Self {
        // What a thermal receipt printer sold in the last decade does.
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
            // A file has no blade and no solenoid, and a test that "cut the paper" would only
            // be writing bytes nobody reads.
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
    /// The paper and the offset both live here, so one shop can run two printers with two
    /// different corrections and one document.
    pub paper: Paper,
    pub engine: Engine,
    pub role: Role,
    pub caps: Capabilities,
    pub drawer: DrawerConfig,
    pub bold_dark: bool,
    /// Copies of every job.
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

    /// The engine this printer can actually use.
    #[must_use]
    pub const fn effective_engine(&self) -> Engine {
        match self.engine {
            Engine::Raster if !self.caps.raster => Engine::Text,
            other => other,
        }
    }
}

/// The nudge.
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
