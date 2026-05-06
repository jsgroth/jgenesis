mod palette;
mod vce;
mod vdc;

use crate::video::vce::Vce;
use crate::video::vdc::Vdc;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{Color, FrameSize};
use jgenesis_common::num::U16Ext;
use jgenesis_proc_macros::{FakeDecode, FakeEncode};

pub const FRAME_BUFFER_LEN: usize = vdc::FRAME_BUFFER_WIDTH * vdc::FRAME_BUFFER_HEIGHT;

// Number of mclk cycles per scanline is fixed, unaffected by dot clock divider
const MCLK_CYCLES_PER_SCANLINE: u64 = 1365;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordByte {
    Low,
    High,
}

impl WordByte {
    fn get(self, word: u16) -> u8 {
        match self {
            Self::Low => word.lsb(),
            Self::High => word.msb(),
        }
    }

    fn set(self, word: &mut u16, byte: u8) {
        match self {
            Self::Low => word.set_lsb(byte),
            Self::High => word.set_msb(byte),
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct VideoState {
    scanline: u16,
    scanline_mclk: u64,
}

impl VideoState {
    fn new() -> Self {
        Self { scanline: 0, scanline_mclk: 0 }
    }
}

#[derive(Debug, Clone, FakeEncode, FakeDecode)]
struct Rgba8FrameBuffer(Box<[Color; FRAME_BUFFER_LEN]>);

impl Default for Rgba8FrameBuffer {
    fn default() -> Self {
        Self(vec![Color::default(); FRAME_BUFFER_LEN].into_boxed_slice().try_into().unwrap())
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct VideoSubsystem {
    vdc: Vdc,
    vce: Vce,
    state: VideoState,
    cycles: u64,
    frame_buffer: Rgba8FrameBuffer,
}

impl VideoSubsystem {
    pub fn new() -> Self {
        Self {
            vdc: Vdc::new(),
            vce: Vce::new(),
            state: VideoState::new(),
            cycles: 0,
            frame_buffer: Rgba8FrameBuffer::default(),
        }
    }

    pub fn step_to(&mut self, cycles: u64) {
        let elapsed_cycles = cycles.saturating_sub(self.cycles);
        if elapsed_cycles == 0 {
            return;
        }
        self.cycles = cycles;

        let mut prev_scanline_mclk = self.state.scanline_mclk;
        self.state.scanline_mclk += elapsed_cycles;

        let dot_clock_divider = self.vce.dot_clock_divider();

        if self.state.scanline_mclk >= MCLK_CYCLES_PER_SCANLINE {
            let elapsed_vdc_dots =
                dot_clock_divider.divide_difference(MCLK_CYCLES_PER_SCANLINE, prev_scanline_mclk);
            self.vdc.tick_dots(elapsed_vdc_dots, &self.vce);

            prev_scanline_mclk = 0;
            self.state.scanline_mclk -= MCLK_CYCLES_PER_SCANLINE;

            self.state.scanline += 1;
            if self.state.scanline >= self.vce.lines_per_frame() {
                self.state.scanline = 0;
                self.vdc.start_new_frame();
            }

            self.vdc.start_new_line(self.state.scanline, dot_clock_divider);
        }

        let elapsed_vdc_dots = self
            .vce
            .dot_clock_divider()
            .divide_difference(self.state.scanline_mclk, prev_scanline_mclk);
        self.vdc.tick_dots(elapsed_vdc_dots, &self.vce);
    }

    pub fn frame_complete(&self) -> bool {
        self.vdc.frame_complete()
    }

    pub fn clear_frame_complete(&mut self) {
        self.vdc.clear_frame_complete();
    }

    pub fn vdc_irq(&self) -> bool {
        self.vdc.irq()
    }

    pub fn render_rgba8_frame_buffer(&mut self) {
        let vdc_frame_buffer = self.vdc.frame_buffer();

        for row in 0..vdc::FRAME_BUFFER_HEIGHT {
            for col in 0..vdc::FRAME_BUFFER_WIDTH {
                let vdc_color = vdc_frame_buffer.colors[row][col];
                let (r, g, b) = palette::read(vdc_color);
                self.frame_buffer.0[row * vdc::FRAME_BUFFER_WIDTH + col] = Color::rgb(r, g, b);
            }
        }
    }

    pub fn frame_buffer(&self) -> &[Color] {
        self.frame_buffer.0.as_slice()
    }

    #[allow(clippy::unused_self)]
    pub fn frame_size(&self) -> FrameSize {
        // TODO handle overscan and downsampling
        FrameSize { width: vdc::FRAME_BUFFER_WIDTH as u32, height: vdc::FRAME_BUFFER_HEIGHT as u32 }
    }

    // $1FE000-$1FE003: VDC ports
    pub fn read_vdc(&mut self, address: u32) -> u8 {
        log::trace!("VDC register read {address:06X}");

        match address & 3 {
            0 => self.vdc.read_status(),
            1 => 0x00, // Unused
            2 => self.vdc.read_data(WordByte::Low),
            3 => self.vdc.read_data(WordByte::High),
            _ => unreachable!("address & 3 is always <= 3"),
        }
    }

    // $1FE000-$1FE003: VDC ports
    pub fn write_vdc(&mut self, address: u32, value: u8) {
        log::trace!("VDC register write {address:06X} {value:02X}");

        match address & 3 {
            0 => self.vdc.write_register_select(value),
            1 => {} // Unused
            2 => self.vdc.write_data(value, WordByte::Low),
            3 => self.vdc.write_data(value, WordByte::High),
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }

    // $1FE400-$1FE407: VCE ports
    #[allow(clippy::match_same_arms)]
    pub fn write_vce(&mut self, address: u32, value: u8) {
        log::trace!("VCE register write {address:06X} {value:02X}");

        match address & 7 {
            0 => self.vce.write_control(value),
            1 => {} // Unused
            2 => self.vce.write_color_address(value, WordByte::Low),
            3 => self.vce.write_color_address(value, WordByte::High),
            4 => self.vce.write_color_data(value, WordByte::Low),
            5 => self.vce.write_color_data(value, WordByte::High),
            6 | 7 => {} // Unused
            _ => unreachable!("value & 7 is always <= 7"),
        }
    }

    pub fn dump_vram(&self, palette: u16, out: &mut [[Color; 64]]) {
        self.vdc.dump_vram(palette, out, &self.vce);
    }

    pub fn dump_palettes(&self, out: &mut [Color]) {
        self.vce.dump_palettes(out);
    }
}
