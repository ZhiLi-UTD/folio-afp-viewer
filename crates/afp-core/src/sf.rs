//! Structured-field framing: split an AFP byte stream into records.
//!
//! An AFP structured field record is:
//! ```text
//! 0x5A | len(2) | sfid(3) | flag(1) | reserved(2) | data(len-8)
//! ```
//! `len` is big-endian and counts from the length bytes through the end of
//! the field data (i.e. the whole record *minus* the leading `0x5A`). So the
//! record occupies `1 + len` bytes and the minimum valid `len` is 8.

use std::ops::Range;

/// A single structured field located within the stream.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StructuredField {
    /// 3-byte structured-field identifier (class/type/category).
    pub sfid: [u8; 3],
    /// Flag byte following the SFID.
    pub flag: u8,
    /// Byte range of the field *data* within the stream (after the 9-byte header).
    pub data_range: Range<usize>,
    /// Byte range of the whole record including the leading `0x5A`.
    pub record_range: Range<usize>,
}

/// Errors that abort framing. Per-field recovery happens at the tree layer;
/// framing only fails when the stream is not AFP or is structurally truncated.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Stream does not begin with the `0x5A` structured-field introducer.
    NotAfp,
    /// A record claims a length that runs past the end of the buffer, or a
    /// record does not begin with `0x5A` where one was expected.
    Truncated { at: usize },
}

/// The structured-field introducer byte that prefixes every AFP record.
pub const INTRODUCER: u8 = 0x5A;

/// Size of the fixed record header after the introducer: len(2)+sfid(3)+flag(1)+reserved(2).
const HEADER_AFTER_INTRODUCER: usize = 8;

/// Split `buf` into its structured fields.
///
/// Returns [`ParseError::NotAfp`] if the stream does not start with `0x5A`, and
/// [`ParseError::Truncated`] if a record's declared length exceeds the buffer.
pub fn parse_fields(buf: &[u8]) -> Result<Vec<StructuredField>, ParseError> {
    if buf.first() != Some(&INTRODUCER) {
        return Err(ParseError::NotAfp);
    }
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < buf.len() {
        if buf[pos] != INTRODUCER {
            return Err(ParseError::Truncated { at: pos });
        }
        // Need the introducer + 2 length bytes to proceed.
        if pos + 3 > buf.len() {
            return Err(ParseError::Truncated { at: pos });
        }
        let len = u16::from_be_bytes([buf[pos + 1], buf[pos + 2]]) as usize;
        if len < HEADER_AFTER_INTRODUCER {
            return Err(ParseError::Truncated { at: pos });
        }
        let record_end = pos + 1 + len; // +1 for the 0x5A introducer
        if record_end > buf.len() {
            return Err(ParseError::Truncated { at: pos });
        }
        let sfid = [buf[pos + 3], buf[pos + 4], buf[pos + 5]];
        let flag = buf[pos + 6];
        // bytes [pos+7, pos+8] are reserved
        let data_start = pos + 9;
        out.push(StructuredField {
            sfid,
            flag,
            data_range: data_start..record_end,
            record_range: pos..record_end,
        });
        pos = record_end;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One structured field: introducer, len=0x0F (15 => 7 data bytes),
    /// SFID=D3A8A8 (Begin Document), flag=0x00, reserved=0x0000, 7 data bytes.
    fn one_bdt() -> Vec<u8> {
        vec![
            0x5A, 0x00, 0x0F, 0xD3, 0xA8, 0xA8, 0x00, 0x00, 0x00, 1, 2, 3, 4, 5, 6, 7,
        ]
    }

    #[test]
    fn parses_single_field() {
        let fields = parse_fields(&one_bdt()).unwrap();
        assert_eq!(fields.len(), 1);
        let f = &fields[0];
        assert_eq!(f.sfid, [0xD3, 0xA8, 0xA8]);
        assert_eq!(f.flag, 0x00);
        assert_eq!(f.data_range, 9..16);
        assert_eq!(f.record_range, 0..16);
    }

    #[test]
    fn parses_two_fields() {
        let mut bytes = one_bdt();
        bytes.extend(one_bdt()); // second record starts at offset 16
        let fields = parse_fields(&bytes).unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[1].record_range, 16..32);
        assert_eq!(fields[1].data_range, 25..32);
    }

    #[test]
    fn rejects_non_afp() {
        let err = parse_fields(b"%PDF-1.7").unwrap_err();
        assert_eq!(err, ParseError::NotAfp);
    }

    #[test]
    fn truncated_field_is_reported_not_panicked() {
        // claims len 15 (record would end at 16) but only 6 bytes present
        let bytes = vec![0x5A, 0x00, 0x0F, 0xD3, 0xA8, 0xA8];
        let err = parse_fields(&bytes).unwrap_err();
        assert_eq!(err, ParseError::Truncated { at: 0 });
    }

    #[test]
    fn length_below_minimum_is_truncated() {
        let bytes = vec![0x5A, 0x00, 0x05, 0xD3, 0xA8, 0xA8];
        let err = parse_fields(&bytes).unwrap_err();
        assert_eq!(err, ParseError::Truncated { at: 0 });
    }

    #[test]
    fn empty_buffer_is_not_afp() {
        assert_eq!(parse_fields(&[]).unwrap_err(), ParseError::NotAfp);
    }
}
