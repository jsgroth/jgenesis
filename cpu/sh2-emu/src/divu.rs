//! SH-2 division unit (DIVU)
//!
//! Supports signed 64-bit ÷ 32-bit division with a 32-bit quotient and a 32-bit remainder

use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Encode, Decode)]
pub struct DivisionUnit {
    pub divisor: i32,
    pub quotient: u32,
    pub remainder: u32,
    pub overflow_interrupt_enabled: bool,
    pub overflow_flag: bool,
}

impl DivisionUnit {
    pub fn new() -> Self {
        Self {
            divisor: 0,
            quotient: 0,
            remainder: 0,
            overflow_interrupt_enabled: false,
            overflow_flag: false,
        }
    }

    pub fn read_register(&self, address: u32) -> u32 {
        log::trace!("DIVU register read {address:08X}");

        match address {
            // DVSR (Divisor)
            0xFFFFFF00 => self.divisor as u32,
            // DVDNT / DVDNTL (Quotient)
            // Virtua Fighter seems to expect $FFFFFF1C to mirror $FFFFFF14
            // TODO are DVDNT and DVDNTL actually the same register? it seems like it
            0xFFFFFF04 | 0xFFFFFF14 | 0xFFFFFF1C => self.quotient,
            // DVDNTH (Remainder)
            // Virtua Fighter seems to expect $FFFFFF18 to mirror $FFFFFF10
            0xFFFFFF10 | 0xFFFFFF18 => self.remainder,
            // DVCNT (Division control)
            0xFFFFFF08 => self.read_control(),
            _ => {
                log::error!("Invalid DIVU register address: {address:08X}");
                0
            }
        }
    }

    pub fn read_control(&self) -> u32 {
        (u32::from(self.overflow_interrupt_enabled) << 1) | u32::from(self.overflow_flag)
    }

    pub fn write_register(&mut self, address: u32, value: u32) {
        log::trace!("DIVU register write {address:08X} {value:08X}");

        match address {
            0xFFFFFF00 => {
                // DVSR: Divisor
                self.divisor = value as i32;
            }
            0xFFFFFF04 => {
                // DVDNT: Dividend for 32-bit division + execute 32-bit division
                let dividend = value as i32;
                if self.divisor == 0 {
                    self.quotient = overflow_result(dividend.into());
                    self.overflow_flag = true;
                    // TODO overflow interrupt
                    return;
                }

                let quotient = dividend / self.divisor;
                let remainder = dividend % self.divisor;

                log::trace!("div32 {dividend} / {} = {quotient} {remainder}", self.divisor);

                self.quotient = quotient as u32;
                self.remainder = remainder as u32;
            }
            0xFFFFFF08 => {
                // DVCR: Division control register
                self.overflow_interrupt_enabled = value.bit(1);
                self.overflow_flag = value.bit(0);

                if self.overflow_interrupt_enabled {
                    log::error!("DIVU overflow interrupt enabled; not emulated");
                }

                log::debug!("DVCR write: {value:08X}");
                log::debug!("  Overflow interrupt enabled: {}", self.overflow_interrupt_enabled);
                log::debug!("  Overflow flag write: {}", self.overflow_flag);
            }
            0xFFFFFF10 | 0xFFFFFF18 => {
                // DVDNTH: High longword of dividend for 64-bit division
                // Store in the remainder register so software will get the same value back on reads
                // if no division operation is executed in between
                self.remainder = value;
            }
            0xFFFFFF14 | 0xFFFFFF1C => {
                // DVDNTL: Low longword of dividend for 64-bit division + execute 64-bit division
                let dividend = (i64::from(self.remainder) << 32) | i64::from(value);
                if self.divisor == 0 {
                    self.quotient = overflow_result(dividend);
                    self.overflow_flag = true;
                    // TODO overflow interrupt
                    return;
                }

                let divisor: i64 = self.divisor.into();
                let quotient = dividend / divisor;
                let remainder = dividend % divisor;

                let clamped_quotient = quotient.clamp(i32::MIN.into(), i32::MAX.into());
                self.overflow_flag |= clamped_quotient != quotient;

                log::trace!("div64 {dividend} / {divisor} = {quotient}, {remainder}");

                self.quotient = clamped_quotient as u32;
                self.remainder = remainder as u32;
            }
            _ => {
                log::warn!("Invalid DIVU register address: {address:08X} {value:08X}");
            }
        }
    }
}

fn overflow_result(dividend: i64) -> u32 {
    if dividend >= 0 { 0x7FFFFFFF } else { 0x80000000 }
}
