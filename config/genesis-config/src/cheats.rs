use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use regex::Regex;
use std::fmt::{Display, Formatter};
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenesisCheatCodeType {
    GameGenie,
    ActionReplay,
    MemoryOverride,
}

impl GenesisCheatCodeType {
    #[must_use]
    pub fn guess_from(code: &str) -> Option<Self> {
        static GAME_GENIE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"^(?i)[A-HJ-NPR-TV-Z0-9]{4}-[A-HJ-NPR-TV-Z0-9]{4}$").unwrap()
        });

        static PRO_ACTION_REPLAY_REGEX: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^[[:xdigit:]]{5}[ -]?[[:xdigit:]]{5}$").unwrap());

        static MEMORY_OVERRIDE_REGEX: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^[[:xdigit:]]{6}:[[:xdigit:]]{4}$").unwrap());

        if GAME_GENIE_REGEX.is_match(code) {
            return Some(Self::GameGenie);
        }

        if PRO_ACTION_REPLAY_REGEX.is_match(code) {
            return Some(Self::ActionReplay);
        }

        if MEMORY_OVERRIDE_REGEX.is_match(code) {
            return Some(Self::MemoryOverride);
        }

        None
    }

    #[must_use]
    pub fn decode(self, code: &str) -> Option<(u32, u16)> {
        match self {
            Self::GameGenie => decode_game_genie(code),
            Self::ActionReplay => decode_action_replay(code),
            Self::MemoryOverride => decode_memory_override(code),
        }
    }
}

