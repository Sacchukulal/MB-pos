//! ESC/POS — **a typed builder, not raw byte soup.**
//!
//! Every command here is re-derived from the ESC/POS specification and carries
//! the spec's own name for it in a comment. v1's byte sequences worked on the
//! owner's TVSE printer and said nothing about what they were, so nobody could
//! change one without a printer on the desk. R1 allows reading them; it does
//! not allow pasting them, and it certainly does not allow inheriting their
//! silence.
//!
//! # The two encoders
//!
//! * [`encode_text`] drives the printer's own font — fast, tiny, and the only
//!   thing that works on a printer with no raster support;
//! * [`encode_raster`] sends dots — crown jewel 17, what-you-see-is-what-you-get,
//!   and the path an Indic script will one day take.
//!
//! Both take the same [`Capabilities`], and **both obey them**: a printer with
//! no blade is never sent a cut, a printer with no drawer socket is never sent
//! a pulse, and a printer with no QR encoder gets the payload as text.

use crate::doc::Align;
use crate::drawer::{DrawerConfig, DrawerPin};
use crate::layout::{Laid, LaidContent};
use crate::printer::Capabilities;
use crate::raster::{Band, Raster};

// --- the bytes, named -------------------------------------------------------

const ESC: u8 = 0x1B;
const GS: u8 = 0x1D;

/// How the paper is cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cut {
    /// Right through. Some printers only have this.
    Full,
    /// Leaves a small tab so the receipt does not fall on the floor. What a
    /// counter wants.
    #[default]
    Partial,
    /// Feed `dots` first, so the cut lands below the last line rather than
    /// through it — the blade sits some millimetres above the head.
    PartialAfterFeed(u8),
}

/// A stream of printer commands.
#[derive(Debug, Clone, Default)]
pub struct EscPos {
    out: Vec<u8>,
}

