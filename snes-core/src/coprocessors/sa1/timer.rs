use bincode::{Decode, Encode};
use jgenesis_common::frontend::TimingMode;
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum TimerMode {
    #[default]
    HV,
    Linear,
}

impl TimerMode {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Linear } else { Self::HV }
    }

    const fn max_h(self) -> u16 {
        match self {
            Self::HV => 341,
            Self::Linear => 512,
        }
    }

    fn max_v(self, timing_mode: TimingMode) -> u16 {
        match (self, timing_mode) {
            (Self::HV, TimingMode::Ntsc) => 262,
            (Self::HV, TimingMode::Pal) => 312,
            (Self::Linear, _) => 512,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum TimerIrqMode {
    #[default]
    Off,
    H,
    V,
    HV,
}

impl TimerIrqMode {
    fn from_byte(byte: u8) -> Self {
        match byte & 0x03 {
            0x00 => Self::Off,
            0x01 => Self::H,
            0x02 => Self::V,
            0x03 => Self::HV,
            _ => unreachable!("value & 0x03 is always <= 0x03"),
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Sa1Timer {
    pub mode: TimerMode,
    pub irq_mode: TimerIrqMode,
    pub timing_mode: TimingMode,
    pub h_cpu_ticks: u16,
    pub v: u16,
    pub max_h_cpu_ticks: u16,
    pub max_v: u16,
    pub irq_htime_cpu_ticks: u16,
    pub irq_vtime: u16,
    pub irq_pending: bool,
    pub latched_h: u16,
    pub latched_v: u16,
}

impl Sa1Timer {
    pub fn new(timing_mode: TimingMode) -> Self {
        Self {
            mode: TimerMode::default(),
            irq_mode: TimerIrqMode::default(),
            timing_mode,
            h_cpu_ticks: 0,
            v: 0,
            max_h_cpu_ticks: TimerMode::default().max_h() << 1,
            max_v: TimerMode::default().max_v(timing_mode),
            irq_htime_cpu_ticks: 0,
            irq_vtime: 0,
            irq_pending: false,
            latched_h: 0,
            latched_v: 0,
        }
    }

    pub fn read_hcr_low(&mut self) -> u8 {
        // Reading HCR low byte latches both H and V
        self.latched_h = self.h_cpu_ticks >> 1;
        self.latched_v = self.v;

        self.latched_h as u8
    }

    pub fn read_hcr_high(&self) -> u8 {
        (self.latched_h >> 8) as u8
    }

    pub fn read_vcr_low(&self) -> u8 {
        self.latched_v as u8
    }

    pub fn read_vcr_high(&self) -> u8 {
        (self.latched_v >> 8) as u8
    }

    pub fn write_tmc(&mut self, value: u8) {
        self.mode = TimerMode::from_bit(value.bit(7));
        self.irq_mode = TimerIrqMode::from_byte(value);

        if self.irq_mode == TimerIrqMode::Off {
            self.irq_pending = false;
        }

        self.max_h_cpu_ticks = self.mode.max_h() << 1;
        self.max_v = self.mode.max_v(self.timing_mode);

        log::trace!("  H/V timer mode: {:?}", self.mode);
        log::trace!("  H/V IRQ mode: {:?}", self.irq_mode);
    }

    pub fn write_hcnt_low(&mut self, value: u8) {
        let htime = ((self.irq_htime_cpu_ticks >> 1) & 0x100) | u16::from(value);
        self.irq_htime_cpu_ticks = (htime << 1) | (self.irq_htime_cpu_ticks & 0x1);

        log::trace!("  IRQ HTIME: {}", self.irq_htime_cpu_ticks >> 1);
    }

    pub fn write_hcnt_high(&mut self, value: u8) {
        let htime = ((self.irq_htime_cpu_ticks >> 1) & 0x0FF) | (u16::from(value & 0x01) << 8);
        self.irq_htime_cpu_ticks = (htime << 1) | (self.irq_htime_cpu_ticks & 0x1);

        log::trace!("  IRQ HTIME: {}", self.irq_htime_cpu_ticks >> 1);
    }

    pub fn write_vcnt_low(&mut self, value: u8) {
        self.irq_vtime = (self.irq_vtime & 0x100) | u16::from(value);

        log::trace!("  IRQ VTIME: {}", self.irq_vtime);
    }

    pub fn write_vcnt_high(&mut self, value: u8) {
        self.irq_vtime = (self.irq_vtime & 0x0FF) | (u16::from(value & 0x01) << 8);

        log::trace!("  IRQ VTIME: {}", self.irq_vtime);
    }

    pub fn tick(&mut self) {
        self.h_cpu_ticks += 1;

        if self.h_cpu_ticks >= self.max_h_cpu_ticks {
            self.h_cpu_ticks -= self.max_h_cpu_ticks;
            self.v += 1;

            if self.v >= self.max_v {
                self.v = 0;
            }

            if self.irq_mode == TimerIrqMode::V && self.v == self.irq_vtime {
                self.irq_pending = true;
            }
        }

        match self.irq_mode {
            TimerIrqMode::H if self.h_cpu_ticks == self.irq_htime_cpu_ticks => {
                self.irq_pending = true;
            }
            TimerIrqMode::HV
                if self.h_cpu_ticks == self.irq_htime_cpu_ticks && self.v == self.irq_vtime =>
            {
                self.irq_pending = true;
            }
            _ => {}
        }
    }

    pub fn reset(&mut self) {
        self.h_cpu_ticks = 0;
        self.v = 0;
    }
}
