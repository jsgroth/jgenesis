use bincode::{Decode, Encode};
use jgenesis_traits::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum StampSizeDots {
    #[default]
    Sixteen,
    ThirtyTwo,
}

impl StampSizeDots {
    fn to_bit(self) -> bool {
        self == Self::ThirtyTwo
    }

    fn from_bit(bit: bool) -> Self {
        if bit { Self::ThirtyTwo } else { Self::Sixteen }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum StampMapSizeScreens {
    #[default]
    One,
    Sixteen,
}

impl StampMapSizeScreens {
    fn to_bit(self) -> bool {
        self == Self::Sixteen
    }

    fn from_bit(bit: bool) -> Self {
        if bit { Self::Sixteen } else { Self::One }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum GraphicsWritePriorityMode {
    Off,
    Underwrite,
    Overwrite,
}

impl GraphicsWritePriorityMode {
    pub fn to_bits(self) -> u8 {
        match self {
            Self::Off => 0x00,
            Self::Underwrite => 0x01,
            Self::Overwrite => 0x02,
        }
    }

    pub fn from_bits(bits: u8) -> Self {
        match bits & 0x03 {
            0x00 | 0x03 => Self::Off,
            0x01 => Self::Underwrite,
            0x02 => Self::Overwrite,
            _ => unreachable!("value & 0x03 is always <= 0x03"),
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GraphicsCoprocessor {
    stamp_size: StampSizeDots,
    stamp_map_size: StampMapSizeScreens,
    stamp_map_repeats: bool,
    stamp_map_base_address: u32,
    write_priority_mode: GraphicsWritePriorityMode,
    image_buffer_v_cell_size: u32,
    image_buffer_start_address: u32,
    image_buffer_v_offset: u32,
    image_buffer_h_offset: u32,
    image_buffer_v_dot_size: u32,
    image_buffer_h_dot_size: u32,
    trace_vector_base_address: u32,
}

impl GraphicsCoprocessor {
    pub fn new() -> Self {
        Self {
            stamp_size: StampSizeDots::default(),
            stamp_map_size: StampMapSizeScreens::default(),
            stamp_map_repeats: false,
            stamp_map_base_address: 0,
            write_priority_mode: GraphicsWritePriorityMode::Off,
            image_buffer_v_cell_size: 1,
            image_buffer_start_address: 0,
            image_buffer_v_offset: 0,
            image_buffer_h_offset: 0,
            image_buffer_v_dot_size: 0,
            image_buffer_h_dot_size: 0,
            trace_vector_base_address: 0,
        }
    }

    #[allow(clippy::match_same_arms)]
    pub fn read_register_byte(&self, address: u32) -> u8 {
        match address {
            0xFF8058 => {
                // Stamp data size, high byte (in progress bit)
                // TODO in progress bit
                0x00
            }
            0xFF8059 => {
                // Stamp data size, low byte
                (u8::from(self.stamp_map_size.to_bit()) << 2)
                    | (u8::from(self.stamp_size.to_bit()) << 1)
                    | u8::from(self.stamp_map_repeats)
            }
            0xFF805A => {
                // Stamp map base address, high byte
                (self.stamp_map_base_address >> 10) as u8
            }
            0xFF805B => {
                // Stamp map base address, low byte
                (self.stamp_map_base_address >> 2) as u8
            }
            0xFF805D => {
                // Image buffer V cell size (minus one)
                (self.image_buffer_v_cell_size - 1) as u8
            }
            0xFF805E => {
                // Image buffer start address, high byte
                (self.image_buffer_start_address >> 10) as u8
            }
            0xFF805F => {
                // Image buffer start address, low byte
                (self.image_buffer_start_address >> 2) as u8
            }
            0xFF8061 => {
                // Image buffer offset
                (((self.image_buffer_v_offset) << 3) | self.image_buffer_h_offset) as u8
            }
            0xFF8062 => {
                // Image buffer H dot size, high byte
                (self.image_buffer_h_dot_size >> 8) as u8
            }
            0xFF8063 => {
                // Image buffer H dot size, low byte
                self.image_buffer_h_dot_size as u8
            }
            0xFF8065 => {
                // Image buffer V dot size
                self.image_buffer_v_dot_size as u8
            }
            _ => 0x00,
        }
    }

    #[allow(clippy::match_same_arms)]
    pub fn read_register_word(&self, address: u32) -> u16 {
        match address {
            0xFF8058 => {
                // Stamp data size
                u16::from_be_bytes([
                    self.read_register_byte(address),
                    self.read_register_byte(address | 1),
                ])
            }
            0xFF805A => {
                // Stamp map base address
                (self.stamp_map_base_address >> 2) as u16
            }
            0xFF805C => {
                // Image buffer V cell size (low byte only)
                self.read_register_byte(address | 1).into()
            }
            0xFF805E => {
                // Image buffer start address
                (self.image_buffer_start_address >> 2) as u16
            }
            0xFF8060 => {
                // Image buffer offset (low byte only)
                self.read_register_byte(address | 1).into()
            }
            0xFF8062 => {
                // Image buffer H dot size
                self.image_buffer_h_dot_size as u16
            }
            0xFF8064 => {
                // Image buffer V dot size (low byte only)
                self.read_register_byte(address | 1).into()
            }
            _ => 0x0000,
        }
    }

    #[allow(clippy::match_same_arms)]
    pub fn write_register_byte(&mut self, address: u32, value: u8) {
        match address {
            0xFF8059 => {
                // Stamp data size
                self.stamp_map_size = StampMapSizeScreens::from_bit(value.bit(2));
                self.stamp_size = StampSizeDots::from_bit(value.bit(1));
                self.stamp_map_repeats = value.bit(0);
            }
            0xFF805A..=0xFF805B => {
                // Stamp map base address (word access only)
                self.write_register_word(address & !1, u16::from_le_bytes([value, value]));
            }
            0xFF805D => {
                // Image buffer V cell size (minus one)
                self.image_buffer_v_cell_size = ((value & 0x1F) + 1).into();
            }
            0xFF805E..=0xFF805F => {
                // Image buffer start address (word access only)
                self.write_register_word(address & !1, u16::from_le_bytes([value, value]));
            }
            0xFF8061 => {
                // Image buffer offset
                self.image_buffer_v_offset = u32::from(value >> 3) & 0x07;
                self.image_buffer_h_offset = (value & 0x07).into();
            }
            0xFF8062..=0xFF8063 => {
                // Image buffer H dot size (word access only)
                self.write_register_word(address & !1, u16::from_le_bytes([value, value]));
            }
            0xFF8064..=0xFF8065 => {
                // Image buffer V dot size (word access only)
                self.write_register_word(address & !1, u16::from_le_bytes([value, value]));
            }
            0xFF8066..=0xFF8067 => {
                // Trace vector base address (word access only)
                self.write_register_word(address & !1, u16::from_le_bytes([value, value]));
            }
            _ => {}
        }
    }

    #[allow(clippy::match_same_arms)]
    pub fn write_register_word(&mut self, address: u32, value: u16) {
        match address {
            0xFF8058 => {
                // Stamp data size (only low byte is writable)
                self.write_register_byte(address | 1, value as u8);
            }
            0xFF805A => {
                // Stamp map base address (bits 17-7)
                self.stamp_map_base_address = u32::from(value & 0xFFE0) << 2;
            }
            0xFF805C => {
                // Image buffer V cell size (only low byte is writable)
                self.write_register_byte(address | 1, value as u8);
            }
            0xFF805E => {
                // Image buffer start address (bits 17-5)
                self.image_buffer_start_address = u32::from(value & 0xFFF8) << 2;
            }
            0xFF8060 => {
                // Image buffer offset (only low byte is writable)
                self.write_register_byte(address | 1, value as u8);
            }
            0xFF8062 => {
                // Image buffer H dot size
                self.image_buffer_h_dot_size = (value & 0x01FF).into();
            }
            0xFF8064 => {
                // Image buffer V dot size
                self.image_buffer_v_dot_size = (value & 0x00FF).into();
            }
            0xFF8066 => {
                // Trace vector base address / begin graphics operation
                self.trace_vector_base_address = u32::from(value & 0xFFFE) << 2;
                // TODO begin graphics operation
            }
            _ => {}
        }
    }

    pub fn write_priority_mode(&self) -> GraphicsWritePriorityMode {
        self.write_priority_mode
    }

    pub fn set_write_priority_mode(&mut self, write_priority_mode: GraphicsWritePriorityMode) {
        self.write_priority_mode = write_priority_mode;
    }
}
