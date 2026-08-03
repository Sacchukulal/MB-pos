//! The logo, as dots — **decision D37.**
//!
//! P06 left `Block::Image`'s bytes deliberately opaque: *"bytes, because this
//! crate does not decode images — the sink that can draw one does."* This
//! session is that sink, and the choice it has to make is what those bytes are.
//!
//! They are **not** a PNG. A PNG decoder is a dependency, an inflate
//! implementation and a parser being fed a file a shopkeeper uploaded, all to
//! answer a question with two possible answers per dot. A thermal printer
//! prints one bit per dot; anything richer is a decision deferred to print time,
//! where it will be made badly and in a hurry.
//!
//! So the format is three fields and a bitmap, and the conversion happens
//! **once, upstream**: P17's settings screen takes the JPEG or PNG the owner
//! uploads, decodes it in the browser — which does that for free and can show
//! the result before it is saved — thresholds it, and stores this. The shop
//! sees exactly what will print, at the moment it chooses the logo, rather than
//! discovering it on paper.
//!
//! ```text
//! 0..3   "MB1"
//! 3      format version, currently 1
//! 4..6   width in dots,  u16 little-endian
//! 6..8   height in dots, u16 little-endian
//! 8..    packed rows, ceil(width / 8) bytes each, most significant bit
//!        leftmost, a set bit meaning ink
//! ```
//!
//! A logo that does not parse is **skipped with a note**, never an error. A
//! shop with a corrupt logo still gets its bill.

// Packing dots into bytes. Nothing here is money.
#![allow(
    clippy::integer_division,
    reason = "dots into bytes, not money"
)]

use serde::{Deserialize, Serialize};

/// The four bytes that say "this is one of ours".
const MAGIC: &[u8; 3] = b"MB1";
const VERSION: u8 = 1;
const HEADER: usize = 8;

/// Why a picture could not be printed. Never fatal — see the module docs.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImageError {
    #[error("this is not a Magic Bill 1-bit image (it starts {found:?})")]
    NotOurs { found: String },
    #[error("this image is version {found}, and this build understands {VERSION}")]
    Version { found: u8 },
    #[error("the image says it is {width}x{height} dots but carries {actual} bytes, not {needed}")]
    Truncated {
        width: u32,
        height: u32,
        needed: usize,
        actual: usize,
    },
    #[error("an image of {width}x{height} dots is not something a receipt can hold")]
    Absurd { width: u32, height: u32 },
}

/// A one-bit picture: exactly what a thermal head can print.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Monochrome {
    pub width: u32,
    pub height: u32,
    /// Packed rows, `stride()` bytes each, MSB leftmost, a set bit is ink.
    pub bits: Vec<u8>,
}

impl Monochrome {
    /// Bytes per row.
    #[must_use]
    pub const fn stride(width: u32) -> usize {
        (width as usize).div_ceil(8)
    }

    #[must_use]
    pub fn blank(width: u32, height: u32) -> Monochrome {
        Monochrome {
            width,
            height,
            bits: vec![0; Monochrome::stride(width) * height as usize],
        }
    }

