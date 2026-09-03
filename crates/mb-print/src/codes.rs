//! The two codes a bill can carry, as modules and bars.
//!
//! The printer's own encoders (`GS ( k`, `GS k`) draw them on paper. This module exists for
//! the sinks that cannot ask a printer: the on-screen preview, which is the printer's raster
//! and must show the real square and the real bars, and a printer that has no encoder. It is
//! also where the printer's encoder is told how big a module to use, so the paper and the
//! screen agree on the size of the square.

// Dots into modules.
#![allow(clippy::integer_division, reason = "dots and modules, not money")]

/// Narrow bar width, in dots — `GS w 2`.
pub const NARROW: u32 = 2;

/// How tall the bars are, in dots — `GS h 60`.
pub const BAR_HEIGHT: u32 = 60;

/// Quiet zone either side of a barcode, in narrow bars — the specification's ten.
const QUIET: u32 = 10;

/// The module count assumed when a payload will not encode, for a module size that still
/// has to be answered — a UPI URI is a 25- to 29-module symbol.
const TYPICAL_MODULES: u32 = 25;

/// How wide a QR square is asked to be, in dots, from the setting's percentage of the paper.
#[must_use]
pub const fn qr_side(usable: u32, width_pct: u8) -> u32 {
    let pct = if width_pct < 1 {
        1
    } else if width_pct > 100 {
        100
    } else {
        width_pct
    };
    usable * (pct as u32) / 100
}

/// A QR symbol as modules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Modules {
    /// Modules across, and down.
    pub size: u32,
    dark: Vec<bool>,
}

impl Modules {
    #[must_use]
    pub fn dark(&self, x: u32, y: u32) -> bool {
        if x >= self.size || y >= self.size {
            return false;
        }
        let index = (y as usize) * (self.size as usize) + (x as usize);
        self.dark.get(index).copied().unwrap_or(false)
    }

    /// Dots per module for a square about `side` dots across — the number `GS ( k` is told
    /// and the number the raster sink draws with, so they agree.
    #[must_use]
    pub fn module_for(&self, side: u32) -> u8 {
        module_for(side, self.size)
    }
}

fn module_for(side: u32, modules: u32) -> u8 {
    u8::try_from(side / modules.max(1)).unwrap_or(u8::MAX).clamp(1, 16)
}

/// The modules of a QR carrying `payload`, at the correction level the printer is told to use
/// (M). `None` when the payload is too long for any QR — about three thousand characters.
#[must_use]
pub fn qr(payload: &str) -> Option<Modules> {
    let code =
        qrcode::QrCode::with_error_correction_level(payload.as_bytes(), qrcode::EcLevel::M).ok()?;
    let size = u32::try_from(code.width()).ok()?;
    let dark = code
        .into_colors()
        .into_iter()
        .map(|c| c == qrcode::Color::Dark)
        .collect();
    Some(Modules { size, dark })
}

/// The module size, in dots, for a square about `side` dots across — for the printer's own
/// encoder, which is told a module size and nothing else.
#[must_use]
pub fn qr_module(payload: &str, side: u32) -> u8 {
    let modules = qr(payload).map_or(TYPICAL_MODULES, |m| m.size);
    module_for(side, modules)
}

/// Code 128, set B: the bar and space widths in narrow units, alternating and starting with a
/// bar — exactly what `GS k 73 … { B` draws. `None` for a payload it cannot carry: empty,
/// longer than the command's one-byte length, or with a character outside printable ASCII.
#[must_use]
pub fn code128(payload: &str) -> Option<Vec<u8>> {
    if payload.is_empty() || payload.len() > 80 {
        return None;
    }
    let mut values = vec![START_B];
    for ch in payload.chars() {
        let byte = u8::try_from(ch).ok()?;
        if !(32..=126).contains(&byte) {
            return None;
        }
        values.push(usize::from(byte - 32));
    }
    // The check character: the start value plus each data value weighted by its position,
    // modulo 103.
    let check = values
        .iter()
        .enumerate()
        .fold(0_usize, |sum, (position, value)| {
            sum + value * position.max(1)
        })
        % 103;
    values.push(check);
    values.push(STOP);

    let mut widths = Vec::with_capacity(values.len() * 6 + 1);
    for value in values {
        let pattern = PATTERNS.get(value)?;
        widths.extend(pattern.bytes().map(|b| b - b'0'));
    }
    Some(widths)
}

