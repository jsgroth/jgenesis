//! Genesis VDP (video display processor)

mod colors;
mod cramdots;
mod debug;
mod fifo;
mod registers;
mod render;
mod sprites;

#[cfg(test)]
mod tests;

use crate::memory::{Memory, PhysicalMedium};
use crate::vdp::colors::ColorModifier;
use crate::vdp::cramdots::CramDotBuffer;
use crate::vdp::fifo::{VdpFifo, VdpFifoEntry, VramWriteSize};
use crate::vdp::registers::{
    DebugRegister, DmaMode, H40_LEFT_BORDER, HorizontalDisplaySize, InterlacingMode,
    NTSC_BOTTOM_BORDER, NTSC_TOP_BORDER, PAL_V28_BOTTOM_BORDER, PAL_V28_TOP_BORDER,
    PAL_V30_BOTTOM_BORDER, PAL_V30_TOP_BORDER, RIGHT_BORDER, Registers, VerticalDisplaySize,
    VramSizeKb,
};
use crate::vdp::sprites::{SpriteBuffers, SpriteState};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{Color, FrameSize, TimingMode};
use jgenesis_common::num::{GetBit, U16Ext};
use jgenesis_proc_macros::{EnumAll, FakeDecode, FakeEncode};
use std::collections::VecDeque;
use std::ops::{Deref, DerefMut, Range};
use std::{array, cmp};
use z80_emu::traits::InterruptLine;

const VRAM_LEN: usize = 64 * 1024;
const CRAM_LEN_WORDS: usize = 64;
const VSRAM_LEN: usize = 80;

const MAX_SCREEN_WIDTH: usize = 320 + H40_LEFT_BORDER as usize + RIGHT_BORDER as usize;
const MAX_SCREEN_HEIGHT: usize = 240 + PAL_V30_TOP_BORDER as usize + PAL_V30_BOTTOM_BORDER as usize;

// Double screen height to account for interlaced 2x mode
pub const FRAME_BUFFER_LEN: usize = MAX_SCREEN_WIDTH * MAX_SCREEN_HEIGHT * 2;

pub const MCLK_CYCLES_PER_SCANLINE: u64 = 3420;
pub const ACTIVE_MCLK_CYCLES_PER_SCANLINE: u64 = 2560;
pub const NTSC_SCANLINES_PER_FRAME: u16 = 262;
pub const PAL_SCANLINES_PER_FRAME: u16 = 313;

const MAX_SPRITES_PER_FRAME: usize = 80;

macro_rules! new_bool256 {
    ($($value:literal),* $(,)?) => {
        {
            let mut bools = [false; 256];
            $(
                bools[$value] = true;
            )*
            bools
        }
    }
}

// Adapted from https://gendev.spritesmind.net/forum/viewtopic.php?t=851 and modified so that 0
// is at H=0x000 rather than the H scroll fetch
const H32_ACCESS_SLOTS: &[bool; 256] =
    &new_bool256![5, 13, 21, 37, 45, 53, 69, 77, 85, 101, 109, 117, 132, 133, 147, 161];
const H40_ACCESS_SLOTS: &[bool; 256] =
    &new_bool256![6, 14, 22, 38, 46, 54, 70, 78, 86, 102, 110, 118, 134, 142, 150, 165, 166, 190];

// Adapted from https://gendev.spritesmind.net/forum/viewtopic.php?p=20921#p20921
// TODO H32 refresh slot locations are probably not accurate
const H32_BLANK_REFRESH_SLOTS: &[bool; 256] = &new_bool256![1, 33, 65, 97, 129];
const H40_BLANK_REFRESH_SLOTS: &[bool; 256] = &new_bool256![26, 58, 90, 122, 154, 204];

// Most H values sourced from https://gendev.spritesmind.net/forum/viewtopic.php?p=17683#p17683
impl HorizontalDisplaySize {
    // Total number of slots minus number of refresh slots
    const fn access_slots_per_blank_line(self) -> u16 {
        match self {
            Self::ThirtyTwoCell => 171 - 5,
            Self::FortyCell => 210 - 6,
        }
    }

    // H range during which the status HBlank flag is _not_ set
    const fn hblank_flag_clear_h_range(self) -> Range<u16> {
        match self {
            Self::ThirtyTwoCell => 0x00A..0x126,
            Self::FortyCell => 0x00B..0x166,
        }
    }

    // H value at which the VDP increments the H interrupt counter, increments the V counter, and
    // potentially sets HINT pending
    const fn h_interrupt_h(self) -> u16 {
        match self {
            Self::ThirtyTwoCell => 0x10A,
            Self::FortyCell => 0x14A,
        }
    }

    const fn h_interrupt_scanline_mclk(self) -> u64 {
        (self.h_interrupt_h() as u64) * self.active_display_mclk_divider()
    }

    // H value at which the VDP sets VINT pending on line 224/240
    const fn v_interrupt_h(self) -> u16 {
        match self {
            Self::ThirtyTwoCell => 0x001,
            Self::FortyCell => 0x002,
        }
    }

    const fn v_interrupt_scanline_mclk(self) -> u64 {
        (self.v_interrupt_h() as u64) * self.active_display_mclk_divider()
    }

    // Range when the VDP is actively displaying pixels
    const fn active_display_h_range(self) -> Range<u16> {
        match self {
            Self::ThirtyTwoCell => 0x018..0x118,
            Self::FortyCell => 0x01A..0x15A,
        }
    }

    const fn rendering_begin_h(self) -> u16 {
        self.active_display_h_range().start - 16
    }

    // H at which to execute sprite processing phase 2 (fetch sprite attributes)
    const fn fetch_sprite_attributes_h(self) -> u16 {
        // Chaekopon demo by Limp Ninja is sensitive to when phase 2 is executed
        // This demo sometimes modifies the sprite attribute table address shortly before HINT,
        // seemingly just after attributes are fetched for the last sprite scanned in phase 1
        self.active_display_h_range().end - 16 - 8
    }

    // H where HBlank begins (should line up with the two consecutive external access slots)
    const fn hblank_begin_h(self) -> u16 {
        self.active_display_h_range().end - 16
    }

    // H by which VDP register latching for the next line is completed
    const fn latch_registers_h(self) -> u16 {
        // Estimated based on latching taking place within 36 CPU cycles of HINT
        // TODO this is probably inaccurate for either H32 or H40 mode
        match self {
            Self::ThirtyTwoCell => 0x121,
            Self::FortyCell => 0x169,
        }
    }

