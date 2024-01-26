use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum CpuSpeed {
    #[default]
    Normal,
    Double,
}

impl CpuSpeed {
    fn to_bit(self) -> bool {
        self == Self::Double
    }

    fn toggle(self) -> Self {
        match self {
            Self::Normal => Self::Double,
            Self::Double => Self::Normal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct SpeedRegister {
    pub speed: CpuSpeed,
    pub switch_armed: bool,
    pub double_speed_odd_cycle: bool,
}

impl SpeedRegister {
    pub fn new() -> Self {
        Self { speed: CpuSpeed::default(), switch_armed: false, double_speed_odd_cycle: false }
    }

    pub fn read_key1(self) -> u8 {
        0x7E | (u8::from(self.speed.to_bit()) << 7) | u8::from(self.switch_armed)
    }

    pub fn write_key1(&mut self, value: u8) {
        self.switch_armed = value.bit(0);

        log::trace!("KEY1 write: {value:02X}");
    }

    pub fn perform_speed_switch(&mut self) {
        // TODO implement speed switch delay?

        self.speed = self.speed.toggle();
        self.switch_armed = false;
        self.double_speed_odd_cycle = false;

        log::trace!("Speed changed to {:?}", self.speed);
    }
}
