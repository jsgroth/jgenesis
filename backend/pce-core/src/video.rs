mod palette;
mod vce;
mod vdc;

use crate::api;
use crate::api::PceEmulatorConfig;
use crate::video::vce::Vce;
use crate::video::vdc::Vdc;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{
    Color, CompositeParams, FiniteF64, FrameSize, NtscPerFrameParams, SamplesPerColorCycle,
};
use jgenesis_common::num::U16Ext;
use jgenesis_proc_macros::{FakeDecode, FakeEncode};

pub const FRAME_BUFFER_LEN: usize = vdc::FRAME_BUFFER_WIDTH * vdc::FRAME_BUFFER_HEIGHT;

// Number of mclk cycles per scanline is fixed, unaffected by dot clock divider
pub const MCLK_CYCLES_PER_SCANLINE: u64 = 1365;

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

struct FrameRenderParams {
    left_offset: usize,
    top_offset: usize,
    width: usize,
    height: usize,
}

impl FrameRenderParams {
    const DEFAULT: Self = Self {
        left_offset: 0,
        top_offset: 0,
        width: vdc::FRAME_BUFFER_WIDTH,
        height: vdc::FRAME_BUFFER_HEIGHT,
    };

    const CROP_OVERSCAN: Self = Self {
        left_offset: 4 * vdc::OVERSCAN_DOTS_DIV_4 as usize,
        top_offset: 9,
        width: vdc::FRAME_BUFFER_WIDTH - 2 * 4 * vdc::OVERSCAN_DOTS_DIV_4 as usize,
        height: vdc::FRAME_BUFFER_HEIGHT - 2 * 9,
    };
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct VideoSubsystem {
    vdc: Vdc,
    vce: Vce,
    state: VideoState,
    cycles: u64,
    frame_start_cycles: u64,
    frame_buffer: Rgba8FrameBuffer,
    frame_x_divider: u32,
    crop_overscan: bool,
}

impl VideoSubsystem {
    pub fn new(config: PceEmulatorConfig) -> Self {
        Self {
            vdc: Vdc::new(config),
            vce: Vce::new(),
            state: VideoState::new(),
            cycles: 0,
            frame_start_cycles: 0,
            frame_buffer: Rgba8FrameBuffer::default(),
            frame_x_divider: 1,
            crop_overscan: config.crop_overscan,
        }
    }

    pub fn step_to(&mut self, cycles: u64, irq1_pending: &mut bool) {
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

            let lines_per_frame = self.vce.lines_per_frame();

            self.state.scanline += 1;
            if self.state.scanline >= lines_per_frame {
                self.state.scanline = 0;
                self.vdc.start_new_frame();
                self.frame_start_cycles = self.cycles - self.state.scanline_mclk;
            }

            self.vdc.start_new_line(self.state.scanline, &self.vce);
        }

        let elapsed_vdc_dots = self
            .vce
            .dot_clock_divider()
            .divide_difference(self.state.scanline_mclk, prev_scanline_mclk);
        self.vdc.tick_dots(elapsed_vdc_dots, &self.vce);

        *irq1_pending = self.vdc.irq();
    }

    pub fn frame_complete(&self) -> bool {
        self.vdc.frame_complete()
    }

    pub fn clear_frame_complete(&mut self) {
        self.vdc.clear_frame_complete();
    }

    pub fn render_rgba8_frame_buffer(&mut self) {
        let vdc_frame_buffer = self.vdc.frame_buffer();

        let mut params = if self.crop_overscan {
            FrameRenderParams::CROP_OVERSCAN
        } else {
            FrameRenderParams::DEFAULT
        };

        let x_stride = if vdc_frame_buffer
            .line_dividers
            .iter()
            .all(|&divider| divider == vdc_frame_buffer.line_dividers[0])
        {
            // If dot clock divider remained unchanged for the entire frame, downsample for more
            // reasonable behavior with shaders
            vdc_frame_buffer.line_dividers[0] as usize
        } else {
            1
        };
        self.frame_x_divider = x_stride as u32;

        params.width /= x_stride;

        for row in 0..params.height {
            for col in 0..params.width {
                let vdc_color = vdc_frame_buffer.colors[row + params.top_offset]
                    [col * x_stride + params.left_offset];
                let (r, g, b) = palette::read(vdc_color);
                self.frame_buffer.0[row * params.width + col] = Color::rgb(r, g, b);
            }
        }
    }

    pub fn frame_buffer(&self) -> &[Color] {
        self.frame_buffer.0.as_slice()
    }

    pub fn frame_size(&self) -> FrameSize {
        let mut frame_size = if self.crop_overscan {
            FrameSize {
                width: FrameRenderParams::CROP_OVERSCAN.width as u32,
                height: FrameRenderParams::CROP_OVERSCAN.height as u32,
            }
        } else {
            FrameSize {
                width: vdc::FRAME_BUFFER_WIDTH as u32,
                height: vdc::FRAME_BUFFER_HEIGHT as u32,
            }
        };

        frame_size.width /= self.frame_x_divider;

        frame_size
    }