impl EscPos {
    #[must_use]
    pub fn new() -> EscPos {
        EscPos { out: Vec::new() }
    }

    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.out
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.out
    }

    /// `ESC @` — initialise. Clears every mode the last job left set, which
    /// matters because the printer remembers them and the last job may have
    /// been somebody else's.
    pub fn init(&mut self) -> &mut Self {
        self.out.extend_from_slice(&[ESC, b'@']);
        self
    }

    /// `ESC t n` — select character code table. 0 is PC437, which every printer
    /// has and which covers the ASCII a receipt is written in.
    pub fn codepage(&mut self, table: u8) -> &mut Self {
        self.out.extend_from_slice(&[ESC, b't', table]);
        self
    }

    /// `ESC a n` — justification. 0 left, 1 centre, 2 right.
    ///
    /// The layout has already padded every line to its alignment, so the text
    /// encoder leaves this at 0 and lets the spaces do the work — otherwise the
    /// printer would centre an already-centred line. It is used for pictures,
    /// which have no spaces to pad with.
    pub fn align(&mut self, align: Align) -> &mut Self {
        let n = match align {
            Align::Left => 0,
            Align::Centre => 1,
            Align::Right => 2,
        };
        self.out.extend_from_slice(&[ESC, b'a', n]);
        self
    }

    /// `ESC E n` — emphasised (bold) on or off.
    pub fn emphasis(&mut self, on: bool) -> &mut Self {
        self.out.extend_from_slice(&[ESC, b'E', u8::from(on)]);
        self
    }

    /// `ESC G n` — double-strike: the same line printed twice.
    ///
    /// This is v1's "Bold & Dark" option, and it is the honest way to do it. A
    /// vendor density command would be one printer's idea; striking twice is in
    /// the specification and works everywhere.
    pub fn double_strike(&mut self, on: bool) -> &mut Self {
        self.out.extend_from_slice(&[ESC, b'G', u8::from(on)]);
        self
    }

    /// `GS ! n` — character size. The low nibble is the height multiplier minus
    /// one, the high nibble the width multiplier minus one, so 1× is `0x00` and
    /// 3× is `0x22`.
    pub fn size(&mut self, scale: u8) -> &mut Self {
        let n = scale.clamp(1, 8) - 1;
        self.out.extend_from_slice(&[GS, b'!', (n << 4) | n]);
        self
    }

    /// `ESC 3 n` — line spacing in dots.
    pub fn line_spacing(&mut self, dots: u8) -> &mut Self {
        self.out.extend_from_slice(&[ESC, b'3', dots]);
        self
    }

    /// `ESC 2` — back to the printer's default line spacing.
    pub fn default_line_spacing(&mut self) -> &mut Self {
        self.out.extend_from_slice(&[ESC, b'2']);
        self
    }

    /// `ESC d n` — feed `n` lines.
    pub fn feed_lines(&mut self, lines: u8) -> &mut Self {
        self.out.extend_from_slice(&[ESC, b'd', lines]);
        self
    }

    /// `ESC J n` — feed `n` dots.
    pub fn feed_dots(&mut self, dots: u8) -> &mut Self {
        self.out.extend_from_slice(&[ESC, b'J', dots]);
        self
    }

    /// `GS V m` — cut. 0 full, 1 partial; `GS V 66 n` feeds `n` dots and then
    /// cuts partially, which is the one a counter actually wants.
    pub fn cut(&mut self, cut: Cut) -> &mut Self {
        match cut {
            Cut::Full => self.out.extend_from_slice(&[GS, b'V', 0]),
            Cut::Partial => self.out.extend_from_slice(&[GS, b'V', 1]),
            Cut::PartialAfterFeed(dots) => self.out.extend_from_slice(&[GS, b'V', 66, dots]),
        }
        self
    }

    /// `ESC p m t1 t2` — pulse the drawer kick-out connector.
    ///
    /// `m` is 0 for pin 2 and 1 for pin 5; `t1` and `t2` are the on and off
    /// times in units of **two milliseconds**.
    pub fn drawer(&mut self, pin: DrawerPin, on_units: u8, off_units: u8) -> &mut Self {
        let m = match pin {
            DrawerPin::Pin2 => 0,
            DrawerPin::Pin5 => 1,
        };
        self.out.extend_from_slice(&[ESC, b'p', m, on_units, off_units]);
        self
    }

    /// Text, folded to the printer's character set.
    ///
    /// Anything outside ASCII becomes `?`. That is not a truncation — the
    /// character is still one character wide and the columns still add up — it
    /// is the printer's ROM font not having the glyph, which is precisely the
    /// limitation [`crate::raster`] exists to lift.
    pub fn text(&mut self, text: &str) -> &mut Self {
        for ch in text.chars() {
            let byte = if ch.is_ascii() { ch as u8 } else { b'?' };
            self.out.push(byte);
        }
        self
    }

    /// A line of text and a line feed.
    pub fn line(&mut self, text: &str) -> &mut Self {
        self.text(text);
        self.out.push(b'\n');
        self
    }

    /// `GS v 0 m xL xH yL yH d…` — raster bit image.
    ///
    /// `m = 0` is normal size; the width is in **bytes** and the height in dot
    /// rows, both little-endian. This is the command crown jewel 17 rides on.
    pub fn raster_image(&mut self, width_dots: u32, height_dots: u32, bits: &[u8]) -> &mut Self {
        let bytes_per_row = u16::try_from((width_dots as usize).div_ceil(8)).unwrap_or(u16::MAX);
        let rows = u16::try_from(height_dots).unwrap_or(u16::MAX);
        self.out.extend_from_slice(&[GS, b'v', b'0', 0]);
        self.out.extend_from_slice(&bytes_per_row.to_le_bytes());
        self.out.extend_from_slice(&rows.to_le_bytes());
        self.out.extend_from_slice(bits);
        self
    }

    /// `GS ( k` — the printer's own QR encoder (D36), in the five calls the
    /// specification requires:
    ///
    /// 1. `fn 165` select the model — 2, which every phone reads;
    /// 2. `fn 167` module size in dots;
    /// 3. `fn 169` error correction — M (15 %), the usual compromise between
    ///    size and a receipt that has been in somebody's pocket;
    /// 4. `fn 180` store the data;
    /// 5. `fn 181` print what was stored.
    ///
    /// Not adding a QR encoder to this product is worth roughly 50 KB and a
    /// dependency, and the printer's own square is better than ours would be.
    pub fn qr(&mut self, payload: &str, module: u8) -> &mut Self {
        // 1. GS ( k pL pH cn fn n1 n2 — model 2.
        self.out
            .extend_from_slice(&[GS, b'(', b'k', 4, 0, 49, 65, 50, 0]);
        // 2. module size, 1–16 dots.
        self.out
            .extend_from_slice(&[GS, b'(', b'k', 3, 0, 49, 67, module.clamp(1, 16)]);
        // 3. error correction level: 48 L, 49 M, 50 Q, 51 H.
        self.out
            .extend_from_slice(&[GS, b'(', b'k', 3, 0, 49, 69, 49]);
        // 4. store the data. pL/pH count the data plus the three bytes of
        //    cn/fn/m that follow them.
        let data = payload.as_bytes();
        // pL/pH count the payload plus the three bytes of cn, fn and m that
        // follow them — little-endian, like every other length in ESC/POS.
        let length = u16::try_from(data.len().saturating_add(3)).unwrap_or(u16::MAX);
        let [pl, ph] = length.to_le_bytes();
        self.out
            .extend_from_slice(&[GS, b'(', b'k', pl, ph, 49, 80, 48]);
        self.out.extend_from_slice(data);
        // 5. print it.
        self.out
            .extend_from_slice(&[GS, b'(', b'k', 3, 0, 49, 81, 48]);
        self
    }

    /// **CODE 128, P29.** Every scanner sold reads it, and unlike EAN it takes
    /// letters — which a bill number has in it.
    ///
    /// `GS k 73 n d1..dn`, the length-prefixed form. The older
    /// NUL-terminated form (`GS k 6`) cannot carry a NUL and is refused by
    /// several current printers, so this uses the one that is universal on
    /// anything made this decade.
    pub fn barcode(&mut self, payload: &str, human_readable: bool, height: u8) -> &mut Self {
        // How tall, in dots. 60 is about 8 mm — readable, and not half the
        // receipt.
        self.out.extend_from_slice(&[GS, b'h', height.clamp(1, 255)]);
        // Narrow bar width, 2 dots. Wider is easier to scan and eats paper.
        self.out.extend_from_slice(&[GS, b'w', 2]);
        // Where the characters go: 0 none, 2 below.
        self.out
            .extend_from_slice(&[GS, b'H', if human_readable { 2 } else { 0 }]);
        // Code set B, which covers the printable ASCII a bill number uses.
        let data = payload.as_bytes();
        let length = u8::try_from(data.len().saturating_add(2)).unwrap_or(u8::MAX);
        self.out.extend_from_slice(&[GS, b'k', 73, length, b'{', b'B']);
        self.out.extend_from_slice(data);
        self
    }
}

