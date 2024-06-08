//! 32X VDP (Video Display Processor)

mod registers;

use crate::vdp::registers::{FrameBufferMode, Registers, SelectedFrameBuffer};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::TimingMode;

const NTSC_SCANLINES_PER_FRAME: u16 = genesis_core::vdp::NTSC_SCANLINES_PER_FRAME;
const PAL_SCANLINES_PER_FRAME: u16 = genesis_core::vdp::PAL_SCANLINES_PER_FRAME;

const MCLK_CYCLES_PER_SCANLINE: u64 = genesis_core::vdp::MCLK_CYCLES_PER_SCANLINE;
const ACTIVE_MCLK_CYCLES_PER_SCANLINE: u64 = genesis_core::vdp::ACTIVE_MCLK_CYCLES_PER_SCANLINE;

const FRAME_BUFFER_LEN_WORDS: usize = 128 * 1024 / 2;
const CRAM_LEN_WORDS: usize = 512 / 2;

type FrameBufferRam = [u16; FRAME_BUFFER_LEN_WORDS];
type Cram = [u16; CRAM_LEN_WORDS];

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
    auto_fill_cycles_remaining: u64,
}

impl State {
    fn new() -> Self {
        Self {
            scanline: 0,
            scanline_mclk: 0,
            display_frame_buffer: SelectedFrameBuffer::default(),
            auto_fill_cycles_remaining: 0,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Vdp {
    frame_buffer_0: Box<FrameBufferRam>,
    frame_buffer_1: Box<FrameBufferRam>,
    cram: Box<Cram>,
    registers: Registers,
    state: State,
    timing_mode: TimingMode,
}

impl Vdp {
    pub fn new(timing_mode: TimingMode) -> Self {
        Self {
            frame_buffer_0: new_frame_buffer(),
            frame_buffer_1: new_frame_buffer(),
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

            if self.state.scanline == self.registers.v_resolution.active_scanlines_per_frame() {
                // Beginning of VBlank; frame buffer switches take effect
                self.state.display_frame_buffer = self.registers.display_frame_buffer;
            } else if self.state.scanline >= self.timing_mode.scanlines_per_frame() {
                self.state.scanline = 0;
            }
        }

        self.state.auto_fill_cycles_remaining =
            self.state.auto_fill_cycles_remaining.saturating_sub(mclk_cycles * 3 / 7);
    }

    pub fn read_register(&self, address: u32) -> u16 {
        match address & 0xF {
            0x0 => self.registers.read_display_mode(self.timing_mode),
            0xA => self.read_frame_buffer_control(),
            _ => todo!("VDP register read {address:08X}"),
        }
    }

    pub fn write_register(&mut self, address: u32, value: u16) {
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

    pub fn write_cram(&mut self, address: u32, value: u16) {
        if matches!(
            self.registers.frame_buffer_mode,
            FrameBufferMode::Blank | FrameBufferMode::DirectColor
        ) || self.in_vblank()
            || self.in_hblank()
        {
            self.cram[((address & 0x1FF) >> 1) as usize] = value;
        }
    }

    fn read_frame_buffer_control(&self) -> u16 {
        let in_vblank = self.in_vblank();
        let in_hblank = self.in_hblank();

        let cram_accessible = in_vblank || in_hblank;
        let frame_buffer_blocked = self.state.auto_fill_cycles_remaining != 0;

        // TODO FEN (bit 1): frame buffer is not accessible during DRAM refresh
        (u16::from(in_vblank) << 15)
            | (u16::from(in_hblank) << 14)
            | (u16::from(cram_accessible) << 13)
            | (u16::from(frame_buffer_blocked) << 1)
            | (self.state.display_frame_buffer as u16)
    }

    fn do_auto_fill(&mut self) {
        let frame_buffer = match self.state.display_frame_buffer {
            SelectedFrameBuffer::Zero => &mut self.frame_buffer_1,
            SelectedFrameBuffer::One => &mut self.frame_buffer_0,
        };

        let data = self.registers.auto_fill_data;
        for _ in 0..self.registers.auto_fill_length {
            frame_buffer[self.registers.auto_fill_start_address as usize] = data;
            self.registers.increment_auto_fill_address();
        }

        self.state.auto_fill_cycles_remaining = 7 + 3 * u64::from(self.registers.auto_fill_length);
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
}

fn new_frame_buffer() -> Box<FrameBufferRam> {
    vec![0; FRAME_BUFFER_LEN_WORDS].into_boxed_slice().try_into().unwrap()
}