/// How wide a barcode is on paper, in dots, quiet zones included.
#[must_use]
pub fn barcode_width(widths: &[u8]) -> u32 {
    let bars: u32 = widths.iter().map(|w| u32::from(*w)).sum();
    (bars + QUIET * 2) * NARROW
}

/// Where the first bar starts, in dots from the barcode's left edge.
#[must_use]
pub const fn barcode_quiet() -> u32 {
    QUIET * NARROW
}

const START_B: usize = 104;
const STOP: usize = 106;

/// The Code 128 symbol patterns, value by value: bar, space, bar, space, bar, space widths.
/// Every one sums to eleven except the stop, which carries a final two-wide bar.
const PATTERNS: [&str; 107] = [
    "212222", "222122", "222221", "121223", "121322", "131222", "122213", "122312", "132212",
    "221213", "221312", "231212", "112232", "122132", "122231", "113222", "123122", "123221",
    "223211", "221132", "221231", "213212", "223112", "312131", "311222", "321122", "321221",
    "312212", "322112", "322211", "212123", "212321", "232121", "111323", "131123", "131321",
    "112313", "132113", "132311", "211313", "231113", "231311", "112133", "112331", "132131",
    "113123", "113321", "133121", "313121", "211331", "231131", "213113", "213311", "213131",
    "311123", "311321", "331121", "312113", "312311", "332111", "314111", "221411", "431111",
    "111224", "111422", "121124", "121421", "141122", "141221", "112214", "112412", "122114",
    "122411", "142112", "142211", "241211", "221114", "413111", "241112", "134111", "111242",
    "121142", "121241", "114212", "124112", "124211", "411212", "421112", "421211", "212141",
    "214121", "412121", "111143", "111341", "131141", "114113", "114311", "411113", "411311",
    "113141", "114131", "311141", "411131", "211412", "211214", "211232", "2331112",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// A transcription error in the table is the bug this test exists for.
    #[test]
    fn every_code128_pattern_is_eleven_wide_and_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for (value, pattern) in PATTERNS.iter().enumerate() {
            let sum: u32 = pattern.bytes().map(|b| u32::from(b - b'0')).sum();
            let expected = if value == STOP { 13 } else { 11 };
            assert_eq!(sum, expected, "pattern {value} ({pattern}) is the wrong width");
            assert!(seen.insert(*pattern), "pattern {value} repeats another");
        }
        assert_eq!(PATTERNS.len(), 107);
    }

    #[test]
    fn a_bill_number_becomes_bars_with_the_right_check_character() {
        // 104 + 48·1 + 42·2 + 42·3 + 17·4 + 18·5 + 19·6 + 35·7 = 879, and 879 mod 103 is 55.
        let widths = code128("PJJ123C").expect("encodes");
        // Start, seven characters, the check and the stop.
        assert_eq!(widths.len(), 9 * 6 + 7);
        let check: String = widths[8 * 6..9 * 6]
            .iter()
            .map(|w| char::from(b'0' + *w))
            .collect();
        assert_eq!(check, PATTERNS[55]);
        assert_eq!(widths.first(), Some(&2), "a barcode starts with a bar");
    }

    #[test]
    fn a_payload_a_barcode_cannot_carry_is_refused_not_mangled() {
        assert!(code128("").is_none());
        assert!(code128("BIR/\u{20B9}12").is_none());
        assert!(code128(&"9".repeat(81)).is_none());
        assert!(code128("BIR-1207").is_some());
    }

    #[test]
    fn the_qr_module_follows_the_side_it_was_asked_for() {
        let payload = "upi://pay?pa=anna@upi&pn=Anna&am=646.00&cu=INR";
        let modules = qr(payload).expect("encodes");
        assert!(modules.size >= 21, "a QR is at least 21 modules across");
        // Two-fifths of 80 mm paper, and a tenth of it.
        assert!(modules.module_for(230) > modules.module_for(57));
        assert_eq!(qr_module(payload, 230), modules.module_for(230));
        // Never more than the printer's sixteen, never less than one.
        assert_eq!(qr_module(payload, 10_000), 16);
        assert_eq!(qr_module(payload, 5), 1);
        // The finder pattern is where a QR keeps it.
        assert!(modules.dark(0, 0) && modules.dark(6, 6) && !modules.dark(1, 1));
    }

    #[test]
    fn the_side_is_a_share_of_the_usable_paper() {
        assert_eq!(qr_side(576, 40), 230);
        assert_eq!(qr_side(576, 0), 5);
        assert_eq!(qr_side(576, 200), 576);
    }
}