    #[must_use]
    pub fn ink(&self, x: u32, y: u32) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let index = (y as usize) * Monochrome::stride(self.width) + (x as usize) / 8;
        let bit = 7 - ((x as usize) % 8);
        self.bits.get(index).is_some_and(|b| b >> bit & 1 == 1)
    }

    pub fn set(&mut self, x: u32, y: u32) {
        if x >= self.width || y >= self.height {
            return;
        }
        let index = (y as usize) * Monochrome::stride(self.width) + (x as usize) / 8;
        let bit = 7 - ((x as usize) % 8);
        if let Some(byte) = self.bits.get_mut(index) {
            *byte |= 1 << bit;
        }
    }

    /// Read the format above.
    pub fn decode(bytes: &[u8]) -> Result<Monochrome, ImageError> {
        if bytes.len() < HEADER || bytes.get(0..3) != Some(MAGIC.as_slice()) {
            let found: String = bytes
                .iter()
                .take(4)
                .map(|b| char::from(*b).escape_default().to_string())
                .collect();
            return Err(ImageError::NotOurs { found });
        }
        let version = bytes.get(3).copied().unwrap_or(0);
        if version != VERSION {
            return Err(ImageError::Version { found: version });
        }
        let width = u32::from(read_u16(bytes, 4));
        let height = u32::from(read_u16(bytes, 6));
        // A receipt is at most 832 dots across; a logo taller than a metre of
        // paper is a mistake somewhere upstream, not a picture.
        if width == 0 || height == 0 || width > 4_096 || height > 4_096 {
            return Err(ImageError::Absurd { width, height });
        }
        let needed = Monochrome::stride(width) * height as usize;
        let body = bytes.get(HEADER..).unwrap_or_default();
        if body.len() < needed {
            return Err(ImageError::Truncated {
                width,
                height,
                needed,
                actual: body.len(),
            });
        }
        Ok(Monochrome {
            width,
            height,
            bits: body.get(..needed).unwrap_or_default().to_vec(),
        })
    }

    /// Write the format above. P17 does this in the browser; this exists so the
    /// tests, and anyone porting the conversion, have one definition to agree
    /// with.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER + self.bits.len());
        out.extend_from_slice(MAGIC);
        out.push(VERSION);
        out.extend_from_slice(&u16::try_from(self.width).unwrap_or(u16::MAX).to_le_bytes());
        out.extend_from_slice(&u16::try_from(self.height).unwrap_or(u16::MAX).to_le_bytes());
        out.extend_from_slice(&self.bits);
        out
    }

    /// Nearest-neighbour resize.
    ///
    /// Nearest neighbour and not anything cleverer, because the source is
    /// already one bit: there is nothing to interpolate between, and a filter
    /// would only produce greys that have to be thresholded back again.
    #[must_use]
    pub fn scaled_to(&self, width: u32) -> Monochrome {
        if width == 0 || width == self.width || self.width == 0 {
            return self.clone();
        }
        let height = (u64::from(self.height) * u64::from(width))
            .div_euclid(u64::from(self.width))
            .max(1);
        let height = u32::try_from(height).unwrap_or(self.height);
        let mut out = Monochrome::blank(width, height);
        for y in 0..height {
            let src_y = scale_index(y, self.height, height);
            for x in 0..width {
                let src_x = scale_index(x, self.width, width);
                if self.ink(src_x, src_y) {
                    out.set(x, y);
                }
            }
        }
        out
    }
}

/// Which source dot a destination dot comes from.
///
/// In 64 bits so that a wide image times a wide target cannot overflow on the
/// way, and back through `try_from` so the narrowing is a decision rather than a
/// truncation.
fn scale_index(index: u32, source: u32, target: u32) -> u32 {
    if target == 0 {
        return 0;
    }
    let scaled = (u64::from(index) * u64::from(source)).div_euclid(u64::from(target));
    u32::try_from(scaled).unwrap_or(source.saturating_sub(1))
}

fn read_u16(bytes: &[u8], at: usize) -> u16 {
    let lo = bytes.get(at).copied().unwrap_or(0);
    let hi = bytes.get(at + 1).copied().unwrap_or(0);
    u16::from_le_bytes([lo, hi])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkerboard(width: u32, height: u32) -> Monochrome {
        let mut image = Monochrome::blank(width, height);
        for y in 0..height {
            for x in 0..width {
                if (x + y) % 2 == 0 {
                    image.set(x, y);
                }
            }
        }
        image
    }

    #[test]
    fn an_image_round_trips() {
        let image = checkerboard(17, 5);
        let bytes = image.encode();
        let back = Monochrome::decode(&bytes).expect("decodes");
        assert_eq!(image, back);
        assert!(back.ink(0, 0));
        assert!(!back.ink(1, 0));
        // Off the edge is not ink, and is not a panic either.
        assert!(!back.ink(17, 0));
        assert!(!back.ink(0, 5));
    }

    #[test]
    fn a_png_is_not_one_of_ours_and_says_so() {
        // Exactly what the shared test fixture carries as its "logo".
        let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        let err = Monochrome::decode(&png).expect_err("must refuse");
        assert!(matches!(err, ImageError::NotOurs { .. }));
    }

    #[test]
    fn a_truncated_image_is_refused_rather_than_read_past() {
        let image = checkerboard(16, 4);
        let mut bytes = image.encode();
        bytes.truncate(bytes.len() - 3);
        assert!(matches!(
            Monochrome::decode(&bytes),
            Err(ImageError::Truncated { .. })
        ));
    }

    #[test]
    fn scaling_keeps_the_shape() {
        let image = checkerboard(40, 20);
        let small = image.scaled_to(20);
        assert_eq!(small.width, 20);
        assert_eq!(small.height, 10);
        assert!(small.bits.iter().any(|b| *b != 0), "everything went white");
    }
}
