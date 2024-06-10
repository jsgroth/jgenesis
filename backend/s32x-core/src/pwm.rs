//! 32X PWM sound chip

use crate::registers::SystemRegisters;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum OutputDirection {
    #[default]
    Off = 0,
    Same = 1,
    Opposite = 2,
    Prohibited = 3,
}

impl OutputDirection {
    fn from_value(value: u16) -> Self {
        match value & 3 {
            0 => Self::Off,
            1 => Self::Same,
            2 => Self::Opposite,
            3 => Self::Prohibited,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }

    fn is_off(self) -> bool {
        matches!(self, Self::Off | Self::Prohibited)
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct PwmControl {
    pub timer_interval: u16,
    pub dreq1_enabled: bool,
    pub l_out: OutputDirection,
    pub r_out: OutputDirection,
}

impl PwmControl {
    fn new() -> Self {
        Self {
            timer_interval: 0,
            dreq1_enabled: false,
            l_out: OutputDirection::default(),
            r_out: OutputDirection::default(),
        }
    }

    fn effective_timer_interval(&self) -> u16 {
        if self.timer_interval == 0 { 16 } else { self.timer_interval }
    }

    // 68000: $A15130
    // SH-2: $4030
    fn read(&self) -> u16 {
        (self.timer_interval << 8)
            | (u16::from(self.dreq1_enabled) << 7)
            | ((self.r_out as u16) << 2)
            | (self.l_out as u16)
    }

    // 68000: $A15130
    fn m68k_write(&mut self, value: u16) {
        self.timer_interval = (value >> 8) & 0xF;
        self.r_out = OutputDirection::from_value(value >> 2);
        self.l_out = OutputDirection::from_value(value);
        // M68K cannot change RTP / DREQ1 enable

        log::debug!("PWM control write: {value:04X}");
        log::debug!("  Effective timer interval: {}", self.effective_timer_interval());
        log::debug!("  L channel output direction: {:?}", self.l_out);
        log::debug!("  R channel output direction: {:?}", self.r_out);
    }

    // SH-2: $4030
    fn sh2_write(&mut self, value: u16) {
        self.timer_interval = (value >> 8) & 0xF;
        self.dreq1_enabled = value.bit(7);
        self.r_out = OutputDirection::from_value(value >> 2);
        self.l_out = OutputDirection::from_value(value);

        log::debug!("PWM control write: {value:04X}");
        log::debug!("  Effective timer interval: {}", self.effective_timer_interval());
        log::debug!("  DREQ1 enabled: {}", self.dreq1_enabled);
        log::debug!("  L channel output direction: {:?}", self.l_out);
        log::debug!("  R channel output direction: {:?}", self.r_out);
    }
}

const FIFO_LEN: usize = 3;

#[derive(Debug, Clone, Encode, Decode)]
pub struct PwmFifo(VecDeque<u16>);

impl PwmFifo {
    pub fn new() -> Self {
        Self(VecDeque::with_capacity(FIFO_LEN))
    }

    pub fn push(&mut self, sample: u16) {
        if self.0.len() == FIFO_LEN {
            self.0.pop_front();
        }
        self.0.push_back(sample);
    }

    fn pop(&mut self) -> Option<u16> {
        self.0.pop_front()
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn is_full(&self) -> bool {
        self.0.len() == FIFO_LEN
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct PwmChip {
    pub control: PwmControl,
    pub cycle_register: u16,
    l_fifo: PwmFifo,
    r_fifo: PwmFifo,
    l_output: u16,
    r_output: u16,
    cycle_counter: u64,
    timer_counter: u16,
}

// Cycle register and pulse width are unsigned 12-bit values
const U12_MASK: u16 = (1 << 12) - 1;

macro_rules! impl_write_register {
    ($name:ident, $control_write_method:ident) => {
        pub fn $name(&mut self, address: u32, value: u16) {
            match address & 0xF {
                0x0 => self.control.$control_write_method(value),
                0x2 => self.write_cycle_register(value),
                0x4 => self.write_l_fifo(value),
                0x6 => self.write_r_fifo(value),
                0x8 => self.write_mono_fifo(value),
                _ => todo!("PWM register write {address:08X} {value:04X}"),
            }
        }
    };
}

impl PwmChip {
    pub fn new() -> Self {
        Self {
            control: PwmControl::new(),
            cycle_register: 0,
            l_fifo: PwmFifo::new(),
            r_fifo: PwmFifo::new(),
            l_output: U12_MASK,
            r_output: U12_MASK,
            timer_counter: 16,
            cycle_counter: U12_MASK.into(),
        }
    }

    pub fn tick(&mut self, mut sh2_cycles: u64, system_registers: &mut SystemRegisters) {
        if self.control.l_out.is_off() && self.control.r_out.is_off() {
            // PWM counters are stopped when both channels are off
            return;
        }

        while sh2_cycles != 0 {
            let prev_cycle_counter = self.cycle_counter;
            self.cycle_counter = self.cycle_counter.saturating_sub(sh2_cycles);
            sh2_cycles -= prev_cycle_counter - self.cycle_counter;

            if self.cycle_counter == 0 {
                // Cycle counter is always set to (register - 1), wrapping from 0 to 4095
                self.cycle_counter = (self.cycle_register.wrapping_sub(1) & U12_MASK).into();

                // TODO only pop one FIFO when only one channel is on?
                // TODO play sound
                self.l_output = self.l_fifo.pop().unwrap_or(self.l_output);
                self.r_output = self.r_fifo.pop().unwrap_or(self.r_output);

                self.timer_counter -= 1;
                if self.timer_counter == 0 {
                    self.timer_counter = self.control.effective_timer_interval();

                    log::trace!("Generating PWM interrupt");
                    system_registers.notify_pwm_timer();

                    if self.control.dreq1_enabled {
                        todo!("generate PWM DREQ1 for SH-2s")
                    }
                }
            }
        }
    }

    pub fn read_register(&self, address: u32) -> u16 {
        match address & 0xF {
            0x0 => self.control.read(),
            0x2 => self.cycle_register,
            0x4 => self.read_l_fifo_status(),
            0x6 => self.read_r_fifo_status(),
            _ => todo!("PWM register read {address:08X}"),
        }
    }

    impl_write_register!(m68k_write_register, m68k_write);
    impl_write_register!(sh2_write_register, sh2_write);

    // 68000: $A15132
    // SH-2: $4032
    fn write_cycle_register(&mut self, value: u16) {
        self.cycle_register = value & U12_MASK;

        log::debug!("Cycle register write: {value:04X}");
        log::debug!(
            "  Effective sample rate: {} Hz",
            53_693_175.0 * 3.0 / 7.0 / f64::from(self.cycle_register.wrapping_sub(1) & U12_MASK)
        );
    }

    // 68000: $A15134
    // SH-2: $4034
    fn read_l_fifo_status(&self) -> u16 {
        (u16::from(self.l_fifo.is_full()) << 15) | (u16::from(self.l_fifo.is_empty()) << 14)
    }

    // 68000: $A15134
    // SH-2: $4034
    fn write_l_fifo(&mut self, value: u16) {
        let sample = value.wrapping_sub(1) & U12_MASK;
        self.l_fifo.push(sample);

        log::trace!("L pulse width FIFO write: {value:04X}");
        log::trace!("  Effective wave height: {sample}");
    }

    // 68000: $A15136
    // SH-2: $4036
    fn read_r_fifo_status(&self) -> u16 {
        (u16::from(self.r_fifo.is_full()) << 15) | (u16::from(self.r_fifo.is_empty()) << 14)
    }

    // 68000: $A15136
    // SH-2: $4036
    fn write_r_fifo(&mut self, value: u16) {
        let sample = value.wrapping_sub(1) & U12_MASK;
        self.r_fifo.push(sample);

        log::trace!("R pulse width FIFO write: {value:04X}");
        log::trace!("  Effective wave height: {sample}");
    }

    // 68000: $A15138
    // SH-2: $4038
    fn write_mono_fifo(&mut self, value: u16) {
        let sample = value.wrapping_sub(1) & U12_MASK;
        self.l_fifo.push(sample);
        self.r_fifo.push(sample);

        log::trace!("Mono pulse width FIFO write: {value:04X}");
        log::trace!("  Effective wave height: {sample}");
    }
}
