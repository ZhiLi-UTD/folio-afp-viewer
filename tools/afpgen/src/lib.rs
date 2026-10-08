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

/// Encode EBCDIC (A-Z, 0-9, space) for names and text. Other bytes become space.
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

/// A Page Descriptor: 1440 L-units/inch, US Letter (8.5in x 11in).
/// XpUnitBase, YpUnitBase (0x00 = 10in), XpUnits, YpUnits (14400 per 10in =
/// 1440/in), XpgSize (12240 = 8.5in), YpgSize (15840 = 11in).
const PGD_LETTER: [u8; 12] = [
    0x00, 0x00, 0x38, 0x40, 0x38, 0x40, 0x00, 0x2F, 0xD0, 0x00, 0x3D, 0xE0,
];

/// One PTOCA control sequence (chained): `[length][type|1][params]`.
fn cs(base_type: u8, params: &[u8]) -> Vec<u8> {
    let mut v = vec![(2 + params.len()) as u8, base_type | 1];
    v.extend_from_slice(params);
    v
}

/// Build PTOCA text data placing each `(x, y, text)` line (coords in L-units).
fn ptoca(lines: &[(u16, u16, &str)]) -> Vec<u8> {
    let mut d = vec![0x2B, 0xD3];
    for (x, y, text) in lines {
        d.extend(cs(0xD2, &y.to_be_bytes())); // AMB: baseline y
        d.extend(cs(0xC6, &x.to_be_bytes())); // AMI: inline x
        d.extend(cs(0xDA, &ebcdic(text))); // TRN: the characters
    }
    d
}

/// Build a Fully Qualified Name triplet (id 0x02) carrying `name`.
fn fqn_triplet(name: &str) -> Vec<u8> {
    let encoded = ebcdic(name);
    let mut t = vec![(2 + 2 + encoded.len()) as u8, 0x02, 0x00, 0x00];
    t.extend_from_slice(&encoded);
    t
}

/// A simple one-page document with a page descriptor and two lines of text.
pub fn simple() -> Vec<u8> {
    StreamBuilder::new()
        .begin(BDT)
        .begin(BPG)
        .other(PGD, &PGD_LETTER)
        .other(
            PTX,
            &ptoca(&[(1440, 1440, "HELLO AFP"), (1440, 2160, "FOLIO VIEWER")]),
        )
        .end(EPG)
        .end(EDT)
        .build()
}

/// A multi-page document (3 pages), each with a page descriptor and text.
pub fn multi_page() -> Vec<u8> {
    let mut b = StreamBuilder::new().begin(BDT);
    for i in 1..=3u8 {
        let label = format!("PAGE {i}");
        b = b
            .begin(BPG)
            .other(PGD, &PGD_LETTER)
            .other(PTX, &ptoca(&[(1440, 1440, &label), (1440, 2880, "STATEMENT")]));
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
        .other(PGD, &PGD_LETTER)
        .other(PTX, &ptoca(&[(1440, 1440, "IMAGE PAGE")]))
        .end(EPG)
        .end(EDT)
        .build()
}

/// Like [`with_image`] but the JPEG is split across several Image Picture Data
/// records, mirroring how real IOCA images exceed the single-record size limit.
pub fn with_image_split() -> Vec<u8> {
    let mut b = StreamBuilder::new()
        .begin(BDT)
        .begin(BRG)
        .field(BR, &fqn_triplet("PIC2"))
        .begin(BIM);
    for chunk in sample_jpeg::SAMPLE_JPEG.chunks(64) {
        b = b.other(IPD, chunk);
    }
    b.end(EIM).end(ER).end(ERG).end(EDT).build()
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
        ("with-image-split.afp", with_image_split()),
        ("malformed.afp", malformed()),
    ]
}
