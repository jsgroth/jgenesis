//! Genesis VDP (video display processor)

mod colors;
mod debug;
mod dma;
mod registers;
mod render;
mod sprites;
mod timing;

use crate::memory::{Memory, PhysicalMedium};
use crate::vdp::colors::ColorModifier;
use crate::vdp::registers::{
    DebugRegister, DmaMode, H40_LEFT_BORDER, HorizontalDisplaySize, InterlacingMode,
    NTSC_BOTTOM_BORDER, NTSC_TOP_BORDER, PAL_V28_BOTTOM_BORDER, PAL_V28_TOP_BORDER,
    PAL_V30_BOTTOM_BORDER, PAL_V30_TOP_BORDER, RIGHT_BORDER, Registers, VerticalDisplaySize,
    VramSizeKb,
};
use crate::vdp::sprites::{SpriteBuffers, SpriteState};
use crate::vdp::timing::{DmaTracker, FifoTracker, LineType};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{Color, FrameSize, TimingMode};
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::array;
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
    data_address: u32,
    latched_high_address_bits: u32,
    // Whether the VINT flag in the status register reads 1
    vint_flag: bool,
    // Whether the VDP is actively raising INT6
    v_interrupt_pending: bool,
    delayed_v_interrupt: bool,
    delayed_v_interrupt_next: bool,
    h_interrupt_pending: bool,
    h_interrupt_counter: u16,
    latched_hv_counter: Option<u16>,
    v_border_forgotten: bool,
    top_border: u16,
    last_scroll_b_palettes: [u8; 2],
    last_h_scroll_a: u16,
    last_h_scroll_b: u16,
    scanline: u16,
    scanline_mclk_cycles: u64,
    pending_dma: Option<ActiveDma>,
    pending_writes: Vec<PendingWrite>,
    interlaced_frame: bool,
    frame_count: u64,
    vdp_event_idx: u8,
}

