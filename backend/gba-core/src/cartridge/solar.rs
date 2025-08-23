//! Solar sensor peripheral
//!
//! Used by the Boktai games

use bincode::{Decode, Encode};

pub struct SolarSensorWrite {
    pub reset: bool,
    pub clock: bool,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SolarSensor {
    brightness: u8,
    counter: u8,
    prev_clock: bool,
}

impl SolarSensor {
    pub fn new() -> Self {
        Self {
            brightness: gba_config::DEFAULT_SOLAR_MIN_BRIGHTNESS,
            counter: 255,
            prev_clock: false,
        }
    }

    pub fn set_brightness(&mut self, brightness: u8) {
        self.brightness = brightness;
    }

    pub fn read(&self) -> bool {
        self.counter < self.brightness
    }

    pub fn write(&mut self, SolarSensorWrite { reset, clock }: SolarSensorWrite) {
        if reset {
            self.counter = 255;
            self.prev_clock = clock;
            return;
        }

        if !self.prev_clock && clock {
            self.counter = self.counter.saturating_sub(1);
        }
        self.prev_clock = clock;
    }
}
