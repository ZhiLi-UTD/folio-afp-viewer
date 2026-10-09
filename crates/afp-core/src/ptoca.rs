//! Decode PTOCA presentation-text control sequences into positioned text runs.
//!
//! PTX field data is a chain of control sequences. The data opens with the
//! introducer `0x2B 0xD3`; thereafter each control sequence is
//! `[length][type][parameters]`, where `length` counts itself through the
//! parameters. The control-sequence type's low bit is the chaining flag, so we
//! mask it off before matching. We track the current inline (x) and baseline
//! (y) position and emit a [`TextRun`] for each Transparent Data sequence.

use crate::resource::decode_ebcdic;

/// A run of text positioned at (x, y) in L-units, top-left origin.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TextRun {
    pub x: i32,
    pub y: i32,
    pub text: String,
    /// Local font id selected (SCFL) when this run was emitted, if any.
    pub font_id: Option<u8>,
}

/// Mutable PTOCA decoding state: inline/baseline position and active font.
/// Threaded across PTX records so a presentation-text object split over several
/// records keeps its position and font selection.
#[derive(Debug, Default, Clone, Copy)]
pub struct Cursor {
    pub x: i32,
    pub y: i32,
    pub font_id: Option<u8>,
}

// Control-sequence base type codes (low/chaining bit masked off).
const AMI: u8 = 0xC6; // Absolute Move Inline   -> set x
const RMI: u8 = 0xC8; // Relative Move Inline   -> add x
const AMB: u8 = 0xD2; // Absolute Move Baseline -> set y
const RMB: u8 = 0xD4; // Relative Move Baseline -> add y
const TRN: u8 = 0xDA; // Transparent Data       -> text
const SCFL: u8 = 0xF0; // Set Coded Font Local  -> select font

/// Nominal per-character inline advance (L-units) used only to separate
/// consecutive Transparent Data runs that are not divided by an explicit move.
const NOMINAL_ADVANCE: i32 = 144;

/// Decode PTOCA text data into absolutely-positioned runs (fresh state).
pub fn parse_text(data: &[u8]) -> Vec<TextRun> {
    let mut runs = Vec::new();
    let mut cur = Cursor::default();
    parse_into(data, &mut cur, &mut runs);
    runs
}

/// Decode PTOCA text data, threading decoding state through `cur` so a
/// presentation-text object split across several PTX records keeps its position
/// and font selection. Appends runs to `runs`.
pub fn parse_into(data: &[u8], cur: &mut Cursor, runs: &mut Vec<TextRun>) {
    let mut pos = 0usize;
    while pos < data.len() {
        // A new (unchained) control sequence is preceded by the 0x2B 0xD3
        // introducer; consume it wherever it appears, not just at the start.
        if pos + 1 < data.len() && data[pos] == 0x2B && data[pos + 1] == 0xD3 {
            pos += 2;
            if pos >= data.len() {
                break;
            }
        }
        let len = data[pos] as usize;
        if len < 2 || pos + len > data.len() {
            break;
        }
        let ty = data[pos + 1] & 0xFE; // mask chaining bit
        let params = &data[pos + 2..pos + len];
        match ty {
            AMI => {
                if let Some(v) = be_u16(params) {
                    cur.x = v;
                }
            }
            RMI => {
                if let Some(v) = be_i16(params) {
                    cur.x = cur.x.saturating_add(v);
                }
            }
            AMB => {
                if let Some(v) = be_u16(params) {
                    cur.y = v;
                }
            }
            RMB => {
                if let Some(v) = be_i16(params) {
                    cur.y = cur.y.saturating_add(v);
                }
            }
            SCFL => {
                // First parameter byte is the local font id to activate.
                if let Some(&id) = params.first() {
                    cur.font_id = Some(id);
                }
            }
            TRN => {
                let text = decode_ebcdic(params);
                let advance = (text.chars().count() as i32).saturating_mul(NOMINAL_ADVANCE);
                runs.push(TextRun {
                    x: cur.x,
                    y: cur.y,
                    text,
                    font_id: cur.font_id,
                });
                cur.x = cur.x.saturating_add(advance);
            }
            _ => {} // unknown control sequence: skip by its length
        }
        pos += len;
    }
}

/// First two bytes of `p` as a big-endian unsigned value widened to i32.
fn be_u16(p: &[u8]) -> Option<i32> {
    if p.len() >= 2 {
        Some(u16::from_be_bytes([p[0], p[1]]) as i32)
    } else {
        None
    }
}

