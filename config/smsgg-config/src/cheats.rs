use jgenesis_common::cheats::ByteCheatCodeU16Address;
use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmsGgCheatCodeType {
    GameGenie,
    ProActionReplay,
}

impl SmsGgCheatCodeType {
    #[must_use]
    pub fn decode(self, code: &str) -> Option<ByteCheatCodeU16Address> {
        match self {
            Self::GameGenie => decode_game_genie(code),
            Self::ProActionReplay => decode_pro_action_replay(code),
        }
    }

    #[must_use]
    pub fn guess_from(code: &str) -> Option<Self> {
        static GAME_GENIE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"^[[:xdigit:]]{3}-[[:xdigit:]]{3}(-[[:xdigit:]]{3})?$").unwrap()
        });

        static PRO_ACTION_REPLAY_REGEX: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^..[[:xdigit:]]{2}[- ]?[[:xdigit:]]{4}$").unwrap());

        if GAME_GENIE_REGEX.is_match(code) {
            return Some(Self::GameGenie);
        }

        if PRO_ACTION_REPLAY_REGEX.is_match(code) {
            return Some(Self::ProActionReplay);
        }

        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct SmsGgCheat {
    pub name: String,
    pub enabled: bool,
    pub codes: Vec<String>,
}

impl Default for SmsGgCheat {
    fn default() -> Self {
        Self { name: String::new(), enabled: true, codes: vec![] }
    }
}

impl SmsGgCheat {
    #[must_use]
    pub fn to_memory_override_vec(&self) -> Vec<ByteCheatCodeU16Address> {
        if !self.enabled {
            return vec![];
        }

        self.codes
            .iter()
            .filter_map(|code| {
                SmsGgCheatCodeType::guess_from(code).and_then(|code_type| code_type.decode(code))
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct SmsGgCheats {
    pub cheats: Vec<SmsGgCheat>,
}

impl SmsGgCheats {
    #[must_use]
    pub fn to_memory_override_vec(&self) -> Vec<ByteCheatCodeU16Address> {
        self.cheats.iter().flat_map(SmsGgCheat::to_memory_override_vec).collect()
    }
}

fn decode_game_genie(code: &str) -> Option<ByteCheatCodeU16Address> {
    // Game Genie codes are in format DDA-AAA or DDA-AAA-RRR with hex chars, per:
    //   https://www.smspower.org/Development/GameGenie
    // First 2 chars are value, next 4 chars are address (obfuscated), and last 3 chars are
    // reference value (optional and obfuscated)
    let bytes = code.as_bytes();

    if bytes.len() < 7 || bytes[3] != b'-' {
        return None;
    }

    let has_reference = bytes.len() > 7;
    if has_reference && (bytes.len() != 11 || bytes[7] != b'-') {
        return None;
    }

    // Parse into a 36-bit value to simplify deobfuscation
    let mut parsed = (u64::from_str_radix(code.get(..3)?, 16).ok()? << 24)
        | (u64::from_str_radix(code.get(4..7)?, 16).ok()? << 12);
    if has_reference {
        parsed |= u64::from_str_radix(code.get(8..)?, 16).ok()?;
    }

    // Override value is first 2 chars, not obfuscated
    let value = (parsed >> 28) as u8;

    // Address is next 4 chars, obfuscated
    // 16-bit value is rotated right by 4, then the highest 4 bits are inverted
    let address = ((parsed >> 12) as u16).rotate_right(4) ^ 0xF000;

    // Reference (if present) is last 3 chars, obfuscated
    // First and third digits form an 8-bit value, that gets rotated right by 2, then XORed with 0xBA
    // There's also a "cloak" value in here but that doesn't seem useful for emulation purposes
    let reference = has_reference.then(|| {
        let scrambled = (((parsed >> 4) & 0xF0) | (parsed & 0xF)) as u8;
        scrambled.rotate_right(2) ^ 0xBA
    });

    Some(ByteCheatCodeU16Address { address, value, reference })
}

fn decode_pro_action_replay(code: &str) -> Option<ByteCheatCodeU16Address> {
    // Pro Action Replay codes are in format "00AA AADD", per:
    //   https://www.smspower.org/Development/ProActionReplay
    // Contains unobfuscated address and value, no support for reference values

    let bytes = code.as_bytes();
    let valid_len = bytes.len() == 8 || (bytes.len() == 9 && matches!(bytes[4], b' ' | b'-'));
    if !valid_len {
        return None;
    }

    let address;
    let value;
    if bytes.len() == 8 {
        address = u16::from_str_radix(code.get(2..6)?, 16).ok()?;
        value = u8::from_str_radix(code.get(6..)?, 16).ok()?;
    } else {
        // Skip 5th char
        address = (u16::from_str_radix(code.get(2..4)?, 16).ok()? << 8)
            | u16::from_str_radix(code.get(5..7)?, 16).ok()?;
        value = u8::from_str_radix(code.get(7..)?, 16).ok()?;
    }

    Some(ByteCheatCodeU16Address { address, value, reference: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guess_code_type() {
        assert_eq!(SmsGgCheatCodeType::guess_from(""), None);

        assert_eq!(SmsGgCheatCodeType::guess_from("FFF-FFF"), Some(SmsGgCheatCodeType::GameGenie));
        assert_eq!(
            SmsGgCheatCodeType::guess_from("000-000-000"),
            Some(SmsGgCheatCodeType::GameGenie)
        );

        assert_eq!(
            SmsGgCheatCodeType::guess_from("00FF FFFF"),
            Some(SmsGgCheatCodeType::ProActionReplay)
        );
        assert_eq!(
            SmsGgCheatCodeType::guess_from("00FFFFFF"),
            Some(SmsGgCheatCodeType::ProActionReplay)
        );
    }
}
