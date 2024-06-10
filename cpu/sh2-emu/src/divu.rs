use bincode::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct DivisionUnit {
    pub divisor: i64,
    pub dividend: i64,
}

impl DivisionUnit {
    pub fn new() -> Self {
        Self { divisor: 0, dividend: 0 }
    }

    pub fn read_register(&self, address: u32) -> u32 {
        match address {
            // DVDNT / DVDNTL
            0xFFFFFF04 | 0xFFFFFF14 | 0xFFFFFF1C => self.dividend as u32,
            // DVDNTH
            0xFFFFFF10 | 0xFFFFFF18 => ((self.dividend as u64) >> 32) as u32,
            _ => todo!("DIVU register read {address:08X}"),
        }
    }

    pub fn write_register(&mut self, address: u32, value: u32) {
        match address {
            0xFFFFFF00 => {
                // DVSR: Divisor
                self.divisor = (value as i32).into();
            }
            0xFFFFFF04 => {
                // DVDNT: Dividend for 32-bit division + execute 32-bit division
                self.dividend = (value as i32).into();

                if self.divisor == 0 {
                    self.dividend = overflow_result(self.dividend);
                    // TODO set overflow flag
                    return;
                }

                let quotient = (self.dividend / self.divisor) as i32;
                self.dividend = quotient.into();
            }
            0xFFFFFF10 => {
                // DVDNTH: High longword of dividend for 64-bit division
                self.dividend = (i64::from(value) << 32) | (self.dividend & 0xFFFFFFFF);
            }
            0xFFFFFF14 => {
                // DVDNTL: Low longword of dividend for 64-bit division + execute 64-bit division
                let dividend = (self.dividend & !0xFFFFFFFF) | (value as i64);
                if self.divisor == 0 {
                    self.dividend = overflow_result(self.dividend);
                    // TODO set overflow flag
                    return;
                }

                let quotient = dividend / self.divisor;
                let remainder = dividend % self.divisor;

                // TODO check for overflow
                self.dividend = (quotient & 0xFFFFFFFF) | (remainder << 32);
            }
            _ => todo!("DIVU register write {address:08X} {value:08X}"),
        }
    }
}

fn overflow_result(dividend: i64) -> i64 {
    if dividend >= 0 { 0x7FFFFFFF } else { -0x80000000 }
}