/// What a finished job needs beyond its document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobOptions {
    pub cut: bool,
    /// Blank lines fed before the cut, so the receipt clears the blade and the
    /// customer can take it without touching the mechanism.
    pub feed_lines: u8,
    /// `Some` when this job should also open the drawer. [`crate::drawer`]
    /// decides; this only carries the answer.
    pub drawer: Option<DrawerConfig>,
    /// v1's "Bold & Dark".
    pub bold_dark: bool,
}

impl Default for JobOptions {
    fn default() -> Self {
        JobOptions {
            cut: true,
            feed_lines: 3,
            drawer: None,
            bold_dark: false,
        }
    }
}

/// The printer's own font path.
#[must_use]
pub fn encode_text(laid: &Laid, caps: &Capabilities, options: &JobOptions) -> Vec<u8> {
    let mut out = EscPos::new();
    out.init().codepage(0);
    if options.bold_dark {
        out.double_strike(true);
    }
    // Left, always: the layout has already padded each line to its alignment,
    // and asking the printer to centre an already-centred line centres the
    // padding too.
    out.align(Align::Left);

    let mut scale = 1_u8;
    let mut bold = false;
    out.size(1).emphasis(false);

    // The layout counts in dots (P32); this engine prints with the printer's
    // own fixed font, so it divides back — exactly, because
    // `Metrics::printer_font` only ever answers in whole multiples of it.
    let advance = laid.base_advance.max(1);
    let columns_of = |dots: u32| -> usize {
        #[expect(
            clippy::integer_division,
            reason = "dots back into whole characters, not money"
        )]
        {
            (dots / advance) as usize
        }
    };

    for line in &laid.lines {
        match &line.content {
            LaidContent::Text { text } => {
                if line.style.scale() != scale {
                    scale = line.style.scale();
                    out.size(scale);
                }
                if line.style.bold != bold {
                    bold = line.style.bold;
                    out.emphasis(bold);
                }
                let indent = " ".repeat(columns_of(line.indent_dots));
                out.line(&format!("{indent}{}", text.trim_end()));
            }
            LaidContent::Separator { pattern, width } => {
                if scale != 1 {
                    scale = 1;
                    out.size(1);
                }
                if bold {
                    bold = false;
                    out.emphasis(false);
                }
                let indent = " ".repeat(columns_of(line.indent_dots));
                let across = columns_of(*width).max(1);
                let body: String = std::iter::repeat_n(pattern.glyph(), across).collect();
                out.line(&format!("{indent}{body}"));
            }
            // **The letterhead's picture cannot be drawn by the printer's own
            // font; its text still must be.** Dropping the whole band would
            // lose the shop's name on every text-engine printer.
            LaidContent::Band { lines, .. } => {
                if scale != 1 {
                    scale = 1;
                    out.size(1);
                }
                for text in lines {
                    if text.style.scale() != scale {
                        scale = text.style.scale();
                        out.size(scale);
                    }
                    if text.style.bold != bold {
                        bold = text.style.bold;
                        out.emphasis(bold);
                    }
                    out.align(text.align).line(text.text.trim()).align(Align::Left);
                }
            }
            LaidContent::QrCode { payload, align, .. } => {
                if caps.native_qr {
                    out.align(*align).qr(payload, 6).align(Align::Left);
                } else {
                    // The same choice the text sink makes: a URI a customer can
                    // read and type beats a blank space.
                    out.line(payload);
                }
            }
            LaidContent::Barcode {
                payload,
                human_readable,
                align,
            } => {
                if caps.native_barcode {
                    out.align(*align)
                        .barcode(payload, *human_readable, 60)
                        .line("")
                        .align(Align::Left);
                } else {
                    // The same choice the QR arm makes above: characters a
                    // person can read beat a blank space.
                    out.line(payload);
                }
            }
            LaidContent::Image { .. } => {
                // A printer's own font path cannot draw a logo. An empty arm is
                // the visible decision the `Sink` trait exists to force.
            }
            LaidContent::Blank => {
                out.line("");
            }
        }
    }

    finish_job(&mut out, caps, options);
    out.finish()
}

