use bincode::{Decode, Encode};
use jgenesis_common::frontend::TimingMode;
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum FrameBufferMode {
    #[default]
    Blank = 0,
    PackedPixel = 1,
    DirectColor = 2,
    RunLength = 3,
}

impl FrameBufferMode {
    fn from_word(value: u16) -> Self {
        match value & 3 {
            0 => Self::Blank,
            1 => Self::PackedPixel,
            2 => Self::DirectColor,
            3 => Self::RunLength,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum VerticalResolution {
    #[default]
    V28 = 0,
    V30 = 1,
}

impl VerticalResolution {
    pub fn active_scanlines_per_frame(self) -> u16 {
        match self {
            Self::V28 => super::V28_FRAME_HEIGHT as u16,
            Self::V30 => super::V30_FRAME_HEIGHT as u16,
        }
    }

    fn from_bit(bit: bool) -> Self {
        if bit { Self::V30 } else { Self::V28 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum SelectedFrameBuffer {
    #[default]
    Zero = 0,
    One = 1,
}

impl SelectedFrameBuffer {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::One } else { Self::Zero }
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct Registers {
    pub frame_buffer_mode: FrameBufferMode,
    pub v_resolution: VerticalResolution,
    pub priority: bool,
    pub display_frame_buffer: SelectedFrameBuffer,
    pub screen_left_shift: bool,
    pub auto_fill_length: u16,
    pub auto_fill_start_address: u16,
    pub auto_fill_data: u16,
    pub h_interrupt_interval: u16,
    pub h_interrupt_in_vblank: bool,
}

impl Registers {
    // 68000: $A15180
    // SH-2: $4100
    pub fn read_display_mode(&self, timing_mode: TimingMode) -> u16 {
        (u16::from(timing_mode == TimingMode::Ntsc) << 15)
            | (u16::from(self.priority) << 7)
            | ((self.v_resolution as u16) << 6)
            | (self.frame_buffer_mode as u16)
    }

    // 68000: $A15180
    // SH-2: $4100
    pub fn write_display_mode(&mut self, value: u16) {
        self.frame_buffer_mode = FrameBufferMode::from_word(value);
        self.v_resolution = VerticalResolution::from_bit(value.bit(6));
        self.priority = value.bit(7);

        log::debug!("Display mode write: {value:04X}");
        log::debug!("  Frame buffer mode: {:?}", self.frame_buffer_mode);
        log::debug!("  Vertical resolution: {:?}", self.v_resolution);
        log::debug!("  Priority: {}", self.priority);
    }

    // 68000: $A15182
    // SH-2: $4102
    pub fn read_screen_shift(&self) -> u16 {
        self.screen_left_shift.into()
    }

    // 68000: $A15182
    // SH-2: $4102
    pub fn write_screen_shift(&mut self, value: u16) {
        self.screen_left_shift = value.bit(0);

        log::debug!("Screen shift control write: {value:04X}");
        log::debug!("  Shift screen left by 1 dot: {}", self.screen_left_shift);
    }

    // 68000: $A15184
    // SH-2: $4104
    pub fn read_auto_fill_length(&self) -> u16 {
        self.auto_fill_length.wrapping_sub(1) & 0xFF
    }

    // 68000: $A15184
    // SH-2: $4104
    pub fn write_auto_fill_length(&mut self, value: u16) {
        self.auto_fill_length = (value & 0xFF) + 1;

        log::trace!("Auto fill length write: {value:04X}");
        log::trace!("  Auto fill length: {}", self.auto_fill_length);
    }

    // 68000: $A15186
    // SH-2: $4106
    pub fn read_auto_fill_start_address(&self) -> u16 {
        self.auto_fill_start_address
    }

    // 68000: $A15186
    // SH-2: $4106
    pub fn write_auto_fill_start_address(&mut self, value: u16) {
        self.auto_fill_start_address = value;
        log::trace!("Auto fill start address write: {value:04X}");
    }

    pub fn increment_auto_fill_address(&mut self) {
        // Highest 8 bits of address do not increment, only the lowest 8 bits
        self.auto_fill_start_address = (self.auto_fill_start_address & 0xFF00)
            | (self.auto_fill_start_address.wrapping_add(1) & 0xFF);
    }

    // 68000: $A15188
    // SH-2: $4108
    pub fn write_auto_fill_data(&mut self, value: u16) {
        self.auto_fill_data = value;
        log::trace!("Auto fill data write: {value:04X}");
    }

    // 68000: $A1518A
    // SH-2: $410A
    pub fn write_frame_buffer_control(&mut self, value: u16) {
        self.display_frame_buffer = SelectedFrameBuffer::from_bit(value.bit(0));

        log::debug!("Frame buffer control write: {value:04X}");
        log::debug!("  Display frame buffer: {:?}", self.display_frame_buffer);
    }
}
