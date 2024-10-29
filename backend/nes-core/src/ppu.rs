//! PPU (pixel/picture processing unit) emulation code.
//!
//! In NTSC, the PPU constantly cycles through 262 scanlines: 240 visible scanlines where the PPU is
//! actively rendering pixels, a 21-scanline vertical blanking period where the PPU is idle, and a
//! pre-render scanline where the PPU fetches data that is needed to render the first visible scanline.
//!
//! PAL is (mostly) the same except the vertical blanking period lasts for 70 scanlines instead of 20,
//! for a total of 312 scanlines.

use crate::api::NesEmulatorConfig;
use crate::bus;
use crate::bus::{PpuBus, PpuRegisters, PpuTrackedRegister, PpuWriteToggle};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::TimingMode;
use jgenesis_common::num::GetBit;
use std::array;
use std::fmt::{Display, Formatter};
use std::ops::RangeInclusive;

pub const SCREEN_WIDTH: u16 = 256;
pub const MAX_SCREEN_HEIGHT: u16 = 240;

const DOTS_PER_SCANLINE: u16 = 341;
// Set/reset flags on dot 2 instead of 1 to resolve some CPU/PPU alignment issues that affect NMI
// timing
const VBLANK_FLAG_SET_DOT: u16 = 2;
const RENDERING_DOTS: RangeInclusive<u16> = 1..=256;
const SPRITE_EVALUATION_DOTS: RangeInclusive<u16> = 65..=256;
const BG_TILE_PRE_FETCH_DOTS: RangeInclusive<u16> = 321..=336;
const RESET_VERTICAL_POS_DOTS: RangeInclusive<u16> = 280..=304;
const INC_VERTICAL_POS_DOT: u16 = 256;
const RESET_HORIZONTAL_POS_DOT: u16 = 257;
const FIRST_SPRITE_TILE_FETCH_DOT: u16 = 257;

const VISIBLE_SCANLINES: RangeInclusive<u16> = 0..=239;
const FIRST_VBLANK_SCANLINE: u16 = 241;
const NTSC_VBLANK_SCANLINES: RangeInclusive<u16> = 241..=260;
const NTSC_ALL_IDLE_SCANLINES: RangeInclusive<u16> = 240..=260;
const NTSC_PRE_RENDER_SCANLINE: u16 = 261;
const PAL_VBLANK_SCANLINES: RangeInclusive<u16> = 241..=310;
const PAL_ALL_IDLE_SCANLINES: RangeInclusive<u16> = 240..=310;
const PAL_PRE_RENDER_SCANLINE: u16 = 311;

const BLACK_NES_COLOR: u8 = 0x0F;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct ColorEmphasis(u8);

impl ColorEmphasis {
    pub const NONE: Self = Self(0);

    pub fn new(red: bool, green: bool, blue: bool) -> Self {
        let emphasis_bits = u8::from(red) | (u8::from(green) << 1) | (u8::from(blue) << 2);
        Self(emphasis_bits)
    }

    pub fn get_current(bus: &PpuBus<'_>, timing_mode: TimingMode) -> Self {
        let ppu_registers = bus.get_ppu_registers();
        Self::new(
            ppu_registers.emphasize_red(timing_mode),
            ppu_registers.emphasize_green(timing_mode),
            ppu_registers.emphasize_blue(),
        )
    }

    pub fn red(self) -> bool {
        self.0.bit(0)
    }

    pub fn green(self) -> bool {
        self.0.bit(1)
    }

    pub fn blue(self) -> bool {
        self.0.bit(2)
    }
}

impl Display for ColorEmphasis {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "ColorEmphasis[R={}, G={}, B={}]", (*self).red(), (*self).green(), (*self).blue())
    }
}

pub type FrameBuffer = [[(u8, ColorEmphasis); SCREEN_WIDTH as usize]; MAX_SCREEN_HEIGHT as usize];

trait TimingModePpuExt {
    fn vblank_scanlines(self) -> RangeInclusive<u16>;

    fn all_idle_scanlines(self) -> RangeInclusive<u16>;

    fn pre_render_scanline(self) -> u16;
}

impl TimingModePpuExt for TimingMode {
    fn vblank_scanlines(self) -> RangeInclusive<u16> {
        match self {
            Self::Ntsc => NTSC_VBLANK_SCANLINES,
            Self::Pal => PAL_VBLANK_SCANLINES,
        }
    }

    fn all_idle_scanlines(self) -> RangeInclusive<u16> {
        match self {
            Self::Ntsc => NTSC_ALL_IDLE_SCANLINES,
            Self::Pal => PAL_ALL_IDLE_SCANLINES,
        }
    }

