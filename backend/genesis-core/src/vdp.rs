//! Genesis VDP (video display processor)

mod colors;
mod debug;
mod dma;
mod fifo;
mod registers;
mod render;

use crate::memory::{Memory, PhysicalMedium};
use crate::vdp::colors::ColorModifier;
use crate::vdp::dma::{DmaTracker, LineType};
use crate::vdp::fifo::FifoTracker;
use crate::vdp::registers::{
    DmaMode, HorizontalDisplaySize, InterlacingMode, Registers, VerticalDisplaySize,
};
use crate::vdp::render::{RenderingArgs, SpriteState};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{Color, TimingMode};
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::ops::{Deref, DerefMut};
use z80_emu::traits::InterruptLine;

const VRAM_LEN: usize = 64 * 1024;
const CRAM_LEN_WORDS: usize = 64;
const VSRAM_LEN: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum ControlWriteFlag {
    First,
    Second,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum DataPortMode {
    Read,
    Write,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum DataPortLocation {
    Vram,
    Cram,
    Vsram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum ActiveDma {
    MemoryToVram,
    VramFill(u16),
    VramCopy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum PendingWrite {
    Control(u16),
    Data(u16),
}

impl Default for PendingWrite {
    fn default() -> Self {
        Self::Control(0)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct InternalState {
    control_write_flag: ControlWriteFlag,
    code: u8,
    data_port_mode: DataPortMode,
    data_port_location: DataPortLocation,
    data_address: u16,
    latched_high_address_bits: u16,
    v_interrupt_pending: bool,
    h_interrupt_pending: bool,
    h_interrupt_counter: u16,
    latched_hv_counter: Option<u16>,
    scanline: u16,
    pending_dma: Option<ActiveDma>,
    pending_writes: Vec<PendingWrite>,
    frame_count: u64,
    // Marks whether a frame has been completed so that frames don't get double rendered if
    // a game switches from V28 to V30 mode on scanlines 224-239
    frame_completed: bool,
}

impl InternalState {
    fn new() -> Self {
        Self {
            control_write_flag: ControlWriteFlag::First,
            code: 0,
            data_port_mode: DataPortMode::Write,
            data_port_location: DataPortLocation::Vram,
            data_address: 0,
            latched_high_address_bits: 0,
            v_interrupt_pending: false,
            h_interrupt_pending: false,
            h_interrupt_counter: 0,
            latched_hv_counter: None,
            scanline: 0,
            pending_dma: None,
            pending_writes: Vec::with_capacity(10),
            frame_count: 0,
            frame_completed: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct CachedSpriteData {
    v_position: u16,
    h_size_cells: u8,
    v_size_cells: u8,
    link_data: u8,
}

impl CachedSpriteData {
    fn update_first_word(&mut self, msb: u8, lsb: u8) {
        self.v_position = u16::from_be_bytes([msb & 0x03, lsb]);
    }

    fn update_second_word(&mut self, msb: u8, lsb: u8) {
        self.h_size_cells = ((msb >> 2) & 0x03) + 1;
        self.v_size_cells = (msb & 0x03) + 1;
        self.link_data = lsb & 0x7F;
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct SpriteData {
    pattern_generator: u16,
    v_position: u16,
    h_position: u16,
    h_size_cells: u8,
    v_size_cells: u8,
    palette: u8,
    vertical_flip: bool,
    horizontal_flip: bool,
    priority: bool,
    link_data: u8,
    // Set if this sprite gets cut off because of the pixels-per-scanline limit
    partial_width: Option<u16>,
}

impl SpriteData {
    fn create(cached_data: CachedSpriteData, uncached_bytes: &[u8]) -> Self {
        // 3rd word
        let priority = uncached_bytes[0].bit(7);
        let palette = (uncached_bytes[0] >> 5) & 0x03;
        let vertical_flip = uncached_bytes[0].bit(4);
        let horizontal_flip = uncached_bytes[0].bit(3);
        let pattern_generator = u16::from_be_bytes([uncached_bytes[0] & 0x07, uncached_bytes[1]]);

        // 4th word
        let h_position = u16::from_be_bytes([uncached_bytes[2] & 0x01, uncached_bytes[3]]);

        Self {
            pattern_generator,
            v_position: cached_data.v_position,
            h_position,
            h_size_cells: cached_data.h_size_cells,
            v_size_cells: cached_data.v_size_cells,
            palette,
            vertical_flip,
            horizontal_flip,
            priority,
            link_data: cached_data.link_data,
            // Will maybe get set later
            partial_width: None,
        }
    }

    fn v_position(&self, interlacing_mode: InterlacingMode) -> u16 {
        // V position is 9 bits in progressive mode and interlaced mode 1, and 10 bits in
        // interlaced mode 2
        match interlacing_mode {
            InterlacingMode::Progressive | InterlacingMode::Interlaced => self.v_position & 0x1FF,
            InterlacingMode::InterlacedDouble => self.v_position & 0x3FF,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct SpriteBitSet([u64; 5]);

impl SpriteBitSet {
    const LEN: u16 = 64 * 5;

    fn new() -> Self {
        Self([0; 5])
    }

    fn clear(&mut self) {
        self.0 = [0; 5];
    }

    fn set(&mut self, bit: u16) {
        self.0[(bit / 64) as usize] |= 1 << (bit % 64);
    }

    fn get(&self, bit: u16) -> bool {
        self.0[(bit / 64) as usize] & (1 << (bit % 64)) != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VdpTickEffect {
    None,
    FrameComplete,
}

#[derive(Debug, Clone, FakeEncode, FakeDecode)]
struct FrameBuffer(Box<[Color; FRAME_BUFFER_LEN]>);

impl FrameBuffer {
    fn new() -> Self {
        Self(vec![Color::default(); FRAME_BUFFER_LEN].into_boxed_slice().try_into().unwrap())
    }
}

impl Default for FrameBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for FrameBuffer {
    type Target = Box<[Color; FRAME_BUFFER_LEN]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for FrameBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

const MAX_SCREEN_WIDTH: usize = 320;
const MAX_SCREEN_HEIGHT: usize = 240;

// Double screen height to account for interlaced 2x mode
const FRAME_BUFFER_LEN: usize = MAX_SCREEN_WIDTH * MAX_SCREEN_HEIGHT * 2;

const MCLK_CYCLES_PER_SCANLINE: u64 = 3420;
const ACTIVE_MCLK_CYCLES_PER_SCANLINE: u64 = 2560;
const NTSC_SCANLINES_PER_FRAME: u16 = 262;
const PAL_SCANLINES_PER_FRAME: u16 = 313;

// 36 CPU cycles
const REGISTER_LATCH_DELAY_MCLK: u64 = 36 * 7;

const MAX_SPRITES_PER_FRAME: usize = 80;

// Master clock cycle on which to trigger VINT on scanline 224/240.
const V_INTERRUPT_DELAY: u64 = 48;

trait TimingModeExt: Copy {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VdpConfig {
    pub enforce_sprite_limits: bool,
    pub emulate_non_linear_dac: bool,
}

type Vram = [u8; VRAM_LEN];
type Cram = [u16; CRAM_LEN_WORDS];
type Vsram = [u8; VSRAM_LEN];

#[derive(Debug, Clone, Encode, Decode)]
pub struct Vdp {
    frame_buffer: FrameBuffer,
    vram: Box<Vram>,
    cram: Box<Cram>,
    vsram: Box<Vsram>,
    timing_mode: TimingMode,
    state: InternalState,
    sprite_state: SpriteState,
    registers: Registers,
    latched_registers: Registers,
    latched_full_screen_v_scroll: (u16, u16),
    cached_sprite_attributes: Box<[CachedSpriteData; MAX_SPRITES_PER_FRAME]>,
    sprite_buffer: Vec<SpriteData>,
    sprite_bit_set: SpriteBitSet,
    enforce_sprite_limits: bool,
    emulate_non_linear_dac: bool,
    master_clock_cycles: u64,
    dma_tracker: DmaTracker,
    fifo_tracker: FifoTracker,
}

impl Vdp {
    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn new(timing_mode: TimingMode, config: VdpConfig) -> Self {
        Self {
            frame_buffer: FrameBuffer::new(),
            vram: vec![0; VRAM_LEN].into_boxed_slice().try_into().unwrap(),
            cram: vec![0; CRAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
            vsram: vec![0; VSRAM_LEN].into_boxed_slice().try_into().unwrap(),
            timing_mode,
            state: InternalState::new(),
            sprite_state: SpriteState::default(),
            registers: Registers::new(),
            latched_registers: Registers::new(),
            latched_full_screen_v_scroll: (0, 0),
            cached_sprite_attributes: vec![CachedSpriteData::default(); MAX_SPRITES_PER_FRAME]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            sprite_buffer: Vec::with_capacity(MAX_SPRITES_PER_FRAME),
            sprite_bit_set: SpriteBitSet::new(),
            enforce_sprite_limits: config.enforce_sprite_limits,
            emulate_non_linear_dac: config.emulate_non_linear_dac,
            master_clock_cycles: 0,
            dma_tracker: DmaTracker::new(),
            fifo_tracker: FifoTracker::new(),
        }
    }

    pub fn write_control(&mut self, value: u16) {
        log::trace!(
            "VDP control write on scanline {} / mclk {} / pixel {}: {value:04X} (flag = {:?}, dma_enabled = {})",
            self.state.scanline,
            self.master_clock_cycles % MCLK_CYCLES_PER_SCANLINE,
            scanline_mclk_to_pixel(
                self.master_clock_cycles % MCLK_CYCLES_PER_SCANLINE,
                self.registers.horizontal_display_size
            ),
            self.state.control_write_flag,
            self.registers.dma_enabled
        );

        if self.maybe_push_pending_write(PendingWrite::Control(value)) {
            return;
        }

        match self.state.control_write_flag {
            ControlWriteFlag::First => {
                // Always latch lowest 2 code bits, even if this is a register write
                self.state.code = (self.state.code & 0xFC) | ((value >> 14) & 0x03) as u8;
                self.update_data_port_location();

                if value & 0xE000 == 0x8000 {
                    // Register set

                    let register_number = ((value >> 8) & 0x1F) as u8;
                    self.registers.write_internal_register(register_number, value as u8);

                    if self.registers.hv_counter_stopped && self.state.latched_hv_counter.is_none()
                    {
                        self.state.latched_hv_counter = Some(self.hv_counter());
                    } else if !self.registers.hv_counter_stopped
                        && self.state.latched_hv_counter.is_some()
                    {
                        self.state.latched_hv_counter = None;
                    }

                    self.update_latched_registers_if_necessary(register_number);

                    // Update enabled pixels in sprite state if register #1 was written
                    if register_number == 1 {
                        let pixel = scanline_mclk_to_pixel(
                            self.master_clock_cycles % MCLK_CYCLES_PER_SCANLINE,
                            self.registers.horizontal_display_size,
                        );
                        self.sprite_state
                            .handle_display_enabled_write(self.registers.display_enabled, pixel);
                    }
                } else {
                    // First word of command write
                    self.state.data_address =
                        (self.state.latched_high_address_bits) | (value & 0x3FFF);

                    self.state.control_write_flag = ControlWriteFlag::Second;
                }
            }
            ControlWriteFlag::Second => {
                let high_address_bits = value << 14;
                self.state.latched_high_address_bits = high_address_bits;
                self.state.data_address = (self.state.data_address & 0x3FFF) | high_address_bits;
                self.state.control_write_flag = ControlWriteFlag::First;

                self.state.code = (((value >> 2) & 0x3C) as u8) | (self.state.code & 0x03);
                self.update_data_port_location();

                if self.state.code.bit(5)
                    && self.registers.dma_enabled
                    && self.registers.dma_mode != DmaMode::VramFill
                {
                    // This is a DMA initiation, not a normal control write
                    log::trace!("DMA transfer initiated, mode={:?}", self.registers.dma_mode);
                    self.state.pending_dma = match self.registers.dma_mode {
                        DmaMode::MemoryToVram => Some(ActiveDma::MemoryToVram),
                        DmaMode::VramCopy => Some(ActiveDma::VramCopy),
                        DmaMode::VramFill => unreachable!("dma_mode != VramFill"),
                    }
                }
            }
        }
    }

    fn update_latched_registers_if_necessary(&mut self, register_number: u8) {
        // Writing to register #2, #3, or #4 immediately updates the corresponding nametable address; these registers
        // are not latched.
        // Writing to register #1 immediately updates the display enabled flag.
        // Writing to register #7 immediately updates the background color.
        // Other register writes do not take effect until the next scanline.
        match register_number {
            1 => {
                self.latched_registers.display_enabled = self.registers.display_enabled;
            }
            2 => {
                self.latched_registers.scroll_a_base_nt_addr = self.registers.scroll_a_base_nt_addr;
            }
            3 => {
                self.latched_registers.window_base_nt_addr = self.registers.window_base_nt_addr;
            }
            4 => {
                self.latched_registers.scroll_b_base_nt_addr = self.registers.scroll_b_base_nt_addr;
            }
            7 => {
                self.latched_registers.background_palette = self.registers.background_palette;
                self.latched_registers.background_color_id = self.registers.background_color_id;
            }
            _ => return,
        }

        // If this write occurred during active display, re-render the current scanline starting from the current pixel
        if self.state.scanline < self.latched_registers.vertical_display_size.active_scanlines()
            && (self.master_clock_cycles % MCLK_CYCLES_PER_SCANLINE)
                < ACTIVE_MCLK_CYCLES_PER_SCANLINE
        {
            let mclk_per_pixel =
                self.latched_registers.horizontal_display_size.mclk_cycles_per_pixel();
            self.render_scanline_from_pixel(
                self.state.scanline,
                ((self.master_clock_cycles % MCLK_CYCLES_PER_SCANLINE) / mclk_per_pixel) as u16,
            );
        }
    }

    fn update_data_port_location(&mut self) {
        let (data_port_location, data_port_mode) = match self.state.code & 0x0F {
            0x01 => (DataPortLocation::Vram, DataPortMode::Write),
            0x03 => (DataPortLocation::Cram, DataPortMode::Write),
            0x05 => (DataPortLocation::Vsram, DataPortMode::Write),
            0x00 => (DataPortLocation::Vram, DataPortMode::Read),
            0x08 => (DataPortLocation::Cram, DataPortMode::Read),
            0x04 => (DataPortLocation::Vsram, DataPortMode::Read),
            _ => {
                // Invalid code
                (DataPortLocation::Vram, DataPortMode::Invalid)
            }
        };

        self.state.data_port_location = data_port_location;
        self.state.data_port_mode = data_port_mode;

        log::trace!(
            "Set data port location to {data_port_location:?} and mode to {data_port_mode:?}"
        );
    }

    pub fn read_data(&mut self) -> u16 {
        log::trace!("VDP data read");

        // Reset write flag
        self.state.control_write_flag = ControlWriteFlag::First;

        if self.state.data_port_mode != DataPortMode::Read {
            return 0xFFFF;
        }

        self.dma_tracker.record_data_port_read();

        let data_port_location = self.state.data_port_location;
        let data = match data_port_location {
            DataPortLocation::Vram => {
                // VRAM reads/writes ignore A0
                let address = (self.state.data_address & !0x01) as usize;
                u16::from_be_bytes([self.vram[address], self.vram[(address + 1) & 0xFFFF]])
            }
            DataPortLocation::Cram => {
                let address = (self.state.data_address & 0x7F) as usize;
                if !address.bit(0) {
                    self.cram[address >> 1]
                } else {
                    let msb_in_low_byte = self.cram[address >> 1] & 0x00FF;
                    let lsb_in_high_byte = self.cram[((address + 1) & 0x7F) >> 1] & 0xFF00;
                    (msb_in_low_byte | lsb_in_high_byte).swap_bytes()
                }
            }
            DataPortLocation::Vsram => {
                let address = (self.state.data_address as usize) % VSRAM_LEN;
                u16::from_be_bytes([self.vsram[address], self.vsram[(address + 1) % VSRAM_LEN]])
            }
        };

        self.increment_data_address();

        let line_type = LineType::from_vdp(self);
        self.fifo_tracker.record_access(line_type, data_port_location);

        data
    }

    pub fn write_data(&mut self, value: u16) {
        log::trace!("VDP data write on scanline {}: {value:04X}", self.state.scanline);

        // Reset write flag
        self.state.control_write_flag = ControlWriteFlag::First;

        if self.state.data_port_mode != DataPortMode::Write {
            return;
        }

        if self.maybe_push_pending_write(PendingWrite::Data(value)) {
            return;
        }

        if self.state.code.bit(5)
            && self.registers.dma_enabled
            && self.registers.dma_mode == DmaMode::VramFill
        {
            log::trace!("Initiated VRAM fill DMA with fill data = {value:04X}");
            self.state.pending_dma = Some(ActiveDma::VramFill(value));
            return;
        }

        let data_port_location = self.state.data_port_location;
        match data_port_location {
            DataPortLocation::Vram => {
                // VRAM reads/writes ignore A0
                log::trace!("Writing to {:04X} in VRAM", self.state.data_address);
                self.write_vram_word(self.state.data_address, value);
            }
            DataPortLocation::Cram => {
                let address = (self.state.data_address & 0x7F) as usize;
                log::trace!("Writing to {address:02X} in CRAM");
                self.write_cram_word(self.state.data_address, value);
            }
            DataPortLocation::Vsram => {
                let address = (self.state.data_address as usize) % VSRAM_LEN;
                log::trace!("Writing to {address:02X} in VSRAM");
                let [msb, lsb] = value.to_be_bytes();
                self.vsram[address] = msb;
                self.vsram[(address + 1) % VSRAM_LEN] = lsb;
            }
        }

        self.increment_data_address();

        let line_type = LineType::from_vdp(self);
        self.fifo_tracker.record_access(line_type, data_port_location);
    }

    fn maybe_push_pending_write(&mut self, write: PendingWrite) -> bool {
        if self.state.pending_dma.is_some()
            || (self.dma_tracker.is_in_progress() && matches!(write, PendingWrite::Data(..)))
        {
            self.state.pending_writes.push(write);
            true
        } else {
            false
        }
    }

    pub fn read_status(&mut self) -> u16 {
        log::trace!("VDP status register read");

        let interlaced_odd =
            self.registers.interlacing_mode.is_interlaced() && self.state.frame_count % 2 == 1;

        let scanline_mclk = self.master_clock_cycles % MCLK_CYCLES_PER_SCANLINE;
        let v_counter: u16 = self.v_counter(scanline_mclk).into();
        let vblank_flag = match self.timing_mode {
            TimingMode::Ntsc => {
                v_counter >= VerticalDisplaySize::TwentyEightCell.active_scanlines()
                    && v_counter != 0xFF
            }
            TimingMode::Pal => {
                let active_scanlines = self.registers.vertical_display_size.active_scanlines();
                // This OR is necessary because the PAL V counter briefly wraps around to $00-$0A
                // during VBlank.
                // >300 comparison is because the V counter hits 0xFF twice, once at scanline 255
                // and again at scanline 312.
                (v_counter >= active_scanlines || self.state.scanline > active_scanlines)
                    && !((v_counter == 0x00 || v_counter == 0xFF) && self.state.scanline > 300)
            }
        };

        // HBlank flag is based on the H counter crossing specific values, not on mclk being >= 2560
        let h_counter = self.h_counter(scanline_mclk);
        let hblank_flag = match self.registers.horizontal_display_size {
            HorizontalDisplaySize::ThirtyTwoCell => h_counter <= 0x04 || h_counter >= 0x93,
            HorizontalDisplaySize::FortyCell => h_counter <= 0x05 || h_counter >= 0xB3,
        };

        let status = (u16::from(self.fifo_tracker.is_empty()) << 9)
            | (u16::from(self.fifo_tracker.is_full()) << 8)
            | (u16::from(self.state.v_interrupt_pending) << 7)
            | (u16::from(self.sprite_state.overflow_flag()) << 6)
            | (u16::from(self.sprite_state.collision_flag()) << 5)
            | (u16::from(interlaced_odd) << 4)
            | (u16::from(vblank_flag) << 3)
            | (u16::from(hblank_flag) << 2)
            | (u16::from(self.dma_tracker.is_in_progress()) << 1)
            | u16::from(self.timing_mode == TimingMode::Pal);

        // Reading status register clears the sprite overflow and collision flags
        self.sprite_state.clear_status_flags();

        // Reset control write flag
        self.state.control_write_flag = ControlWriteFlag::First;

        status
    }

    #[must_use]
    pub fn hv_counter(&self) -> u16 {
        if let Some(latched_hv_counter) = self.state.latched_hv_counter {
            return latched_hv_counter;
        }

        let scanline_mclk = self.master_clock_cycles % MCLK_CYCLES_PER_SCANLINE;

        let h_counter = self.h_counter(scanline_mclk);
        let v_counter = self.v_counter(scanline_mclk);

        log::trace!(
            "HV counter read on scanline {}; H={h_counter:02X}, V={v_counter:02X}",
            self.state.scanline
        );

        u16::from_be_bytes([v_counter, h_counter])
    }

    #[inline]
    fn h_counter(&self, scanline_mclk: u64) -> u8 {
        // Values from https://gendev.spritesmind.net/forum/viewtopic.php?t=768
        match self.registers.horizontal_display_size {
            HorizontalDisplaySize::ThirtyTwoCell => {
                let h = (scanline_mclk / 20) as u8;
                if h <= 0x93 { h } else { h + (0xE9 - 0x94) }
            }
            HorizontalDisplaySize::FortyCell => {
                let pixel = scanline_mclk_to_pixel_h40(scanline_mclk);
                match pixel {
                    0..=364 => (pixel / 2) as u8,
                    365..=419 => (0xE4 + (pixel - 364) / 2) as u8,
                    _ => panic!("H40 pixel values should always be < 420"),
                }
            }
        }
    }

    #[inline]
    fn v_counter(&self, scanline_mclk: u64) -> u8 {
        // Values from https://gendev.spritesmind.net/forum/viewtopic.php?t=768

        // V counter increments for the next line shortly after the start of HBlank
        let in_hblank = scanline_mclk >= ACTIVE_MCLK_CYCLES_PER_SCANLINE;
        let scanline = if in_hblank {
            (self.state.scanline + 1) % self.timing_mode.scanlines_per_frame()
        } else {
            self.state.scanline
        };

        match self.registers.interlacing_mode {
            InterlacingMode::Progressive | InterlacingMode::Interlaced => {
                match (self.timing_mode, self.registers.vertical_display_size) {
                    (TimingMode::Ntsc, _) => {
                        if scanline <= 0xEA {
                            scanline as u8
                        } else {
                            (scanline - 6) as u8
                        }
                    }
                    (TimingMode::Pal, VerticalDisplaySize::TwentyEightCell) => {
                        if scanline <= 0x102 {
                            scanline as u8
                        } else {
                            (scanline - (0x103 - 0xCA)) as u8
                        }
                    }
                    (TimingMode::Pal, VerticalDisplaySize::ThirtyCell) => {
                        if scanline <= 0x10A {
                            scanline as u8
                        } else {
                            (scanline - (0x10B - 0xD2)) as u8
                        }
                    }
                }
            }
            InterlacingMode::InterlacedDouble => {
                // TODO this is not accurate
                let scanline = scanline << 1;
                (scanline as u8) | u8::from(scanline.bit(8))
            }
        }
    }

    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn tick<Medium: PhysicalMedium>(
        &mut self,
        master_clock_cycles: u64,
        memory: &mut Memory<Medium>,
    ) -> VdpTickEffect {
        // The longest 68k instruction (DIVS (xxx).l, Dn) takes 172 68k cycles / 1204 mclk cycles
        assert!(master_clock_cycles < 1250);

        // Count down DMA time before checking if a DMA was initiated in the last CPU instruction
        let line_type = LineType::from_vdp(self);
        let h_display_size = self.registers.horizontal_display_size;
        self.dma_tracker.tick(master_clock_cycles, h_display_size, line_type);
        self.fifo_tracker.tick(master_clock_cycles, h_display_size, line_type);

        if let Some(active_dma) = self.state.pending_dma {
            // TODO accurate DMA timing
            self.run_dma(memory, active_dma);
        }

        if !self.dma_tracker.is_in_progress() && !self.state.pending_writes.is_empty() {
            self.apply_pending_writes();
        }

        let scanlines_per_frame = self.timing_mode.scanlines_per_frame();
        let active_scanlines = self.registers.vertical_display_size.active_scanlines();

        let prev_mclk_cycles = self.master_clock_cycles;
        self.master_clock_cycles += master_clock_cycles;

        let prev_scanline_mclk = prev_mclk_cycles % MCLK_CYCLES_PER_SCANLINE;
        let scanline_mclk = prev_scanline_mclk + master_clock_cycles;

        if prev_scanline_mclk < ACTIVE_MCLK_CYCLES_PER_SCANLINE
            && scanline_mclk >= ACTIVE_MCLK_CYCLES_PER_SCANLINE
        {
            // HBlank start
            self.sprite_state.handle_hblank_start(
                self.registers.horizontal_display_size,
                self.registers.display_enabled,
            );
        }

        // H interrupts occur a set number of mclk cycles after the end of the active display,
        // not right at the start of HBlank
        let h_interrupt_delay = match self.registers.horizontal_display_size {
            // 12 pixels after active display
            HorizontalDisplaySize::ThirtyTwoCell => 120,
            // 16 pixels after active display
            HorizontalDisplaySize::FortyCell => 128,
        };
        if prev_scanline_mclk < ACTIVE_MCLK_CYCLES_PER_SCANLINE + h_interrupt_delay
            && scanline_mclk >= ACTIVE_MCLK_CYCLES_PER_SCANLINE + h_interrupt_delay
        {
            // Check if an H interrupt has occurred
            if self.state.scanline < active_scanlines
                || self.state.scanline == scanlines_per_frame - 1
            {
                if self.state.h_interrupt_counter == 0 {
                    self.state.h_interrupt_counter = self.registers.h_interrupt_interval;

                    log::trace!("Generating H interrupt (scanline {})", self.state.scanline);
                    self.state.h_interrupt_pending = true;
                } else {
                    self.state.h_interrupt_counter -= 1;
                }
            } else {
                // H interrupt counter is constantly refreshed during VBlank
                self.state.h_interrupt_counter = self.registers.h_interrupt_interval;
            }
        }

        if prev_scanline_mclk
            < ACTIVE_MCLK_CYCLES_PER_SCANLINE + h_interrupt_delay + REGISTER_LATCH_DELAY_MCLK
            && scanline_mclk
                >= ACTIVE_MCLK_CYCLES_PER_SCANLINE + h_interrupt_delay + REGISTER_LATCH_DELAY_MCLK
        {
            // Almost all VDP registers and the full screen V scroll values are latched within the 36 CPU cycles after
            // HINT is generated. Changing values after this point will not take effect until after the next scanline
            // is rendered.
            // The only VDP registers that are not latched are the nametable addresses, the display enabled bit, and
            // the background color.
            self.latched_registers = self.registers.clone();
            self.latched_full_screen_v_scroll = (
                u16::from_be_bytes([self.vsram[0], self.vsram[1]]),
                u16::from_be_bytes([self.vsram[2], self.vsram[3]]),
            );
        }

        // Check if a V interrupt has triggered
        if self.state.scanline == active_scanlines
            && prev_scanline_mclk < V_INTERRUPT_DELAY
            && scanline_mclk >= V_INTERRUPT_DELAY
        {
            log::trace!("Generating V interrupt");
            self.state.v_interrupt_pending = true;
        }

        // Check if the VDP has advanced to a new scanline
        if scanline_mclk >= MCLK_CYCLES_PER_SCANLINE {
            self.sprite_state.handle_line_end(self.registers.horizontal_display_size);
            self.render_next_scanline();

            self.state.scanline += 1;
            if self.state.scanline == scanlines_per_frame {
                self.state.scanline = 0;
                self.state.frame_count += 1;
                self.state.frame_completed = false;
            }

            // Check if we already passed the VINT threshold
            if self.state.scanline == active_scanlines
                && prev_scanline_mclk + master_clock_cycles - MCLK_CYCLES_PER_SCANLINE
                    >= V_INTERRUPT_DELAY
            {
                log::trace!("Generating V interrupt");
                self.state.v_interrupt_pending = true;
            }

            if self.state.scanline == active_scanlines && !self.state.frame_completed {
                self.state.frame_completed = true;
                return VdpTickEffect::FrameComplete;
            }
        }

        VdpTickEffect::None
    }

    fn apply_pending_writes(&mut self) {
        let mut pending_writes = [PendingWrite::default(); 10];
        let pending_writes_len = self.state.pending_writes.len();
        pending_writes[..pending_writes_len].copy_from_slice(&self.state.pending_writes);
        self.state.pending_writes.clear();

        for &pending_write in &pending_writes[..pending_writes_len] {
            match pending_write {
                PendingWrite::Control(value) => {
                    self.write_control(value);
                }
                PendingWrite::Data(value) => {
                    self.write_data(value);
                }
            }
        }
    }

    fn increment_data_address(&mut self) {
        self.state.data_address =
            self.state.data_address.wrapping_add(self.registers.data_port_auto_increment);
    }

    fn write_vram_word(&mut self, address: u16, value: u16) {
        let [msb, lsb] = value.to_be_bytes();
        self.vram[address as usize] = msb;
        self.vram[(address ^ 0x01) as usize] = lsb;

        self.maybe_update_sprite_cache(address);
    }

    fn write_cram_word(&mut self, address: u16, value: u16) {
        if !address.bit(0) {
            self.cram[((address & 0x7F) >> 1) as usize] = value;
        } else {
            let msb_addr = ((address & 0x7F) >> 1) as usize;
            self.cram[msb_addr] = (self.cram[msb_addr] & 0xFF00) | (value >> 8);

            let lsb_addr = (((address + 1) & 0x7F) >> 1) as usize;
            self.cram[lsb_addr] = (self.cram[lsb_addr] & 0x00FF) | (value << 8);
        }
    }

    #[inline]
    fn maybe_update_sprite_cache(&mut self, address: u16) {
        let sprite_table_addr = self.registers.masked_sprite_attribute_table_addr();
        let h_size = self.registers.horizontal_display_size;

        let (sprite_table_end, overflowed) =
            sprite_table_addr.overflowing_add(8 * h_size.sprite_table_len());
        let is_in_sprite_table = if overflowed {
            // Address overflowed; this can happen if a game puts the SAT at the very end of VRAM (e.g. Snatcher)
            // Address overflow is only possible in H32 mode when the table is located at $FE00-$FFFF, so simply check
            // if address is past start address
            address >= sprite_table_addr
        } else {
            (sprite_table_addr..sprite_table_end).contains(&address)
        };

        if !address.bit(2) && is_in_sprite_table {
            let idx = ((address - sprite_table_addr) / 8) as usize;
            let msb = self.vram[(address & !0x01) as usize];
            let lsb = self.vram[(address | 0x01) as usize];
            if !address.bit(1) {
                self.cached_sprite_attributes[idx].update_first_word(msb, lsb);
            } else {
                self.cached_sprite_attributes[idx].update_second_word(msb, lsb);
            }
        }
    }

    fn in_vblank(&self) -> bool {
        self.state.scanline >= self.registers.vertical_display_size.active_scanlines()
            && self.state.scanline < self.timing_mode.scanlines_per_frame() - 1
    }

    #[must_use]
    pub fn m68k_interrupt_level(&self) -> u8 {
        // TODO external interrupts at level 2
        if self.state.v_interrupt_pending && self.registers.v_interrupt_enabled {
            6
        } else if self.state.h_interrupt_pending && self.registers.h_interrupt_enabled {
            4
        } else {
            0
        }
    }

    pub fn acknowledge_m68k_interrupt(&mut self) {
        let interrupt_level = self.m68k_interrupt_level();
        log::trace!("M68K interrupt acknowledged; level {interrupt_level}");
        if interrupt_level == 6 {
            self.state.v_interrupt_pending = false;
        } else if interrupt_level == 4 {
            self.state.h_interrupt_pending = false;
        }
    }

    #[must_use]
    pub fn should_halt_cpu(&self) -> bool {
        self.dma_tracker.should_halt_cpu(&self.state.pending_writes)
            || self.fifo_tracker.should_halt_cpu()
    }

    #[must_use]
    pub fn z80_interrupt_line(&self) -> InterruptLine {
        // Z80 INT line is low only during the first scanline of VBlank
        if self.state.scanline == self.registers.vertical_display_size.active_scanlines() {
            InterruptLine::Low
        } else {
            InterruptLine::High
        }
    }

    #[inline]
    fn render_next_scanline(&mut self) {
        match (self.timing_mode, self.registers.vertical_display_size, self.state.scanline) {
            (TimingMode::Ntsc, _, 261) | (TimingMode::Pal, _, 311) => {
                self.render_scanline_from_pixel(0, 0);
            }
            (_, VerticalDisplaySize::TwentyEightCell, scanline @ 0..=222)
            | (_, VerticalDisplaySize::ThirtyCell, scanline @ 0..=238) => {
                self.render_scanline_from_pixel(scanline + 1, 0);
            }
            _ => {}
        }
    }

    #[inline]
    fn render_scanline_from_pixel(&mut self, scanline: u16, pixel: u16) {
        let rendering_args = RenderingArgs {
            frame_buffer: &mut self.frame_buffer,
            sprite_buffer: &mut self.sprite_buffer,
            sprite_bit_set: &mut self.sprite_bit_set,
            sprite_state: &mut self.sprite_state,
            vram: &self.vram,
            cram: &self.cram,
            vsram: &self.vsram,
            registers: &self.latched_registers,
            cached_sprite_attributes: &self.cached_sprite_attributes,
            full_screen_v_scroll_a: self.latched_full_screen_v_scroll.0,
            full_screen_v_scroll_b: self.latched_full_screen_v_scroll.1,
            enforce_sprite_limits: self.enforce_sprite_limits,
            emulate_non_linear_dac: self.emulate_non_linear_dac,
        };
        render::render_scanline(rendering_args, scanline, pixel);
    }

    #[must_use]
    pub fn frame_buffer(&self) -> &[Color; FRAME_BUFFER_LEN] {
        &self.frame_buffer
    }

    #[must_use]
    pub fn screen_width(&self) -> u32 {
        self.registers.horizontal_display_size.active_display_pixels().into()
    }

    #[must_use]
    pub fn screen_height(&self) -> u32 {
        let screen_height: u32 = self.registers.vertical_display_size.active_scanlines().into();
        match self.registers.interlacing_mode {
            InterlacingMode::Progressive | InterlacingMode::Interlaced => screen_height,
            InterlacingMode::InterlacedDouble => 2 * screen_height,
        }
    }

    #[must_use]
    pub fn config(&self) -> VdpConfig {
        VdpConfig {
            enforce_sprite_limits: self.enforce_sprite_limits,
            emulate_non_linear_dac: self.emulate_non_linear_dac,
        }
    }

    pub fn reload_config(&mut self, config: VdpConfig) {
        self.enforce_sprite_limits = config.enforce_sprite_limits;
        self.emulate_non_linear_dac = config.emulate_non_linear_dac;
    }
}

fn scanline_mclk_to_pixel(scanline_mclk: u64, h_display_size: HorizontalDisplaySize) -> u16 {
    match h_display_size {
        HorizontalDisplaySize::ThirtyTwoCell => scanline_mclk_to_pixel_h32(scanline_mclk),
        HorizontalDisplaySize::FortyCell => scanline_mclk_to_pixel_h40(scanline_mclk),
    }
}

fn scanline_mclk_to_pixel_h32(scanline_mclk: u64) -> u16 {
    (scanline_mclk / 10) as u16
}

fn scanline_mclk_to_pixel_h40(scanline_mclk: u64) -> u16 {
    // Special cases due to pixel clock varying during HSYNC in H40 mode
    // https://gendev.spritesmind.net/forum/viewtopic.php?t=3221
    match scanline_mclk {
        // 320 pixels of active display + 14 pixels of right border + 9 pixels of right blanking,
        // all at mclk/8
        0..=2743 => (scanline_mclk / 8) as u16,
        // 34 pixels of HSYNC in a pattern of 1 mclk/8, 7 mclk/10, 2 mclk/9, 7 mclk/10
        2744..=3075 => {
            let hsync_mclk = scanline_mclk - 2744;
            let pattern_pixel = match hsync_mclk % 166 {
                0..=7 => 0,
                pattern_mclk @ 8..=77 => 1 + (pattern_mclk - 8) / 10,
                pattern_mclk @ 78..=95 => 8 + (pattern_mclk - 78) / 9,
                pattern_cmlk @ 96..=165 => 10 + (pattern_cmlk - 96) / 10,
                _ => unreachable!("value % 166 is always < 166"),
            };

            if hsync_mclk < 166 {
                343 + pattern_pixel as u16
            } else {
                343 + 17 + pattern_pixel as u16
            }
        }
        // 30 pixels of left blanking + 13 pixels of left border, all at mclk/8
        3076..=3419 => (377 + (scanline_mclk - 3076) / 8) as u16,
        _ => panic!("scanline mclk must be < 3420"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_vdp() -> Vdp {
        Vdp::new(
            TimingMode::Ntsc,
            VdpConfig { enforce_sprite_limits: true, emulate_non_linear_dac: false },
        )
    }

    #[test]
    fn h_counter_basic_functionality() {
        let mut vdp = new_vdp();

        vdp.registers.horizontal_display_size = HorizontalDisplaySize::ThirtyTwoCell;
        assert_eq!(vdp.h_counter(0), 0);
        assert_eq!(vdp.h_counter(80), 4);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE - 1), 0x7F);

        vdp.registers.horizontal_display_size = HorizontalDisplaySize::FortyCell;
        assert_eq!(vdp.h_counter(0), 0);
        assert_eq!(vdp.h_counter(80), 5);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE - 1), 0x9F);
    }

    #[test]
    fn h_counter_hblank_h32() {
        let mut vdp = new_vdp();

        vdp.registers.horizontal_display_size = HorizontalDisplaySize::ThirtyTwoCell;
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE), 0x80);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE + 80), 0x84);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE + 380), 0x93);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE + 400), 0xE9);
        assert_eq!(vdp.h_counter(MCLK_CYCLES_PER_SCANLINE - 41), 0xFD);
        assert_eq!(vdp.h_counter(MCLK_CYCLES_PER_SCANLINE - 21), 0xFE);
        assert_eq!(vdp.h_counter(MCLK_CYCLES_PER_SCANLINE - 1), 0xFF);
    }

    #[test]
    fn h_counter_hblank_h40() {
        let mut vdp = new_vdp();

        vdp.registers.horizontal_display_size = HorizontalDisplaySize::FortyCell;
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE), 0xA0);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE + 200), 0xAC);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE + 208), 0xAC);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE + 218), 0xAD);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE + 288), 0xB0);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE + 386), 0xB5);
        assert_eq!(vdp.h_counter(ACTIVE_MCLK_CYCLES_PER_SCANLINE + 404), 0xE4);
        assert_eq!(vdp.h_counter(MCLK_CYCLES_PER_SCANLINE - 17), 0xFE);
        assert_eq!(vdp.h_counter(MCLK_CYCLES_PER_SCANLINE - 16), 0xFF);
        assert_eq!(vdp.h_counter(MCLK_CYCLES_PER_SCANLINE - 1), 0xFF);
    }
}