    pub fn ntsc_aspect_ratio(&self) -> FiniteF64 {
        // NTSC aspect ratio should be 8:7 for 5 MHz dot clock (H256px); compute others based on that
        debug_assert_ne!(self.frame_x_divider, 0);
        let multiplier = f64::from(self.frame_x_divider) / 4.0;
        FiniteF64::try_from(multiplier * 8.0 / 7.0).unwrap()
    }

    pub fn composite_params(&self) -> CompositeParams {
        CompositeParams {
            upscale_factor: 2 * self.frame_x_divider,
            samples_per_color_cycle: SamplesPerColorCycle::Twelve,
        }
    }

    pub fn ntsc_per_frame_params(&self) -> NtscPerFrameParams {
        NtscPerFrameParams {
            frame_phase_offset: 2 * self.frame_start_cycles,
            per_line_phase_offset: 2 * MCLK_CYCLES_PER_SCANLINE,
        }
    }

    pub fn target_fps(&self) -> f64 {
        // ~60.05 Hz in 262-line mode, ~59.83 Hz in 263-line mode
        api::MASTER_CLOCK_FREQUENCY
            / (MCLK_CYCLES_PER_SCANLINE as f64)
            / f64::from(self.vce.lines_per_frame())
    }

    // $1FE000-$1FE003: VDC ports
    pub fn read_vdc(&mut self, address: u32, bus_cycles: &mut u64, irq1_pending: &mut bool) -> u8 {
        log::trace!(
            "VDC register read {address:06X}, line {} mclk {}",
            self.state.scanline,
            self.state.scanline_mclk
        );

        let address = address & 3;

        if matches!(address, 2 | 3) {
            while self.vdc.is_cpu_read_blocked() {
                log::trace!(
                    "CPU read {address} stalling! Line {} cycles {}",
                    self.state.scanline,
                    self.state.scanline_mclk
                );

                *bus_cycles += u64::from(self.vce.dot_clock_divider());
                self.step_to(*bus_cycles, irq1_pending);
            }
        }

        match address {
            0 => self.vdc.read_status(),
            1 => 0x00, // Unused
            2 => self.vdc.read_data(WordByte::Low),
            3 => self.vdc.read_data(WordByte::High),
            _ => unreachable!("address & 3 is always <= 3"),
        }
    }

    // $1FE000-$1FE003: VDC ports
    pub fn write_vdc(
        &mut self,
        address: u32,
        value: u8,
        bus_cycles: &mut u64,
        irq1_pending: &mut bool,
    ) {
        log::trace!(
            "VDC register write {address:06X} {value:02X}, line {} mclk {}",
            self.state.scanline,
            self.state.scanline_mclk
        );

        let address = address & 3;

        if matches!(address, 2 | 3) {
            while self.vdc.is_cpu_write_blocked() {
                log::trace!(
                    "CPU write {address} {value:02X} stalling! Line {} cycles {}",
                    self.state.scanline,
                    self.state.scanline_mclk
                );

                *bus_cycles += u64::from(self.vce.dot_clock_divider());
                self.step_to(*bus_cycles, irq1_pending);
            }
        }

        match address {
            0 => self.vdc.write_register_select(value),
            1 => {} // Unused
            2 => self.vdc.write_data(value, WordByte::Low),
            3 => self.vdc.write_data(value, WordByte::High),
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }

    // $1FE400-$1FE407: VCE ports
    #[allow(clippy::match_same_arms)]
    pub fn read_vce(&mut self, address: u32) -> u8 {
        log::trace!(
            "VCE register read {address:06X}, line {} mclk {}",
            self.state.scanline,
            self.state.scanline_mclk
        );

        match address & 7 {
            0..=3 => 0xFF, // Write-only / unused
            4 => self.vce.read_color_data(WordByte::Low),
            5 => self.vce.read_color_data(WordByte::High),
            6 | 7 => 0xFF, // Unused
            _ => unreachable!("value & 7 is always <= 7"),
        }
    }

    // $1FE400-$1FE407: VCE ports
    #[allow(clippy::match_same_arms)]
    pub fn write_vce(&mut self, address: u32, value: u8) {
        log::trace!(
            "VCE register write {address:06X} {value:02X}, line {} mclk {}",
            self.state.scanline,
            self.state.scanline_mclk
        );

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

    pub fn reload_config(&mut self, config: PceEmulatorConfig) {
        self.crop_overscan = config.crop_overscan;
        self.vdc.reload_config(config);
    }

    pub fn dump_vram(&self, palette: u16, out: &mut [[Color; 64]]) {
        self.vdc.dump_vram(palette, out, &self.vce);
    }

    pub fn dump_palettes(&self, out: &mut [Color]) {
        self.vce.dump_palettes(out);
    }
}