/// Crown jewel 17's path: dots.
#[must_use]
pub fn encode_raster(raster: &Raster, caps: &Capabilities, options: &JobOptions) -> Vec<u8> {
    let mut out = EscPos::new();
    out.init();
    if options.bold_dark {
        out.double_strike(true);
    }
    // Zero line spacing between bands: the bands are already exactly as tall as
    // the picture, and the printer's default 30-dot spacing would insert a gap
    // between every slice of the same receipt.
    out.line_spacing(0).align(Align::Left);

    for band in &raster.bands {
        match band {
            Band::Ink { image } => {
                out.raster_image(image.width, image.height, &image.bits);
            }
            Band::Qr {
                payload,
                module,
                align,
            } => {
                if caps.native_qr {
                    out.align(*align).qr(payload, *module).align(Align::Left);
                } else {
                    out.line(payload);
                }
            }
        }
    }

    out.default_line_spacing();
    finish_job(&mut out, caps, options);
    out.finish()
}

/// The end of every job: the drawer, the feed and the cut, each only if this
/// printer can do it.
fn finish_job(out: &mut EscPos, caps: &Capabilities, options: &JobOptions) {
    if let Some(drawer) = options.drawer
        && caps.drawer
    {
        // Before the feed and the cut, so the drawer opens as the receipt comes
        // out rather than after the cashier has already torn it off.
        out.drawer(drawer.pin, drawer.on_units(), drawer.off_units());
    }
    if options.bold_dark {
        out.double_strike(false);
    }
    if options.feed_lines > 0 {
        out.feed_lines(options.feed_lines);
    }
    if options.cut && caps.cut {
        out.cut(Cut::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T2. Every command, against the bytes the specification gives.
    ///
    /// Written out literally rather than computed, because a test that builds
    /// the expected bytes the same way the code does proves only that the code
    /// is consistent with itself.
    /// A command, how to emit it, and the bytes the specification gives.
    type Case = (&'static str, Box<dyn Fn(&mut EscPos)>, Vec<u8>);

    #[test]
    fn t2_every_command_matches_the_spec() {
        let cases: Vec<Case> = vec![
            (
                "ESC @ initialise",
                Box::new(|e: &mut EscPos| {
                    e.init();
                }),
                vec![0x1B, 0x40],
            ),
            (
                "ESC t 0 code table PC437",
                Box::new(|e: &mut EscPos| {
                    e.codepage(0);
                }),
                vec![0x1B, 0x74, 0x00],
            ),
            (
                "ESC a 1 centre",
                Box::new(|e: &mut EscPos| {
                    e.align(Align::Centre);
                }),
                vec![0x1B, 0x61, 0x01],
            ),
            (
                "ESC a 2 right",
                Box::new(|e: &mut EscPos| {
                    e.align(Align::Right);
                }),
                vec![0x1B, 0x61, 0x02],
            ),
            (
                "ESC E 1 emphasis on",
                Box::new(|e: &mut EscPos| {
                    e.emphasis(true);
                }),
                vec![0x1B, 0x45, 0x01],
            ),
            (
                "ESC G 1 double strike on",
                Box::new(|e: &mut EscPos| {
                    e.double_strike(true);
                }),
                vec![0x1B, 0x47, 0x01],
            ),
            (
                "GS ! 0x00 single size",
                Box::new(|e: &mut EscPos| {
                    e.size(1);
                }),
                vec![0x1D, 0x21, 0x00],
            ),
            (
                "GS ! 0x11 double size",
                Box::new(|e: &mut EscPos| {
                    e.size(2);
                }),
                vec![0x1D, 0x21, 0x11],
            ),
            (
                "GS ! 0x22 treble size",
                Box::new(|e: &mut EscPos| {
                    e.size(3);
                }),
                vec![0x1D, 0x21, 0x22],
            ),
            (
                "ESC 3 24 line spacing",
                Box::new(|e: &mut EscPos| {
                    e.line_spacing(24);
                }),
                vec![0x1B, 0x33, 24],
            ),
            (
                "ESC 2 default line spacing",
                Box::new(|e: &mut EscPos| {
                    e.default_line_spacing();
                }),
                vec![0x1B, 0x32],
            ),
            (
                "ESC d 3 feed three lines",
                Box::new(|e: &mut EscPos| {
                    e.feed_lines(3);
                }),
                vec![0x1B, 0x64, 3],
            ),
            (
                "ESC J 40 feed forty dots",
                Box::new(|e: &mut EscPos| {
                    e.feed_dots(40);
                }),
                vec![0x1B, 0x4A, 40],
            ),
            (
                "GS V 0 full cut",
                Box::new(|e: &mut EscPos| {
                    e.cut(Cut::Full);
                }),
                vec![0x1D, 0x56, 0x00],
            ),
            (
                "GS V 1 partial cut",
                Box::new(|e: &mut EscPos| {
                    e.cut(Cut::Partial);
                }),
                vec![0x1D, 0x56, 0x01],
            ),
            (
                "GS V 66 n feed and partial cut",
                Box::new(|e: &mut EscPos| {
                    e.cut(Cut::PartialAfterFeed(60));
                }),
                vec![0x1D, 0x56, 66, 60],
            ),
            (
                "ESC p 0 25 25 drawer pin 2",
                Box::new(|e: &mut EscPos| {
                    e.drawer(DrawerPin::Pin2, 25, 25);
                }),
                vec![0x1B, 0x70, 0x00, 25, 25],
            ),
            (
                "ESC p 1 25 25 drawer pin 5",
                Box::new(|e: &mut EscPos| {
                    e.drawer(DrawerPin::Pin5, 25, 25);
                }),
                vec![0x1B, 0x70, 0x01, 25, 25],
            ),
        ];

        for (name, build, expected) in cases {
            let mut out = EscPos::new();
            build(&mut out);
            assert_eq!(out.finish(), expected, "{name} does not match the spec");
        }
    }

    #[test]
    fn t2_the_raster_header_carries_bytes_and_rows() {
        let mut out = EscPos::new();
        // 80 mm paper: 576 dots across is 72 bytes; 200 rows tall.
        out.raster_image(576, 200, &[0xFF; 72 * 200]);
        let bytes = out.finish();
        assert_eq!(
            bytes.get(0..8),
            Some([0x1D, 0x76, 0x30, 0x00, 72, 0, 200, 0].as_slice()),
            "GS v 0 header is wrong — width is in BYTES and height in ROWS, \
             both little-endian"
        );
        assert_eq!(bytes.len(), 8 + 72 * 200);
    }

    #[test]
    fn t2_the_qr_is_five_commands_in_the_right_order() {
        let mut out = EscPos::new();
        out.qr("upi://pay?pa=anna@upi", 6);
        let bytes = out.finish();

        // 1. model 2
        assert_eq!(
            bytes.get(0..9),
            Some([0x1D, 0x28, 0x6B, 4, 0, 49, 65, 50, 0].as_slice())
        );
        // 2. module size
        assert_eq!(
            bytes.get(9..17),
            Some([0x1D, 0x28, 0x6B, 3, 0, 49, 67, 6].as_slice())
        );
        // 3. error correction M
        assert_eq!(
            bytes.get(17..25),
            Some([0x1D, 0x28, 0x6B, 3, 0, 49, 69, 49].as_slice())
        );
        // 4. store — pL/pH count the payload plus cn, fn and m.
        let payload = "upi://pay?pa=anna@upi";
        let length = payload.len() + 3;
        assert_eq!(
            bytes.get(25..33),
            Some(
                [
                    0x1D,
                    0x28,
                    0x6B,
                    u8::try_from(length).expect("short"),
                    0,
                    49,
                    80,
                    48
                ]
                .as_slice()
            )
        );
        assert_eq!(
            bytes.get(33..33 + payload.len()),
            Some(payload.as_bytes())
        );
        // 5. print
        assert_eq!(
            bytes.get(33 + payload.len()..),
            Some([0x1D, 0x28, 0x6B, 3, 0, 49, 81, 48].as_slice())
        );
    }

    #[test]
    fn non_ascii_folds_rather_than_disappearing() {
        // The width of a line is what the layout counted, so a character that
        // the printer's ROM cannot draw must still occupy one column.
        let mut out = EscPos::new();
        out.text("Anna Kuteera \u{20B9}240");
        let bytes = out.finish();
        assert_eq!(bytes.len(), "Anna Kuteera ?240".len());
        assert!(bytes.contains(&b'?'));
    }
}
