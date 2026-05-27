use crate::num::GetBit;
use bincode::{Decode, Encode};
use rustc_hash::FxHashMap;

// End address inclusive
#[derive(Debug, Clone, Encode, Decode)]
pub struct CheatWordOverrides<const START_ADDRESS: usize, const END_ADDRESS: usize> {
    address_bitset: Box<[u64]>,
    memory_overrides: FxHashMap<u32, u16>,
}

impl<const START_ADDRESS: usize, const END_ADDRESS: usize>
    CheatWordOverrides<START_ADDRESS, END_ADDRESS>
{
    #[must_use]
    pub fn new(cheat_codes: &[(u32, u16)]) -> Self {
        let mut overrides = Self {
            address_bitset: vec![0; Self::bitset_len()].into_boxed_slice(),
            memory_overrides: FxHashMap::default(),
        };

        overrides.update_cheat_codes(cheat_codes);
        overrides
    }

    pub fn update_cheat_codes(&mut self, cheat_codes: &[(u32, u16)]) {
        self.address_bitset.fill(0);
        self.memory_overrides.clear();

        for &(address, value) in cheat_codes {
            if !(START_ADDRESS..=END_ADDRESS).contains(&(address as usize)) {
                continue;
            }

            self.address_bitset[Self::address_to_bitset_idx(address)] |=
                1 << Self::address_to_bitset_bit(address);
            self.memory_overrides.insert(address & !1, value);
        }

        if !self.memory_overrides.is_empty() {
            log::debug!("Cheat codes: {:X?}", self.memory_overrides);
        }
    }

    #[must_use]
    pub fn get(&self, address: u32) -> Option<u16> {
        if self.memory_overrides.is_empty() {
            return None;
        }

        if !self
            .address_bitset
            .get(Self::address_to_bitset_idx(address))
            .is_some_and(|&bits| bits.bit(Self::address_to_bitset_bit(address)))
        {
            return None;
        }

        self.memory_overrides.get(&(address & !1)).copied()
    }

    const fn address_range_words() -> usize {
        ((END_ADDRESS - START_ADDRESS) >> 1) + 1
    }

    const fn bitset_len() -> usize {
        Self::address_range_words().div_ceil(64)
    }

    const fn address_to_bitset_idx(address: u32) -> usize {
        let address = address as usize;
        ((address.wrapping_sub(START_ADDRESS)) >> 1) / 64
    }

    const fn address_to_bitset_bit(address: u32) -> u8 {
        ((address >> 1) & 63) as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct ByteCheatCodeU16Address {
    pub address: u16,
    pub value: u8,
    // If present, only override reads when the value in memory matches the reference value
    pub reference: Option<u8>,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct CheatByteOverridesU16Address {
    address_bitset: Box<[u64]>,
    overrides: FxHashMap<u16, (u8, Option<u8>)>,
}

impl CheatByteOverridesU16Address {
    #[must_use]
    pub fn new(cheat_codes: &[ByteCheatCodeU16Address]) -> Self {
        let mut overrides = Self {
            address_bitset: vec![0; 0x10000 / 64].into_boxed_slice(),
            overrides: FxHashMap::default(),
        };

        overrides.update_cheat_codes(cheat_codes);
        overrides
    }

    pub fn update_cheat_codes(&mut self, cheat_codes: &[ByteCheatCodeU16Address]) {
        self.address_bitset.fill(0);
        self.overrides.clear();

        for &ByteCheatCodeU16Address { address, value, reference } in cheat_codes {
            self.address_bitset[(address / 64) as usize] |= 1 << (address & 63);
            self.overrides.insert(address, (value, reference));
        }
    }

    #[must_use]
    pub fn get(&self, address: u16, memory_value: u8) -> Option<u8> {
        if self.overrides.is_empty() {
            return None;
        }

        if !self.address_bitset.get((address / 64) as usize)?.bit((address & 63) as u8) {
            return None;
        }

        let (override_value, reference) = *self.overrides.get(&address)?;
        if reference.is_some_and(|reference| reference != memory_value) {
            return None;
        }

        Some(override_value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_overrides() {
        type TestOverrides = CheatWordOverrides<0xFF0000, 0xFFFFFF>;

        let mut overrides = TestOverrides::new(&[]);

        overrides.update_cheat_codes(&[(0xFFFFFF, 0x1234)]);
        assert_eq!(overrides.get(0xFFFFFE), Some(0x1234));
        assert_eq!(overrides.get(0xFFFFFF), Some(0x1234));
        for address in 0xFF0000..=0xFF00FF {
            assert_eq!(overrides.get(address), None);
        }
        assert_eq!(overrides.get(0x0000FF), None);

        overrides.update_cheat_codes(&[(0x001234, 0x5678), (0x1000000, 0xABCD)]);
        assert_eq!(overrides.get(0x001234), None);
        assert_eq!(overrides.get(0xFFFFFF), None);
        assert_eq!(overrides.get(0x1000000), None);
    }
}