/// First two bytes of `p` as a big-endian *signed* value (relative moves).
fn be_i16(p: &[u8]) -> Option<i32> {
    if p.len() >= 2 {
        Some(i16::from_be_bytes([p[0], p[1]]) as i32)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EBCDIC-encode an uppercase/digit ASCII string (test helper).
    fn ebcdic(s: &str) -> Vec<u8> {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'I' => 0xC1 + (b - b'A'),
                b'J'..=b'R' => 0xD1 + (b - b'J'),
                b'S'..=b'Z' => 0xE2 + (b - b'S'),
                b'0'..=b'9' => 0xF0 + (b - b'0'),
                b' ' => 0x40,
                _ => 0x40,
            })
            .collect()
    }

    fn cs(ty: u8, params: &[u8]) -> Vec<u8> {
        let mut v = vec![(2 + params.len()) as u8, ty];
        v.extend_from_slice(params);
        v
    }

    #[test]
    fn decodes_positioned_text() {
        let mut data = vec![0x2B, 0xD3];
        data.extend(cs(AMB | 1, &300i32.to_be_bytes()[2..])); // y=300
        data.extend(cs(AMI | 1, &120i32.to_be_bytes()[2..])); // x=120
        data.extend(cs(TRN | 1, &ebcdic("HI"))); // text
        let runs = parse_text(&data);
        assert_eq!(runs.len(), 1);
        assert_eq!(
            runs[0],
            TextRun { x: 120, y: 300, text: "HI".into(), font_id: None }
        );
    }

    #[test]
    fn scfl_selects_active_font_for_following_runs() {
        let mut data = vec![0x2B, 0xD3];
        data.extend(cs(SCFL, &[0x02])); // select local font 2
        data.extend(cs(AMI, &100u16.to_be_bytes()));
        data.extend(cs(TRN, &ebcdic("A")));
        let runs = parse_text(&data);
        assert_eq!(runs[0].font_id, Some(0x02));
    }

    #[test]
    fn relative_move_adds_to_inline() {
        let mut data = vec![0x2B, 0xD3];
        data.extend(cs(AMI, &100u16.to_be_bytes())); // x=100
        data.extend(cs(RMI, &50u16.to_be_bytes())); // x=150
        data.extend(cs(TRN, &ebcdic("A")));
        let runs = parse_text(&data);
        assert_eq!(runs[0].x, 150);
    }

    #[test]
    fn two_runs_without_move_do_not_overlap() {
        let mut data = vec![0x2B, 0xD3];
        data.extend(cs(AMI, &0u16.to_be_bytes()));
        data.extend(cs(TRN, &ebcdic("AB")));
        data.extend(cs(TRN, &ebcdic("CD")));
        let runs = parse_text(&data);
        assert_eq!(runs.len(), 2);
        assert!(runs[1].x > runs[0].x);
    }

    #[test]
    fn negative_relative_move_goes_backward() {
        let mut data = vec![0x2B, 0xD3];
        data.extend(cs(AMI, &100u16.to_be_bytes())); // x=100
        data.extend(cs(RMI, &(-40i16).to_be_bytes())); // x=60
        data.extend(cs(TRN, &ebcdic("A")));
        let runs = parse_text(&data);
        assert_eq!(runs[0].x, 60);
    }

    #[test]
    fn handles_multiple_unchained_introducers() {
        // Two separate control-sequence chains, each introduced by 0x2B 0xD3.
        let mut data = vec![0x2B, 0xD3];
        data.extend(cs(AMI, &100u16.to_be_bytes()));
        data.extend(cs(TRN, &ebcdic("A")));
        data.extend_from_slice(&[0x2B, 0xD3]); // second introducer
        data.extend(cs(AMI, &500u16.to_be_bytes()));
        data.extend(cs(TRN, &ebcdic("B")));
        let runs = parse_text(&data);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].x, 100);
        assert_eq!(runs[1].x, 500);
    }

    #[test]
    fn stateful_parse_preserves_cursor_across_calls() {
        let mut cur = Cursor::default();
        let mut runs = Vec::new();
        let mut a = vec![0x2B, 0xD3];
        a.extend(cs(SCFL, &[0x01])); // font selection also persists
        a.extend(cs(AMI, &300u16.to_be_bytes()));
        a.extend(cs(AMB, &400u16.to_be_bytes()));
        parse_into(&a, &mut cur, &mut runs);
        // Second record continues without repeating absolute moves or font.
        let mut b = vec![0x2B, 0xD3];
        b.extend(cs(TRN, &ebcdic("X")));
        parse_into(&b, &mut cur, &mut runs);
        assert_eq!(runs.len(), 1);
        assert_eq!(
            runs[0],
            TextRun { x: 300, y: 400, text: "X".into(), font_id: Some(0x01) }
        );
    }

    #[test]
    fn stops_on_truncated_sequence_without_panic() {
        let data = vec![0x2B, 0xD3, 0x05, TRN]; // claims len 5, only 2 bytes follow
        assert!(parse_text(&data).is_empty());
    }

    #[test]
    fn empty_data_is_no_runs() {
        assert!(parse_text(&[]).is_empty());
        assert!(parse_text(&[0x2B, 0xD3]).is_empty());
    }
}
