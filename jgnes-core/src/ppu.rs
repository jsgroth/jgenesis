use crate::bus::{PpuBus, PpuTrackedRegister, PpuWriteToggle};
use std::ops::RangeInclusive;

pub const SCREEN_WIDTH: u16 = 256;
pub const SCREEN_HEIGHT: u16 = 240;
pub const VISIBLE_SCREEN_HEIGHT: u16 = 224;

const DOTS_PER_SCANLINE: u16 = 341;
// Set/reset flags on dot 2 instead of 1 to resolve some CPU/PPU alignment issues that affect NMI
// timing
const VBLANK_FLAG_SET_DOT: u16 = 2;
const RENDERING_DOTS: RangeInclusive<u16> = 1..=256;
const SPRITE_EVALUATION_DOTS: RangeInclusive<u16> = 65..=256;
const SPRITE_TILE_FETCH_DOTS: RangeInclusive<u16> = 257..=320;
const BG_TILE_PRE_FETCH_DOTS: RangeInclusive<u16> = 321..=336;
const RESET_VERTICAL_POS_DOTS: RangeInclusive<u16> = 280..=304;
const INC_VERTICAL_POS_DOT: u16 = 256;
const RESET_HORIZONTAL_POS_DOT: u16 = 257;
const FIRST_SPRITE_TILE_FETCH_DOT: u16 = 257;

const VISIBLE_SCANLINES: RangeInclusive<u16> = 0..=239;
const FIRST_VBLANK_SCANLINE: u16 = 241;
const VBLANK_SCANLINES: RangeInclusive<u16> = 241..=260;
const ALL_IDLE_SCANLINES: RangeInclusive<u16> = 240..=260;
const PRE_RENDER_SCANLINE: u16 = 261;

pub type FrameBuffer = [[u8; SCREEN_WIDTH as usize]; SCREEN_HEIGHT as usize];

#[derive(Debug, Clone)]
struct InternalRegisters {
    vram_address: u16,
    temp_vram_address: u16,
    fine_x_scroll: u8,
}

