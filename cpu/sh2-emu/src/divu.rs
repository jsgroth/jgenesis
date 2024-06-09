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
            0xFFFFFF04 => self.dividend as u32,
            _ => todo!("DIVU register read {address:08X}"),
        }
    }

    pub fn write_register(&mut self, address: u32, value: u32) {
        match address {
            0xFFFFFF00 => {
                self.divisor = (value as i32).into();
            }
            0xFFFFFF04 => {
                // Dividend for 32-bit division; sign extended to 64 bits
                self.dividend = (value as i32).into();

                // Writing to this register initiates 32-bit / 32-bit division
                if self.divisor == 0 {
                    todo!("division by zero")
                }

                let quotient = (self.dividend / self.divisor) as i32;
                self.dividend =
                    (self.dividend & (0xFFFFFFFF_00000000_u64 as i64)) | i64::from(quotient as u32);
            }
            _ => todo!("DIVU register write {address:08X} {value:08X}"),
        }
    }
}
