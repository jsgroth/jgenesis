//! 32X VDP (Video Display Processor)

mod registers;

use crate::vdp::registers::{FrameBufferMode, Registers, SelectedFrameBuffer};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{Color, TimingMode};
use jgenesis_common::num::{GetBit, U16Ext};

const NTSC_SCANLINES_PER_FRAME: u16 = genesis_core::vdp::NTSC_SCANLINES_PER_FRAME;
const PAL_SCANLINES_PER_FRAME: u16 = genesis_core::vdp::PAL_SCANLINES_PER_FRAME;

const MCLK_CYCLES_PER_SCANLINE: u64 = genesis_core::vdp::MCLK_CYCLES_PER_SCANLINE;
const ACTIVE_MCLK_CYCLES_PER_SCANLINE: u64 = genesis_core::vdp::ACTIVE_MCLK_CYCLES_PER_SCANLINE;

const FRAME_BUFFER_LEN_WORDS: usize = 128 * 1024 / 2;
const CRAM_LEN_WORDS: usize = 512 / 2;

// 32X VDP only supports 320x224 and 320x240 frame sizes
const FRAME_WIDTH: u32 = 320;
const V28_FRAME_HEIGHT: u32 = 224;
const V30_FRAME_HEIGHT: u32 = 240;

const RGB_5_TO_8: &[u8; 32] = &[
    0, 8, 16, 25, 33, 41, 49, 58, 66, 74, 82, 90, 99, 107, 115, 123, 132, 140, 148, 156, 165, 173,
    181, 189, 197, 206, 214, 222, 230, 239, 247, 255,
];

type FrameBufferRam = [u16; FRAME_BUFFER_LEN_WORDS];
type Cram = [u16; CRAM_LEN_WORDS];

type RenderedFrame = [[u16; FRAME_WIDTH as usize]; V30_FRAME_HEIGHT as usize];

fn new_frame_buffer() -> Box<FrameBufferRam> {
    vec![0; FRAME_BUFFER_LEN_WORDS].into_boxed_slice().try_into().unwrap()
}

fn new_rendered_frame() -> Box<RenderedFrame> {
    vec![[0; FRAME_WIDTH as usize]; V30_FRAME_HEIGHT as usize]
        .into_boxed_slice()
        .try_into()
        .unwrap()
}

trait TimingModeExt {
    fn scanlines_per_frame(self) -> u16;
}

