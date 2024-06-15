//! SH-2 free-running timer

use bincode::{Decode, Encode};
use jgenesis_common::num::{GetBit, U16Ext};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum CompareRegister {
    #[default]
    A = 0,
    B = 1,
}

impl CompareRegister {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::B } else { Self::A }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum ClockSource {
    #[default]
    InternalDiv8 = 0,
    InternalDiv32 = 1,
    InternalDiv128 = 2,
    External = 3,
}

impl ClockSource {
    fn from_byte(byte: u8) -> Self {
        match byte & 3 {
            0 => Self::InternalDiv8,
            1 => Self::InternalDiv32,
            2 => Self::InternalDiv128,
            3 => Self::External,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct FreeRunTimer {
    counter: u16,
    clock_source: ClockSource,
    compare_a: u16,
    compare_b: u16,
    compare_register: CompareRegister,
    compare_a_flag: bool,
    compare_b_flag: bool,
    overflow_flag: bool,
    clear_counter_on_a_match: bool,
    compare_a_interrupt_enabled: bool,
    compare_b_interrupt_enabled: bool,
    overflow_interrupt_enabled: bool,
    // Stored only to emulate these bits being R/W
    compare_a_pin_output: bool,
    compare_b_pin_output: bool,
    // 16-bit temporary register used to prevent data races when the CPU accesses 16-bit registers,
    // since the timer only supports 8-bit memory accesses
    temp_register: u16,
}

impl FreeRunTimer {
    pub fn new() -> Self {
        Self {
            counter: 0,
            clock_source: ClockSource::default(),
            compare_a: 0xFFFF,
            compare_b: 0xFFFF,
            compare_register: CompareRegister::default(),
            compare_a_flag: false,
            compare_b_flag: false,
            overflow_flag: false,
            clear_counter_on_a_match: false,
            compare_a_interrupt_enabled: false,
            compare_b_interrupt_enabled: false,
            overflow_interrupt_enabled: false,
            compare_a_pin_output: false,
            compare_b_pin_output: false,
            temp_register: 0,
        }
    }

    pub fn read_register(&self, address: u32) -> u8 {
        log::trace!("Timer register read: {address:08X}");

        match address & 0xF {
            0x7 => self.read_tocr(),
            _ => todo!("FRT register read {address:08X}"),
        }
    }

    pub fn write_register(&mut self, address: u32, value: u8) {
        match address & 0xF {
            0x0 => self.write_tier(value),
            0x1 => self.write_ftcsr(value),
            0x2 | 0x4 => {
                log::trace!("Temp register MSB write: {address:08X} {value:02X}");
                self.temp_register.set_msb(value);
            }
            0x3 => self.write_frc(value),
            0x5 => self.write_ocr(value),
            0x6 => self.write_tcr(value),
            0x7 => self.write_tocr(value),
            _ => todo!("FRT register write {address:08X} {value:02X}"),
        }
    }

    // $FFFFFE10: TIER (Timer interrupt enable register)
    fn write_tier(&mut self, value: u8) {
        if value.bit(7) {
            log::warn!("Input capture interrupt enabled; not implemented");
        }

        self.compare_a_interrupt_enabled = value.bit(3);
        self.compare_b_interrupt_enabled = value.bit(2);
        self.overflow_interrupt_enabled = value.bit(1);
        self.clear_counter_on_a_match = value.bit(0);

        log::trace!("TIER write: {value:02X}");
        log::trace!("  Compare match A interrupt enabled: {}", self.compare_a_interrupt_enabled);
        log::trace!("  Compare match B interrupt enabled: {}", self.compare_b_interrupt_enabled);
        log::trace!("  Overflow interrupt enabled: {}", self.overflow_interrupt_enabled);
        log::trace!("  Clear counter on compare match A: {}", self.clear_counter_on_a_match);
    }

    // $FFFFFE11: FTCSR (Free-running timer control/status register)
    fn write_ftcsr(&mut self, value: u8) {
        self.compare_a_flag &= value.bit(3);
        self.compare_b_flag &= value.bit(2);
        self.overflow_flag &= value.bit(1);
        self.clear_counter_on_a_match = value.bit(0);

        log::trace!("FTCSR write: {value:02X}");
        log::trace!("  Compare A flag clear: {}", !value.bit(3));
        log::trace!("  Compare B flag clear: {}", !value.bit(2));
        log::trace!("  Overflow flag clear: {}", !value.bit(1));
        log::trace!("  Clear counter on compare match A: {}", self.clear_counter_on_a_match);
    }

    // $FFFFFE12-$FFFFFE13: FRC (Free-running counter)
    fn write_frc(&mut self, value: u8) {
        self.counter = (self.temp_register & 0xFF00) | u16::from(value);
        log::trace!("FRC write: {value:02X}");
        log::trace!("  Counter: {:04X}", self.counter);
    }

    // $FFFFFE14-$FFFFFE15: OCRA/B (Output compare register A/B)
    fn write_ocr(&mut self, value: u8) {
        let word = (self.temp_register & 0xFF00) | u16::from(value);

        match self.compare_register {
            CompareRegister::A => {
                self.compare_a = word;
                log::trace!("Compare match A value: {word:04X}");
            }
            CompareRegister::B => {
                self.compare_b = word;
                log::trace!("Compare match B value: {word:04X}");
            }
        }
    }

    // $FFFFFE16: TCR (Timer control register)
    fn write_tcr(&mut self, value: u8) {
        self.clock_source = ClockSource::from_byte(value);

        if self.clock_source == ClockSource::External {
            log::warn!("Timer clock source set to external clock; not implemented");
        }

        log::trace!("TCR write: {value:02X}");
        log::trace!("  Clock source: {:?}", self.clock_source);
        log::trace!("  Input edge select: {}", if value.bit(7) { "Rising" } else { "Falling" });
    }

    // $FFFFFE17: TOCR (Timer output compare control register)
    fn read_tocr(&self) -> u8 {
        0xE0 | ((self.compare_register as u8) << 4)
            | (u8::from(self.compare_a_pin_output) << 1)
            | u8::from(self.compare_b_pin_output)
    }

    // $FFFFFE17: TOCR (Timer output compare control register)
    fn write_tocr(&mut self, value: u8) {
        self.compare_register = CompareRegister::from_bit(value.bit(4));
        self.compare_a_pin_output = value.bit(1);
        self.compare_b_pin_output = value.bit(0);

        log::trace!("TOCR write: {value:02X}");
        log::trace!("  Compare match register: {:?}", self.compare_register);
        log::trace!("  Compare A output pin level: {}", self.compare_a_pin_output);
        log::trace!("  Compare B output pin level: {}", self.compare_b_pin_output);
    }
}