    const fn active_display_mclk_divider(self) -> u64 {
        match self {
            Self::ThirtyTwoCell => 10,
            Self::FortyCell => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum ControlWriteFlag {
    First,
    Second,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum DataPortMode {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum DataPortLocation {
    Vram,
    Vram8Bit,
    Cram,
    Vsram,
    Invalid,
}

#[derive(Debug, Clone, Encode, Decode)]
struct ControlPort {
    mode: DataPortMode,
    location_bits: u8,
    location: DataPortLocation,
    control_address: u32,
    data_port_address: u32,
    write_flag: ControlWriteFlag,
    dma_active: bool,
}

impl ControlPort {
    fn new() -> Self {
        Self {
            mode: DataPortMode::Read,
            location_bits: 0,
            location: DataPortLocation::Vram,
            control_address: 0,
            data_port_address: 0,
            write_flag: ControlWriteFlag::First,
            dma_active: false,
        }
    }

    fn write_first_command(&mut self, value: u16) {
        // First command word: Lowest 14 bits of address and lowest 2 bits of code
        self.control_address = (self.control_address & !0x3FFF) | u32::from(value & 0x3FFF);
        self.data_port_address = self.control_address;

        self.mode = if value.bit(14) { DataPortMode::Write } else { DataPortMode::Read };
        self.location_bits = (self.location_bits & !1) | (value >> 15) as u8;
        self.location = parse_location_bits(self.location_bits, self.mode);
    }

    fn write_second_command(&mut self, value: u16, registers: &Registers) {
        // Second command word: Highest 3 bits of address (A14-A16) and highest 4 bits of code (CD2-CD5)
        self.control_address = (self.control_address & 0x3FFF) | (u32::from(value & 7) << 14);
        self.data_port_address = self.control_address;

        self.location_bits = (self.location_bits & 1) | ((value >> 3) & 0b110) as u8;
        self.location = parse_location_bits(self.location_bits, self.mode);

        // CD5 is only writable if DMA is enabled in register #1
        if registers.dma_enabled {
            self.dma_active = value.bit(7);
        }
    }

    fn new_fifo_entry(&self, word: u16, vram_size: VramSizeKb) -> VdpFifoEntry {
        // Perform 128KB mode address conversion on FIFO push rather than pop; Overdrive 2 depends on this
        // TODO does 128KB mode also cause invalid target FIFO entries to only take 1 slot?
        let (address, size) = match (self.location, vram_size) {
            (DataPortLocation::Vram | DataPortLocation::Invalid, VramSizeKb::OneTwentyEight) => {
                (convert_128kb_vram_address(self.data_port_address), VramWriteSize::Byte)
            }
            _ => (self.data_port_address, VramWriteSize::Word),
        };

        VdpFifoEntry::new(self.mode, self.location, address, word, size)
    }

    fn increment_data_port_address(&mut self, registers: &Registers) {
        self.data_port_address =
            self.data_port_address.wrapping_add(registers.data_port_auto_increment.into());
    }
}

fn parse_location_bits(bits: u8, mode: DataPortMode) -> DataPortLocation {
    match (bits, mode) {
        (0b000, _) => DataPortLocation::Vram,
        (0b010, _) => DataPortLocation::Vsram,
        (0b001, DataPortMode::Write) | (0b100, DataPortMode::Read) => DataPortLocation::Cram,
        // Undocumented: Code 01100 enables 8-bit VRAM reads (verified by VDPFIFOTesting ROM)
        (0b110, DataPortMode::Read) => DataPortLocation::Vram8Bit,
        _ => DataPortLocation::Invalid,
    }
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
    // Whether the VDP is actively raising INT6
    v_interrupt_pending: bool,
    delayed_v_interrupt: bool,
    h_interrupt_pending: bool,
    delayed_h_interrupt: bool,
    h_interrupt_counter: u16,
    latched_hv_counter: Option<u16>,
    v_border_forgotten: bool,
    top_border: u16,
    last_scroll_b_palettes: [u8; 2],
    last_h_scroll_a: u16,
    last_h_scroll_b: u16,
    scanline: u16,
    scanline_mclk_cycles: u64,
    pixel: u16,
    in_vblank: bool,
    // Used to store writes to either VDP port while a memory-to-VRAM DMA is in progress
    // (Can happen if a DMA is initiated using a longword write)
    pending_writes: Vec<PendingWrite>,
    pending_write_delay_pixels: u8,
    data_port_read_wait: bool,
    vram_fill_data: Option<u16>,
    vram_copy_odd_slot: bool,
    interlaced_frame: bool,
    interlaced_odd: bool,
    // Latched at start of VBlank
    // This is not accurate to actual hardware, but nothing should change H resolution mid-frame
    // during active display
    frame_h_resolution: HorizontalDisplaySize,
}

impl InternalState {
    fn new(timing_mode: TimingMode) -> Self {
        Self {
            v_interrupt_pending: false,
            delayed_v_interrupt: false,
            delayed_h_interrupt: false,
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
            pixel: 0,
            in_vblank: false,
            pending_writes: Vec::with_capacity(10),
            pending_write_delay_pixels: 0,
            data_port_read_wait: false,
            vram_fill_data: None,
            vram_copy_odd_slot: false,
            interlaced_frame: false,
            interlaced_odd: false,
            frame_h_resolution: HorizontalDisplaySize::default(),
        }
    }

    fn interlaced_odd(&self) -> bool {
        self.interlaced_frame && self.interlaced_odd
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

pub(crate) trait TimingModeExt: Copy {
    fn scanlines_per_frame(self, interlaced: bool, interlaced_odd: bool) -> u16;

    fn rendered_lines_per_frame(self) -> u16;
}

impl TimingModeExt for TimingMode {
    fn scanlines_per_frame(self, interlaced: bool, interlaced_odd: bool) -> u16 {
        match self {
            Self::Ntsc => NTSC_SCANLINES_PER_FRAME + u16::from(interlaced_odd),
            Self::Pal => PAL_SCANLINES_PER_FRAME - 1 + u16::from(!interlaced || interlaced_odd),
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
    pub non_linear_color_scale: bool,
    pub deinterlace: bool,
    pub render_vertical_border: bool,
    pub render_horizontal_border: bool,
    pub plane_a_enabled: bool,
    pub plane_b_enabled: bool,
    pub sprites_enabled: bool,
    pub window_enabled: bool,
    pub backdrop_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, EnumAll)]
enum VdpEvent {
    VInterrupt,
    RenderLine,
    FetchSpriteAttributes,
    HBlankStart,
    HInterrupt,
    LatchRegisters,
    None,
}

impl VdpEvent {
    const NUM: usize = 7;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
struct VdpEventWithTime {
    event: VdpEvent,
    h: u16,
}

impl VdpEventWithTime {
    const fn new(h: u16, event: VdpEvent) -> Self {
        Self { event, h }
    }
}

#[derive(Debug, Clone, Copy)]
struct VCounter {
    counter: u8,
    vblank_flag: bool,
}

#[derive(Debug, Clone, Copy)]
struct HVCounter {
    internal_h: u16,
    internal_v: u8,
    hv_counter: u16,
    vblank_flag: bool,
}

type Vram = [u8; VRAM_LEN];
type Cram = [u16; CRAM_LEN_WORDS];
type Vsram = [u8; VSRAM_LEN];

#[derive(Debug, Clone, Encode, Decode)]
pub struct Vdp {
    frame_buffer: FrameBuffer,
    cram_dots: CramDotBuffer,
    vram: Box<Vram>,
    cram: Box<Cram>,
    vsram: Box<Vsram>,
    fifo: VdpFifo,
    dma_latency: u8,
    // Used to store writes to data port while FIFO is full
    pending_fifo_writes: VecDeque<VdpFifoEntry>,
    timing_mode: TimingMode,
    control_port: ControlPort,
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
    vdp_event_times: [VdpEventWithTime; VdpEvent::NUM],
    vdp_event_idx: u8,
}

impl Vdp {
    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn new(timing_mode: TimingMode, config: VdpConfig) -> Self {
        Self {
            frame_buffer: FrameBuffer::new(),
            cram_dots: CramDotBuffer::new(),
            vram: vec![0; VRAM_LEN].into_boxed_slice().try_into().unwrap(),
            cram: vec![0; CRAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
            vsram: vec![0; VSRAM_LEN].into_boxed_slice().try_into().unwrap(),
            fifo: VdpFifo::new(),
            dma_latency: 0,
            pending_fifo_writes: VecDeque::with_capacity(8),
            timing_mode,
            control_port: ControlPort::new(),
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
            vdp_event_times: Self::vdp_event_times(HorizontalDisplaySize::default()),
            vdp_event_idx: 0,
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
            self.control_port.write_flag,
            self.registers.dma_enabled
        );

        if self.control_port.dma_active && self.registers.dma_mode == DmaMode::MemoryToVram {
            // VDP is locking the bus; buffer the write until the DMA is done
            // Some games depend on this - they'll do a longword write where the first word starts
            // a DMA and then the second word changes the control port address
            self.state.pending_writes.push(PendingWrite::Control(value));
            return;
        }

        match self.control_port.write_flag {
            ControlWriteFlag::First => {
                // VDP register write OR first word of command write

                // Always write first word to control port, even if this is a register write
                self.control_port.write_first_command(value);

                if value & 0xC000 == 0x8000 {
                    // VDP register write
                    self.write_vdp_register(value);
                } else {
                    // First word of command write
                    self.control_port.write_flag = ControlWriteFlag::Second;
                }
            }
            ControlWriteFlag::Second => {
                // Second word of command write
                self.control_port.write_second_command(value, &self.registers);
                self.control_port.write_flag = ControlWriteFlag::First;

                if self.control_port.dma_active {
                    // DMA started
                    self.state.vram_fill_data = None;
                    self.state.vram_copy_odd_slot = false;

                    // OutRunners depends on there being a delay to FIFO writes when starting
                    // memory-to-VRAM DMA
                    if self.registers.dma_mode == DmaMode::MemoryToVram {
                        // Hack: Fewer than 7 slots doesn't consistently fix OutRunners, but 7 slots
                        // breaks Overdrive 2's plasma twisters effect. Use a shorter delay when
                        // DMAing to VSRAM (almost certainly working around other timing issues)
                        self.dma_latency = if self.control_port.location == DataPortLocation::Vsram
                        {
                            5
                        } else {
                            7
                        };
                    }

                    log::trace!(
                        "DMA of type {:?} initiated at line {} mclk {} pixel {}",
                        self.registers.dma_mode,
                        self.state.scanline,
                        self.state.scanline_mclk_cycles,
                        self.state.pixel
                    );
                }
            }
        }

        log::trace!("  Mode: {:?}", self.control_port.mode);
        log::trace!("  Location bits: {:03b}", self.control_port.location_bits);
        log::trace!("  Location: {:?}", self.control_port.location);
        log::trace!("  Address: {:05X}", self.control_port.control_address);
        log::trace!("  DMA active: {}", self.control_port.dma_active);
    }

    fn write_vdp_register(&mut self, value: u16) {
        let prev_h_interrupt_enabled = self.registers.h_interrupt_enabled;
        let prev_v_interrupt_enabled = self.registers.v_interrupt_enabled;
        let prev_h_display_size = self.registers.horizontal_display_size;
        let prev_v_display_size = self.registers.vertical_display_size;

        let register_number = ((value >> 8) & 0x1F) as u8;
        self.registers.write_internal_register(register_number, value as u8);

        if self.registers.hv_counter_stopped && self.state.latched_hv_counter.is_none() {
            let HVCounter { hv_counter, .. } =
                self.hv_counter_internal(self.state.scanline_mclk_cycles);
            self.state.latched_hv_counter = Some(hv_counter);
        } else if !self.registers.hv_counter_stopped && self.state.latched_hv_counter.is_some() {
            self.state.latched_hv_counter = None;
        }

        self.update_latched_registers_if_necessary(register_number);

        if register_number == 1 {
            // Update enabled pixels in sprite state if register #1 was written
            self.sprite_state.handle_display_enabled_write(
                self.registers.horizontal_display_size,
                self.registers.display_enabled,
                self.state.pixel,
            );

            // Mark vertical border "forgotten" if V size was switched from V30 to V28 between lines 224-239
            // This has a few effects:
            // - The HINT counter continues to tick down every line throughout VBlank instead of getting reset
            // - The VDP continues to render normally inside the vertical border
            if prev_v_display_size == VerticalDisplaySize::ThirtyCell
                && self.registers.vertical_display_size == VerticalDisplaySize::TwentyEightCell
                && (VerticalDisplaySize::TwentyEightCell.active_scanlines()
                    ..VerticalDisplaySize::ThirtyCell.active_scanlines())
                    .contains(&self.state.scanline)
            {
                log::trace!(
                    "V border forgotten; line {} pixel {}",
                    self.state.scanline,
                    self.state.pixel
                );
                self.state.v_border_forgotten = true;
            }

            // V interrupts must be delayed by 1 CPU instruction if they are enabled
            // while a V interrupt is pending; Sesame Street Counting Cafe depends on this
            self.state.delayed_v_interrupt =
                !prev_v_interrupt_enabled && self.registers.v_interrupt_enabled;
        }

        // Fatal Rewind / The Killing Game Show depends on both HINT and VINT being delayed by 1
        // instruction when enabled by software while an interrupt is pending
        self.state.delayed_h_interrupt |=
            !prev_h_interrupt_enabled && self.registers.h_interrupt_enabled;

        if prev_h_display_size != self.registers.horizontal_display_size {
            self.handle_h_resolution_change();
        }
    }

    fn handle_h_resolution_change(&mut self) {
        let h_display_size = self.registers.horizontal_display_size;
        self.state.pixel = scanline_mclk_to_pixel(self.state.scanline_mclk_cycles, h_display_size);

        let internal_h = pixel_to_internal_h(self.state.pixel, h_display_size);
        let effective_v = if internal_h >= h_display_size.hblank_begin_h() {
            self.state.scanline + 1
        } else {
            self.state.scanline
        };
        let active_scanlines = self.registers.vertical_display_size.active_scanlines();
        let scanlines_per_frame = self.scanlines_in_current_frame();
        self.state.in_vblank = (active_scanlines..scanlines_per_frame - 1).contains(&effective_v);

        self.vdp_event_times = Self::vdp_event_times(h_display_size);
        self.vdp_event_idx = 0;
        while internal_h >= self.vdp_event_times[self.vdp_event_idx as usize].h {
            self.vdp_event_idx += 1;
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
        {
            self.maybe_render_partial_line();
        }
    }

    fn maybe_render_partial_line(&mut self) {
        let render_start = self.registers.horizontal_display_size.rendering_begin_h();
        let active_display_range = self.registers.horizontal_display_size.active_display_h_range();
        if (render_start..active_display_range.end).contains(&self.state.pixel) {
            log::trace!(
                "Re-rendering line {} from pixel {} (frame pixel {})",
                self.state.scanline,
                self.state.pixel,
                self.state.pixel.saturating_sub(active_display_range.start)
            );
            self.render_scanline(
                self.state.scanline,
                self.state.pixel.saturating_sub(active_display_range.start),
            );
        }
    }

    pub fn read_data(&mut self) -> u16 {
        log::trace!(
            "VDP data read at line {} mclk {} pixel {}",
            self.state.scanline,
            self.state.scanline_mclk_cycles,
            self.state.pixel
        );

        // Reset write flag on all data port accesses
        self.control_port.write_flag = ControlWriteFlag::First;

        if self.control_port.mode != DataPortMode::Read {
            // TODO return previous read buffer contents?
            return 0xFFFF;
        }

        let mut value = match self.control_port.location {
            DataPortLocation::Vram => match self.registers.vram_size {
                VramSizeKb::SixtyFour => {
                    let vram_addr = (self.control_port.data_port_address & 0xFFFF & !1) as usize;
                    let msb = self.vram[vram_addr];
                    let lsb = self.vram[vram_addr + 1];
                    u16::from_be_bytes([msb, lsb])
                }
                VramSizeKb::OneTwentyEight => {
                    let vram_addr = convert_128kb_vram_address(self.control_port.data_port_address);
                    let byte = self.vram[vram_addr as usize];
                    u16::from_be_bytes([byte, byte])
                }
            },
            DataPortLocation::Vram8Bit => {
                // TODO 128KB mode?
                let vram_addr = ((self.control_port.data_port_address & 0xFFFF) ^ 1) as usize;
                let lsb = self.vram[vram_addr];
                u16::from_le_bytes([lsb, self.fifo.next_slot_word().msb()])
            }
            DataPortLocation::Cram => {
                let cram_addr = (self.control_port.data_port_address & 0x7F) >> 1;
                self.cram[cram_addr as usize]
            }
            DataPortLocation::Vsram => {
                let vsram_addr = (self.control_port.data_port_address & 0x7F & !1) as usize;
                if vsram_addr < VSRAM_LEN {
                    u16::from_be_bytes([self.vsram[vsram_addr], self.vsram[vsram_addr + 1]])
                } else {
                    // TODO return most recently used VSRAM entry?
                    u16::from_be_bytes([self.vsram[0], self.vsram[1]])
                }
            }
            DataPortLocation::Invalid => {
                // TODO return previous read buffer contents?
                0xFFFF
            }
        };

        if !self.fifo.is_empty() {
            self.state.data_port_read_wait = true;

            if let Some(read_override) = self.data_port_fifo_override() {
                value = read_override;
            }
        }

        self.control_port.increment_data_port_address(&self.registers);

        // CRAM is 9-bit memory and VSRAM is 11-bit memory
        // Remaining bits are filled from the last word written to the next available FIFO slot
        match self.control_port.location {
            DataPortLocation::Cram => (value & 0x0EEE) | (self.fifo.next_slot_word() & !0x0EEE),
            DataPortLocation::Vsram => (value & 0x07FF) | (self.fifo.next_slot_word() & !0x07FF),
            _ => value,
        }
    }

    // TODO this function is not well-tested
    fn data_port_fifo_override(&self) -> Option<u16> {
        let mut read_override: Option<u16> = None;

        let address_mask = match self.control_port.location {
            DataPortLocation::Vram | DataPortLocation::Vram8Bit => match self.registers.vram_size {
                VramSizeKb::SixtyFour => 0xFFFF & !1,
                VramSizeKb::OneTwentyEight => 0x1FFFF & !1,
            },
            DataPortLocation::Cram | DataPortLocation::Vsram => 0x7F & !1,
            DataPortLocation::Invalid => !0,
        };

        for entry in self.fifo.iter() {
            if entry.mode != DataPortMode::Write
                || entry.location != self.control_port.location
                || (entry.address & address_mask)
                    != (self.control_port.data_port_address & address_mask)
            {
                continue;
            }

            match self.control_port.location {
                DataPortLocation::Vram => match self.registers.vram_size {
                    VramSizeKb::SixtyFour => {
                        read_override = Some(if !entry.address.bit(0) {
                            entry.word
                        } else {
                            entry.word.swap_bytes()
                        });
                    }
                    VramSizeKb::OneTwentyEight => {
                        let byte = entry.word.lsb();
                        read_override = Some(u16::from_le_bytes([byte, byte]));
                    }
                },
                DataPortLocation::Cram | DataPortLocation::Vsram => {
                    read_override = Some(entry.word);
                }
                DataPortLocation::Vram8Bit | DataPortLocation::Invalid => {}
            }
        }

        if let Some(read_override) = read_override {
            log::trace!(
                "Overriding {:?} read of {:04X} to {read_override:04X}",
                self.control_port.location,
                self.control_port.data_port_address
            );
        }

        read_override
    }

    pub fn write_data(&mut self, value: u16) {
        log::trace!(
            "VDP data write on scanline {} / mclk {} / pixel {}: {value:04X}, data port addr {:04X}",
            self.state.scanline,
            self.state.scanline_mclk_cycles,
            self.state.pixel,
            self.control_port.data_port_address
        );

        if self.control_port.dma_active && self.registers.dma_mode == DmaMode::MemoryToVram {
            // VDP is locking the bus; buffer the write until the DMA is done
            self.state.pending_writes.push(PendingWrite::Data(value));
            return;
        }

        // Reset write flag on all data port accesses
        self.control_port.write_flag = ControlWriteFlag::First;

        let fifo_entry = self.control_port.new_fifo_entry(value, self.registers.vram_size);
        self.control_port.increment_data_port_address(&self.registers);

        if self.fifo.is_full() {
            self.pending_fifo_writes.push_back(fifo_entry);
        } else {
            self.push_fifo(fifo_entry);
        }
    }

    fn push_fifo(&mut self, entry: VdpFifoEntry) {
        // Check sprite table cache on FIFO push; this works around some timing issues in rendering
        // Overdrive 2's textured cube effect
        if entry.mode == DataPortMode::Write && entry.location == DataPortLocation::Vram {
            match self.registers.vram_size {
                VramSizeKb::SixtyFour => {
                    // A16 is checked for sprite table cache even in 64KB mode
                    if !entry.address.bit(16) {
                        self.maybe_update_sprite_cache(entry.address as u16, entry.word.msb());
                        self.maybe_update_sprite_cache(
                            (entry.address ^ 1) as u16,
                            entry.word.lsb(),
                        );
                    }
                }
                VramSizeKb::OneTwentyEight => {
                    // Both sprite cache bytes are updated even in 128KB mode, but byteswapped
                    self.maybe_update_sprite_cache(entry.address as u16, entry.word.lsb());
                    self.maybe_update_sprite_cache((entry.address ^ 1) as u16, entry.word.msb());
                }
            }
        }

        log::trace!(
            "FIFO push (line {} pixel {}): {entry:04X?}",
            self.state.scanline,
            self.state.pixel
        );

        self.fifo.push(entry);
    }

    fn pop_fifo(&mut self) {
        let entry = self.fifo.front();

        log::trace!(
            "FIFO pop (line {} pixel {} len {}): {entry:04X?}",
            self.state.scanline,
            self.state.pixel,
            self.fifo.len()
        );

        if entry.mode == DataPortMode::Write {
            match entry.location {
                DataPortLocation::Vram => {
                    let vram_addr = (entry.address & 0xFFFF) as usize;
                    match entry.size {
                        VramWriteSize::Word => {
                            self.vram[vram_addr] = entry.word.msb();
                            self.vram[vram_addr ^ 1] = entry.word.lsb();
                        }
                        VramWriteSize::Byte => {
                            self.vram[vram_addr] = entry.word.lsb();
                        }
                    }
                }
                DataPortLocation::Cram => {
                    let cram_addr = (entry.address & 0x7F) >> 1;
                    self.cram[cram_addr as usize] = entry.word;

                    self.cram_dots.check_for_dot(
                        &self.registers,
                        &self.fifo,
                        self.state.pixel,
                        cram_addr,
                        entry.word,
                    );
                }
                DataPortLocation::Vsram => {
                    let vsram_addr = (entry.address & 0x7F & !1) as usize;
                    if vsram_addr < VSRAM_LEN {
                        self.vsram[vsram_addr] = entry.word.msb();
                        self.vsram[vsram_addr + 1] = entry.word.lsb();
                    }
                }
                DataPortLocation::Vram8Bit | DataPortLocation::Invalid => {}
            }
        }

        // VRAM fill begins when an entry is popped from the FIFO after starting VRAM fill DMA
        // Writing to the FIFO after the DMA begins will update the data used for the fill
        self.state.vram_fill_data = Some(entry.word);

        self.fifo.pop();
        if !self.fifo.is_full() {
            if let Some(pending_write) = self.pending_fifo_writes.pop_front() {
                self.push_fifo(pending_write);
            }
        }

        self.state.data_port_read_wait &= !self.fifo.is_empty();
    }

    pub fn write_debug_register(&mut self, value: u16) {
        self.debug_register.write(value);

        log::trace!("VDP debug register write: {:?}", self.debug_register);
    }

    pub fn read_status(&mut self, m68k_opcode: u16, m68k_divider: u64) -> u16 {
        log::trace!("VDP status register read");

        let read_adjustment = Self::status_read_mclk_adjustment(m68k_opcode, m68k_divider);
        let mut scanline_mclk = self.state.scanline_mclk_cycles + read_adjustment;
        let HVCounter { internal_h, internal_v: v_counter, vblank_flag, .. } =
            self.hv_counter_internal(scanline_mclk);

        let hblank_flag = !self
            .registers
            .horizontal_display_size
            .hblank_flag_clear_h_range()
            .contains(&internal_h);

        if scanline_mclk >= MCLK_CYCLES_PER_SCANLINE {
            scanline_mclk -= MCLK_CYCLES_PER_SCANLINE;
        }

        // It must be possible for VINT to read 1 before the 68000 handles the interrupt; several
        // games depend on this (e.g. Ex-Mutants and Tyrants: Fight Through Time)
        let active_scanlines = self.registers.vertical_display_size.active_scanlines();
        let passed_vint = u16::from(v_counter) == active_scanlines && {
            let vint_mclk = self.registers.horizontal_display_size.v_interrupt_scanline_mclk();
            let original_scanline_mclk = self.state.scanline_mclk_cycles;
            scanline_mclk >= vint_mclk
                && (original_scanline_mclk < vint_mclk
                    || original_scanline_mclk >= MCLK_CYCLES_PER_SCANLINE - vint_mclk)
        };
        let vint_flag = self.state.v_interrupt_pending || passed_vint;

        let status = (u16::from(self.fifo.is_empty()) << 9)
            | (u16::from(self.fifo.is_full()) << 8)
            | (u16::from(vint_flag) << 7)
            | (u16::from(self.sprite_state.overflow_flag()) << 6)
            | (u16::from(self.sprite_state.collision_flag()) << 5)
            | (u16::from(self.state.interlaced_odd) << 4)
            | (u16::from(vblank_flag || !self.registers.display_enabled) << 3)
            | (u16::from(hblank_flag) << 2)
            | (u16::from(self.control_port.dma_active) << 1)
            | u16::from(self.timing_mode == TimingMode::Pal);

        // Reading status register clears the sprite overflow and collision flags
        self.sprite_state.clear_status_flags();

        // Reset control write flag on all status register reads
        self.control_port.write_flag = ControlWriteFlag::First;

        status
    }

    fn status_read_mclk_adjustment(m68k_opcode: u16, m68k_divider: u64) -> u64 {
        // Timing hack: When the CPU reads the status register or the HV counter, return values
        // from slightly in the future to account for the actual read occurring towards the end
        // of the instruction.
        // Using the same value for everything will break either Overdrive 1 (background on the
        // heart screen) or Overdrive 2 (plasma twisters). It needs to vary based on what instruction
        // is performing the read
        match m68000_emu::cycles_if_move_btst_cmp(m68k_opcode) {
            Some(cycles) => u64::from(cycles - 4) * m68k_divider,
            None => 8 * m68k_divider,
        }
    }

    #[must_use]
    pub fn hv_counter(&self, m68k_opcode: u16, m68k_divider: u64) -> u16 {
        let read_adjustment = Self::status_read_mclk_adjustment(m68k_opcode, m68k_divider);
        let hv = self.hv_counter_internal(self.state.scanline_mclk_cycles + read_adjustment);

        log::trace!(
            "HV counter read on scanline {}; H={:02X}, V={:02X}, internal H={:03X}",
            self.state.scanline,
            hv.hv_counter.lsb(),
            hv.hv_counter.msb(),
            hv.internal_h,
        );

        hv.hv_counter
    }

    fn hv_counter_internal(&self, mut scanline_mclk: u64) -> HVCounter {
        let VCounter { counter: v_counter, vblank_flag } = self.v_counter(scanline_mclk);

        if scanline_mclk >= MCLK_CYCLES_PER_SCANLINE {
            scanline_mclk -= MCLK_CYCLES_PER_SCANLINE;
        }
        let pixel = scanline_mclk_to_pixel(scanline_mclk, self.registers.horizontal_display_size);
        let internal_h = pixel_to_internal_h(pixel, self.registers.horizontal_display_size);

        let hv_counter = self.state.latched_hv_counter.unwrap_or_else(|| {
            let h_counter = (internal_h >> 1) as u8;
            u16::from_be_bytes([v_counter, h_counter])
        });

        HVCounter { internal_h, internal_v: v_counter, hv_counter, vblank_flag }
    }

    #[inline]
    fn v_counter(&self, scanline_mclk: u64) -> VCounter {
        // Values from https://gendev.spritesmind.net/forum/viewtopic.php?t=768

        // V counter increments for the next line when HINT is generated
        let in_hblank =
            scanline_mclk >= self.registers.horizontal_display_size.h_interrupt_scanline_mclk();
        let scanline = if in_hblank {
            let scanlines_per_frame = self.scanlines_in_current_frame();
            if self.state.scanline == scanlines_per_frame - 1 { 0 } else { self.state.scanline + 1 }
        } else {
            self.state.scanline
        };

        let active_scanlines = match self.timing_mode {
            TimingMode::Ntsc => VerticalDisplaySize::TwentyEightCell.active_scanlines(),
            TimingMode::Pal => self.registers.vertical_display_size.active_scanlines(),
        };

        let interlacing_mode = if self.state.interlaced_frame {
            self.registers.interlacing_mode
        } else {
            InterlacingMode::Progressive
        };
        match interlacing_mode {
            InterlacingMode::Progressive => {
                let threshold = match (self.timing_mode, self.registers.vertical_display_size) {
                    (TimingMode::Ntsc, _) => 0xEA,
                    (TimingMode::Pal, VerticalDisplaySize::TwentyEightCell) => 0x102,
                    (TimingMode::Pal, VerticalDisplaySize::ThirtyCell) => 0x10A,
                };

                let scanlines_per_frame = match self.timing_mode {
                    TimingMode::Ntsc => NTSC_SCANLINES_PER_FRAME,
                    TimingMode::Pal => PAL_SCANLINES_PER_FRAME,
                };

                let counter = if scanline <= threshold {
                    scanline
                } else {
                    scanline.wrapping_sub(scanlines_per_frame) & 0x1FF
                };
                let vblank_flag = counter >= active_scanlines && counter != 0x1FF;
                VCounter { counter: counter as u8, vblank_flag }
            }
            InterlacingMode::Interlaced | InterlacingMode::InterlacedDouble => {
                let threshold = match (self.timing_mode, self.registers.vertical_display_size) {
                    (TimingMode::Ntsc, _) => 0xEA,
                    (TimingMode::Pal, VerticalDisplaySize::TwentyEightCell) => 0x101,
                    (TimingMode::Pal, VerticalDisplaySize::ThirtyCell) => 0x109,
                };
                let scanlines_per_frame =
                    self.timing_mode.scanlines_per_frame(true, self.state.interlaced_odd);

                let internal_counter = if scanline <= threshold {
                    scanline
                } else {
                    scanline.wrapping_sub(scanlines_per_frame) & 0x1FF
                };
                let vblank_flag = internal_counter >= active_scanlines && internal_counter != 0x1FF;

                let external_counter = match interlacing_mode {
                    InterlacingMode::Interlaced => {
                        (internal_counter & 0xFE) | ((internal_counter >> 8) & 1)
                    }
                    InterlacingMode::InterlacedDouble => {
                        ((internal_counter << 1) & 0xFE) | ((internal_counter >> 7) & 1)
                    }
                    InterlacingMode::Progressive => unreachable!("nested matches"),
                };

                VCounter { counter: external_counter as u8, vblank_flag }
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

        let mut tick_effect = VdpTickEffect::None;
        self.state.scanline_mclk_cycles += master_clock_cycles;
        if self.state.scanline_mclk_cycles >= MCLK_CYCLES_PER_SCANLINE {
            let end_of_line = self.registers.horizontal_display_size.pixels_including_hblank();
            self.advance_to_pixel(end_of_line, memory);

            tick_effect = self.advance_to_next_line();
        }

        let pixel = scanline_mclk_to_pixel(
            self.state.scanline_mclk_cycles,
            self.registers.horizontal_display_size,
        );
        self.advance_to_pixel(pixel, memory);

        tick_effect
    }

    fn advance_to_pixel<Medium: PhysicalMedium>(
        &mut self,
        end_pixel: u16,
        memory: &mut Memory<Medium>,
    ) {
        let check_access_slots = self.control_port.dma_active || !self.fifo.is_empty();
        match (self.registers.horizontal_display_size, check_access_slots) {
            (HorizontalDisplaySize::ThirtyTwoCell, false) => {
                self.advance_to_pixel_no_slot_check::<false>(end_pixel);
            }
            (HorizontalDisplaySize::ThirtyTwoCell, true) => {
                self.advance_to_pixel_check_slots::<false, _>(end_pixel, memory);
            }
            (HorizontalDisplaySize::FortyCell, false) => {
                self.advance_to_pixel_no_slot_check::<true>(end_pixel);
            }
            (HorizontalDisplaySize::FortyCell, true) => {
                self.advance_to_pixel_check_slots::<true, _>(end_pixel, memory);
            }
        }
    }

    #[inline]
    fn advance_to_pixel_check_slots<const H40: bool, Medium: PhysicalMedium>(
        &mut self,
        end_pixel: u16,
        memory: &mut Memory<Medium>,
    ) {
        // Slower loop - check for access slots for DMA/FIFO progress
        let access_slots = if H40 { H40_ACCESS_SLOTS } else { H32_ACCESS_SLOTS };
        let blank_refresh_slots =
            if H40 { H40_BLANK_REFRESH_SLOTS } else { H32_BLANK_REFRESH_SLOTS };

        while self.state.pixel < end_pixel {
            let pixel = self.state.pixel;
            if !pixel.bit(0) {
                let slot_idx = (pixel >> 1) as u8;
                let blank = !self.registers.display_enabled || self.state.in_vblank;
                if (blank && !blank_refresh_slots[slot_idx as usize])
                    || (!blank && access_slots[slot_idx as usize])
                {
                    self.handle_access_slot(blank, slot_idx, blank_refresh_slots, memory);
                }

                // TODO correct refresh slot locations for active display
                if !blank_refresh_slots[slot_idx as usize] {
                    self.fifo.decrement_latency();
                }
            }

            let internal_h =
                if H40 { pixel_to_internal_h_h40(pixel) } else { pixel_to_internal_h_h32(pixel) };
            while internal_h >= self.vdp_event_times[self.vdp_event_idx as usize].h {
                let event = self.vdp_event_times[self.vdp_event_idx as usize].event;
                self.vdp_event_idx += 1;

                self.handle_vdp_event(event);
            }

            self.state.pixel += 1;

            if self.state.pending_write_delay_pixels != 0 {
                self.state.pending_write_delay_pixels -= 1;
                if self.state.pending_write_delay_pixels == 0 {
                    self.apply_pending_writes();
                }
            }
        }
    }

    #[inline]
    fn advance_to_pixel_no_slot_check<const H40: bool>(&mut self, end_pixel: u16) {
        // Faster loop - only check for passed VDP events
        debug_assert!(!self.control_port.dma_active && self.fifo.is_empty());

        let end_internal_h = if H40 {
            pixel_to_internal_h_h40(end_pixel)
        } else {
            pixel_to_internal_h_h32(end_pixel)
        };
        while end_internal_h >= self.vdp_event_times[self.vdp_event_idx as usize].h {
            let internal_h = self.vdp_event_times[self.vdp_event_idx as usize].h;
            let pixel = if H40 {
                internal_h_to_pixel_h40(internal_h)
            } else {
                internal_h_to_pixel_h32(internal_h)
            };
            let event = self.vdp_event_times[self.vdp_event_idx as usize].event;
            self.vdp_event_idx += 1;

            self.state.pixel = pixel;
            self.handle_vdp_event(event);
        }

        self.state.pixel = end_pixel;
    }

    fn handle_access_slot<Medium: PhysicalMedium>(
        &mut self,
        blank: bool,
        slot_idx: u8,
        blank_refresh_slots: &[bool; 256],
        memory: &mut Memory<Medium>,
    ) {
        self.dma_latency = self.dma_latency.saturating_sub(1);

        // Bus read slot for memory-to-VRAM DMA
        if self.control_port.dma_active
            && self.registers.dma_mode == DmaMode::MemoryToVram
            && !self.fifo.is_full()
        {
            // Lose an extra read slot for every VRAM refresh slot during blanking
            // Direct color DMA demos depend on this
            let should_skip_read =
                blank && slot_idx != 0 && blank_refresh_slots[(slot_idx - 1) as usize];
            if !should_skip_read {
                self.progress_memory_to_vram_dma(memory);
            }
        }

        // Video memory write slot
        // FIFO takes priority over VRAM fill/copy DMA
        if !self.fifo.is_empty() {
            if self.dma_latency == 0 && self.fifo.front().latency == 0 {
                self.pop_fifo();
            }
        } else if self.control_port.dma_active {
            match self.registers.dma_mode {
                DmaMode::VramFill => self.progress_vram_fill_dma(),
                DmaMode::VramCopy => self.progress_vram_copy_dma(),
                DmaMode::MemoryToVram => {}
            }
        }
    }

    fn progress_memory_to_vram_dma<Medium: PhysicalMedium>(&mut self, memory: &mut Memory<Medium>) {
        let word = memory.read_word_for_dma(self.registers.dma_source_address);
        self.increment_dma_source_address();

        self.push_fifo(self.control_port.new_fifo_entry(word, self.registers.vram_size));
        self.control_port.increment_data_port_address(&self.registers);

        self.decrement_dma_length();
    }

    fn progress_vram_fill_dma(&mut self) {
        let Some(fill_data) = self.state.vram_fill_data else { return };

        // VRAM fill increments source address on every write even though it does not use the address
        self.increment_dma_source_address();

        match self.control_port.location {
            DataPortLocation::Vram | DataPortLocation::Vram8Bit => {
                let byte = fill_data.msb();
                let vram_addr = (self.control_port.data_port_address ^ 1) & 0xFFFF;
                self.vram[vram_addr as usize] = byte;
                self.maybe_update_sprite_cache(vram_addr as u16, byte);
            }
            DataPortLocation::Cram => {
                // CRAM fill is bugged: uses the value from the next FIFO slot instead of fill data
                let word = self.fifo.next_slot_word();
                let cram_addr = (self.control_port.data_port_address & 0x7F) >> 1;
                self.cram[cram_addr as usize] = word;
            }
            DataPortLocation::Vsram => {
                // VSRAM fill is bugged; uses the value from the next FIFO slot instead of fill data
                let word = self.fifo.next_slot_word();
                let vsram_addr = (self.control_port.data_port_address & 0x7F & !1) as usize;
                if vsram_addr < VSRAM_LEN {
                    self.vsram[vsram_addr] = word.msb();
                    self.vsram[vsram_addr + 1] = word.lsb();
                }
            }
            DataPortLocation::Invalid => {}
        }

        self.control_port.increment_data_port_address(&self.registers);
        self.decrement_dma_length();
    }

    fn progress_vram_copy_dma(&mut self) {
        self.state.vram_copy_odd_slot = !self.state.vram_copy_odd_slot;
        if self.state.vram_copy_odd_slot {
            return;
        }

        let source_addr = (self.registers.dma_source_address >> 1) ^ 1;
        self.increment_dma_source_address();

        let dest_addr = (self.control_port.data_port_address & 0xFFFF) ^ 1;
        self.control_port.increment_data_port_address(&self.registers);

        let byte = self.vram[source_addr as usize];
        self.vram[dest_addr as usize] = byte;
        self.maybe_update_sprite_cache(dest_addr as u16, byte);

        self.decrement_dma_length();
    }

    fn increment_dma_source_address(&mut self) {
        // DMA source address always wraps within a 0x20000-byte block
        self.registers.dma_source_address = (self.registers.dma_source_address & !0x1FFFF)
            | (self.registers.dma_source_address.wrapping_add(2) & 0x1FFFF);
    }

    fn decrement_dma_length(&mut self) {
        // Check for 0 after decrementing; an initial DMA length of 0 should function as 65536
        self.registers.dma_length = self.registers.dma_length.wrapping_sub(1);
        if self.registers.dma_length != 0 {
            return;
        }

        self.control_port.dma_active = false;

        // Hack: If a memory-to-VRAM DMA finishes in fewer than 5 slots, keep a small latency
        // before allowing FIFO writes through. This fixes the VDPFIFOTesting FIFO wait states
        // test from occasionally failing
        self.dma_latency = cmp::min(2, self.dma_latency);

        if !self.state.pending_writes.is_empty() {
            // If any port writes were enqueued after starting a memory-to-VRAM DMA, wait 5
            // pixels before applying them (slightly longer than 5 CPU cycles)
            // Fewer than 5 pixels causes a glitchy line in Mickey Mania's 3D stages
            self.state.pending_write_delay_pixels = 5;
        }

        log::trace!(
            "DMA of type {:?} complete at line {} mclk {}; FIFO len {}",
            self.registers.dma_mode,
            self.state.scanline,
            self.state.scanline_mclk_cycles,
            self.fifo.len()
        );
    }

    fn vdp_event_times(h_display_size: HorizontalDisplaySize) -> [VdpEventWithTime; VdpEvent::NUM] {
        let events = [
            VdpEventWithTime::new(h_display_size.v_interrupt_h(), VdpEvent::VInterrupt),
            VdpEventWithTime::new(
                h_display_size.active_display_h_range().start,
                VdpEvent::RenderLine,
            ),
            VdpEventWithTime::new(
                h_display_size.fetch_sprite_attributes_h(),
                VdpEvent::FetchSpriteAttributes,
            ),
            VdpEventWithTime::new(h_display_size.hblank_begin_h(), VdpEvent::HBlankStart),
            VdpEventWithTime::new(h_display_size.h_interrupt_h(), VdpEvent::HInterrupt),
            VdpEventWithTime::new(h_display_size.latch_registers_h(), VdpEvent::LatchRegisters),
            VdpEventWithTime::new(u16::MAX, VdpEvent::None),
        ];

        debug_assert!(
            (0..events.len() - 1).all(|i| events[i + 1].h >= events[i].h),
            "Events must be in H-sorted order"
        );

        let count_occurrences =
            |event: VdpEvent| events.iter().filter(|e| e.event == event).count();
        debug_assert!(
            VdpEvent::ALL.into_iter().all(|event| count_occurrences(event) == 1),
            "Every VdpEvent value must be present exactly once"
        );

        events
    }

    fn handle_vdp_event(&mut self, event: VdpEvent) {
        match event {
            VdpEvent::VInterrupt => {
                let active_scanlines = self.registers.vertical_display_size.active_scanlines();
                if self.state.scanline == active_scanlines {
                    log::trace!("Generating V interrupt");
                    self.state.v_interrupt_pending = true;

                    // Latch H resolution at start of VBlank in case the game changes resolution
                    // at start of VBlank, e.g. Bugs Bunny in Double Trouble
                    self.state.frame_h_resolution = self.registers.horizontal_display_size;
                }
            }
            VdpEvent::RenderLine => {
                self.sprite_state
                    .handle_line_end(self.registers.horizontal_display_size, self.state.pixel);

                // Sprite processing phase 1
                // In actual hardware this takes place during HBlank
                log::trace!("Scanning sprites");
                self.scan_sprites_one_line_ahead();

                // Render current line
                self.render_scanline(self.state.scanline, 0);
            }
            VdpEvent::FetchSpriteAttributes => {
                // Sprite processing phase 2
                // In actual hardware this takes place during active display
                log::trace!("Fetching sprite attributes");
                self.fetch_sprite_attributes();
            }
            VdpEvent::HBlankStart => {
                self.sprite_state.handle_hblank_start(
                    self.registers.horizontal_display_size,
                    self.registers.display_enabled,
                );

                let active_scanlines = self.registers.vertical_display_size.active_scanlines();
                let scanlines_per_frame = self.scanlines_in_current_frame();
                if self.state.scanline == active_scanlines - 1
                    || (self.state.v_border_forgotten
                        && self.state.scanline
                            == VerticalDisplaySize::ThirtyCell.active_scanlines() - 1)
                {
                    self.state.in_vblank = true;
                } else if self.state.scanline == scanlines_per_frame - 2 {
                    self.state.in_vblank = false;
                }
            }
            VdpEvent::HInterrupt => {
                log::trace!("Latching cached sprite table");
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
                log::trace!("Latching VDP registers");
                self.latched_registers = self.registers.clone();
                self.latched_full_screen_v_scroll = (
                    u16::from_be_bytes([self.vsram[0], self.vsram[1]]),
                    u16::from_be_bytes([self.vsram[2], self.vsram[3]]),
                );
            }
            VdpEvent::None => {}
        }
    }

    fn decrement_h_interrupt_counter(&mut self) {
        let active_scanlines = self.registers.vertical_display_size.active_scanlines();
        let scanlines_per_frame = self.scanlines_in_current_frame();

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
        let scanlines_per_frame = self.scanlines_in_current_frame();

        self.cram_dots.swap_buffers_if_needed();

        self.state.scanline_mclk_cycles -= MCLK_CYCLES_PER_SCANLINE;
        self.vdp_event_idx = 0;
        self.state.scanline += 1;
        self.state.pixel = 0;
        if self.state.scanline == scanlines_per_frame {
            self.state.scanline = 0;
            self.state.v_border_forgotten = false;

            let next_frame_interlaced = matches!(
                self.registers.interlacing_mode,
                InterlacingMode::Interlaced | InterlacingMode::InterlacedDouble
            );

            if next_frame_interlaced && !self.state.interlaced_frame {
                self.state.interlaced_odd = false;

                if !self.config.deinterlace {
                    self.prepare_frame_buffer_for_interlaced();
                }
            }

            self.state.interlaced_frame = next_frame_interlaced;

            // Top border length needs to be saved at start-of-frame in case there is a mid-frame swap between V28
            // mode and V30 mode. Titan Overdrive 2 depends on this for the arcade scene
            self.state.top_border =
                self.registers.vertical_display_size.top_border(self.timing_mode);

            // Re-latch H display mode at start of frame in case a game changed it mid-VBlank
            // Not doing this causes a glitchy frame on mode switches
            self.state.frame_h_resolution = self.registers.horizontal_display_size;
        } else if self.state.interlaced_frame {
            let toggle_odd_line = match self.registers.vertical_display_size {
                VerticalDisplaySize::TwentyEightCell => 240,
                VerticalDisplaySize::ThirtyCell => 256,
            };
            if self.state.scanline == toggle_odd_line {
                // TODO this actually happens at H=0x001 or H=0x002
                self.state.interlaced_odd = !self.state.interlaced_odd;
            }
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

    #[must_use]
    pub fn m68k_interrupt_level(&self) -> u8 {
        // TODO external interrupts at level 2
        if self.state.v_interrupt_pending
            && self.registers.v_interrupt_enabled
            && !self.state.delayed_v_interrupt
        {
            6
        } else if self.state.h_interrupt_pending
            && self.registers.h_interrupt_enabled
            && !self.state.delayed_h_interrupt
        {
            4
        } else {
            0
        }
    }

    #[inline]
    pub fn clear_interrupt_delays(&mut self) {
        self.state.delayed_v_interrupt = false;
        self.state.delayed_h_interrupt = false;
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

    #[inline]
    #[must_use]
    pub fn should_halt_cpu(&self) -> bool {
        (self.control_port.dma_active && self.registers.dma_mode == DmaMode::MemoryToVram)
            || self.state.data_port_read_wait
            || !self.pending_fifo_writes.is_empty()
    }

    #[inline]
    #[must_use]
    pub fn long_halting_dma_in_progress(&self) -> bool {
        self.control_port.dma_active
            && self.registers.dma_mode == DmaMode::MemoryToVram
            && self.registers.dma_length
                >= self.registers.horizontal_display_size.access_slots_per_blank_line()
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

    fn scan_sprites_one_line_ahead(&mut self) {
        let scanlines_per_frame = self.scanlines_in_current_frame();
        let scanline_for_sprite_scan = if self.state.scanline == scanlines_per_frame - 1 {
            0
        } else {
            self.state.scanline + 1
        };

        self.scan_sprites(scanline_for_sprite_scan);
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
    pub fn scanlines_in_current_frame(&self) -> u16 {
        self.timing_mode.scanlines_per_frame(self.state.interlaced_frame, self.state.interlaced_odd)
    }

    #[inline]
    #[must_use]
    pub fn is_interlaced_frame(&self) -> bool {
        self.state.interlaced_frame
    }

    #[inline]
    #[must_use]
    pub fn is_interlaced_odd(&self) -> bool {
        self.state.interlaced_frame && self.state.interlaced_odd
    }

    #[inline]
    #[must_use]
    pub fn average_scanlines_per_frame(&self) -> f64 {
        let interlaced_frame = self.state.interlaced_frame;
        match self.timing_mode {
            TimingMode::Ntsc => {
                f64::from(NTSC_SCANLINES_PER_FRAME) + 0.5 * f64::from(interlaced_frame)
            }
            TimingMode::Pal => {
                f64::from(PAL_SCANLINES_PER_FRAME) - 0.5 * f64::from(interlaced_frame)
            }
        }
    }

    #[inline]
    #[must_use]
    pub fn screen_width(&self) -> u32 {
        let h_display_size = self.state.frame_h_resolution;
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
            let h_display_size = self.state.frame_h_resolution;
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

fn pixel_to_internal_h(pixel: u16, h_display_size: HorizontalDisplaySize) -> u16 {
    match h_display_size {
        HorizontalDisplaySize::ThirtyTwoCell => pixel_to_internal_h_h32(pixel),
        HorizontalDisplaySize::FortyCell => pixel_to_internal_h_h40(pixel),
    }
}

fn scanline_mclk_to_pixel_h32(scanline_mclk: u64) -> u16 {
    // H32 pixel clock is always mclk/10
    (scanline_mclk / 10) as u16
}

fn pixel_to_internal_h_h32(pixel: u16) -> u16 {
    if pixel <= 0x127 { pixel } else { pixel + (0x1D2 - 0x128) }
}

fn internal_h_to_pixel_h32(internal_h: u16) -> u16 {
    if internal_h <= 0x127 { internal_h } else { internal_h - (0x1D2 - 0x128) }
}

fn scanline_mclk_to_pixel_h40(scanline_mclk: u64) -> u16 {
    // Note H jumps 0x16C to 0x1C9 right before HSYNC
    const JUMP_DIFF: u64 = 0x1C9 - 0x16D;

    // Special cases due to pixel clock varying during HSYNC in H40 mode
    // https://gendev.spritesmind.net/forum/viewtopic.php?t=3221

    // Pixel clock is mclk/8 from H=0x000 through H=0x1CB
    if scanline_mclk < (0x1CC - JUMP_DIFF) * 8 {
        return (scanline_mclk / 8) as u16;
    }

    // From H=0x1CC through H=0x1ED, follows this pattern, repeated twice:
    //   1 mclk/8, 7 mclk/10, 2 mclk/9, 7 mclk/10
    let hsync_start_mclk = (0x1CC - JUMP_DIFF) * 8;
    let hsync_end_mclk = hsync_start_mclk + 2 * (8 + 7 * 10 + 2 * 9 + 7 * 10);
    if (hsync_start_mclk..hsync_end_mclk).contains(&scanline_mclk) {
        let hsync_mclk = scanline_mclk - hsync_start_mclk;
        let pattern_pixel = match hsync_mclk % (8 + 7 * 10 + 2 * 9 + 7 * 10) {
            // 1 pixel at mclk/8
            0..=7 => 0,
            // 7 pixels at mclk/10
            pattern_mclk @ 8..=77 => 1 + (pattern_mclk - 8) / 10,
            // 2 pixels at mclk/9 (effectively)
            pattern_mclk @ 78..=95 => 8 + (pattern_mclk - 78) / 9,
            // 7 pixels at mclk/10
            pattern_mclk @ 96..=165 => 10 + (pattern_mclk - 96) / 10,
            _ => unreachable!("value % 166 is always < 166"),
        };

        return if hsync_mclk < 166 {
            // First repetition
            (0x1CC - JUMP_DIFF + pattern_pixel) as u16
        } else {
            // Second repetition
            (0x1CC - JUMP_DIFF + 17 + pattern_pixel) as u16
        };
    }

    // From H=0x1EE to H=0x1FF, stays at mclk/8
    let post_hsync_mclk = scanline_mclk - hsync_end_mclk;
    (0x1CC - JUMP_DIFF + 34 + post_hsync_mclk / 8) as u16
}

fn pixel_to_internal_h_h40(pixel: u16) -> u16 {
    if pixel <= 0x16C { pixel } else { pixel + (0x1C9 - 0x16D) }
}

fn internal_h_to_pixel_h40(internal_h: u16) -> u16 {
    if internal_h <= 0x16C { internal_h } else { internal_h - (0x1C9 - 0x16D) }
}
