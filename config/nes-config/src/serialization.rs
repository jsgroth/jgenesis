//! Code to make NES palettes serialize as a base64 string rather than raw bytes, so that they
//! serialize nicer in TOML and other string-based config formats

use crate::NesPalette;
use base64::Engine;
use serde::de::{Error, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::array;
use std::fmt::Formatter;

const BASE64_ENCODER: base64::engine::GeneralPurpose =
    base64::engine::general_purpose::STANDARD_NO_PAD;

impl Serialize for NesPalette {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut bytes = [0_u8; 512 * 3];
        for (i, chunk) in bytes.chunks_exact_mut(3).enumerate() {
            let (r, g, b) = self.0[i];
            chunk.copy_from_slice(&[r, g, b]);
        }

        let s = BASE64_ENCODER.encode(bytes);
        serializer.serialize_str(&s)
    }
}

struct DeserializeVisitor;

impl Visitor<'_> for DeserializeVisitor {
    type Value = NesPalette;

    fn expecting(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "NES palette as a hex string")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: Error,
    {
        let bytes =
            BASE64_ENCODER.decode(v.as_bytes()).map_err(|err| Error::custom(err.to_string()))?;

        if bytes.len() != 512 * 3 {
            return Err(Error::custom(format!(
                "Expected palette of length {}, was {}",
                512 * 3,
                bytes.len()
            )));
        }

        Ok(NesPalette(array::from_fn(|i| (bytes[3 * i], bytes[3 * i + 1], bytes[3 * i + 2]))))
    }
}

impl<'de> Deserialize<'de> for NesPalette {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(DeserializeVisitor)
    }
}
