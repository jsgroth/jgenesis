use bincode::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
pub struct Timer<const MCLK_DIVIDER: u8> {
    enabled: bool,
    mclk_divider: u8,
    timer_divider: u16,
    timer_counter: u16,
    timer_output: u8,
}

impl<const MCLK_DIVIDER: u8> Timer<MCLK_DIVIDER> {
    pub fn new() -> Self {
        Self {
            enabled: false,
            mclk_divider: MCLK_DIVIDER,
            timer_divider: 255,
            timer_counter: 0,
            timer_output: 0,
        }
    }

    pub fn tick(&mut self) {
        self.mclk_divider -= 1;
        if self.mclk_divider == 0 {
            self.mclk_divider = MCLK_DIVIDER;
            self.clock();
        }
    }

    fn clock(&mut self) {
        self.timer_counter += 1;
        if self.timer_counter >= self.timer_divider {
            self.timer_counter = 0;
            self.timer_output = self.timer_output.wrapping_add(1);
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.timer_counter = 0;
            self.timer_output = 0;
        }
    }

    pub fn divider(&self) -> u8 {
        if self.timer_divider == 256 { 0 } else { self.timer_divider as u8 }
    }

    pub fn set_divider(&mut self, divider: u8) {
        self.timer_divider = if divider == 0 { 256 } else { divider.into() };
    }

    pub fn read_output(&mut self) -> u8 {
        let output = self.timer_output & 0x0F;
        self.timer_output = 0;
        output
    }
}

pub type SlowTimer = Timer<128>;
pub type FastTimer = Timer<16>;
