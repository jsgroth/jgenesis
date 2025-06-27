use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum CpuSpeed {
    #[default]
    Normal = 0,
    Double = 1,
}

impl CpuSpeed {
    fn toggle(self) -> Self {
        match self {
            Self::Normal => Self::Double,
            Self::Double => Self::Normal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum ObjPriority {
    // CGB behavior
    #[default]
    OamIndex = 0,
    // DMG behavior
    XCoordinate = 1,
}

impl ObjPriority {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::XCoordinate } else { Self::OamIndex }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct CgbRegisters {
    pub speed: CpuSpeed,
    pub speed_switch_armed: bool,
    pub double_speed_odd_cycle: bool,
    pub obj_priority: ObjPriority,
    pub dmg_compatibility: bool,
}

impl CgbRegisters {
    pub fn new() -> Self {
        Self {
            speed: CpuSpeed::default(),
            speed_switch_armed: false,
            double_speed_odd_cycle: false,
            obj_priority: ObjPriority::default(),
            dmg_compatibility: false,
        }
    }

    pub fn write_key0(&mut self, value: u8) {
        self.dmg_compatibility = value.bit(2);

        log::trace!(
            "KEY0 write: {value:02X} (DMG compatibility mode = {})",
            self.dmg_compatibility
        );
    }

    pub fn read_key1(self) -> u8 {
        0x7E | ((self.speed as u8) << 7) | u8::from(self.speed_switch_armed)
    }

    pub fn write_key1(&mut self, value: u8) {
        self.speed_switch_armed = value.bit(0);

        log::trace!("KEY1 write: {value:02X} (speed switch armed = {})", self.speed_switch_armed);
    }

    pub fn perform_speed_switch(&mut self) {
        // TODO implement speed switch delay?

        self.speed = self.speed.toggle();
        self.speed_switch_armed = false;
        self.double_speed_odd_cycle = false;

        log::trace!("Speed changed to {:?}", self.speed);
    }

    pub fn read_opri(self) -> u8 {
        0xFE | (self.obj_priority as u8)
    }

    pub fn write_opri(&mut self, value: u8) {
        self.obj_priority = ObjPriority::from_bit(value.bit(0));

        log::trace!("OPRI write: {value:02X} (OBJ priority = {:?})", self.obj_priority);
    }
}
