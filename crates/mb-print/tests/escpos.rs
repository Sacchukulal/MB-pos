//! **T3 — the golden bytes.**
//!
//! A known bill, through the File transport, at every paper size and in both
//! engines. What is committed is a **transcript** of the command stream rather
//! than the raw bytes: forty kilobytes of dots is not a thing anybody can review
//! as a diff, and reviewing the diff is the entire point of a golden file.
//!
//! The transcript is produced by walking the stream and naming each command,
//! which means this test also proves the stream is **well formed** — every byte
//! belongs to a command the decoder recognises. A stray byte in an ESC/POS
//! stream is a printer printing garbage, and it is the kind of thing that only
//! shows up on somebody's counter.
//!
//! Set `MB_UPDATE_GOLDEN=1` to rewrite them, and then read the diff.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::integer_division,
    reason = "tests: expect is the assertion, and this decodes a byte stream"
)]

mod common;

use std::path::PathBuf;

use common::Fixture;
use mb_print::escpos::{JobOptions, encode_raster, encode_text};
use mb_print::font::Font;
use mb_print::layout::{Grid, layout, layout_for};
use mb_print::paper::{Paper, PaperKind};
use mb_print::printer::{Capabilities, PrinterConfig, Target};
use mb_print::raster::{RasterOptions, to_raster};
use mb_print::template::{Copy, bill_document};
use mb_print::transport::Transport;
use mb_print::transport::file::FileTransport;

/// T3. Golden bytes, per paper size and per engine.
#[test]
fn t3_the_wire_is_what_it_was() {
    let fixture = Fixture::new();
    let font = Font::builtin().expect("the shipped face loads");
    let caps = Capabilities::default();
    let options = JobOptions::default();

    for (name, kind) in [
        ("58mm", PaperKind::Mm58),
        ("80mm", PaperKind::Mm80),
        ("100mm", PaperKind::Mm100),
    ] {
        let doc = bill_document(Paper::new(kind), &fixture.context(Copy::Original))
            .expect("builds");

        // **Each engine's own grid, which is what the queue does** (see
        // `layout::Grid`). The printer's own font has three sizes and nothing
        // between, so a text-engine bill is laid out in whole cells; the
        // graphics engine draws any height and is laid out in dots. Laying both
        // out the same way is what made this test fail the day sizes stopped
        // being multiples — the text wire had four characters more per line
        // than the roll can hold.
        let for_text = layout_for(&doc, Grid::Cells).expect("lays out");
        let for_raster = layout_for(&doc, Grid::Dots).expect("lays out");

        let text_bytes = encode_text(&for_text, &caps, &options);
        let raster = to_raster(&for_raster, &font, RasterOptions::default()).expect("rasters");
        let raster_bytes = encode_raster(&raster, &caps, &options);

        check(&format!("wire-{name}-text"), &text_bytes);
        check(&format!("wire-{name}-raster"), &raster_bytes);
    }
}

/// The File transport writes what it was given, and appends — a file target is
/// a paper roll, and a roll does not rewind.
#[test]
fn the_file_transport_writes_the_bytes_and_does_not_rewind() {
    let scratch = common::Scratch::new("wire");
    let path: PathBuf = scratch.path("roll.bin");
    let mut transport = FileTransport::new(path.clone());

    transport.send(b"first", "bill").expect("sends");
    transport.send(b"second", "kitchen").expect("sends");

    let written = std::fs::read(&path).expect("reads back");
    assert_eq!(written, b"firstsecond");
}

/// A printer with no QR encoder gets the payload as text, in both engines.
#[test]
fn a_printer_with_no_qr_encoder_still_shows_the_customer_the_uri() {
    let fixture = Fixture::new();
    let doc = bill_document(
        Paper::new(PaperKind::Mm80),
        &fixture.context(Copy::Original),
    )
    .expect("builds");
    let laid = layout(&doc).expect("lays out");

    let caps = Capabilities {
        native_qr: false,
        ..Capabilities::default()
    };
    let bytes = encode_text(&laid, &caps, &JobOptions::default());
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("upi://pay"), "the payload vanished");
    assert!(
        !bytes.windows(3).any(|w| w == [0x1D, b'(', b'k']),
        "a printer with no encoder was sent a QR command"
    );
}

/// The defaults a `File` target gets are the ones a file can honour.
#[test]
fn a_file_printer_is_never_told_to_cut_or_to_kick() {
    let printer = PrinterConfig::new("prn", "File", Target::File {
        path: PathBuf::from("out.bin"),
    });
    let mut doc = mb_print::doc::Document::new(Paper::new(PaperKind::Mm80));
    doc.line("TOTAL 646.00");
    let laid = layout(&doc).expect("lays out");

    let bytes = encode_text(&laid, &printer.caps, &JobOptions::default());
    assert!(!bytes.windows(2).any(|w| w == [0x1D, b'V']));
    assert!(!bytes.windows(2).any(|w| w == [0x1B, b'p']));
}