impl TimingModeExt for TimingMode {
    fn scanlines_per_frame(self) -> u16 {
        match self {
            Self::Ntsc => NTSC_SCANLINES_PER_FRAME,
            Self::Pal => PAL_SCANLINES_PER_FRAME,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct State {
    scanline: u16,
    scanline_mclk: u64,
    display_frame_buffer: SelectedFrameBuffer,
}

impl State {
    fn new() -> Self {
        Self { scanline: 0, scanline_mclk: 0, display_frame_buffer: SelectedFrameBuffer::default() }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Vdp {
    frame_buffer_0: Box<FrameBufferRam>,
    frame_buffer_1: Box<FrameBufferRam>,
    rendered_frame: Box<RenderedFrame>,
    cram: Box<Cram>,
    registers: Registers,
    state: State,
    timing_mode: TimingMode,
}

macro_rules! front_frame_buffer {
    ($self:expr) => {
        match $self.state.display_frame_buffer {
            SelectedFrameBuffer::Zero => &$self.frame_buffer_0,
            SelectedFrameBuffer::One => &$self.frame_buffer_1,
        }
    };
}

macro_rules! back_frame_buffer {
    ($self:expr) => {
        match $self.state.display_frame_buffer {
            SelectedFrameBuffer::Zero => &$self.frame_buffer_1,
            SelectedFrameBuffer::One => &$self.frame_buffer_0,
        }
    };
}

macro_rules! back_frame_buffer_mut {
    ($self:expr) => {
        match $self.state.display_frame_buffer {
            SelectedFrameBuffer::Zero => &mut $self.frame_buffer_1,
            SelectedFrameBuffer::One => &mut $self.frame_buffer_0,
        }
    };
}

impl Vdp {
    pub fn new(timing_mode: TimingMode) -> Self {
        Self {
            frame_buffer_0: new_frame_buffer(),
            frame_buffer_1: new_frame_buffer(),
            rendered_frame: new_rendered_frame(),
            cram: vec![0; CRAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
            registers: Registers::default(),
            state: State::new(),
            timing_mode,
        }
    }

    pub fn tick(&mut self, mclk_cycles: u64) {
        // TODO VINT/HINT

        self.state.scanline_mclk += mclk_cycles;
        if self.state.scanline_mclk >= MCLK_CYCLES_PER_SCANLINE {
            self.state.scanline_mclk -= MCLK_CYCLES_PER_SCANLINE;
            self.state.scanline += 1;

            let active_lines_per_frame = self.registers.v_resolution.active_scanlines_per_frame();
            if self.state.scanline == active_lines_per_frame {
                // Beginning of VBlank; frame buffer switches take effect
                self.state.display_frame_buffer = self.registers.display_frame_buffer;
            } else if self.state.scanline >= self.timing_mode.scanlines_per_frame() {
                self.state.scanline = 0;
            }

            if self.state.scanline < active_lines_per_frame {
                self.render_line();
            }
        }
    }

    fn render_line(&mut self) {
        match self.registers.frame_buffer_mode {
            FrameBufferMode::Blank => {
                self.rendered_frame[self.state.scanline as usize].fill(0);
            }
            FrameBufferMode::PackedPixel => self.render_packed_pixel(),
            FrameBufferMode::DirectColor => todo!("direct color render"),
            FrameBufferMode::RunLength => todo!("run length render"),
        }
    }

    fn render_packed_pixel(&mut self) {
        let line = self.state.scanline as usize;
        let frame_buffer = front_frame_buffer!(self);
        let line_addr = frame_buffer[line];

        for pixel in (0..FRAME_WIDTH as u16).step_by(2) {
            let frame_buffer_addr = line_addr.wrapping_add(pixel >> 1);
            let [msb, lsb] = frame_buffer[frame_buffer_addr as usize].to_be_bytes();

            self.rendered_frame[line][pixel as usize] = self.cram[msb as usize];
            self.rendered_frame[line][(pixel + 1) as usize] = self.cram[lsb as usize];
        }
    }

    pub fn read_register(&self, address: u32) -> u16 {
        match address & 0xF {
            0x0 => self.registers.read_display_mode(self.timing_mode),
            0x2 => self.registers.read_screen_shift(),
            0xA => self.read_frame_buffer_control(),
            _ => todo!("VDP register read {address:08X}"),
        }
    }

    pub fn write_register(&mut self, address: u32, value: u16) {
        log::trace!("VDP register write: {address:08X} {value:04X}");

        match address & 0xF {
            0x0 => self.registers.write_display_mode(value),
            0x2 => self.registers.write_screen_shift(value),
            0x4 => self.registers.write_auto_fill_length(value),
            0x6 => self.registers.write_auto_fill_start_address(value),
            0x8 => {
                self.registers.write_auto_fill_data(value);

                // Writing auto fill data initiates auto fill
                self.do_auto_fill();
            }
            0xA => {
                self.registers.write_frame_buffer_control(value);
                if self.in_vblank() {
                    self.state.display_frame_buffer = self.registers.display_frame_buffer;
                }
            }
            _ => todo!("VDP register write {address:08X} {value:04X}"),
        }
    }

    pub fn read_frame_buffer(&self, address: u32) -> u16 {
        let frame_buffer = back_frame_buffer!(self);
        frame_buffer[((address & 0x1FFFF) >> 1) as usize]
    }

    pub fn write_frame_buffer(&mut self, address: u32, value: u16) {
        let frame_buffer = back_frame_buffer_mut!(self);
        frame_buffer[((address & 0x1FFFF) >> 1) as usize] = value;
    }

    pub fn frame_buffer_overwrite_byte(&mut self, address: u32, value: u8) {
        if value == 0 {
            return;
        }

        let frame_buffer = back_frame_buffer_mut!(self);
        let frame_buffer_addr = ((address & 0x1FFFF) >> 1) as usize;

        if !address.bit(0) {
            frame_buffer[frame_buffer_addr].set_msb(value);
        } else {
            frame_buffer[frame_buffer_addr].set_lsb(value);
        }
    }

    pub fn frame_buffer_overwrite_word(&mut self, address: u32, value: u16) {
        if value == 0 {
            return;
        }

        let frame_buffer = back_frame_buffer_mut!(self);
        let frame_buffer_addr = ((address & 0x1FFFF) >> 1) as usize;

        let [msb, lsb] = value.to_be_bytes();

        if msb != 0 {
            frame_buffer[frame_buffer_addr].set_msb(msb);
        }

        if lsb != 0 {
            frame_buffer[frame_buffer_addr].set_lsb(lsb);
        }
    }

    pub fn read_cram(&self, address: u32) -> u16 {
        // TODO block access to CRAM when in use?
        self.cram[((address & 0x1FF) >> 1) as usize]
    }

    pub fn write_cram(&mut self, address: u32, value: u16) {
        // TODO block access to CRAM while in use?
        self.cram[((address & 0x1FF) >> 1) as usize] = value;
    }

    // Interrupt mask bit 7: HEN (H interrupts enabled during VBlank)(
    pub fn hen_bit(&self) -> bool {
        self.registers.h_interrupt_in_vblank
    }

    // Interrupt mask bit 7: HEN (H interrupts enabled during VBlank)(
    pub fn write_hen_bit(&mut self, hen: bool) {
        self.registers.h_interrupt_in_vblank = hen;
    }

    // SH-2: $4004
    pub fn h_interrupt_interval(&self) -> u16 {
        self.registers.h_interrupt_interval
    }

    // SH-2: $4004
    pub fn write_h_interrupt_interval(&mut self, value: u16) {
        self.registers.h_interrupt_interval = value & 0xFF;
        log::trace!("H interrupt interval write: {value:04X}");
    }

    fn read_frame_buffer_control(&self) -> u16 {
        let in_vblank = self.in_vblank();
        let in_hblank = self.in_hblank();

        let cram_accessible = in_vblank || in_hblank;

        // TODO FEN (bit 1): frame buffer is not accessible during DRAM refresh (or during auto fill?)
        (u16::from(in_vblank) << 15)
            | (u16::from(in_hblank) << 14)
            | (u16::from(cram_accessible) << 13)
            | (self.state.display_frame_buffer as u16)
    }

    fn do_auto_fill(&mut self) {
        let frame_buffer = back_frame_buffer_mut!(self);

        let data = self.registers.auto_fill_data;
        for _ in 0..self.registers.auto_fill_length {
            frame_buffer[self.registers.auto_fill_start_address as usize] = data;
            self.registers.increment_auto_fill_address();
        }
    }

    fn in_vblank(&self) -> bool {
        self.state.scanline >= self.registers.v_resolution.active_scanlines_per_frame()
    }

    fn in_hblank(&self) -> bool {
        self.state.scanline_mclk >= ACTIVE_MCLK_CYCLES_PER_SCANLINE
    }

    pub fn scanline(&self) -> u16 {
        self.state.scanline
    }

    pub fn scanline_mclk(&self) -> u64 {
        self.state.scanline_mclk
    }

    pub fn composite_frame(
        &self,
        genesis_frame_buffer: &mut [Color; genesis_core::vdp::FRAME_BUFFER_LEN],
    ) {
        if self.registers.frame_buffer_mode == FrameBufferMode::Blank {
            // Leave Genesis frame as-is
            return;
        }

        let priority = self.registers.priority;

        // TODO properly handle Genesis frame buffer size
        let active_lines_per_frame: u32 =
            self.registers.v_resolution.active_scanlines_per_frame().into();
        for line in 0..active_lines_per_frame {
            for pixel in 0..FRAME_WIDTH {
                let genesis_fb_addr = (line * FRAME_WIDTH + pixel) as usize;

                let s32x_pixel = self.rendered_frame[line as usize][pixel as usize];
                if s32x_pixel.bit(15) != priority || genesis_frame_buffer[genesis_fb_addr].a == 0 {
                    // Replace Genesis pixel with 32X pixel
                    let r = s32x_pixel & 0x1F;
                    let g = (s32x_pixel >> 5) & 0x1F;
                    let b = (s32x_pixel >> 10) & 0x1F;

                    genesis_frame_buffer[genesis_fb_addr] = Color::rgb(
                        RGB_5_TO_8[r as usize],
                        RGB_5_TO_8[g as usize],
                        RGB_5_TO_8[b as usize],
                    );
                }
            }
        }
    }
}