impl InternalState {
    fn new(timing_mode: TimingMode) -> Self {
        Self {
            control_write_flag: ControlWriteFlag::First,
            code: 0,
            data_port_mode: DataPortMode::Write,
            data_port_location: DataPortLocation::Vram,
            data_address: 0,
            latched_high_address_bits: 0,
            vint_flag: false,
            v_interrupt_pending: false,
            delayed_v_interrupt: false,
            delayed_v_interrupt_next: false,
            h_interrupt_pending: false,
            h_interrupt_counter: 0,
            latched_hv_counter: None,
            v_border_forgotten: false,
            top_border: VerticalDisplaySize::default().top_border(timing_mode),
            last_scroll_b_palettes: [0; 2],
            last_h_scroll_a: 0,
            last_h_scroll_b: 0,
            scanline: 0,
            scanline_mclk_cycles: 0,
            pending_dma: None,
            pending_writes: Vec::with_capacity(10),
            interlaced_frame: false,
            frame_count: 0,
            vdp_event_idx: 0,
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
    fn update_first_word_msb(&mut self, msb: u8) {
        self.v_position = (self.v_position & 0x00FF) | (u16::from(msb & 0x03) << 8);
    }

    fn update_first_word_lsb(&mut self, lsb: u8) {
        self.v_position = (self.v_position & 0xFF00) | u16::from(lsb);
    }

    fn update_second_word_msb(&mut self, msb: u8) {
        self.h_size_cells = ((msb >> 2) & 0x03) + 1;
        self.v_size_cells = (msb & 0x03) + 1;
    }

    fn update_second_word_lsb(&mut self, lsb: u8) {
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
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct TilePixel {
    color: u8,
    palette: u8,
    priority: bool,
}

#[derive(Debug, Clone, Encode, Decode)]
struct BgBuffers {
    plane_a_pixels: [TilePixel; MAX_SCREEN_WIDTH],
    plane_b_pixels: [TilePixel; MAX_SCREEN_WIDTH],
}

impl BgBuffers {
    fn new() -> Self {
        Self {
            plane_a_pixels: array::from_fn(|_| TilePixel::default()),
            plane_b_pixels: array::from_fn(|_| TilePixel::default()),
        }
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

const MAX_SCREEN_WIDTH: usize = 320 + H40_LEFT_BORDER as usize + RIGHT_BORDER as usize;
const MAX_SCREEN_HEIGHT: usize = 240 + PAL_V30_TOP_BORDER as usize + PAL_V30_BOTTOM_BORDER as usize;

// Double screen height to account for interlaced 2x mode
pub const FRAME_BUFFER_LEN: usize = MAX_SCREEN_WIDTH * MAX_SCREEN_HEIGHT * 2;

pub const MCLK_CYCLES_PER_SCANLINE: u64 = 3420;
pub const ACTIVE_MCLK_CYCLES_PER_SCANLINE: u64 = 2560;
pub const NTSC_SCANLINES_PER_FRAME: u16 = 262;
pub const PAL_SCANLINES_PER_FRAME: u16 = 313;

// 36 CPU cycles
const REGISTER_LATCH_DELAY_MCLK: u64 = 36 * 7;

const MAX_SPRITES_PER_FRAME: usize = 80;

// Master clock cycle on which to trigger VINT on scanline 224/240.
const V_INTERRUPT_DELAY: u64 = 48;

// Have the VINT flag in the VDP status register read 1 about 20 CPU cycles before the VDP raises
// the interrupt. This fixes several games failing to boot (e.g. Tyrants: Fight Through Time, Ex-Mutants)
const M68K_DIVIDER: u64 = crate::timing::NATIVE_M68K_DIVIDER;
const VINT_FLAG_MCLK: u64 = MCLK_CYCLES_PER_SCANLINE - (20 * M68K_DIVIDER - V_INTERRUPT_DELAY);

pub(crate) trait TimingModeExt: Copy {
    fn scanlines_per_frame(self) -> u16;

    fn rendered_lines_per_frame(self) -> u16;
}

impl TimingModeExt for TimingMode {
    fn scanlines_per_frame(self) -> u16 {
        match self {
            Self::Ntsc => NTSC_SCANLINES_PER_FRAME,
            Self::Pal => PAL_SCANLINES_PER_FRAME,
        }
    }

    // Includes border lines
    fn rendered_lines_per_frame(self) -> u16 {
        match self {
            Self::Ntsc => 224 + NTSC_TOP_BORDER + NTSC_BOTTOM_BORDER,
            Self::Pal => 224 + PAL_V28_TOP_BORDER + PAL_V28_BOTTOM_BORDER,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BorderSize {
    pub left: u32,
    pub right: u32,
    pub top: u32,
    pub bottom: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct VdpConfig {
    pub enforce_sprite_limits: bool,
    pub emulate_non_linear_dac: bool,
    pub deinterlace: bool,
    pub render_vertical_border: bool,
    pub render_horizontal_border: bool,
    pub plane_a_enabled: bool,
    pub plane_b_enabled: bool,
    pub sprites_enabled: bool,
    pub window_enabled: bool,
    pub backdrop_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum VdpEvent {
    VInterrupt,
    HBlankStart,
    HInterrupt,
    LatchRegisters,
    VIntStatusFlag,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
struct VdpEventWithTime {
    event: VdpEvent,
    mclk: u64,
}

impl VdpEventWithTime {
    const fn new(mclk: u64, event: VdpEvent) -> Self {
        Self { event, mclk }
    }
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
    debug_register: DebugRegister,
    latched_registers: Registers,
    latched_full_screen_v_scroll: (u16, u16),
    cached_sprite_attributes: Box<[CachedSpriteData; MAX_SPRITES_PER_FRAME]>,
    latched_sprite_attributes: Box<[CachedSpriteData; MAX_SPRITES_PER_FRAME]>,
    bg_buffers: Box<BgBuffers>,
    sprite_buffers: SpriteBuffers,
    interlaced_sprite_buffers: SpriteBuffers,
    config: VdpConfig,
    dma_tracker: DmaTracker,
    fifo_tracker: FifoTracker,
    vdp_event_times: [VdpEventWithTime; 6],
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
            state: InternalState::new(timing_mode),
            sprite_state: SpriteState::default(),
            registers: Registers::new(),
            debug_register: DebugRegister::new(),
            latched_registers: Registers::new(),
            latched_full_screen_v_scroll: (0, 0),
            cached_sprite_attributes: vec![CachedSpriteData::default(); MAX_SPRITES_PER_FRAME]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            latched_sprite_attributes: vec![CachedSpriteData::default(); MAX_SPRITES_PER_FRAME]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            bg_buffers: Box::new(BgBuffers::new()),
            sprite_buffers: SpriteBuffers::new(),
            interlaced_sprite_buffers: SpriteBuffers::new(),
            config,
            dma_tracker: DmaTracker::new(),
            fifo_tracker: FifoTracker::new(),
            vdp_event_times: Self::vdp_event_times(HorizontalDisplaySize::default()),
        }
    }

    pub fn write_control(&mut self, value: u16) {
        log::trace!(
            "VDP control write on scanline {} / mclk {} / pixel {}: {value:04X} (flag = {:?}, dma_enabled = {})",
            self.state.scanline,
            self.state.scanline_mclk_cycles,
            scanline_mclk_to_pixel(
                self.state.scanline_mclk_cycles,
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

                    let prev_v_interrupt_enabled = self.registers.v_interrupt_enabled;
                    let prev_v_display_size = self.registers.vertical_display_size;

                    let register_number = ((value >> 8) & 0x1F) as u8;
                    self.registers.write_internal_register(register_number, value as u8);
                    self.vdp_event_times =
                        Self::vdp_event_times(self.registers.horizontal_display_size);

                    if self.registers.hv_counter_stopped && self.state.latched_hv_counter.is_none()
                    {
                        self.state.latched_hv_counter = Some(self.hv_counter());
                    } else if !self.registers.hv_counter_stopped
                        && self.state.latched_hv_counter.is_some()
                    {
                        self.state.latched_hv_counter = None;
                    }

                    self.update_latched_registers_if_necessary(register_number);

                    if register_number == 1 {
                        // Update enabled pixels in sprite state if register #1 was written
                        let pixel = scanline_mclk_to_pixel(
                            self.state.scanline_mclk_cycles,
                            self.registers.horizontal_display_size,
                        );
                        self.sprite_state
                            .handle_display_enabled_write(self.registers.display_enabled, pixel);

                        // Mark vertical border "forgotten" if V size was switched from V30 to V28 between lines 224-239
                        // This has a few effects:
                        // - The HINT counter continues to tick down every line throughout VBlank instead of getting reset
                        // - The VDP continues to render normally inside the vertical border
                        if prev_v_display_size == VerticalDisplaySize::ThirtyCell
                            && self.registers.vertical_display_size
                                == VerticalDisplaySize::TwentyEightCell
                            && (VerticalDisplaySize::TwentyEightCell.active_scanlines()
                                ..VerticalDisplaySize::ThirtyCell.active_scanlines())
                                .contains(&self.state.scanline)
                        {
                            self.state.v_border_forgotten = true;
                        }

                        // V interrupts must be delayed by 1 CPU instruction if they are enabled
                        // while a V interrupt is pending; Sesame Street Counting Cafe depends on this
                        self.state.delayed_v_interrupt_next =
                            !prev_v_interrupt_enabled && self.registers.v_interrupt_enabled;
                    }
                } else {
                    // First word of command write
                    self.state.data_address =
                        (self.state.latched_high_address_bits) | u32::from(value & 0x3FFF);

                    self.state.control_write_flag = ControlWriteFlag::Second;
                }
            }
            ControlWriteFlag::Second => {
                let high_address_bits = u32::from(value & 0x7) << 14;
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
        macro_rules! relatch_registers {
            ($self:expr, [$($field:ident),* $(,)?]) => {
                {
                    let mut changed = false;
                    $(
                        changed |= self.latched_registers.$field != self.registers.$field;
                        self.latched_registers.$field = self.registers.$field;
                    )*
                    changed
                }
            }
        }

        let changed = match register_number {
            1 => {
                relatch_registers!(self, [display_enabled])
            }
            2 => {
                relatch_registers!(self, [scroll_a_base_nt_addr])
            }
            3 => {
                relatch_registers!(self, [window_base_nt_addr])
            }
            4 => {
                relatch_registers!(self, [scroll_b_base_nt_addr])
            }
            7 => {
                relatch_registers!(self, [background_palette, background_color_id])
            }
            _ => return,
        };

        // If this write occurred during active display, re-render the current scanline starting from the current pixel
        if changed
            && self.state.scanline < self.latched_registers.vertical_display_size.active_scanlines()
            && self.state.scanline_mclk_cycles < ACTIVE_MCLK_CYCLES_PER_SCANLINE
        {
            let mclk_per_pixel =
                self.latched_registers.horizontal_display_size.mclk_cycles_per_pixel();
            let pixel = (self.state.scanline_mclk_cycles / mclk_per_pixel) as u16;
            self.render_scanline(self.state.scanline, pixel);
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
                match self.registers.vram_size {
                    VramSizeKb::SixtyFour => {
                        // VRAM reads/writes ignore A0
                        let address = (self.state.data_address & 0xFFFE) as usize;
                        u16::from_be_bytes([self.vram[address], self.vram[(address + 1) & 0xFFFF]])
                    }
                    VramSizeKb::OneTwentyEight => {
                        // Reads in 128KB mode duplicate a single byte to both halves of the word
                        let address = convert_128kb_vram_address(self.state.data_address);
                        let byte = self.vram[address as usize];
                        u16::from_be_bytes([byte, byte])
                    }
                }
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
        self.fifo_tracker.record_access(line_type, data_port_location, self.registers.vram_size);

        data
    }

    pub fn write_data(&mut self, value: u16) {
        log::trace!(
            "VDP data write on scanline {} / mclk {} / pixel {}: {value:04X}",
            self.state.scanline,
            self.state.scanline_mclk_cycles,
            scanline_mclk_to_pixel(
                self.state.scanline_mclk_cycles,
                self.registers.horizontal_display_size
            )
        );

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
        self.fifo_tracker.record_access(line_type, data_port_location, self.registers.vram_size);
    }

    fn maybe_push_pending_write(&mut self, write: PendingWrite) -> bool {
        if self.state.pending_dma.is_some()
            || self.fifo_tracker.should_halt_cpu()
            || (self.dma_tracker.is_in_progress() && matches!(write, PendingWrite::Data(..)))
        {
            self.state.pending_writes.push(write);
            true
        } else {
            false
        }
    }

    pub fn write_debug_register(&mut self, value: u16) {
        self.debug_register.write(value);

        log::trace!("VDP debug register write: {:?}", self.debug_register);
    }

    pub fn read_status(&mut self) -> u16 {
        log::trace!("VDP status register read");

        let interlaced_odd =
            self.registers.interlacing_mode.is_interlaced() && self.state.frame_count % 2 == 1;

        let scanline_mclk = self.state.scanline_mclk_cycles;
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
            | (u16::from(self.state.vint_flag) << 7)
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

        let h_counter = self.h_counter(self.state.scanline_mclk_cycles);
        let v_counter = self.v_counter(self.state.scanline_mclk_cycles);

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
        assert!(
            master_clock_cycles < 1250,
            "VDP tick {master_clock_cycles} mclk cycles, expected <1250"
        );

        self.state.delayed_v_interrupt = self.state.delayed_v_interrupt_next;
        self.state.delayed_v_interrupt_next = false;

        self.state.scanline_mclk_cycles += master_clock_cycles;
        self.process_events_after_mclk_update();

        // Check if the VDP has advanced to a new scanline
        let mut tick_effect = VdpTickEffect::None;
        if self.state.scanline_mclk_cycles >= MCLK_CYCLES_PER_SCANLINE {
            tick_effect = self.advance_to_next_line();
            self.process_events_after_mclk_update();
        }

        let line_type = LineType::from_vdp(self);
        let h_display_size = self.registers.horizontal_display_size;
        let pixel = scanline_mclk_to_pixel(self.state.scanline_mclk_cycles, h_display_size);

        self.fifo_tracker.advance_to_pixel(self.state.scanline, pixel, h_display_size, line_type);

        // Count down DMA time before checking if a DMA was initiated in the last CPU instruction
        self.dma_tracker.advance_to_pixel(self.state.scanline, pixel, h_display_size, line_type);

        if let Some(active_dma) = self.state.pending_dma {
            // TODO accurate DMA timing
            self.run_dma(memory, active_dma);
        }

        if !self.dma_tracker.is_in_progress()
            && !self.fifo_tracker.should_halt_cpu()
            && !self.state.pending_writes.is_empty()
        {
            self.apply_pending_writes();
        }

        tick_effect
    }

    fn vdp_event_times(h_display_size: HorizontalDisplaySize) -> [VdpEventWithTime; 6] {
        // H interrupts occur a set number of mclk cycles after the end of the active display,
        // not right at the start of HBlank
        let h_interrupt_delay = match h_display_size {
            // 12 pixels after active display
            HorizontalDisplaySize::ThirtyTwoCell => 120,
            // 16 pixels after active display
            HorizontalDisplaySize::FortyCell => 128,
        };

        [
            VdpEventWithTime::new(V_INTERRUPT_DELAY, VdpEvent::VInterrupt),
            VdpEventWithTime::new(ACTIVE_MCLK_CYCLES_PER_SCANLINE, VdpEvent::HBlankStart),
            VdpEventWithTime::new(
                ACTIVE_MCLK_CYCLES_PER_SCANLINE + h_interrupt_delay,
                VdpEvent::HInterrupt,
            ),
            VdpEventWithTime::new(
                ACTIVE_MCLK_CYCLES_PER_SCANLINE + h_interrupt_delay + REGISTER_LATCH_DELAY_MCLK,
                VdpEvent::LatchRegisters,
            ),
            VdpEventWithTime::new(VINT_FLAG_MCLK, VdpEvent::VIntStatusFlag),
            VdpEventWithTime::new(u64::MAX, VdpEvent::None),
        ]
    }

    fn process_events_after_mclk_update(&mut self) {
        let scanline_mclk = self.state.scanline_mclk_cycles;
        while scanline_mclk >= self.vdp_event_times[self.state.vdp_event_idx as usize].mclk {
            match self.vdp_event_times[self.state.vdp_event_idx as usize].event {
                VdpEvent::VInterrupt => {
                    let active_scanlines = self.registers.vertical_display_size.active_scanlines();
                    if self.state.scanline == active_scanlines {
                        log::trace!("Generating V interrupt");
                        self.state.v_interrupt_pending = true;
                    }
                }
                VdpEvent::HBlankStart => {
                    self.sprite_state.handle_hblank_start(
                        self.registers.horizontal_display_size,
                        self.registers.display_enabled,
                    );

                    // Sprite processing phase 2
                    self.fetch_sprite_attributes();
                }
                VdpEvent::HInterrupt => {
                    self.latched_sprite_attributes
                        .copy_from_slice(self.cached_sprite_attributes.as_ref());

                    self.decrement_h_interrupt_counter();
                }
                VdpEvent::LatchRegisters => {
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
                VdpEvent::VIntStatusFlag => {
                    let active_scanlines = self.registers.vertical_display_size.active_scanlines();
                    if self.state.scanline == active_scanlines - 1 {
                        self.state.vint_flag = true;
                    }
                }
                VdpEvent::None => {}
            }

            self.state.vdp_event_idx += 1;
        }
    }

    fn decrement_h_interrupt_counter(&mut self) {
        let active_scanlines = self.registers.vertical_display_size.active_scanlines();
        let scanlines_per_frame = self.timing_mode.scanlines_per_frame();

        if self.state.scanline < active_scanlines
            || self.state.scanline == scanlines_per_frame - 1
            || self.state.v_border_forgotten
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

    fn advance_to_next_line(&mut self) -> VdpTickEffect {
        let h_display_size = self.registers.horizontal_display_size;
        self.sprite_state.handle_line_end(h_display_size);

        // Sprite processing phase 1
        self.scan_sprites_two_lines_ahead();

        // Render the next line before advancing counters
        self.render_next_scanline();

        let scanlines_per_frame = self.timing_mode.scanlines_per_frame();

        self.state.scanline_mclk_cycles -= MCLK_CYCLES_PER_SCANLINE;
        self.state.vdp_event_idx = 0;
        self.state.scanline += 1;
        if self.state.scanline == scanlines_per_frame {
            self.state.scanline = 0;
            self.state.frame_count += 1;
            self.state.v_border_forgotten = false;

            let next_frame_interlaced = match self.registers.interlacing_mode {
                InterlacingMode::Progressive => false,
                InterlacingMode::Interlaced => !self.config.deinterlace,
                InterlacingMode::InterlacedDouble => true,
            };
            if next_frame_interlaced && !self.state.interlaced_frame && !self.config.deinterlace {
                self.prepare_frame_buffer_for_interlaced();
            }
            self.state.interlaced_frame = next_frame_interlaced;

            // Top border length needs to be saved at start-of-frame in case there is a mid-frame swap between V28
            // mode and V30 mode. Titan Overdrive 2 depends on this for the arcade scene
            self.state.top_border =
                self.registers.vertical_display_size.top_border(self.timing_mode);
        }

        let last_scanline_of_frame =
            self.timing_mode.rendered_lines_per_frame() - self.state.top_border;
        if self.state.scanline == last_scanline_of_frame {
            VdpTickEffect::FrameComplete
        } else {
            VdpTickEffect::None
        }
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
            self.state.data_address.wrapping_add(self.registers.data_port_auto_increment.into());
    }

    fn write_vram_word(&mut self, address: u32, value: u16) {
        let [msb, lsb] = value.to_be_bytes();

        match self.registers.vram_size {
            VramSizeKb::SixtyFour => {
                let vram_addr = address & 0xFFFF;
                self.vram[vram_addr as usize] = msb;
                self.vram[(vram_addr ^ 0x1) as usize] = lsb;

                // Address bit 16 is always checked for sprite cache, even in 64KB mode
                if !address.bit(16) {
                    self.maybe_update_sprite_cache(vram_addr as u16, msb);
                    self.maybe_update_sprite_cache((vram_addr ^ 0x1) as u16, lsb);
                }
            }
            VramSizeKb::OneTwentyEight => {
                // Only LSB is written in 128KB mode
                let vram_addr = convert_128kb_vram_address(address);
                self.vram[vram_addr as usize] = lsb;

                // Both bytes are written to the sprite cache even in 128KB mode
                self.maybe_update_sprite_cache(vram_addr as u16, lsb);
                self.maybe_update_sprite_cache((vram_addr ^ 0x1) as u16, msb);
            }
        }
    }

    fn write_cram_word(&mut self, address: u32, value: u16) {
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
    fn maybe_update_sprite_cache(&mut self, address: u16, value: u8) {
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
            match address & 0x3 {
                0x0 => self.cached_sprite_attributes[idx].update_first_word_msb(value),
                0x1 => self.cached_sprite_attributes[idx].update_first_word_lsb(value),
                0x2 => self.cached_sprite_attributes[idx].update_second_word_msb(value),
                0x3 => self.cached_sprite_attributes[idx].update_second_word_lsb(value),
                _ => unreachable!("value & 0x3 is always <= 0x3"),
            }
        }
    }

    fn prepare_frame_buffer_for_interlaced(&mut self) {
        // Duplicate every line to avoid a flickering frame if a game enables interlacing without
        // first blanking the screen
        let screen_width = self.screen_width();
        let screen_height = self.screen_height();
        for scanline in (0..screen_height).rev() {
            for pixel in 0..screen_width {
                let color = self.frame_buffer[(scanline * screen_width + pixel) as usize];
                self.frame_buffer[((2 * scanline) * screen_width + pixel) as usize] = color;
                self.frame_buffer[((2 * scanline + 1) * screen_width + pixel) as usize] = color;
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
        if self.state.v_interrupt_pending
            && self.registers.v_interrupt_enabled
            && !self.state.delayed_v_interrupt
        {
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
            self.state.vint_flag = false;
        } else if interrupt_level == 4 {
            self.state.h_interrupt_pending = false;
        }
    }

    #[inline]
    #[must_use]
    pub fn should_halt_cpu(&self) -> bool {
        self.dma_tracker.should_halt_cpu(&self.state.pending_writes)
            || self.fifo_tracker.should_halt_cpu()
    }

    #[inline]
    #[must_use]
    pub fn long_halting_dma_in_progress(&self) -> bool {
        self.should_halt_cpu() && self.dma_tracker.long_dma_in_progress()
    }

    #[inline]
    #[must_use]
    pub fn z80_interrupt_line(&self) -> InterruptLine {
        // Z80 INT line is low only during the first scanline of VBlank
        if self.state.scanline == self.registers.vertical_display_size.active_scanlines() {
            InterruptLine::Low
        } else {
            InterruptLine::High
        }
    }

    fn scan_sprites_two_lines_ahead(&mut self) {
        let scanline_for_sprite_scan =
            (self.state.scanline + 2) % self.timing_mode.scanlines_per_frame();

        self.scan_sprites(scanline_for_sprite_scan);
    }

    #[inline]
    fn render_next_scanline(&mut self) {
        let scanlines_per_frame = self.timing_mode.scanlines_per_frame();
        let render_scanline = if self.state.scanline == scanlines_per_frame - 1 {
            0
        } else {
            self.state.scanline + 1
        };
        self.render_scanline(render_scanline, 0);
    }

    #[inline]
    #[must_use]
    pub fn frame_buffer(&self) -> &[Color; FRAME_BUFFER_LEN] {
        &self.frame_buffer
    }

    #[inline]
    #[must_use]
    pub fn frame_buffer_mut(&mut self) -> &mut [Color; FRAME_BUFFER_LEN] {
        &mut self.frame_buffer
    }

    #[inline]
    #[must_use]
    pub fn frame_size(&self) -> FrameSize {
        FrameSize { width: self.screen_width(), height: self.screen_height() }
    }

    #[inline]
    #[must_use]
    pub fn screen_width(&self) -> u32 {
        let h_display_size = self.registers.horizontal_display_size;
        let active_display_pixels: u32 = h_display_size.active_display_pixels().into();

        if self.config.render_horizontal_border {
            u32::from(h_display_size.left_border())
                + active_display_pixels
                + u32::from(RIGHT_BORDER)
        } else {
            active_display_pixels
        }
    }

    #[inline]
    #[must_use]
    pub fn screen_height(&self) -> u32 {
        let screen_height: u32 = if self.config.render_vertical_border {
            self.timing_mode.rendered_lines_per_frame().into()
        } else {
            self.registers.vertical_display_size.active_scanlines().into()
        };

        if self.state.interlaced_frame { 2 * screen_height } else { screen_height }
    }

    #[inline]
    #[must_use]
    pub fn border_size(&self) -> BorderSize {
        let (left, right) = if self.config.render_horizontal_border {
            let h_display_size = self.registers.horizontal_display_size;
            (h_display_size.left_border(), RIGHT_BORDER)
        } else {
            (0, 0)
        };

        let (top, bottom) = if self.config.render_vertical_border {
            let v_display_size = self.registers.vertical_display_size;
            (
                v_display_size.top_border(self.timing_mode),
                v_display_size.bottom_border(self.timing_mode),
            )
        } else {
            (0, 0)
        };

        BorderSize {
            left: left.into(),
            right: right.into(),
            top: top.into(),
            bottom: bottom.into(),
        }
    }

    #[inline]
    #[must_use]
    pub fn config(&self) -> VdpConfig {
        self.config
    }

    #[inline]
    pub fn reload_config(&mut self, config: VdpConfig) {
        self.config = config;
    }

    #[inline]
    #[must_use]
    pub fn scanline(&self) -> u16 {
        self.state.scanline
    }

    #[inline]
    #[must_use]
    pub fn scanline_mclk(&self) -> u64 {
        self.state.scanline_mclk_cycles
    }
}

fn convert_128kb_vram_address(address: u32) -> u32 {
    // Formula from https://plutiedev.com/mirror/kabuto-hardware-notes#128k-abuse
    (((address & 0x2) ^ 0x2) >> 1)
        | ((address & 0x400) >> 9)
        | (address & 0x3FC)
        | ((address & 0x1F800) >> 1)
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
            VdpConfig {
                enforce_sprite_limits: true,
                emulate_non_linear_dac: false,
                deinterlace: true,
                render_vertical_border: false,
                render_horizontal_border: false,
                plane_a_enabled: true,
                plane_b_enabled: true,
                window_enabled: true,
                sprites_enabled: true,
                backdrop_enabled: true,
            },
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