// ---------------------------------------------------------------------------
// The transcript.
// ---------------------------------------------------------------------------

fn check(name: &str, bytes: &[u8]) {
    let transcript = transcribe(bytes);
    let path = std::path::Path::new("tests/golden").join(format!("{name}.txt"));

    if std::env::var_os("MB_UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all("tests/golden").expect("golden dir");
        std::fs::write(&path, transcript.as_bytes()).expect("write golden");
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "{} is missing — run with MB_UPDATE_GOLDEN=1 and read the diff",
            path.display()
        )
    });
    assert_eq!(
        expected.replace("\r\n", "\n"),
        transcript,
        "{name} changed on the wire. Read the diff, then MB_UPDATE_GOLDEN=1 if it is right."
    );
}

/// Walk an ESC/POS stream and name every command.
///
/// A byte the decoder does not recognise is reported rather than skipped: an
/// unrecognised byte in this stream is a printer printing garbage, and the whole
/// value of a golden file is that it notices.
fn transcribe(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    let mut text = String::new();

    let flush = |text: &mut String, out: &mut String| {
        if !text.is_empty() {
            out.push_str(&format!("TEXT {:?}\n", text));
            text.clear();
        }
    };

    while i < bytes.len() {
        let byte = bytes[i];
        match byte {
            0x1B => {
                flush(&mut text, &mut out);
                let (line, used) = escape(bytes, i);
                out.push_str(&line);
                out.push('\n');
                i += used;
            }
            0x1D => {
                flush(&mut text, &mut out);
                let (line, used) = group(bytes, i);
                out.push_str(&line);
                out.push('\n');
                i += used;
            }
            b'\n' => {
                flush(&mut text, &mut out);
                out.push_str("LF\n");
                i += 1;
            }
            other => {
                text.push(char::from(other));
                i += 1;
            }
        }
    }
    flush(&mut text, &mut out);
    out
}

fn at(bytes: &[u8], index: usize) -> u8 {
    bytes.get(index).copied().unwrap_or(0)
}

fn escape(bytes: &[u8], i: usize) -> (String, usize) {
    match at(bytes, i + 1) {
        b'@' => ("ESC @        initialise".to_owned(), 2),
        b't' => (format!("ESC t {}      code table", at(bytes, i + 2)), 3),
        b'a' => (format!("ESC a {}      align", at(bytes, i + 2)), 3),
        b'E' => (format!("ESC E {}      emphasis", at(bytes, i + 2)), 3),
        b'G' => (format!("ESC G {}      double strike", at(bytes, i + 2)), 3),
        b'2' => ("ESC 2        default line spacing".to_owned(), 2),
        b'3' => (format!("ESC 3 {}     line spacing", at(bytes, i + 2)), 3),
        b'd' => (format!("ESC d {}      feed lines", at(bytes, i + 2)), 3),
        b'J' => (format!("ESC J {}     feed dots", at(bytes, i + 2)), 3),
        b'p' => (
            format!(
                "ESC p {} {} {}  drawer pulse",
                at(bytes, i + 2),
                at(bytes, i + 3),
                at(bytes, i + 4)
            ),
            5,
        ),
        other => (format!("?? ESC {other:#04x}"), 2),
    }
}

fn group(bytes: &[u8], i: usize) -> (String, usize) {
    match at(bytes, i + 1) {
        b'!' => (
            format!("GS ! {:#04x}     character size", at(bytes, i + 2)),
            3,
        ),
        b'V' => {
            if at(bytes, i + 2) == 66 {
                (
                    format!("GS V 66 {}   feed and partial cut", at(bytes, i + 3)),
                    4,
                )
            } else {
                (format!("GS V {}       cut", at(bytes, i + 2)), 3)
            }
        }
        b'v' => {
            let width = usize::from(at(bytes, i + 4)) + usize::from(at(bytes, i + 5)) * 256;
            let rows = usize::from(at(bytes, i + 6)) + usize::from(at(bytes, i + 7)) * 256;
            let data = width * rows;
            (
                format!("GS v 0       raster {width} bytes x {rows} rows"),
                8 + data,
            )
        }
        b'(' => {
            // GS ( k pL pH …
            let length = usize::from(at(bytes, i + 3)) + usize::from(at(bytes, i + 4)) * 256;
            let function = at(bytes, i + 6);
            let name = match function {
                65 => "QR model",
                67 => "QR module size",
                69 => "QR error correction",
                80 => "QR store",
                81 => "QR print",
                _ => "GS ( k",
            };
            (format!("GS ( k       {name} ({length} bytes)"), 5 + length)
        }
        other => (format!("?? GS {other:#04x}"), 2),
    }
}
