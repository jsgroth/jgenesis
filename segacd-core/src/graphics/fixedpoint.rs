use jgenesis_traits::num::GetBit;
use std::ops::{Add, AddAssign};

// 13 integer bits + 11 fractional bits
const MASK: u32 = (1 << 24) - 1;

// 13.11 fixed point decimal number
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedPointDecimal(u32);

impl FixedPointDecimal {
    pub fn from_position(position_word: u16) -> Self {
        // Positions have 13 integer bits and 3 fractional bits; shift left 8 to get to 11
        // fractional bits
        Self(u32::from(position_word) << 8)
    }

    pub fn from_delta(delta_word: u16) -> Self {
        // Deltas have a sign bit, 4 integer bits, and 11 fractional bits
        // Handle the sign bit by sign extending to 32 bits
        Self(delta_word as i16 as u32)
    }

    pub fn integer_part(self) -> u32 {
        // Shift right 11 to drop all the fractional bits
        (self.0 & MASK) >> 11
    }
}

impl Add for FixedPointDecimal {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self((self.0 + rhs.0) & MASK)
    }
}

impl AddAssign for FixedPointDecimal {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}
