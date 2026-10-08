//! A small builder that emits valid AFP structured-field byte streams.
//!
//! Used by unit tests and by the `afpgen` tool (Task 6) so fixtures and tests
//! share one definition of the record format.

use crate::sf::INTRODUCER;

/// Accumulates structured-field records into an AFP byte stream.
#[derive(Default)]
pub struct StreamBuilder {
    buf: Vec<u8>,
}

impl StreamBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one structured field with the given SFID, flag, and data.
    pub fn field_with_flag(mut self, sfid: [u8; 3], flag: u8, data: &[u8]) -> Self {
        // len counts from the length bytes through the data: 2+3+1+2 + data.
        let len = (8 + data.len()) as u16;
        self.buf.push(INTRODUCER);
        self.buf.extend_from_slice(&len.to_be_bytes());
        self.buf.extend_from_slice(&sfid);
        self.buf.push(flag);
        self.buf.extend_from_slice(&[0x00, 0x00]); // reserved
        self.buf.extend_from_slice(data);
        self
    }

    /// Append a structured field with flag `0x00`.
    pub fn field(self, sfid: [u8; 3], data: &[u8]) -> Self {
        self.field_with_flag(sfid, 0x00, data)
    }

    /// Append a Begin field (no data).
    pub fn begin(self, sfid: [u8; 3]) -> Self {
        self.field(sfid, &[])
    }

    /// Append an End field (no data).
    pub fn end(self, sfid: [u8; 3]) -> Self {
        self.field(sfid, &[])
    }

    /// Append a self-contained field carrying data.
    pub fn other(self, sfid: [u8; 3], data: &[u8]) -> Self {
        self.field(sfid, data)
    }

    /// Finish and return the accumulated bytes.
    pub fn build(self) -> Vec<u8> {
        self.buf
    }
}