impl InternalRegisters {
    fn new() -> Self {
        Self {
            vram_address: 0,
            temp_vram_address: 0,
            fine_x_scroll: 0,
        }
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
struct SpriteBuffers {
    y_positions: [u8; 8],
    x_positions: [u8; 8],
    attributes: [u8; 8],
    tile_indices: [u8; 8],
    pattern_table_low: [u8; 8],
    pattern_table_high: [u8; 8],
    buffer_len: u8,
    sprite_0_buffered: bool,
}

impl SpriteBuffers {
    fn new() -> Self {
        Self {
            y_positions: [0xFF; 8],
            x_positions: [0xFF; 8],
            attributes: [0xFF; 8],
            tile_indices: [0xFF; 8],
            pattern_table_low: [0; 8],
            pattern_table_high: [0; 8],
            buffer_len: 0,
            sprite_0_buffered: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpriteEvaluationState {
    ScanningOam {
        primary_oam_index: u8,
    },
    CopyingOam {
        primary_oam_index: u8,
        byte_index: u8,
    },
    CheckingForOverflow {
        oam_index: u8,
        oam_offset: u8,
        skip_bytes_remaining: u8,
    },
    Done,
}

#[derive(Debug, Clone)]
struct SpriteEvaluationData {
    secondary_oam: [u8; 32],
    sprites_found: u8,
    sprite_0_found: bool,
    state: SpriteEvaluationState,
}

impl SpriteEvaluationData {
    fn new() -> Self {
        Self {
            secondary_oam: [0xFF; 32],
            sprites_found: 0,
            sprite_0_found: false,
            state: SpriteEvaluationState::ScanningOam {
                primary_oam_index: 0,
            },
        }
    }

    fn to_sprite_buffers(&self) -> SpriteBuffers {
        let mut y_positions = [0xFF; 8];
        let mut x_positions = [0xFF; 8];
        let mut attributes = [0xFF; 8];
        let mut tile_indices = [0xFF; 8];

        for (i, chunk) in self
            .secondary_oam
            .chunks_exact(4)
            .take(self.sprites_found as usize)
            .enumerate()
        {
            let &[y_position, tile_index, attributes_byte, x_position] = chunk
            else {
                unreachable!("all chunks from chunks_exact(4) should be of size 4")
            };

            y_positions[i] = y_position;
            x_positions[i] = x_position;
            attributes[i] = attributes_byte;
            tile_indices[i] = tile_index;
        }

        SpriteBuffers {
            y_positions,
            x_positions,
            attributes,
            tile_indices,
            pattern_table_low: [0; 8],
            pattern_table_high: [0; 8],
            buffer_len: self.sprites_found,
            sprite_0_buffered: self.sprite_0_found,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct SpriteData {
    color_id: u8,
    is_sprite_0: bool,
    attributes: u8,
}

#[derive(Debug, Clone)]
pub struct PpuState {
    frame_buffer: FrameBuffer,
    registers: InternalRegisters,
    bg_buffers: BgBuffers,
    sprite_buffers: SpriteBuffers,
    sprite_evaluation_data: SpriteEvaluationData,
    scanline: u16,
    dot: u16,
    odd_frame: bool,
    rendering_disabled_backdrop_color: Option<u8>,
    pending_sprite_0_hit: bool,
}

impl PpuState {
    pub fn new() -> Self {
        Self {
            frame_buffer: [[0; SCREEN_WIDTH as usize]; SCREEN_HEIGHT as usize],
            registers: InternalRegisters::new(),
            bg_buffers: BgBuffers::new(),
            sprite_buffers: SpriteBuffers::new(),
            sprite_evaluation_data: SpriteEvaluationData::new(),
            scanline: PRE_RENDER_SCANLINE,
            dot: 0,
            odd_frame: false,
            // 0x0F == Black
            rendering_disabled_backdrop_color: Some(0x0F),
            pending_sprite_0_hit: false,
        }
    }

    pub fn in_vblank(&self) -> bool {
        VBLANK_SCANLINES.contains(&self.scanline)
    }

    pub fn frame_buffer(&self) -> &FrameBuffer {
        &self.frame_buffer
    }
}

pub fn tick(state: &mut PpuState, bus: &mut PpuBus<'_>) {
    let rendering_enabled =
        bus.get_ppu_registers().bg_enabled() || bus.get_ppu_registers().sprites_enabled();

    process_register_updates(state, bus, rendering_enabled);

    if state.scanline == PRE_RENDER_SCANLINE && state.dot == VBLANK_FLAG_SET_DOT {
        // Clear per-frame flags at the start of the pre-render scanline
        let ppu_registers = bus.get_ppu_registers_mut();
        ppu_registers.set_vblank_flag(false);
        ppu_registers.set_sprite_0_hit(false);
        ppu_registers.set_sprite_overflow(false);
    } else if state.scanline == FIRST_VBLANK_SCANLINE && state.dot == VBLANK_FLAG_SET_DOT {
        bus.get_ppu_registers_mut().set_vblank_flag(true);
    }

    if rendering_enabled {
        state.rendering_disabled_backdrop_color = None;
        process_scanline(state, bus);
    } else {
        // When rendering is disabled, pixels should use whatever the backdrop color was set to
        // at disable time until rendering is enabled again
        let backdrop_color = *state
            .rendering_disabled_backdrop_color
            .get_or_insert_with(|| {
                // "Background palette hack": If the current VRAM address is inside the palette RAM
                // address range when rendering is disabled, use the color at that address instead
                // of the standard backdrop color
                let palette_ram_addr = if (0x3F00..=0x3FFF).contains(&state.registers.vram_address)
                {
                    state.registers.vram_address & 0x001F
                } else {
                    0
                };
                bus.get_palette_ram()[palette_ram_addr as usize] & 0x3F
            });

        if VISIBLE_SCANLINES.contains(&state.scanline) && RENDERING_DOTS.contains(&state.dot) {
            state.frame_buffer[state.scanline as usize][(state.dot - 1) as usize] = backdrop_color;
        }
    }

    // Copy v register to where the CPU can see it
    if !rendering_enabled || ALL_IDLE_SCANLINES.contains(&state.scanline) {
        bus.set_bus_address(state.registers.vram_address & 0x3FFF);
    }

    state.dot += 1;
    if state.dot == DOTS_PER_SCANLINE {
        state.scanline += 1;
        state.dot = 0;

        if state.scanline == PRE_RENDER_SCANLINE + 1 {
            state.scanline = 0;

            if state.odd_frame && rendering_enabled {
                // Skip the idle cycle in the first visible scanline on odd frames
                state.dot = 1;
            }
            state.odd_frame = !state.odd_frame;
        }
    }
}

fn process_scanline(state: &mut PpuState, bus: &mut PpuBus<'_>) {
    log::trace!("Rendering at scanline {} dot {}", state.scanline, state.dot);

    if state.pending_sprite_0_hit {
        // If sprite 0 hit triggered on the last cycle, set the flag in PPUSTATUS
        state.pending_sprite_0_hit = false;
        bus.get_ppu_registers_mut().set_sprite_0_hit(true);
    }

    match state.scanline {
        0..=239 | 261 => {
            if state.scanline == PRE_RENDER_SCANLINE && RESET_VERTICAL_POS_DOTS.contains(&state.dot)
            {
                // Repeatedly reset vertical position during the pre-render scanline
                reset_vertical_pos(&mut state.registers);
            }

            if state.scanline != PRE_RENDER_SCANLINE && state.dot == 1 {
                // Clear sprite evaluation data at the beginning of each visible scanline
                state.sprite_evaluation_data = SpriteEvaluationData::new();
            }

            // Render scanlines
            #[allow(clippy::match_same_arms)]
            match state.dot {
                0 => {
                    // Idle cycle
                }
                1..=256 => {
                    // Rendering + sprite evaluation cycles

                    if state.scanline != PRE_RENDER_SCANLINE {
                        render_pixel(state, bus);
                    }

                    if state.dot > 1 && (state.dot - 1).trailing_zeros() >= 3 {
                        // Increment horizontal position on cycles 9, 17, 25, ..
                        // before fetching BG tile data
                        increment_horizontal_pos(&mut state.registers);
                    }

                    // Start fetching data for the next tile cycle if appropriate
                    fetch_bg_tile_data(state, bus);

                    // Evaluate sprites on odd cycles during 65-256
                    if state.scanline != PRE_RENDER_SCANLINE
                        && SPRITE_EVALUATION_DOTS.contains(&state.dot)
                        && state.dot & 0x01 != 0
                    {
                        evaluate_sprites(state, bus);
                    }

                    if state.scanline != PRE_RENDER_SCANLINE && state.dot == INC_VERTICAL_POS_DOT {
                        // Increment effective vertical position at the end of the rendering phase
                        increment_vertical_pos(&mut state.registers);
                    }

                    // TODO figure out properly why aligning the sprite data fetches this way fixes
                    // the MMC3 IRQ tests
                    if state.dot == 255 {
                        // Spurious nametable read
                        bus.read_address(0x2000);
                    }
                }
                257..=320 => {
                    // Cycles for fetching sprite data for the next scanline

                    if state.dot == RESET_HORIZONTAL_POS_DOT {
                        // Reset horizontal position immediately after the rendering phase
                        reset_horizontal_pos(&mut state.registers);

                        // Fill sprite buffers with sprite data for the next scanline
                        state.sprite_buffers = state.sprite_evaluation_data.to_sprite_buffers();
                    }

                    fetch_sprite_tile_data(state, bus);
                }
                321..=336 => {
                    // Cycles for fetching BG tile data for the first 2 tiles of the next scanline

                    fetch_bg_tile_data(state, bus);
                    state.bg_buffers.shift();

                    if state.dot.trailing_zeros() >= 3 {
                        // Increment horizontal position and reload buffers at the end of each tile
                        // (dots 328 and 336)
                        increment_horizontal_pos(&mut state.registers);
                        state.bg_buffers.reload();
                    }
                }
                337 | 339 => {
                    // Idle cycles that do spurious reads
                    // At least one mapper depends on these reads happening (MMC5)
                    fetch_nametable_byte(&state.registers, bus);
                }
                338 | 340 => {
                    // Truly idle cycles at the end of each scanline
                }
                _ => panic!("invalid dot: {}", state.dot),
            }
        }
        240..=260 => {
            // PPU idle scanlines
        }
        _ => panic!("invalid scanline: {}", state.scanline),
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
            let value = bus.get_ppu_registers().get_open_bus_value();
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
            let value = bus.get_ppu_registers().get_open_bus_value();
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
                    || state.scanline == PRE_RENDER_SCANLINE)
            {
                // Accessing PPUDATA during rendering causes a coarse X increment + Y increment
                log::trace!(
                    "PPU: PPUDATA was accessed during rendering (scanline {} / dot {}), incrementing coarse X and Y in v register",
                    state.scanline, state.dot
                );

                increment_horizontal_pos(&mut state.registers);
                increment_vertical_pos(&mut state.registers);
            } else {
                log::trace!(
                    "PPU: PPUDATA was accessed on scanline {} / dot {}, incrementing internal v register by {}",
                    state.scanline, state.dot, bus.get_ppu_registers().ppu_data_addr_increment()
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
    let bg_fine_x = state.bg_buffers.get_palette_index(state.registers.fine_x());
    state.bg_buffers.shift();

    // Find the first overlapping sprite by OAM index, if any; use transparent if none found
    let sprite = (state.scanline != 0
        && state.scanline != PRE_RENDER_SCANLINE
        && sprites_enabled
        && (pixel >= 8 || left_edge_sprites_enabled))
        .then(|| find_first_overlapping_sprite(pixel, &state.sprite_buffers))
        .flatten()
        .unwrap_or(SpriteData {
            color_id: 0,
            is_sprite_0: false,
            attributes: 0x00,
        });

    if sprite.is_sprite_0 && bg_color_id != 0 && sprite.color_id != 0 && pixel < 255 {
        // Set sprite 0 hit when a non-transparent sprite pixel overlaps a non-transparent BG pixel
        // at x < 255.
        // Set the actual flag in PPUSTATUS on a 1-PPU-cycle delay to avoid some CPU/PPU alignment
        // issues.
        state.pending_sprite_0_hit = true;
    }

    let sprite_bg_priority = sprite.attributes & 0x20 != 0;
    let sprite_palette_index = sprite.attributes & 0x03;

    // Determine whether to show BG pixel color, sprite pixel color, or backdrop color
    let palette_ram = bus.get_palette_ram();
    let backdrop_color = palette_ram[0];
    let pixel_color = if sprite.color_id != 0 && (bg_color_id == 0 || !sprite_bg_priority) {
        let palette_addr = 0x10 | (sprite_palette_index << 2) | sprite.color_id;
        palette_ram[palette_addr as usize]
    } else if bg_color_id != 0 {
        let palette_addr = (bg_fine_x << 2) | bg_color_id;
        palette_ram[palette_addr as usize]
    } else {
        backdrop_color
    };

    // Discard the highest two bits, colors range from 0 to 63
    let pixel_color = pixel_color & 0x3F;

    // Render the pixel to the frame buffer
    state.frame_buffer[state.scanline as usize][pixel as usize] = pixel_color;
}

fn fetch_bg_tile_data(state: &mut PpuState, bus: &mut PpuBus<'_>) {
    assert!(RENDERING_DOTS.contains(&state.dot) || BG_TILE_PRE_FETCH_DOTS.contains(&state.dot));

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
            state.bg_buffers.next_palette_indices = (0..8)
                .map(|i| next_palette_index << (2 * i))
                .fold(0, |a, b| a | b);
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

fn fetch_sprite_tile_data(state: &mut PpuState, bus: &mut PpuBus<'_>) {
    assert!(SPRITE_TILE_FETCH_DOTS.contains(&state.dot));

    let sprite_pattern_table_address = bus.get_ppu_registers().sprite_pattern_table_address();
    let double_height_sprites = bus.get_ppu_registers().double_height_sprites();

    // 8 cycles per sprite
    let sprite_index = ((state.dot - FIRST_SPRITE_TILE_FETCH_DOT) >> 3) as u8;

    let y_position = state.sprite_buffers.y_positions[sprite_index as usize];
    let tile_index = state.sprite_buffers.tile_indices[sprite_index as usize];
    let attributes = state.sprite_buffers.attributes[sprite_index as usize];

    // TODO figure out properly why aligning the dots to accesses this way fixes the MMC3 IRQ tests
    let tile_cycle_offset = (state.dot - 1) & 0x07;
    match tile_cycle_offset {
        0 | 6 => {
            if state.dot < 319 {
                // Spurious nametable fetch
                // Address doesn't matter, but needs to vary based on sprite to avoid triggering
                // MMC5 scanline counter
                bus.read_address(0x2000 + u16::from(sprite_index) + 1);
            }
        }
        2 => {
            if sprite_index < state.sprite_buffers.buffer_len {
                let pattern_table_low = fetch_sprite_pattern_table_byte(
                    sprite_pattern_table_address,
                    double_height_sprites,
                    y_position,
                    attributes,
                    tile_index,
                    state.scanline as u8,
                    PatternTableByte::Low,
                    bus,
                );
                state.sprite_buffers.pattern_table_low[sprite_index as usize] = pattern_table_low;
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
            if sprite_index < state.sprite_buffers.buffer_len {
                let pattern_table_high = fetch_sprite_pattern_table_byte(
                    sprite_pattern_table_address,
                    double_height_sprites,
                    y_position,
                    attributes,
                    tile_index,
                    state.scanline as u8,
                    PatternTableByte::High,
                    bus,
                );
                state.sprite_buffers.pattern_table_high[sprite_index as usize] = pattern_table_high;
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

fn evaluate_sprites(state: &mut PpuState, bus: &mut PpuBus<'_>) {
    let sprite_height = if bus.get_ppu_registers().double_height_sprites() {
        16
    } else {
        8
    };

    let evaluation_data = &mut state.sprite_evaluation_data;
    let oam = bus.get_oam();
    evaluation_data.state = match evaluation_data.state {
        SpriteEvaluationState::ScanningOam { primary_oam_index } => {
            assert!(primary_oam_index < 64 && evaluation_data.sprites_found < 8);

            let y_position = oam[(primary_oam_index << 2) as usize];

            evaluation_data.secondary_oam[(evaluation_data.sprites_found << 2) as usize] =
                y_position;

            if (y_position..y_position.saturating_add(sprite_height))
                .contains(&(state.scanline as u8))
            {
                if primary_oam_index == 0 {
                    evaluation_data.sprite_0_found = true;
                }

                SpriteEvaluationState::CopyingOam {
                    primary_oam_index,
                    byte_index: 1,
                }
            } else if primary_oam_index < 63 {
                SpriteEvaluationState::ScanningOam {
                    primary_oam_index: primary_oam_index + 1,
                }
            } else {
                SpriteEvaluationState::Done
            }
        }
        SpriteEvaluationState::CopyingOam {
            primary_oam_index,
            byte_index,
        } => {
            assert!(primary_oam_index < 64 && byte_index < 4);

            evaluation_data.secondary_oam
                [((evaluation_data.sprites_found << 2) | byte_index) as usize] =
                oam[((primary_oam_index << 2) | byte_index) as usize];
            if byte_index < 3 {
                SpriteEvaluationState::CopyingOam {
                    primary_oam_index,
                    byte_index: byte_index + 1,
                }
            } else {
                evaluation_data.sprites_found += 1;

                let next_oam_index = primary_oam_index + 1;
                if next_oam_index == 64 {
                    SpriteEvaluationState::Done
                } else if evaluation_data.sprites_found == 8 {
                    SpriteEvaluationState::CheckingForOverflow {
                        oam_index: next_oam_index,
                        oam_offset: 0,
                        skip_bytes_remaining: 0,
                    }
                } else {
                    SpriteEvaluationState::ScanningOam {
                        primary_oam_index: next_oam_index,
                    }
                }
            }
        }
        SpriteEvaluationState::CheckingForOverflow {
            oam_index,
            oam_offset,
            skip_bytes_remaining,
        } => {
            if skip_bytes_remaining > 0 {
                SpriteEvaluationState::CheckingForOverflow {
                    oam_index,
                    oam_offset: (oam_offset + 1) & 0x03,
                    skip_bytes_remaining: skip_bytes_remaining - 1,
                }
            } else {
                let y_position = oam[((oam_index << 2) | oam_offset) as usize];
                if (y_position..y_position.saturating_add(sprite_height))
                    .contains(&(state.scanline as u8))
                {
                    bus.get_ppu_registers_mut().set_sprite_overflow(true);

                    SpriteEvaluationState::Done
                } else if oam_index < 63 {
                    // Yes, increment both index and offset; this is replicating a hardware bug that
                    // makes the sprite overflow flag essentially useless
                    SpriteEvaluationState::CheckingForOverflow {
                        oam_index: oam_index + 1,
                        oam_offset: (oam_offset + 1) & 0x03,
                        skip_bytes_remaining: 0,
                    }
                } else {
                    SpriteEvaluationState::Done
                }
            }
        }
        SpriteEvaluationState::Done => SpriteEvaluationState::Done,
    };
}

fn find_first_overlapping_sprite(pixel: u8, sprites: &SpriteBuffers) -> Option<SpriteData> {
    (0..sprites.buffer_len as usize).find_map(|i| {
        let x_pos = sprites.x_positions[i];

        if !(x_pos..=x_pos.saturating_add(7)).contains(&pixel) {
            return None;
        }

        let attributes = sprites.attributes[i];

        let sprite_flip_x = attributes & 0x40 != 0;

        // Determine sprite pixel color ID
        let sprite_fine_x = if sprite_flip_x {
            7 - (pixel - x_pos)
        } else {
            pixel - x_pos
        };
        let sprite_color_id = get_color_id(
            sprites.pattern_table_low[i],
            sprites.pattern_table_high[i],
            sprite_fine_x,
        );

        (sprite_color_id != 0).then_some(SpriteData {
            color_id: sprite_color_id,
            attributes,
            is_sprite_0: i == 0 && sprites.sprite_0_buffered,
        })
    })
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

    let flip_y = attributes & 0x80 != 0;
    let (sprite_pattern_table_address, tile_index, fine_y_scroll) = if double_height_sprites {
        let sprite_pattern_table_address = u16::from(tile_index & 0x01) << 12;
        let fine_y_scroll = if flip_y {
            15 - scanline.saturating_sub(y_position)
        } else {
            scanline.saturating_sub(y_position)
        };
        let tile_index = (tile_index & 0xFE) | u8::from(fine_y_scroll >= 8);
        (
            sprite_pattern_table_address,
            tile_index,
            fine_y_scroll & 0x07,
        )
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
    get_color_id(
        (pattern_table_low >> 8) as u8,
        (pattern_table_high >> 8) as u8,
        fine_x,
    )
}

fn get_color_id(pattern_table_low: u8, pattern_table_high: u8, fine_x: u8) -> u8 {
    assert!(fine_x < 8, "fine_x must be less than 8: {fine_x}");

    ((pattern_table_low & (1 << (7 - fine_x))) >> (7 - fine_x))
        | (((pattern_table_high & (1 << (7 - fine_x))) >> (7 - fine_x)) << 1)
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