    fn pre_render_scanline(self) -> u16 {
        match self {
            Self::Ntsc => NTSC_PRE_RENDER_SCANLINE,
            Self::Pal => PAL_PRE_RENDER_SCANLINE,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct InternalRegisters {
    // v register (15-bit)
    vram_address: u16,
    // t register (15-bit)
    temp_vram_address: u16,
    // x register (3-bit)
    fine_x_scroll: u8,
}

impl InternalRegisters {
    fn new() -> Self {
        Self { vram_address: 0, temp_vram_address: 0, fine_x_scroll: 0 }
    }

    fn fine_y(&self) -> u16 {
        self.vram_address >> 12
    }

    fn fine_x(&self) -> u8 {
        self.fine_x_scroll
    }

    fn coarse_y(&self) -> u16 {
        (self.vram_address >> 5) & 0x001F
    }

    fn coarse_x(&self) -> u16 {
        self.vram_address & 0x001F
    }

    fn nametable_bits(&self) -> u16 {
        self.vram_address & 0x0C00
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct BgBuffers {
    pattern_table_low: u16,
    pattern_table_high: u16,
    palette_indices: u32,
    next_nametable_byte: u8,
    next_palette_indices: u16,
    next_pattern_table_low: u8,
    next_pattern_table_high: u8,
}

impl BgBuffers {
    fn new() -> Self {
        Self {
            pattern_table_low: 0,
            pattern_table_high: 0,
            palette_indices: 0,
            next_nametable_byte: 0,
            next_palette_indices: 0,
            next_pattern_table_low: 0,
            next_pattern_table_high: 0,
        }
    }

    fn reload(&mut self) {
        self.pattern_table_low |= u16::from(self.next_pattern_table_low);
        self.pattern_table_high |= u16::from(self.next_pattern_table_high);
        self.palette_indices |= u32::from(self.next_palette_indices);
    }

    fn shift(&mut self) {
        self.pattern_table_low <<= 1;
        self.pattern_table_high <<= 1;
        self.palette_indices <<= 2;
    }

    fn get_palette_index(&self, fine_x_scroll: u8) -> u8 {
        ((self.palette_indices & (0xC0000000 >> (2 * fine_x_scroll))) >> (30 - 2 * fine_x_scroll))
            as u8
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct SpriteBufferData {
    y_position: u8,
    x_position: u8,
    attributes: u8,
    tile_index: u8,
}

impl Default for SpriteBufferData {
    fn default() -> Self {
        Self { y_position: 0xFF, x_position: 0xFF, attributes: 0xFF, tile_index: 0xFF }
    }
}

const SPRITE_BUFFER_LEN: usize = 64;

#[derive(Debug, Clone, Encode, Decode)]
struct SpriteBuffers {
    sprites: [SpriteBufferData; SPRITE_BUFFER_LEN],
    pattern_table_low: [u8; SPRITE_BUFFER_LEN],
    pattern_table_high: [u8; SPRITE_BUFFER_LEN],
    buffer_len: u8,
    sprite_0_buffered: bool,
}

impl SpriteBuffers {
    fn new() -> Self {
        Self {
            sprites: array::from_fn(|_| SpriteBufferData::default()),
            pattern_table_low: array::from_fn(|_| 0),
            pattern_table_high: array::from_fn(|_| 0),
            buffer_len: 0,
            sprite_0_buffered: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum SpriteEvaluationState {
    ScanningOam { primary_oam_index: u8 },
    CopyingOam { primary_oam_index: u8, byte_index: u8 },
    CheckingForOverflow { oam_index: u8, oam_offset: u8, skip_bytes_remaining: u8 },
    Done { oam_index: u8 },
}

#[derive(Debug, Clone, Encode, Decode)]
struct SpriteEvaluationData {
    secondary_oam: [u8; SPRITE_BUFFER_LEN * 4],
    sprites_found: u8,
    sprite_0_found: bool,
    state: SpriteEvaluationState,
}

impl SpriteEvaluationData {
    fn new() -> Self {
        Self {
            secondary_oam: [0xFF; SPRITE_BUFFER_LEN * 4],
            sprites_found: 0,
            sprite_0_found: false,
            state: SpriteEvaluationState::ScanningOam { primary_oam_index: 0 },
        }
    }

    fn update_sprite_buffers(&self, buffers: &mut SpriteBuffers) {
        buffers.sprites.fill(SpriteBufferData::default());

        for (i, chunk) in
            self.secondary_oam.chunks_exact(4).take(self.sprites_found as usize).enumerate()
        {
            let &[y_position, tile_index, attributes_byte, x_position] = chunk else {
                unreachable!("all chunks from chunks_exact(4) should be of size 4")
            };

            buffers.sprites[i] = SpriteBufferData {
                y_position,
                x_position,
                attributes: attributes_byte,
                tile_index,
            };
        }

        buffers.pattern_table_low.fill(0);
        buffers.pattern_table_high.fill(0);
        buffers.buffer_len = self.sprites_found;
        buffers.sprite_0_buffered = self.sprite_0_found;
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct SpriteData {
    color_id: u8,
    is_sprite_0: bool,
    attributes: u8,
}

impl SpriteData {
    // Color 0 is transparent and will never display
    const NONE: Self = Self { color_id: 0, is_sprite_0: false, attributes: 0x00 };
}

type SpriteLineBuffer = [SpriteData; SCREEN_WIDTH as usize];

#[derive(Debug, Clone, Encode, Decode)]
pub struct PpuState {
    timing_mode: TimingMode,
    frame_buffer: Box<FrameBuffer>,
    registers: InternalRegisters,
    bg_buffers: BgBuffers,
    sprite_buffers: SpriteBuffers,
    sprite_evaluation_data: SpriteEvaluationData,
    sprite_line_buffer: SpriteLineBuffer,
    scanline: u16,
    dot: u16,
    odd_frame: bool,
    pending_sprite_0_hit: bool,
}

impl PpuState {
    pub fn new(timing_mode: TimingMode) -> Self {
        Self {
            timing_mode,
            frame_buffer: vec![
                [(0, ColorEmphasis::default()); SCREEN_WIDTH as usize];
                MAX_SCREEN_HEIGHT as usize
            ]
            .into_boxed_slice()
            .try_into()
            .unwrap(),
            registers: InternalRegisters::new(),
            bg_buffers: BgBuffers::new(),
            sprite_buffers: SpriteBuffers::new(),
            sprite_evaluation_data: SpriteEvaluationData::new(),
            sprite_line_buffer: array::from_fn(|_| SpriteData::NONE),
            scanline: timing_mode.pre_render_scanline(),
            dot: 0,
            odd_frame: false,
            pending_sprite_0_hit: false,
        }
    }

    /// Return whether the PPU is currently in the vertical blanking period.
    ///
    /// While the PPU's first idle scanline is scanline 240, this method will not return true
    /// until scanline 241 in order to align with when the PPU sets the VBlank flag in PPUSTATUS.
    pub fn in_vblank(&self) -> bool {
        self.timing_mode.vblank_scanlines().contains(&self.scanline)
    }

    /// Retrieve a reference the PPU's frame buffer.
    ///
    /// The frame buffer is a 256x240 grid storing 6-bit NES colors. These colors
    /// do not map directly to RGB; some sort of palette is needed to convert these colors to RGB
    /// colors that are appropriate for display.
    pub fn frame_buffer(&self) -> &FrameBuffer {
        &self.frame_buffer
    }

    fn set_in_frame_buffer(
        &mut self,
        y: u16,
        x: u16,
        pixel: u8,
        color_emphasis: ColorEmphasis,
        bus: &mut PpuBus<'_>,
    ) {
        self.frame_buffer[y as usize][x as usize] = (pixel, color_emphasis);
        bus.handle_pixel_rendered(pixel, x, y, self.timing_mode);
    }
}

pub fn render_pal_black_border(state: &mut PpuState) {
    // Clear top scanline
    for (color, emphasis) in &mut state.frame_buffer[0] {
        *color = BLACK_NES_COLOR;
        *emphasis = ColorEmphasis::default();
    }

    // Clear leftmost two columns and rightmost two columns
    for col in [0, 1, (SCREEN_WIDTH - 2) as usize, (SCREEN_WIDTH - 1) as usize] {
        for row in 1..MAX_SCREEN_HEIGHT as usize {
            state.frame_buffer[row][col] = (BLACK_NES_COLOR, ColorEmphasis::default());
        }
    }
}

/// Run the PPU for one PPU cycle. Pixels will be written to `PpuState`'s frame buffer as appropriate.
pub fn tick(state: &mut PpuState, bus: &mut PpuBus<'_>, config: NesEmulatorConfig) {
    let rendering_enabled =
        bus.get_ppu_registers().bg_enabled() || bus.get_ppu_registers().sprites_enabled();

    process_register_updates(state, bus, rendering_enabled);

    if state.scanline == state.timing_mode.pre_render_scanline() && state.dot == VBLANK_FLAG_SET_DOT
    {
        // Clear per-frame flags at the start of the pre-render scanline
        let ppu_registers = bus.get_ppu_registers_mut();
        ppu_registers.set_vblank_flag(false);
        ppu_registers.set_sprite_0_hit(false);
        ppu_registers.set_sprite_overflow(false);
    } else if state.scanline == FIRST_VBLANK_SCANLINE && state.dot == VBLANK_FLAG_SET_DOT {
        bus.get_ppu_registers_mut().set_vblank_flag(true);
    }

    let color_mask = get_color_mask(bus.get_ppu_registers());
    if rendering_enabled {
        process_scanline(state, bus, config.remove_sprite_limit);
    } else {
        bus.get_ppu_registers_mut().set_oam_open_bus(None);

        if VISIBLE_SCANLINES.contains(&state.scanline) && RENDERING_DOTS.contains(&state.dot) {
            // When rendering is disabled, the PPU normally always outputs the backdrop color (index 0),
            // but if the current VRAM address is in the palette RAM range ($3F00-$3FFF) then it will
            // use the color at the current palette RAM address instead.
            // Micro Machines depends on this for correct rendering, as do certain test roms (e.g. full_palette.nes)
            let vram_addr = state.registers.vram_address & 0x3FFF;
            let palette_ram_addr = if (0x3F00..=0x3FFF).contains(&vram_addr) {
                vram_addr & bus::PALETTE_RAM_MASK
            } else {
                0
            };
            let backdrop_color = bus.get_palette_ram()[palette_ram_addr as usize] & color_mask;

            let color_emphasis = ColorEmphasis::get_current(bus, state.timing_mode);
            state.set_in_frame_buffer(
                state.scanline,
                state.dot - 1,
                backdrop_color,
                color_emphasis,
                bus,
            );
        }
    }

    // Copy v register to where the CPU can see it
    if !rendering_enabled || state.timing_mode.all_idle_scanlines().contains(&state.scanline) {
        bus.set_bus_address(state.registers.vram_address & 0x3FFF);
    }

    state.dot += 1;
    if state.dot == DOTS_PER_SCANLINE {
        state.scanline += 1;
        state.dot = 0;

        if state.scanline == state.timing_mode.pre_render_scanline() + 1 {
            state.scanline = 0;

            if state.timing_mode == TimingMode::Ntsc && state.odd_frame && rendering_enabled {
                // In NTSC, skip the idle cycle in the first visible scanline on odd frames
                state.dot = 1;
            }
            state.odd_frame = !state.odd_frame;
        }
    }
}

fn get_color_mask(registers: &PpuRegisters) -> u8 {
    // NES colors are 6 bits normally, and greyscale mode masks out the lower 4 bits
    if registers.greyscale() { 0x30 } else { 0x3F }
}

/// Reset the PPU, as if the console's reset button was pressed.
///
/// This resets all PPU state except for the internal v register, and also clears most of the
/// memory-mapped PPU regsiters.
pub fn reset(state: &mut PpuState, bus: &mut PpuBus<'_>) {
    let vram_address = state.registers.vram_address;
    *state = PpuState::new(state.timing_mode);
    state.registers.vram_address = vram_address;

    bus.reset();
}

fn process_scanline(state: &mut PpuState, bus: &mut PpuBus<'_>, remove_sprite_limit: bool) {
    let scanline = state.scanline;
    let dot = state.dot;
    let timing_mode = state.timing_mode;

    log::trace!("Rendering at scanline {} dot {}", scanline, dot);

    if state.pending_sprite_0_hit {
        // If sprite 0 hit triggered on the last cycle, set the flag in PPUSTATUS
        state.pending_sprite_0_hit = false;
        bus.get_ppu_registers_mut().set_sprite_0_hit(true);
    }

    match (timing_mode, scanline) {
        (_, 0..=239) | (TimingMode::Ntsc, 261) | (TimingMode::Pal, 311) => {
            let is_pre_render_scanline = scanline == timing_mode.pre_render_scanline();

            if is_pre_render_scanline && RESET_VERTICAL_POS_DOTS.contains(&dot) {
                // Repeatedly reset vertical position during the pre-render scanline
                reset_vertical_pos(&mut state.registers);
            }

            if !is_pre_render_scanline && dot == 1 {
                // Clear sprite evaluation data at the beginning of each visible scanline
                state.sprite_evaluation_data = SpriteEvaluationData::new();
            }

            // Render scanlines
            #[allow(clippy::match_same_arms)]
            match dot {
                0 => {
                    // Idle cycle

                    bus.get_ppu_registers_mut()
                        .set_oam_open_bus(Some(state.sprite_evaluation_data.secondary_oam[0]));
                }
                1..=256 => {
                    // Rendering + sprite evaluation cycles

                    if !is_pre_render_scanline {
                        render_pixel(state, bus);
                    }

                    if dot > 1 && (dot - 1).trailing_zeros() >= 3 {
                        // Increment horizontal position on cycles 9, 17, 25, ..
                        // before fetching BG tile data
                        increment_horizontal_pos(&mut state.registers);
                    }

                    // Start fetching data for the next tile cycle if appropriate
                    fetch_bg_tile_data(state, bus);

                    // Evaluate sprites on odd cycles during 65-256
                    if !is_pre_render_scanline
                        && SPRITE_EVALUATION_DOTS.contains(&dot)
                        && dot.bit(0)
                    {
                        evaluate_sprites(state, bus, remove_sprite_limit);

                        if remove_sprite_limit && dot == 255 {
                            finish_sprite_evaluation_no_limit(state, bus);
                        }
                    }

                    if !SPRITE_EVALUATION_DOTS.contains(&dot) {
                        // OAMDATA always reads $FF during cycles 1-64
                        bus.get_ppu_registers_mut().set_oam_open_bus(Some(0xFF));
                    }

                    if !is_pre_render_scanline && dot == INC_VERTICAL_POS_DOT {
                        // Increment effective vertical position at the end of the rendering phase
                        increment_vertical_pos(&mut state.registers);
                    }
                }
                257..=320 => {
                    // Cycles for fetching sprite data for the next scanline

                    if dot == RESET_HORIZONTAL_POS_DOT {
                        // Reset horizontal position immediately after the rendering phase
                        reset_horizontal_pos(&mut state.registers);

                        // Fill sprite buffers with sprite data for the next scanline
                        state
                            .sprite_evaluation_data
                            .update_sprite_buffers(&mut state.sprite_buffers);
                    }

                    fetch_sprite_tile_data(bus, scanline, dot, &mut state.sprite_buffers);

                    if dot == 320 {
                        if remove_sprite_limit {
                            finish_sprite_tile_fetches(
                                bus,
                                scanline,
                                dot,
                                &mut state.sprite_buffers,
                            );
                        }
                        fill_sprite_line_buffer(
                            &state.sprite_buffers,
                            &mut state.sprite_line_buffer,
                        );
                    }
                }
                321..=336 => {
                    // Cycles for fetching BG tile data for the first 2 tiles of the next scanline

                    fetch_bg_tile_data(state, bus);
                    state.bg_buffers.shift();

                    if dot.trailing_zeros() >= 3 {
                        // Increment horizontal position and reload buffers at the end of each tile
                        // (dots 328 and 336)
                        increment_horizontal_pos(&mut state.registers);
                        state.bg_buffers.reload();
                    }

                    bus.get_ppu_registers_mut()
                        .set_oam_open_bus(Some(state.sprite_evaluation_data.secondary_oam[0]));
                }
                337 | 339 => {
                    // Idle cycles that do spurious reads
                    // At least one mapper depends on these reads happening (MMC5)
                    fetch_nametable_byte(&state.registers, bus);

                    bus.get_ppu_registers_mut()
                        .set_oam_open_bus(Some(state.sprite_evaluation_data.secondary_oam[0]));
                }
                338 | 340 => {
                    // Truly idle cycles at the end of each scanline

                    bus.get_ppu_registers_mut()
                        .set_oam_open_bus(Some(state.sprite_evaluation_data.secondary_oam[0]));
                }
                _ => panic!("invalid dot: {dot}"),
            }
        }
        (TimingMode::Ntsc, 240..=260) | (TimingMode::Pal, 240..=310) => {
            // PPU idle scanlines

            bus.get_ppu_registers_mut().set_oam_open_bus(None);
        }
        _ => panic!("invalid scanline: {scanline}"),
    }
}

fn process_register_updates(state: &mut PpuState, bus: &mut PpuBus<'_>, rendering_enabled: bool) {
    match bus.get_ppu_registers_mut().take_last_accessed_register() {
        Some(PpuTrackedRegister::PPUCTRL) => {
            let ppu_ctrl = bus.get_ppu_registers().ppu_ctrl();
            log::trace!(
                "PPU: {ppu_ctrl:02X} written to PPUCTRL on scanline {}, dot {}",
                state.scanline,
                state.dot
            );

            // Set nametable bits
            state.registers.temp_vram_address =
                (state.registers.temp_vram_address & 0xF3FF) | (u16::from(ppu_ctrl & 0x03) << 10);
        }
        Some(PpuTrackedRegister::PPUSCROLL) => {
            let value = bus.get_ppu_registers().get_ppu_open_bus_value();
            log::trace!(
                "PPU: {value:02X} written to PPUSCROLL, write_toggle={:?} on scanline {}, dot {}",
                bus.get_ppu_registers().get_write_toggle(),
                state.scanline,
                state.dot,
            );

            match bus.get_ppu_registers().get_write_toggle() {
                PpuWriteToggle::Second => {
                    // Write was with w=0, set coarse X and fine X
                    state.registers.temp_vram_address =
                        (state.registers.temp_vram_address & 0xFFE0) | u16::from(value >> 3);
                    state.registers.fine_x_scroll = value & 0x07;
                }
                PpuWriteToggle::First => {
                    // Write was with w=1, set coarse Y and fine Y
                    state.registers.temp_vram_address = (state.registers.temp_vram_address
                        & 0x0C1F)
                        | (u16::from(value & 0x07) << 12)
                        | (u16::from(value & 0xF8) << 2);
                }
            }
        }
        Some(PpuTrackedRegister::PPUADDR) => {
            let value = bus.get_ppu_registers().get_ppu_open_bus_value();
            log::trace!(
                "PPU: {value:02X} written to PPUADDR, write_toggle={:?} on scanline {}, dot {}",
                bus.get_ppu_registers().get_write_toggle(),
                state.scanline,
                state.dot
            );

            match bus.get_ppu_registers().get_write_toggle() {
                PpuWriteToggle::Second => {
                    // Write was with w=0, set bits 13-8 and clear bit 14
                    state.registers.temp_vram_address = (state.registers.temp_vram_address
                        & 0x00FF)
                        | (u16::from(value & 0x3F) << 8);
                }
                PpuWriteToggle::First => {
                    // Write was with w=1, set bits 7-0 and copy from t to v
                    state.registers.temp_vram_address =
                        (state.registers.temp_vram_address & 0xFF00) | u16::from(value);
                    state.registers.vram_address = state.registers.temp_vram_address;
                }
            }
        }
        Some(PpuTrackedRegister::PPUDATA) => {
            if rendering_enabled
                && (VISIBLE_SCANLINES.contains(&state.scanline)
                    || state.scanline == state.timing_mode.pre_render_scanline())
            {
                // Accessing PPUDATA during rendering causes a coarse X increment + Y increment
                log::trace!(
                    "PPU: PPUDATA was accessed during rendering (scanline {} / dot {}), incrementing coarse X and Y in v register",
                    state.scanline,
                    state.dot
                );

                increment_horizontal_pos(&mut state.registers);
                increment_vertical_pos(&mut state.registers);
            } else {
                log::trace!(
                    "PPU: PPUDATA was accessed on scanline {} / dot {}, incrementing internal v register by {}",
                    state.scanline,
                    state.dot,
                    bus.get_ppu_registers().ppu_data_addr_increment()
                );

                // Any time the CPU accesses PPUDATA outside of rendering, increment VRAM address by
                // 1 or 32 based on PPUCTRL
                state.registers.vram_address = state
                    .registers
                    .vram_address
                    .wrapping_add(bus.get_ppu_registers().ppu_data_addr_increment());
            }
        }
        None => {}
    }
}

fn increment_horizontal_pos(registers: &mut InternalRegisters) {
    // Increment coarse X
    let coarse_x = registers.coarse_x();
    if coarse_x == 0x001F {
        // Clear coarse X
        registers.vram_address &= !0x001F;

        // Wrap nametable horizontally
        registers.vram_address ^= 0x0400;
    } else {
        registers.vram_address += 1;
    }
}

fn increment_vertical_pos(registers: &mut InternalRegisters) {
    let fine_y = registers.fine_y();
    if fine_y < 7 {
        // Increment fine Y
        registers.vram_address = ((fine_y + 1) << 12) | (registers.vram_address & 0x0FFF);
    } else {
        let coarse_y = registers.coarse_y();
        if coarse_y == 29 {
            // Clear fine Y and coarse Y
            registers.vram_address &= 0x0C1F;

            // Wrap nametable vertically
            registers.vram_address ^= 0x0800;
        } else if coarse_y == 31 {
            // Clear fine Y and coarse Y, don't wrap nametable
            registers.vram_address &= 0x0C1F;
        } else {
            // Clear fine Y and increment coarse Y
            registers.vram_address = (registers.vram_address & 0x0C1F) | ((coarse_y + 1) << 5);
        }
    }
}

fn reset_horizontal_pos(registers: &mut InternalRegisters) {
    // Copy coarse X and nametable horizontal bit from t to v
    registers.vram_address =
        (registers.vram_address & 0xFBE0) | (registers.temp_vram_address & 0x041F);
}

fn reset_vertical_pos(registers: &mut InternalRegisters) {
    // Copy fine Y, coarse Y, and nametable vertical bit from t to v
    registers.vram_address =
        (registers.vram_address & 0x041F) | (registers.temp_vram_address & 0xFBE0);
}

fn render_pixel(state: &mut PpuState, bus: &mut PpuBus<'_>) {
    let pixel = (state.dot - 1) as u8;

    let tile_cycle_offset = pixel & 0x07;
    if state.dot > 1 && tile_cycle_offset == 0 {
        // Reload the BG buffers on cycles 9, 17, 25, ...
        state.bg_buffers.reload();
    }

    let ppu_registers = bus.get_ppu_registers();
    let bg_enabled = ppu_registers.bg_enabled();
    let sprites_enabled = ppu_registers.sprites_enabled();
    let left_edge_bg_enabled = ppu_registers.left_edge_bg_enabled();
    let left_edge_sprites_enabled = ppu_registers.left_edge_sprites_enabled();

    // Get next BG pixel color ID
    let bg_color_id = if bg_enabled && (pixel >= 8 || left_edge_bg_enabled) {
        get_bg_color_id(
            state.bg_buffers.pattern_table_low,
            state.bg_buffers.pattern_table_high,
            state.registers.fine_x(),
        )
    } else {
        0
    };
    let bg_palette_index = state.bg_buffers.get_palette_index(state.registers.fine_x());
    state.bg_buffers.shift();

    // Find the first overlapping sprite by OAM index, if any; use transparent if none found
    let sprite = if state.scanline != 0
        && state.scanline != state.timing_mode.pre_render_scanline()
        && sprites_enabled
        && (pixel >= 8 || left_edge_sprites_enabled)
    {
        state.sprite_line_buffer[pixel as usize]
    } else {
        SpriteData::NONE
    };

    if sprite.is_sprite_0 && bg_color_id != 0 && sprite.color_id != 0 && pixel < 255 {
        // Set sprite 0 hit when a non-transparent sprite pixel overlaps a non-transparent BG pixel
        // at x < 255.
        // Set the actual flag in PPUSTATUS on a 1-PPU-cycle delay to avoid some CPU/PPU alignment
        // issues.
        state.pending_sprite_0_hit = true;
    }

    let sprite_bg_priority = sprite.attributes.bit(5);
    let sprite_palette_index = sprite.attributes & 0x03;

    // Determine whether to show BG pixel color, sprite pixel color, or backdrop color
    let palette_ram = bus.get_palette_ram();
    let backdrop_color = palette_ram[0];
    let pixel_color = if sprite.color_id != 0 && (bg_color_id == 0 || !sprite_bg_priority) {
        let palette_addr = 0x10 | (sprite_palette_index << 2) | sprite.color_id;
        palette_ram[palette_addr as usize]
    } else if bg_color_id != 0 {
        let palette_addr = (bg_palette_index << 2) | bg_color_id;
        palette_ram[palette_addr as usize]
    } else {
        backdrop_color
    };

    let pixel_color = pixel_color & get_color_mask(bus.get_ppu_registers());
    let color_emphasis = ColorEmphasis::get_current(bus, state.timing_mode);

    // Render the pixel to the frame buffer
    state.set_in_frame_buffer(state.scanline, pixel.into(), pixel_color, color_emphasis, bus);
}

fn fetch_bg_tile_data(state: &mut PpuState, bus: &mut PpuBus<'_>) {
    debug_assert!(
        RENDERING_DOTS.contains(&state.dot) || BG_TILE_PRE_FETCH_DOTS.contains(&state.dot)
    );

    let tile_cycle_offset = (state.dot - 1) & 0x07;
    let bg_pattern_table_address = bus.get_ppu_registers().bg_pattern_table_address();

    // These offsets are not cycle accurate, but for some reason these timings cause the MMC3 IRQ
    // tests to pass
    match tile_cycle_offset {
        0 => {
            state.bg_buffers.next_nametable_byte = fetch_nametable_byte(&state.registers, bus);
        }
        1 => {
            let next_palette_index = u16::from(fetch_palette_index(&state.registers, bus));
            state.bg_buffers.next_palette_indices =
                (0..8).map(|i| next_palette_index << (2 * i)).reduce(|a, b| a | b).unwrap();
        }
        2 => {
            state.bg_buffers.next_pattern_table_low = fetch_bg_pattern_table_byte(
                bg_pattern_table_address,
                state.bg_buffers.next_nametable_byte,
                state.registers.fine_y(),
                PatternTableByte::Low,
                bus,
            );
        }
        4 => {
            state.bg_buffers.next_pattern_table_high = fetch_bg_pattern_table_byte(
                bg_pattern_table_address,
                state.bg_buffers.next_nametable_byte,
                state.registers.fine_y(),
                PatternTableByte::High,
                bus,
            );
        }
        _ => {}
    }
}

fn fetch_sprite_tile_data(
    bus: &mut PpuBus<'_>,
    scanline: u16,
    dot: u16,
    sprite_buffers: &mut SpriteBuffers,
) {
    debug_assert!(dot >= FIRST_SPRITE_TILE_FETCH_DOT);

    let sprite_pattern_table_address = bus.get_ppu_registers().sprite_pattern_table_address();
    let double_height_sprites = bus.get_ppu_registers().double_height_sprites();

    // 8 cycles per sprite
    let sprite_index = sprite_fetch_index(dot);

    let SpriteBufferData { y_position, attributes, tile_index, .. } =
        sprite_buffers.sprites[sprite_index as usize];

    // This is not completely accurate but it's close enough
    // In reality, during cycles 1-4 the value will be Y position, tile index, attributes, and X position in that order
    // During cycles 5-8 it will stay X position
    // Once past the end of the sprite buffer, the value will be sprite 63's Y position once, then $FF for the rest of this period
    if sprite_index < sprite_buffers.buffer_len {
        bus.get_ppu_registers_mut()
            .set_oam_open_bus(Some(sprite_buffers.sprites[sprite_index as usize].x_position));
    } else {
        bus.get_ppu_registers_mut().set_oam_open_bus(Some(0xFF));
    }

    // These offsets are not cycle accurate, but for some reason these timings cause the MMC3 IRQ
    // tests to pass
    let tile_cycle_offset = (dot - 1) & 0x07;
    match tile_cycle_offset {
        0 | 1 => {
            // Spurious nametable fetch
            // Address doesn't matter, but needs to vary based on sprite to avoid triggering
            // MMC5 scanline counter
            bus.read_address(0x2000 + u16::from(sprite_index) + 1);
        }
        2 => {
            if sprite_index < sprite_buffers.buffer_len {
                let pattern_table_low = fetch_sprite_pattern_table_byte(
                    sprite_pattern_table_address,
                    double_height_sprites,
                    y_position,
                    attributes,
                    tile_index,
                    scanline as u8,
                    PatternTableByte::Low,
                    bus,
                );
                sprite_buffers.pattern_table_low[sprite_index as usize] = pattern_table_low;
            } else {
                // Spurious read
                fetch_sprite_pattern_table_byte(
                    sprite_pattern_table_address,
                    double_height_sprites,
                    0xFF,
                    0xFF,
                    0xFF,
                    0xFF,
                    PatternTableByte::Low,
                    bus,
                );
            }
        }
        4 => {
            if sprite_index < sprite_buffers.buffer_len {
                let pattern_table_high = fetch_sprite_pattern_table_byte(
                    sprite_pattern_table_address,
                    double_height_sprites,
                    y_position,
                    attributes,
                    tile_index,
                    scanline as u8,
                    PatternTableByte::High,
                    bus,
                );
                sprite_buffers.pattern_table_high[sprite_index as usize] = pattern_table_high;
            } else {
                // Spurious read
                fetch_sprite_pattern_table_byte(
                    sprite_pattern_table_address,
                    double_height_sprites,
                    0xFF,
                    0xFF,
                    0xFF,
                    0xFF,
                    PatternTableByte::High,
                    bus,
                );
            }
        }
        _ => {}
    }
}

fn sprite_fetch_index(dot: u16) -> u8 {
    ((dot - FIRST_SPRITE_TILE_FETCH_DOT) >> 3) as u8
}

fn finish_sprite_tile_fetches(
    bus: &mut PpuBus<'_>,
    scanline: u16,
    dot: u16,
    sprite_buffers: &mut SpriteBuffers,
) {
    let mut dot = dot + 1;

    while sprite_fetch_index(dot) < sprite_buffers.buffer_len {
        fetch_sprite_tile_data(bus, scanline, dot, sprite_buffers);
        dot += 1;
    }
}

const SPRITE_PER_SCANLINE_LIMIT: u8 = 8;

fn evaluate_sprites(state: &mut PpuState, bus: &mut PpuBus<'_>, remove_sprite_limit: bool) {
    let sprite_height = if bus.get_ppu_registers().double_height_sprites() { 16 } else { 8 };

    let evaluation_data = &mut state.sprite_evaluation_data;
    let oam = bus.get_oam();
    evaluation_data.state = match evaluation_data.state {
        SpriteEvaluationState::ScanningOam { primary_oam_index } => {
            debug_assert!(
                primary_oam_index < 64
                    && (remove_sprite_limit
                        || evaluation_data.sprites_found < SPRITE_PER_SCANLINE_LIMIT)
            );

            let y_position = oam[(primary_oam_index << 2) as usize];

            bus.get_ppu_registers_mut().set_oam_open_bus(Some(y_position));

            evaluation_data.secondary_oam[(evaluation_data.sprites_found << 2) as usize] =
                y_position;

            if (y_position..y_position.saturating_add(sprite_height))
                .contains(&(state.scanline as u8))
            {
                if primary_oam_index == 0 {
                    evaluation_data.sprite_0_found = true;
                }

                SpriteEvaluationState::CopyingOam { primary_oam_index, byte_index: 1 }
            } else if primary_oam_index < 63 {
                SpriteEvaluationState::ScanningOam { primary_oam_index: primary_oam_index + 1 }
            } else {
                SpriteEvaluationState::Done { oam_index: primary_oam_index }
            }
        }
        SpriteEvaluationState::CopyingOam { primary_oam_index, byte_index } => {
            debug_assert!(primary_oam_index < 64 && byte_index < 4);

            let next_byte = oam[((primary_oam_index << 2) | byte_index) as usize];
            evaluation_data.secondary_oam
                [((evaluation_data.sprites_found << 2) | byte_index) as usize] = next_byte;

            bus.get_ppu_registers_mut().set_oam_open_bus(Some(next_byte));

            if byte_index < 3 {
                SpriteEvaluationState::CopyingOam { primary_oam_index, byte_index: byte_index + 1 }
            } else {
                evaluation_data.sprites_found += 1;

                let next_oam_index = primary_oam_index + 1;
                if next_oam_index == 64 {
                    SpriteEvaluationState::Done { oam_index: primary_oam_index }
                } else if !remove_sprite_limit
                    && evaluation_data.sprites_found == SPRITE_PER_SCANLINE_LIMIT
                {
                    SpriteEvaluationState::CheckingForOverflow {
                        oam_index: next_oam_index,
                        oam_offset: 0,
                        skip_bytes_remaining: 0,
                    }
                } else {
                    // This isn't completely accurate because of the buggy nature of how the
                    // PPU checks for sprite overflow on actual hardware, but it seems to work
                    // well enough to not break games that depend on this flag
                    if remove_sprite_limit
                        && evaluation_data.sprites_found == SPRITE_PER_SCANLINE_LIMIT + 1
                    {
                        bus.get_ppu_registers_mut().set_sprite_overflow(true);
                    }

                    SpriteEvaluationState::ScanningOam { primary_oam_index: next_oam_index }
                }
            }
        }
        SpriteEvaluationState::CheckingForOverflow {
            oam_index,
            oam_offset,
            skip_bytes_remaining,
        } => {
            if skip_bytes_remaining > 0 {
                let dummy_read = oam[((oam_index << 2) | oam_offset) as usize];
                bus.get_ppu_registers_mut().set_oam_open_bus(Some(dummy_read));

                SpriteEvaluationState::CheckingForOverflow {
                    oam_index,
                    oam_offset: (oam_offset + 1) & 0x03,
                    skip_bytes_remaining: skip_bytes_remaining - 1,
                }
            } else {
                let y_position = oam[((oam_index << 2) | oam_offset) as usize];

                bus.get_ppu_registers_mut().set_oam_open_bus(Some(y_position));

                if (y_position..y_position.saturating_add(sprite_height))
                    .contains(&(state.scanline as u8))
                {
                    bus.get_ppu_registers_mut().set_sprite_overflow(true);

                    SpriteEvaluationState::Done { oam_index }
                } else if oam_index < 63 {
                    // Yes, increment both index and offset; this is replicating a hardware bug that
                    // makes the sprite overflow flag essentially useless
                    SpriteEvaluationState::CheckingForOverflow {
                        oam_index: oam_index + 1,
                        oam_offset: (oam_offset + 1) & 0x03,
                        skip_bytes_remaining: 0,
                    }
                } else {
                    SpriteEvaluationState::Done { oam_index }
                }
            }
        }
        SpriteEvaluationState::Done { oam_index } => {
            let dummy_read = oam[(oam_index << 2) as usize];
            bus.get_ppu_registers_mut().set_oam_open_bus(Some(dummy_read));

            SpriteEvaluationState::Done { oam_index }
        }
    };
}

fn finish_sprite_evaluation_no_limit(state: &mut PpuState, bus: &mut PpuBus<'_>) {
    while !matches!(&state.sprite_evaluation_data.state, SpriteEvaluationState::Done { .. }) {
        evaluate_sprites(state, bus, true);
    }
}

fn fill_sprite_line_buffer(sprite_buffers: &SpriteBuffers, line_buffer: &mut SpriteLineBuffer) {
    line_buffer.fill(SpriteData::NONE);

    for i in 0..sprite_buffers.buffer_len {
        let SpriteBufferData { x_position: x_pos, attributes, .. } =
            sprite_buffers.sprites[i as usize];

        let sprite_flip_x = attributes.bit(6);
        let is_sprite_0 = i == 0 && sprite_buffers.sprite_0_buffered;

        for x in x_pos..=x_pos.saturating_add(7) {
            if line_buffer[x as usize].color_id != 0 {
                // There is already a non-transparent sprite pixel in this position with a lower OAM index
                continue;
            }

            // Determine sprite pixel color ID
            let sprite_fine_x = if sprite_flip_x { 7 - (x - x_pos) } else { x - x_pos };
            let color_id = get_color_id(
                sprite_buffers.pattern_table_low[i as usize],
                sprite_buffers.pattern_table_high[i as usize],
                sprite_fine_x,
            );

            if color_id == 0 {
                // Sprite pixel is transparent
                continue;
            }

            line_buffer[x as usize] = SpriteData { color_id, is_sprite_0, attributes };
        }
    }
}

fn fetch_nametable_byte(registers: &InternalRegisters, bus: &mut PpuBus<'_>) -> u8 {
    bus.read_address(0x2000 | (registers.vram_address & 0x0FFF))
}

fn fetch_palette_index(registers: &InternalRegisters, bus: &mut PpuBus<'_>) -> u8 {
    let coarse_y = registers.coarse_y();
    let coarse_x = registers.coarse_x();
    let nametable_bits = registers.nametable_bits();
    let attributes_byte =
        bus.read_address(0x23C0 | nametable_bits | ((coarse_y & 0x001C) << 1) | (coarse_x >> 2));

    match (coarse_x & 0x0002, coarse_y & 0x0002) {
        (0x0000, 0x0000) => attributes_byte & 0x03,
        (0x0002, 0x0000) => (attributes_byte >> 2) & 0x03,
        (0x0000, 0x0002) => (attributes_byte >> 4) & 0x03,
        (0x0002, 0x0002) => (attributes_byte >> 6) & 0x03,
        _ => unreachable!("masking with 0x0002 should always produce either 0 or 0x0002"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PatternTableByte {
    Low,
    High,
}

fn fetch_bg_pattern_table_byte(
    bg_pattern_table_address: u16,
    nametable_byte: u8,
    fine_y_scroll: u16,
    byte: PatternTableByte,
    bus: &mut PpuBus<'_>,
) -> u8 {
    let offset = match byte {
        PatternTableByte::Low => 0x0000,
        PatternTableByte::High => 0x0008,
    };

    bus.read_address(
        bg_pattern_table_address | (u16::from(nametable_byte) << 4) | offset | fine_y_scroll,
    )
}

#[allow(clippy::too_many_arguments)]
fn fetch_sprite_pattern_table_byte(
    sprite_pattern_table_address: u16,
    double_height_sprites: bool,
    y_position: u8,
    attributes: u8,
    tile_index: u8,
    scanline: u8,
    byte: PatternTableByte,
    bus: &mut PpuBus<'_>,
) -> u8 {
    let offset = match byte {
        PatternTableByte::Low => 0x0000,
        PatternTableByte::High => 0x0008,
    };

    let flip_y = attributes.bit(7);
    let (sprite_pattern_table_address, tile_index, fine_y_scroll) = if double_height_sprites {
        let sprite_pattern_table_address = u16::from(tile_index & 0x01) << 12;
        let fine_y_scroll = if flip_y {
            15 - scanline.saturating_sub(y_position)
        } else {
            scanline.saturating_sub(y_position)
        };
        let tile_index = (tile_index & 0xFE) | u8::from(fine_y_scroll >= 8);
        (sprite_pattern_table_address, tile_index, fine_y_scroll & 0x07)
    } else {
        let fine_y_scroll = if flip_y {
            7 - scanline.saturating_sub(y_position)
        } else {
            scanline.saturating_sub(y_position)
        };
        (sprite_pattern_table_address, tile_index, fine_y_scroll)
    };

    bus.read_address(
        sprite_pattern_table_address
            | (u16::from(tile_index) << 4)
            | offset
            | u16::from(fine_y_scroll),
    )
}

fn get_bg_color_id(pattern_table_low: u16, pattern_table_high: u16, fine_x: u8) -> u8 {
    get_color_id((pattern_table_low >> 8) as u8, (pattern_table_high >> 8) as u8, fine_x)
}

fn get_color_id(pattern_table_low: u8, pattern_table_high: u8, fine_x: u8) -> u8 {
    debug_assert!(fine_x < 8, "fine_x must be less than 8: {fine_x}");

    let shift = 7 - fine_x;
    let mask = 1 << shift;
    ((pattern_table_low & mask) >> shift) | (((pattern_table_high & mask) >> shift) << 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_id() {
        assert_eq!(0, get_color_id(0, 0, 0));

        assert_eq!(1, get_color_id(0x80, 0, 0));
        assert_eq!(2, get_color_id(0, 0x80, 0));
        assert_eq!(3, get_color_id(0x80, 0x80, 0));

        assert_eq!(0, get_color_id(0x80, 0x80, 1));

        assert_eq!(3, get_color_id(0x10, 0x10, 3));

        assert_eq!(3, get_color_id(0x01, 0x01, 7));
    }
}
