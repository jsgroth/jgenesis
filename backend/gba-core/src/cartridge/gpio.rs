//! Cartridge GPIO port
//!
//! Used to communicate with any cartridge peripherals (RTC, solar sensor, gyro sensor, etc.)

use crate::cartridge::rtc::{RtcWrite, SeikoRealTimeClock};
use crate::cartridge::solar::{SolarSensor, SolarSensorWrite};
use bincode::{Decode, Encode};
use jgenesis_common::define_bit_enum;
use jgenesis_common::num::GetBit;
use std::array;

define_bit_enum!(GpioPinDirection, [Input, Output]);
define_bit_enum!(GpioMode, [WriteOnly, ReadWrite]);

#[derive(Debug, Clone, Encode, Decode)]
pub struct GpioPort {
    pin_directions: [GpioPinDirection; 4],
    mode: GpioMode,
}

impl GpioPort {
    pub fn new() -> Self {
        Self { pin_directions: [GpioPinDirection::default(); 4], mode: GpioMode::default() }
    }

    // $080000C4
    pub fn write_data(
        &mut self,
        value: u16,
        rtc: Option<&mut SeikoRealTimeClock>,
        solar: Option<&mut SolarSensor>,
    ) {
        let pin = |i: u8| match self.pin_directions[i as usize] {
            GpioPinDirection::Input => true,
            GpioPinDirection::Output => value.bit(i),
        };

        log::trace!("GPIO data write: {:04b}", value & 0b1111);

        if let Some(rtc) = rtc {
            rtc.write(RtcWrite { chip_select: pin(2), clock: pin(0), data: pin(1) });
        }

        if let Some(solar) = solar {
            solar.write(SolarSensorWrite { reset: pin(1), clock: pin(0) });
        }
    }

    // $080000C4
    pub fn read_data(
        &self,
        rtc: Option<&SeikoRealTimeClock>,
        solar: Option<&SolarSensor>,
    ) -> Option<u16> {
        if self.mode == GpioMode::WriteOnly {
            return None;
        }

        let data1 = match self.pin_directions[1] {
            GpioPinDirection::Input => rtc.is_none_or(SeikoRealTimeClock::read),
            GpioPinDirection::Output => true,
        };

        let data3 = match self.pin_directions[3] {
            GpioPinDirection::Input => solar.is_none_or(SolarSensor::read),
            GpioPinDirection::Output => true,
        };

        Some(0b0101 | (u16::from(data1) << 1) | (u16::from(data3) << 3))
    }

    // $080000C6
    pub fn write_pin_directions(&mut self, value: u16) {
        self.pin_directions = array::from_fn(|i| GpioPinDirection::from_bit(value.bit(i as u8)));

        log::trace!("GPIO pin directions write ({value:04X}): {:?}", self.pin_directions);
    }

    // $080000C6
    pub fn read_pin_directions(&self) -> Option<u16> {
        if self.mode == GpioMode::WriteOnly {
            return None;
        }

        let value = self
            .pin_directions
            .into_iter()
            .enumerate()
            .map(|(i, direction)| (direction as u16) << i)
            .reduce(|a, b| a | b)
            .unwrap();

        Some(value)
    }

    // $080000C8
    pub fn write_mode(&mut self, value: u16) {
        self.mode = GpioMode::from_bit(value.bit(0));

        log::debug!("GPIO mode write ({value:04X}): {:?}", self.mode);
    }

    // $080000C8
    pub fn read_mode(&self) -> Option<u16> {
        if self.mode == GpioMode::WriteOnly {
            return None;
        }

        Some(self.mode as u16)
    }
}
