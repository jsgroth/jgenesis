use crate::interrupts::InterruptRegisters;
use crate::sm83::InterruptType;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum ClockSelect {
    Zero,
    One,
    Two,
    Three,
}

impl ClockSelect {
    fn from_byte(byte: u8) -> Self {
        match byte & 0x3 {
            0x0 => Self::Zero,
            0x1 => Self::One,
            0x2 => Self::Two,
            0x3 => Self::Three,
            _ => unreachable!("value & 0x3 is always <= 0x3"),
        }
    }

    fn to_bits(self) -> u8 {
        match self {
            Self::Zero => 0,
            Self::One => 1,
            Self::Two => 2,
            Self::Three => 3,
        }
    }

    fn timer_bit(self) -> u8 {
        match self {
            // 4 KHz
            Self::Zero => 9,
            // 256 KHz
            Self::One => 3,
            // 64 KHz
            Self::Two => 5,
            // 16 KHz
            Self::Three => 7,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GbTimer {
    timer: u16,
    enabled: bool,
    counter: u8,
    modulo: u8,
    clock_select: ClockSelect,
    previous_timer_bit: bool,
    overflow: bool,
}

impl GbTimer {
    pub fn new() -> Self {
        Self {
            timer: 0,
            enabled: false,
            counter: 0,
            modulo: 0,
            clock_select: ClockSelect::Zero,
            previous_timer_bit: false,
            overflow: false,
        }
    }

    pub fn tick_m_cycle(&mut self, interrupt_registers: &mut InterruptRegisters) {
        // Full 16-bit timer always ticks, even when the timer is disabled
        self.timer = self.timer.wrapping_add(4);

        if !self.enabled {
            return;
        }

        // Reset counter and flag interrupt if counter increment overflowed on the last M-cycle
        if self.overflow {
            self.counter = self.modulo;
            interrupt_registers.set_flag(InterruptType::Timer);
            self.overflow = false;

            return;
        }

        self.check_for_counter_increment();
    }

    fn check_for_counter_increment(&mut self) {
        let counter_bit = self.timer.bit(self.clock_select.timer_bit());
        if self.previous_timer_bit && !counter_bit {
            let (new_counter, overflow) = self.counter.overflowing_add(1);
            self.counter = new_counter;
            self.overflow = overflow;
        }

        self.previous_timer_bit = counter_bit;
    }

    pub fn write_div(&mut self) {
        // Writing any value resets the timer to 0
        self.timer = 0;

        self.check_for_counter_increment();
    }

    // DIV: Divider
    pub fn read_div(&self) -> u8 {
        // DIV reads out as the highest 8 bits of the internal timer
        (self.timer >> 8) as u8
    }

    // TIMA: Timer counter
    pub fn write_tima(&mut self, value: u8) {
        self.counter = value;
    }

    pub fn read_tima(&self) -> u8 {
        self.counter
    }

    // TMA: Timer modulo
    pub fn write_tma(&mut self, value: u8) {
        self.modulo = value;
    }

    pub fn read_tma(&self) -> u8 {
        self.modulo
    }

    // TAC: Timer control
    pub fn write_tac(&mut self, value: u8) {
        self.enabled = value.bit(2);
        self.clock_select = ClockSelect::from_byte(value);

        self.check_for_counter_increment();
    }

    pub fn read_tac(&self) -> u8 {
        0xF8 | (u8::from(self.enabled) << 2) | self.clock_select.to_bits()
    }
}
