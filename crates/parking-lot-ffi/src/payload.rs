//! How a task or result payload is stored.
//!
//! Payloads cross the binding boundary as bytes (`ByteArray` in Kotlin, `Data` in
//! Swift): the queue does not interpret them, so an app can put JSON, protobuf or a
//! file reference in them. The embedded store serialises every payload as JSON, and
//! JSON has no byte type; `serde`'s default for `Vec<u8>` is an array of numbers
//! (about 3.5 bytes per payload byte). [`Payload`] stores lowercase hex instead
//! (2 bytes per byte) so a payload is a readable string in the database file.

use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Opaque payload bytes, stored as a hex string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payload(pub Vec<u8>);

const HEX: &[u8; 16] = b"0123456789abcdef";

const fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

impl Serialize for Payload {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut text = String::with_capacity(self.0.len() * 2);
        for &byte in &self.0 {
            text.push(char::from(HEX[usize::from(byte >> 4)]));
            text.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        serializer.serialize_str(&text)
    }
}

struct HexVisitor;

impl Visitor<'_> for HexVisitor {
    type Value = Payload;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a hex string of even length")
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<Payload, E> {
        let digits = text.as_bytes();
        if !digits.len().is_multiple_of(2) {
            return Err(E::custom("odd number of hex digits"));
        }
        let (pairs, _) = digits.as_chunks::<2>();
        let mut bytes = Vec::with_capacity(pairs.len());
        for pair in pairs {
            match (nibble(pair[0]), nibble(pair[1])) {
                (Some(high), Some(low)) => bytes.push(high << 4 | low),
                _ => return Err(E::custom("non-hex character in payload")),
            }
        }
        Ok(Payload(bytes))
    }
}

impl<'de> Deserialize<'de> for Payload {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(HexVisitor)
    }
}
