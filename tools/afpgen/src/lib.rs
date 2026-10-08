//! Builders that emit synthetic AFP byte streams for tests and fixtures.
//!
//! Reuses [`afp_core::build::StreamBuilder`] so the record format is defined in
//! exactly one place. Image objects embed [`sample_jpeg::SAMPLE_JPEG`] inside an
//! Image Picture Data (IPD) field, matching the JPEG-marker extraction path in
//! `afp_core::ioca`.

use afp_core::build::StreamBuilder;

pub mod sample_jpeg;

// Structured-field identifiers used by the fixtures.
const BDT: [u8; 3] = [0xD3, 0xA8, 0xA8];
const EDT: [u8; 3] = [0xD3, 0xA9, 0xA8];
const BPG: [u8; 3] = [0xD3, 0xA8, 0xAF];
const EPG: [u8; 3] = [0xD3, 0xA9, 0xAF];
const PGD: [u8; 3] = [0xD3, 0xA6, 0xAF];
const PTX: [u8; 3] = [0xD3, 0xEE, 0x9B];
const BRG: [u8; 3] = [0xD3, 0xA8, 0xC6];
const ERG: [u8; 3] = [0xD3, 0xA9, 0xC6];
const BR: [u8; 3] = [0xD3, 0xA8, 0xCE];
const ER: [u8; 3] = [0xD3, 0xA9, 0xCE];
const BIM: [u8; 3] = [0xD3, 0xA8, 0xFB];
const EIM: [u8; 3] = [0xD3, 0xA9, 0xFB];
const IPD: [u8; 3] = [0xD3, 0xEE, 0xFB];

/// Encode an EBCDIC resource name (A-Z, 0-9) for a Fully Qualified Name triplet.
fn ebcdic(name: &str) -> Vec<u8> {
    name.bytes()
        .map(|b| match b {
            b'A'..=b'I' => 0xC1 + (b - b'A'),
            b'J'..=b'R' => 0xD1 + (b - b'J'),
            b'S'..=b'Z' => 0xE2 + (b - b'S'),
            b'0'..=b'9' => 0xF0 + (b - b'0'),
            _ => 0x40,
        })
        .collect()
}

/// Build a Fully Qualified Name triplet (id 0x02) carrying `name`.
fn fqn_triplet(name: &str) -> Vec<u8> {
    let encoded = ebcdic(name);
    let mut t = vec![(2 + 2 + encoded.len()) as u8, 0x02, 0x00, 0x00];
    t.extend_from_slice(&encoded);
    t
}

/// A simple one-page document with a page descriptor and a line of text.
pub fn simple() -> Vec<u8> {
    StreamBuilder::new()
        .begin(BDT)
        .begin(BPG)
        .other(PGD, &[0x00, 0x00, 0x2E, 0xE0, 0x00, 0x00, 0x21, 0x60])
        .other(PTX, b"\x2b\xd3\x04Hello, AFP")
        .end(EPG)
        .end(EDT)
        .build()
}

/// A multi-page document (3 pages), each with a page descriptor and text.
pub fn multi_page() -> Vec<u8> {
    let mut b = StreamBuilder::new().begin(BDT);
    for i in 1..=3u8 {
        b = b
            .begin(BPG)
            .other(PGD, &[0x00, 0x00, 0x2E, 0xE0, 0x00, 0x00, 0x21, 0x60])
            .other(PTX, &[0x2b, 0xd3, 0x02, b'P', b'0' + i]);
        b = b.end(EPG);
    }
    b.end(EDT).build()
}

/// A document carrying a named image resource (JPEG) plus a page that would
/// include it. The image object embeds a real baseline JPEG in its IPD field.
pub fn with_image() -> Vec<u8> {
    StreamBuilder::new()
        .begin(BDT)
        .begin(BRG)
        .field(BR, &fqn_triplet("PIC1"))
        .begin(BIM)
        .other(IPD, sample_jpeg::SAMPLE_JPEG)
        .end(EIM)
        .end(ER)
        .end(ERG)
        .begin(BPG)
        .other(PTX, b"\x2b\xd3\x05Image")
        .end(EPG)
        .end(EDT)
        .build()
}

/// A deliberately malformed stream: a page is opened but never closed, and a
/// stray End appears. Used to verify the parser stays resilient.
pub fn malformed() -> Vec<u8> {
    StreamBuilder::new()
        .begin(BDT)
        .begin(BPG)
        .other(PTX, b"\x2b\xd3\x04oops")
        // no EPG
        .end(EDT)
        .end(EPG) // stray end after document closed
        .build()
}

/// All named fixtures as `(filename, bytes)`.
pub fn all_fixtures() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("simple.afp", simple()),
        ("multi-page.afp", multi_page()),
        ("with-image.afp", with_image()),
        ("malformed.afp", malformed()),
    ]
}
