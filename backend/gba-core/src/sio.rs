//! SIO / the serial port is largely not emulated, only stubbed out enough for games that access
//! it to kind of work

use crate::interrupts::{InterruptRegisters, InterruptType};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum Mode {
    #[default]
    Normal,
    MultiPlayer,
    Uart,
    JoyBus,
    GeneralPurpose,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SerialPort {
    mode: Mode,
    rcnt: u16,
    siocnt: u16,
    siodata32: u32,
    siodata8: u16,
    next_irq_cycles: Option<u64>,
}

impl SerialPort {
    pub fn new() -> Self {
        Self {
            mode: Mode::default(),
            rcnt: 0x8000, // Sonic Advance boots into multiplayer mode if this defaults to 0
            siocnt: 0,
            siodata32: !0,
            siodata8: !0,
            next_irq_cycles: None,
        }
    }

    pub fn read_register(&mut self, address: u32) -> u16 {
        log::trace!("SIO read {address:08X}");

        match address {
            0x4000120 => self.siodata32 as u16,
            0x4000122 => (self.siodata32 >> 16) as u16,
            0x4000128 => self.read_siocnt(),
            0x400012A => self.siodata8,
            0x4000134 => self.read_rcnt(),
            0x4000136 | 0x4000142 | 0x400015A => 0, // Invalid addresses that return 0
            _ => {
                log::debug!("Unimplemented SIO read {address:08X}");
                !0
            }
        }
    }

    pub fn write_register(&mut self, address: u32, value: u16, cycles: u64) {
        match address {
            0x4000120 => {
                self.siodata32 = (self.siodata32 & !0xFFFF) | u32::from(value);
                log::trace!("SIODATA32: {:08X}", self.siodata32);
            }
            0x4000122 => {
                self.siodata32 = (self.siodata32 & 0xFFFF) | (u32::from(value) << 16);
                log::trace!("SIODATA32: {:08X}", self.siodata32);
            }
            0x4000128 => self.write_siocnt(value, cycles),
            0x400012A => {
                self.siodata8 = value;
                log::trace!("SIODATA8: {:04X}", self.siodata8);
            }
            0x4000134 => self.write_rcnt(value),
            _ => {
                log::debug!("Unimplemented SIO write {address:08X} {value:04X}");
            }
        }
    }

    fn read_rcnt(&self) -> u16 {
        self.rcnt
    }

    fn write_rcnt(&mut self, value: u16) {
        self.rcnt = value;
        self.update_mode();

        log::trace!("RCNT write: {value:04X}");
        log::trace!("  SIO mode: {:?}", self.mode);
    }

    fn read_siocnt(&self) -> u16 {
        self.siocnt
    }

    fn write_siocnt(&mut self, value: u16, cycles: u64) {
        let prev_active = self.siocnt.bit(7);

        self.siocnt = value;
        self.update_mode();

        let active = self.siocnt.bit(7);
        if self.mode == Mode::Normal && !prev_active && active {
            log::trace!("SIO transfer started in Normal mode");

            let internal_clock = self.siocnt.bit(0);
            if internal_clock {
                let clock_rate = if self.siocnt.bit(1) {
                    // 2 MHz
                    2 * 1024 * 1024
                } else {
                    // 256 KHz
                    256 * 1024
                };

                let cycles_per_bit = crate::GBA_CLOCK_SPEED / clock_rate;
                let bits = if self.siocnt.bit(12) { 32 } else { 8 };

                self.next_irq_cycles = Some(cycles + bits * cycles_per_bit);
            }
        }

        log::trace!("SIOCNT write: {value:04X}");
        log::trace!("  SIO mode: {:?}", self.mode);
    }

    fn update_mode(&mut self) {
        let bits = ((self.rcnt >> 14) << 2) | ((self.siocnt >> 12) & 3);

        self.mode = if bits & 0b1010 == 0b0000 {
            Mode::Normal
        } else if bits & 0b1011 == 0b0010 {
            Mode::MultiPlayer
        } else if bits & 0b1011 == 0b0011 {
            Mode::Uart
        } else if bits & 0b1100 == 0b1000 {
            Mode::GeneralPurpose
        } else {
            // bits & 0b1100 == 0b1100
            Mode::JoyBus
        };

        if self.mode == Mode::MultiPlayer {
            // Pretend transfer finished with no GBAs connected
            self.siocnt &= !(1 << 7);
            self.siodata32 = u32::from(self.siodata8) | (0xFFFF << 16);
        }
    }

    pub fn check_for_interrupt(&mut self, cycles: u64, interrupts: &mut InterruptRegisters) {
        let Some(next_irq_cycles) = self.next_irq_cycles else { return };

        if next_irq_cycles >= cycles {
            if self.siocnt.bit(14) {
                interrupts.set_flag(InterruptType::Serial, next_irq_cycles);
            }

            // Act like nothing is connected
            if self.siocnt.bit(12) {
                self.siodata32 = !0;
            } else {
                self.siodata8 = !0;
            }

            self.siocnt &= !(1 << 7);
            self.next_irq_cycles = None;
        }
    }
}
