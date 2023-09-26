mod fixedpoint;

use crate::graphics::fixedpoint::FixedPointDecimal;
use crate::memory::wordram;
use crate::memory::wordram::WordRam;
use bincode::{Decode, Encode};
use jgenesis_traits::num::GetBit;
use std::array;

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

    fn one_dimension_in_pixels(self) -> u32 {
        match self {
            Self::Sixteen => 16,
            Self::ThirtyTwo => 32,
        }
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

    fn one_dimension_in_pixels(self) -> u32 {
        // One "screen" is 256x256 pixels
        match self {
            Self::One => 256,
            Self::Sixteen => 4096,
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StampRotation {
    Zero,
    Ninety,
    OneEighty,
    TwoSeventy,
}

#[derive(Debug, Clone, Copy)]
struct StampData {
    stamp_number: u16,
    rotation: StampRotation,
    horizontal_flip: bool,
}

impl StampData {
    fn from_word(word: u16) -> Self {
        let horizontal_flip = word.bit(15);
        let rotation = match word & 0x6000 {
            0x0000 => StampRotation::Zero,
            0x2000 => StampRotation::Ninety,
            0x4000 => StampRotation::OneEighty,
            0x6000 => StampRotation::TwoSeventy,
            _ => unreachable!("value & 0x6000 is always 0x0000/0x2000/0x4000/0x6000"),
        };
        let stamp_number = word & 0x07FF;

        Self { stamp_number, rotation, horizontal_flip }
    }
}

#[derive(Debug, Clone)]
struct TraceVectorData {
    start_x: FixedPointDecimal,
    start_y: FixedPointDecimal,
    delta_x: FixedPointDecimal,
    delta_y: FixedPointDecimal,
}

impl TraceVectorData {
    fn from_bytes(bytes: [u8; 8]) -> Self {
        let start_x_word = u16::from_be_bytes([bytes[0], bytes[1]]);
        let start_y_word = u16::from_be_bytes([bytes[2], bytes[3]]);
        let delta_x_word = u16::from_be_bytes([bytes[4], bytes[5]]);
        let delta_y_word = u16::from_be_bytes([bytes[6], bytes[7]]);

        Self {
            start_x: FixedPointDecimal::from_position(start_x_word),
            start_y: FixedPointDecimal::from_position(start_y_word),
            delta_x: FixedPointDecimal::from_delta(delta_x_word),
            delta_y: FixedPointDecimal::from_delta(delta_y_word),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum State {
    Idle,
    Processing { mclk_cycles_remaining: u64, operation_performed: bool },
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
    state: State,
    interrupt_pending: bool,
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
            state: State::Idle,
            interrupt_pending: false,
        }
    }

    #[allow(clippy::match_same_arms)]
    pub fn read_register_byte(&self, address: u32) -> u8 {
        match address {
            0xFF8058 => {
                // Stamp data size, high byte (in progress bit)
                let in_progress = matches!(self.state, State::Processing { .. });
                u8::from(in_progress) << 7
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

                // Pretty much a complete guess on timing; the documentation is extremely unclear
                let estimated_mclk_cycles =
                    8 * self.image_buffer_v_dot_size * self.image_buffer_h_dot_size;
                self.state = State::Processing {
                    mclk_cycles_remaining: estimated_mclk_cycles.into(),
                    operation_performed: false,
                }
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

    pub fn tick(
        &mut self,
        mclk_cycles: u64,
        word_ram: &mut WordRam,
        graphics_interrupt_enabled: bool,
    ) {
        let State::Processing { mclk_cycles_remaining, operation_performed } = self.state else {
            return;
        };

        if !operation_performed {
            self.perform_graphics_operation(word_ram);
        }

        if mclk_cycles >= mclk_cycles_remaining {
            self.state = State::Idle;
            // In actual hardware V dot size is decremented as the operation goes; here, we're just
            // clearing at the end
            self.image_buffer_v_dot_size = 0;

            if graphics_interrupt_enabled {
                self.interrupt_pending = true;
            }
        } else {
            self.state = State::Processing {
                mclk_cycles_remaining: mclk_cycles_remaining - mclk_cycles,
                operation_performed: true,
            };
        }
    }

    pub fn interrupt_pending(&self) -> bool {
        self.interrupt_pending
    }

    pub fn acknowledge_interrupt(&mut self) {
        self.interrupt_pending = false;
    }

    fn perform_graphics_operation(&self, word_ram: &mut WordRam) {
        log::trace!("Beginning graphics operation with current state:\n{self:#X?}");

        let stamp_map_size = self.stamp_map_size;
        let stamp_map_dimension_pixels = stamp_map_size.one_dimension_in_pixels();
        let stamp_map_repeats = self.stamp_map_repeats;
        let stamp_size = self.stamp_size;

        let stamp_map_base_address = self.stamp_map_base_address_masked();
        let trace_vector_base_address = self.trace_vector_base_address;

        // 8 lines per cell
        let image_buffer_v_cell_size = self.image_buffer_v_cell_size;
        let image_buffer_line_size = 8 * image_buffer_v_cell_size;
        let image_buffer_v_dot_size = self.image_buffer_v_dot_size;
        let image_buffer_h_dot_size = self.image_buffer_h_dot_size;
        let image_buffer_h_offset = self.image_buffer_h_offset;

        let write_priority_mode = self.write_priority_mode;

        let mut image_buffer_start_address = self.image_buffer_start_address;
        let mut image_buffer_line = self.image_buffer_v_offset;
        for line in 0..image_buffer_v_dot_size {
            // One trace vector per line
            let trace_vector_address =
                (trace_vector_base_address + 8 * line) & wordram::ADDRESS_MASK;
            let trace_vector = TraceVectorData::from_bytes(array::from_fn(|i| {
                read_word_ram(word_ram, trace_vector_address + i as u32)
            }));

            log::trace!(
                "  line={line}, image_buffer_line={image_buffer_line}, trace_vector={trace_vector:08X?}"
            );

            let mut trace_x_position = trace_vector.start_x;
            let mut trace_y_position = trace_vector.start_y;
            for dot in 0..image_buffer_h_dot_size {
                let x = trace_x_position.integer_part();
                let y = trace_y_position.integer_part();
                let position_out_of_bounds =
                    x >= stamp_map_dimension_pixels || y >= stamp_map_dimension_pixels;

                let sample = if !stamp_map_repeats && position_out_of_bounds {
                    // Sampling outside of a non-repeating stamp map is always 0
                    0
                } else {
                    let stamp_map_addr = compute_stamp_map_address(
                        stamp_map_base_address,
                        stamp_size,
                        stamp_map_size,
                        x,
                        y,
                    );
                    let stamp = StampData::from_word(u16::from_be_bytes([
                        read_word_ram(word_ram, stamp_map_addr),
                        read_word_ram(word_ram, stamp_map_addr + 1),
                    ]));

                    sample_stamp(word_ram, stamp, stamp_size, x, y)
                };

                let image_buffer_dot = image_buffer_h_offset + dot;
                let image_buffer_addr = image_buffer_start_address
                    + compute_relative_addr_v_then_h(
                        image_buffer_line_size,
                        image_buffer_dot,
                        image_buffer_line,
                    );

                let current_byte = read_word_ram(word_ram, image_buffer_addr);
                let current_pixel =
                    if image_buffer_dot.bit(0) { current_byte & 0x0F } else { current_byte >> 4 };

                let should_write = match write_priority_mode {
                    GraphicsWritePriorityMode::Off => true,
                    GraphicsWritePriorityMode::Underwrite => current_pixel == 0,
                    GraphicsWritePriorityMode::Overwrite => sample != 0,
                };

                if should_write {
                    let new_pixel = if image_buffer_dot.bit(0) {
                        (current_byte & 0xF0) | sample
                    } else {
                        (current_byte & 0x0F) | (sample << 4)
                    };
                    write_word_ram(word_ram, image_buffer_addr, new_pixel);
                }

                trace_x_position += trace_vector.delta_x;
                trace_y_position += trace_vector.delta_y;
            }

            image_buffer_line += 1;
            if image_buffer_line == image_buffer_line_size {
                image_buffer_line = 0;

                // "Wrap" by shifting the image buffer start address right 1 cell
                let image_buffer_size_pixels = image_buffer_line_size * 8;
                image_buffer_start_address = (image_buffer_start_address
                    + image_buffer_size_pixels / 2)
                    & wordram::ADDRESS_MASK;
            }
        }
    }

    fn stamp_map_base_address_masked(&self) -> u32 {
        use StampMapSizeScreens as Screens;
        use StampSizeDots as Dots;

        let stamp_map_base_address_mask = match (self.stamp_map_size, self.stamp_size) {
            (Screens::One, Dots::Sixteen) => {
                // Bits 17-9
                0x03FE00
            }
            (Screens::One, Dots::ThirtyTwo) => {
                // Bits 17-7
                0x03FF80
            }
            (Screens::Sixteen, Dots::Sixteen) => {
                // Bit 17 only
                0x020000
            }
            (Screens::Sixteen, Dots::ThirtyTwo) => {
                // Bits 17-15
                0x038000
            }
        };

        self.stamp_map_base_address & stamp_map_base_address_mask
    }
}

fn read_word_ram(word_ram: &WordRam, address: u32) -> u8 {
    word_ram.sub_cpu_read_ram(wordram::SUB_BASE_ADDRESS | address)
}

fn write_word_ram(word_ram: &mut WordRam, address: u32, value: u8) {
    word_ram.sub_cpu_write_ram(wordram::SUB_BASE_ADDRESS | address, value);
}

fn compute_stamp_map_address(
    stamp_map_base_address: u32,
    stamp_size: StampSizeDots,
    stamp_map_size: StampMapSizeScreens,
    x: u32,
    y: u32,
) -> u32 {
    let stamp_dimension_pixels = stamp_size.one_dimension_in_pixels();
    let stamp_map_dimension_pixels = stamp_map_size.one_dimension_in_pixels();

    let stamp_map_x = (x & (stamp_map_dimension_pixels - 1)) / stamp_dimension_pixels;
    let stamp_map_y = (y & (stamp_map_dimension_pixels - 1)) / stamp_dimension_pixels;

    // 2 bytes per stamp
    let stamp_map_relative_addr =
        2 * (stamp_map_y * stamp_map_dimension_pixels / stamp_dimension_pixels + stamp_map_x);
    stamp_map_base_address + stamp_map_relative_addr
}

fn sample_stamp(
    word_ram: &WordRam,
    stamp: StampData,
    stamp_size: StampSizeDots,
    x: u32,
    y: u32,
) -> u8 {
    let stamp_number = match stamp_size {
        StampSizeDots::Sixteen => stamp.stamp_number,
        StampSizeDots::ThirtyTwo => {
            // Lowest 2 bits are ignored in 32x32 stamp mode; treat the remaining bits as a stamp
            // number for 32x32 tiles (4x the byte size of 16x16 tiles)
            stamp.stamp_number >> 2
        }
    };
    let stamp_number: u32 = stamp_number.into();

    if stamp_number == 0 {
        // Sampling stamp 0 always results in 0 regardless of what is in word RAM
        return 0;
    }

    let stamp_size_dimension_pixels = stamp_size.one_dimension_in_pixels();
    let stamp_addr = stamp_number * (stamp_size_dimension_pixels * stamp_size_dimension_pixels / 2);

    let x = x & (stamp_size_dimension_pixels - 1);
    let y = y & (stamp_size_dimension_pixels - 1);

    let x = if stamp.horizontal_flip {
        flip_stamp_coordinate(x, stamp_size_dimension_pixels)
    } else {
        x
    };
    let (x, y) = match stamp.rotation {
        StampRotation::Zero => (x, y),
        StampRotation::Ninety => (y, flip_stamp_coordinate(x, stamp_size_dimension_pixels)),
        StampRotation::OneEighty => (
            flip_stamp_coordinate(x, stamp_size_dimension_pixels),
            flip_stamp_coordinate(y, stamp_size_dimension_pixels),
        ),
        StampRotation::TwoSeventy => (flip_stamp_coordinate(y, stamp_size_dimension_pixels), x),
    };

    let sample_addr = stamp_addr
        + compute_relative_addr_v_then_h(
            stamp_size_dimension_pixels,
            x & (stamp_size_dimension_pixels - 1),
            y & (stamp_size_dimension_pixels - 1),
        );
    let byte = read_word_ram(word_ram, sample_addr);
    if x.bit(0) { byte & 0x0F } else { byte >> 4 }
}

fn flip_stamp_coordinate(coordinate: u32, stamp_size_dimension_pixels: u32) -> u32 {
    stamp_size_dimension_pixels - 1 - (coordinate & (stamp_size_dimension_pixels - 1))
}

fn compute_relative_addr_v_then_h(v_size_pixels: u32, x: u32, y: u32) -> u32 {
    assert!(y < v_size_pixels);

    let v_size_cells = v_size_pixels / 8;

    let cell_x = x / 8;
    let cell_y = y / 8;
    let cell_number = cell_x * v_size_cells + cell_y;

    // 32 bytes per cell
    let cell_addr = 32 * cell_number;

    // 4 bytes per row
    let addr_in_cell = 4 * (y & 0x07) + ((x & 0x07) >> 1);
    cell_addr + addr_in_cell
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_map_address() {
        let stamp_size = StampSizeDots::Sixteen;
        let stamp_map_size = StampMapSizeScreens::Sixteen;

        assert_eq!(0, compute_stamp_map_address(0, stamp_size, stamp_map_size, 0, 0));
        assert_eq!(0x20000, compute_stamp_map_address(0x20000, stamp_size, stamp_map_size, 0, 0));

        assert_eq!(0x20000, compute_stamp_map_address(0x20000, stamp_size, stamp_map_size, 15, 15));
        assert_eq!(0x20002, compute_stamp_map_address(0x20000, stamp_size, stamp_map_size, 16, 15));
        assert_eq!(0x20200, compute_stamp_map_address(0x20000, stamp_size, stamp_map_size, 15, 16));
        assert_eq!(
            0x3FFFE,
            compute_stamp_map_address(0x20000, stamp_size, stamp_map_size, 4095, 4095)
        );
    }
}