impl Display for GenesisCheatCodeType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GameGenie => write!(f, "Game Genie"),
            Self::ActionReplay => write!(f, "Action Replay"),
            Self::MemoryOverride => write!(f, "Memory Override"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct GenesisCheat {
    pub name: String,
    pub enabled: bool,
    pub codes: Vec<String>,
}

impl GenesisCheat {
    #[must_use]
    pub fn to_memory_override_vec(&self) -> Vec<(u32, u16)> {
        if !self.enabled {
            return vec![];
        }

        self.codes
            .iter()
            .filter_map(|code| {
                GenesisCheatCodeType::guess_from(code).and_then(|code_type| code_type.decode(code))
            })
            .collect()
    }
}

impl GenesisCheat {
    #[must_use]
    pub fn new(name: String) -> Self {
        Self { name, enabled: true, codes: vec![] }
    }
}

impl Default for GenesisCheat {
    fn default() -> Self {
        Self::new(String::new())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Encode, Decode)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct GenesisCheats {
    pub cheats: Vec<GenesisCheat>,
}

impl GenesisCheats {
    /// Convert to a [`Vec`] of 68000 address/value pairs
    ///
    /// Excludes disabled cheats and any cheats that have invalid codes
    #[must_use]
    pub fn to_memory_override_vec(&self) -> Vec<(u32, u16)> {
        self.cheats
            .iter()
            .filter(|cheat| cheat.enabled)
            .flat_map(GenesisCheat::to_memory_override_vec)
            .collect()
    }
}

#[must_use]
pub fn decode_game_genie(code: &str) -> Option<(u32, u16)> {
    let code = code.as_bytes();

    // Codes should always be in ABCD-EFGH format
    if code.len() != 9 || code[4] != b'-' {
        return None;
    }

    // The 8 non-hyphen characters decode into a scrambled 40-bit value, 5 bits per char
    let mut scrambled: u64 = 0;

    for &c in &code[..4] {
        scrambled = (scrambled << 5) | decode_game_genie_char(c)?;
    }

    for &c in &code[5..] {
        scrambled = (scrambled << 5) | decode_game_genie_char(c)?;
    }

    // Scrambled bit layout:
    //     A     B     C     D  -  E     F     G     H
    //   ijklm nopIJ KLMNO PABCD EFGHd efgha bcQRS TUVWX
    //      35    30    25    20    15    10     5     0
    //
    // Descrambled:
    //   Address (24-bit): ABCD EFGH IJKL MNOP QRST UVWX
    //   Value (16-bit):   abcd efgh ijkl mnop

    let address =
        (scrambled.bits(16..=23) << 16) | (scrambled.bits(24..=31) << 8) | scrambled.bits(0..=7);

    let value =
        (scrambled.bits(8..=10) << 13) | (scrambled.bits(11..=15) << 8) | scrambled.bits(32..=39);

    Some((address as u32, value as u16))
}

fn decode_game_genie_char(c: u8) -> Option<u64> {
    // Codes are encoded in base 32 using the characters A-Z and 0-9 excluding I, O, Q, and U
    // Any other character is invalid
    let bits = match c {
        b'A'..=b'H' => c - b'A',
        b'J'..=b'N' => c - b'A' - 1,
        b'P' => c - b'A' - 2,
        b'R'..=b'T' => c - b'A' - 3,
        b'V'..=b'Z' => c - b'A' - 4,
        b'0'..=b'9' => c - b'0' + 22,
        // Support lowercase letters too, why not
        b'a'..=b'h' => c - b'a',
        b'j'..=b'n' => c - b'a' - 1,
        b'p' => c - b'a' - 2,
        b'r'..=b't' => c - b'a' - 3,
        b'v'..=b'z' => c - b'a' - 4,
        _ => return None,
    };

    debug_assert!(bits < 32);

    Some(bits.into())
}

#[must_use]
pub fn decode_action_replay(code: &str) -> Option<(u32, u16)> {
    // Pro Action Replay codes are in format "98765-43210" or "98765 43210"
    // With hyphen/space removed, first 6 digits are address and last 4 are value (in hex)
    let bytes = code.as_bytes();

    if bytes.len() != 11 || !matches!(bytes[5], b'-' | b' ') {
        return None;
    }

    let address =
        16 * u32::from_str_radix(code.get(..5)?, 16).ok()? + (bytes[6] as char).to_digit(16)?;
    let value = u16::from_str_radix(code.get(7..)?, 16).ok()?;

    // Action Replay memory values seem to be little-endian instead of big-endian; byteswap
    let value = value.swap_bytes();

    Some((address, value))
}

#[must_use]
pub fn decode_memory_override(code: &str) -> Option<(u32, u16)> {
    // Memory override codes are in format 987654:3210 (address:value in hex)
    if code.len() != 11 || code.as_bytes()[6] != b':' {
        return None;
    }

    let address = u32::from_str_radix(code.get(..6)?, 16).ok()?;
    let value = u16::from_str_radix(code.get(7..)?, 16).ok()?;

    Some((address, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_game_genie() {
        assert_eq!(decode_game_genie("97JT-EVSG"), Some((0x0251E6, 0xA8FF)));
        assert_eq!(decode_game_genie("AFJT-EVSG"), Some((0x0251E6, 0xA801)));

        assert_eq!(decode_game_genie(""), None);
        assert_eq!(decode_game_genie("AAAI-AAAA"), None);
    }

    #[test]
    fn test_decode_pro_action_replay() {
        assert_eq!(decode_action_replay("98765-43210"), Some((0x987654, 0x1032)));
        assert_eq!(decode_action_replay("FFFFF-EABCD"), Some((0xFFFFFE, 0xCDAB)));

        assert_eq!(decode_action_replay(""), None);
        assert_eq!(decode_action_replay("QWERT-YUIOP"), None);
    }

    #[test]
    fn test_decode_memory_override() {
        assert_eq!(decode_memory_override("987654:3210"), Some((0x987654, 0x3210)));
        assert_eq!(decode_memory_override("FFFFFE:ABCD"), Some((0xFFFFFE, 0xABCD)));

        assert_eq!(decode_memory_override(""), None);
        assert_eq!(decode_memory_override("QWERTY:UIOP"), None);
    }

    #[test]
    fn guess_code_type() {
        assert_eq!(GenesisCheatCodeType::guess_from(""), None);
        assert_eq!(GenesisCheatCodeType::guess_from("ABCDEFGH"), None);

        assert_eq!(
            GenesisCheatCodeType::guess_from("ABCD-EFGH"),
            Some(GenesisCheatCodeType::GameGenie)
        );
        assert_eq!(
            GenesisCheatCodeType::guess_from("abcd-efgh"),
            Some(GenesisCheatCodeType::GameGenie)
        );
        assert_eq!(GenesisCheatCodeType::guess_from("ABCD-EFGI"), None);

        assert_eq!(
            GenesisCheatCodeType::guess_from("02468-ACEFF"),
            Some(GenesisCheatCodeType::ActionReplay)
        );
        assert_eq!(
            GenesisCheatCodeType::guess_from("02468-aceff"),
            Some(GenesisCheatCodeType::ActionReplay)
        );
        assert_eq!(
            GenesisCheatCodeType::guess_from("02468 ACEFF"),
            Some(GenesisCheatCodeType::ActionReplay)
        );

        assert_eq!(
            GenesisCheatCodeType::guess_from("02468A:CEFF"),
            Some(GenesisCheatCodeType::MemoryOverride)
        );
        assert_eq!(
            GenesisCheatCodeType::guess_from("02468a:ceff"),
            Some(GenesisCheatCodeType::MemoryOverride)
        );
    }
}
