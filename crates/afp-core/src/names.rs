//! SFID classification and human-readable names.
//!
//! In a 3-byte structured-field identifier the **second byte encodes the
//! function** (`0xA8` = Begin, `0xA9` = End) and the **third byte encodes the
//! object category**. A Begin/End pair shares the same third byte, which lets
//! the tree builder match nesting without a complete name table.
//!
//! The [`name`] table starts as a verified subset and is extended from the IBM
//! MO:DCA Reference (SC31-6802). Unknown SFIDs return `None`; the UI then shows
//! the raw hex rather than a guessed label — an unknown is safer than a wrong name.

/// Functional role of a structured field, derived from the SFID's second byte.
#[derive(Debug, PartialEq, Eq, Clone, Copy, serde::Serialize)]
pub enum Kind {
    /// Opens a nesting scope (second byte `0xA8`).
    Begin,
    /// Closes a nesting scope (second byte `0xA9`).
    End,
    /// A self-contained field that neither opens nor closes a scope.
    Other,
}

/// Classify an SFID by its functional (second) byte.
pub fn classify(sfid: [u8; 3]) -> Kind {
    match sfid[1] {
        0xA8 => Kind::Begin,
        0xA9 => Kind::End,
        _ => Kind::Other,
    }
}

/// The object-category byte (third byte) shared by a Begin/End pair.
pub fn category(sfid: [u8; 3]) -> u8 {
    sfid[2]
}

/// Human-readable name for a known SFID, or `None` if unrecognised.
///
/// Verified subset — extend from the MO:DCA Reference, keeping entries
/// append-only and the acronym in parentheses.
pub fn name(sfid: [u8; 3]) -> Option<&'static str> {
    Some(match sfid {
        [0xD3, 0xA8, 0xA8] => "Begin Document (BDT)",
        [0xD3, 0xA9, 0xA8] => "End Document (EDT)",
        [0xD3, 0xA8, 0xAD] => "Begin Named Page Group (BNG)",
        [0xD3, 0xA9, 0xAD] => "End Named Page Group (ENG)",
        [0xD3, 0xA8, 0xAF] => "Begin Page (BPG)",
        [0xD3, 0xA9, 0xAF] => "End Page (EPG)",
        [0xD3, 0xA8, 0xC6] => "Begin Resource Group (BRG)",
        [0xD3, 0xA9, 0xC6] => "End Resource Group (ERG)",
        [0xD3, 0xA8, 0xCE] => "Begin Resource (BR)",
        [0xD3, 0xA9, 0xCE] => "End Resource (ER)",
        [0xD3, 0xA8, 0x5F] => "Begin Page Segment (BPS)",
        [0xD3, 0xA9, 0x5F] => "End Page Segment (EPS)",
        [0xD3, 0xA8, 0xFB] => "Begin Image Object (BIM)",
        [0xD3, 0xA9, 0xFB] => "End Image Object (EIM)",
        [0xD3, 0xA8, 0x9B] => "Begin Presentation Text (BPT)",
        [0xD3, 0xA9, 0x9B] => "End Presentation Text (EPT)",
        [0xD3, 0xEE, 0x9B] => "Presentation Text (PTX)",
        [0xD3, 0xA6, 0xAF] => "Page Descriptor (PGD)",
        [0xD3, 0xA0, 0x90] => "Tag Logical Element (TLE)",
        [0xD3, 0xEE, 0xEE] => "No Operation (NOP)",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_begin_end_other() {
        assert_eq!(classify([0xD3, 0xA8, 0xA8]), Kind::Begin);
        assert_eq!(classify([0xD3, 0xA9, 0xA8]), Kind::End);
        assert_eq!(classify([0xD3, 0xEE, 0x9B]), Kind::Other); // PTX
    }

    #[test]
    fn begin_end_pair_shares_category() {
        assert_eq!(category([0xD3, 0xA8, 0xAF]), category([0xD3, 0xA9, 0xAF]));
    }

    #[test]
    fn known_name_lookup() {
        assert_eq!(name([0xD3, 0xA8, 0xA8]), Some("Begin Document (BDT)"));
        assert_eq!(name([0xD3, 0xEE, 0x9B]), Some("Presentation Text (PTX)"));
    }

    #[test]
    fn unknown_name_is_none() {
        assert_eq!(name([0x00, 0x00, 0x00]), None);
    }
}
