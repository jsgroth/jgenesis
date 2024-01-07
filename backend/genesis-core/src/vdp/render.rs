use crate::vdp::colors::ColorModifier;
use crate::vdp::registers::{
    DebugRegister, HorizontalDisplaySize, HorizontalScrollMode, InterlacingMode, Plane, Registers,
    ScrollSize, VerticalScrollMode,
};
use crate::vdp::{
    colors, CachedSpriteData, Cram, FrameBuffer, SpriteData, Vram, Vsram, MAX_SCREEN_WIDTH,
};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

// Sprites with X = $080 display at the left edge of the screen
const SPRITE_H_DISPLAY_START: u16 = 0x080;

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct SpriteState {
    overflow: bool,
    collision: bool,
    dot_overflow_on_prev_line: bool,
    pixels_disabled_during_hblank: u16,
    display_enabled: bool,
    display_enabled_pixel: u16,
}

impl SpriteState {
    pub fn overflow_flag(&self) -> bool {
        self.overflow
    }

    pub fn collision_flag(&self) -> bool {
        self.collision
    }

    pub fn clear_status_flags(&mut self) {
        self.overflow = false;
        self.collision = false;
    }

    pub fn handle_hblank_start(
        &mut self,
        h_display_size: HorizontalDisplaySize,
        display_enabled: bool,
    ) {
        self.pixels_disabled_during_hblank = 0;

        self.display_enabled = display_enabled;
        self.display_enabled_pixel = h_display_size.active_display_pixels();
    }

    pub fn handle_display_enabled_write(&mut self, display_enabled: bool, pixel: u16) {
        if pixel < self.display_enabled_pixel {
            // Pre-HBlank write on the next scanline; ignore
            return;
        }

        if !self.display_enabled {
            self.pixels_disabled_during_hblank += pixel - self.display_enabled_pixel;
        }

        self.display_enabled = display_enabled;
        self.display_enabled_pixel = pixel;
    }

    pub fn handle_line_end(&mut self, h_display_size: HorizontalDisplaySize) {
        if !self.display_enabled {
            self.pixels_disabled_during_hblank +=
                h_display_size.pixels_including_hblank() - self.display_enabled_pixel;
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct SpritePixel {
    palette: u8,
    color_id: u8,
    priority: bool,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SpriteBuffers {
    scanned_ids: Vec<u8>,
    sprites: Vec<SpriteData>,
    pixels: Box<[SpritePixel; MAX_SCREEN_WIDTH]>,
}

impl SpriteBuffers {
    pub fn new() -> Self {
        Self {
            scanned_ids: Vec::with_capacity(20),
            sprites: Vec::with_capacity(20),
            pixels: vec![SpritePixel::default(); MAX_SCREEN_WIDTH]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
        }
    }

    pub fn clear_all(&mut self) {
        self.scanned_ids.clear();
        self.sprites.clear();
        self.pixels.fill(SpritePixel::default());
    }
}

// Scan sprites (Phase 1 according to Overdrive 2 documentation).
//
// This should be called at the end of HBlank 2 scanlines before the line to be rendered, with sprite attributes latched
// from around when HINT is generated. Actual hardware does sprite scanning in parallel with sprite pixel fetching for
// the next scanline, but here we want to know if there were any pixels where display was disabled during HBlank.
pub fn scan_sprites(
    scanline: u16,
    registers: &Registers,
    cached_sprite_attributes: &[CachedSpriteData],
    buffers: &mut SpriteBuffers,
    state: &mut SpriteState,
    enforce_sprite_limits: bool,
) {
    buffers.scanned_ids.clear();

    let h_size = registers.horizontal_display_size;

    // If display was disabled during part of HBlank on the scanline before the previous scanline,
    // the number of sprites scanned for the current scanline is reduced roughly by the number of
    // pixels that display was disabled for.
    // Actual hardware doesn't work exactly this way (it depends on exactly which VRAM access slots
    // display was disabled during), but this approximation works well enough for Mickey Mania's
    // 3D stages and Titan Overdrive's "your emulator suxx" screen
    let sprites_not_scanned = if state.pixels_disabled_during_hblank != 0 {
        // Not sure exactly why, but adding ~8 here is necessary to fully remove the "your emulator
        // suxx" text from Titan Overdrive's 512-color screen
        state.pixels_disabled_during_hblank + 8
    } else {
        0
    };
    let max_sprites_to_scan = h_size.sprite_table_len().saturating_sub(sprites_not_scanned);

    let interlacing_mode = registers.interlacing_mode;
    let sprite_scanline = interlacing_mode.sprite_display_top() + scanline;
    let cell_height = interlacing_mode.cell_height();

    let max_sprites_per_line = h_size.max_sprites_per_line() as usize;

    // Sprite 0 is always populated
    let mut sprite_idx = 0_u16;
    for _ in 0..max_sprites_to_scan {
        let CachedSpriteData { v_position, v_size_cells, link_data, .. } =
            cached_sprite_attributes[sprite_idx as usize];

        // Check if sprite falls on this scanline
        let sprite_top = sprite_y_position(v_position, interlacing_mode);
        let sprite_bottom = sprite_top + cell_height * u16::from(v_size_cells);
        if (sprite_top..sprite_bottom).contains(&sprite_scanline) {
            // Check if sprite-per-scanline limit has been hit
            if buffers.scanned_ids.len() == max_sprites_per_line {
                state.overflow = true;
                if enforce_sprite_limits {
                    break;
                }
            }

            buffers.scanned_ids.push(sprite_idx as u8);
        }

        sprite_idx = link_data.into();
        if sprite_idx == 0 || sprite_idx >= h_size.sprite_table_len() {
            break;
        }
    }
}

// Fetch sprite attributes from VRAM (Phase 2 in the Overdrive 2 documentation), as well as re-fetch the cached Y
// position and sprite size fields. Uses the sprite IDs that were scanned during Phase 1.
//
// This should be called at the start of HBlank on the scanline before the sprites are to be displayed. On actual
// hardware, this occurs in parallel with rendering on the scanline before the sprites are to be displayed.
pub fn fetch_sprite_attributes(
    vram: &Vram,
    registers: &Registers,
    cached_sprite_attributes: &[CachedSpriteData],
    buffers: &mut SpriteBuffers,
) {
    buffers.sprites.clear();

    let sprite_table_addr = registers.masked_sprite_attribute_table_addr();

    for &sprite_idx in &buffers.scanned_ids {
        let sprite_addr = sprite_table_addr.wrapping_add(8 * u16::from(sprite_idx)) as usize;
        let sprite = SpriteData::create(
            cached_sprite_attributes[sprite_idx as usize],
            &vram[sprite_addr + 4..sprite_addr + 8],
        );
        buffers.sprites.push(sprite);
    }
}

// Fetch and render sprite pixels into the line buffer (Phase 3 in the Overdrive 2 documentation). Uses the sprite
// attributes that were fetched from VRAM during Phase 2.
//
// Similar to Phase 1, in actual hardware this occurs throughout HBlank using latched registers. Here, it should be
// called at the end of HBlank so that we know how many pixels the display was disabled during HBlank.
fn render_sprite_pixels(
    scanline: u16,
    vram: &Vram,
    registers: &Registers,
    buffers: &mut SpriteBuffers,
    state: &mut SpriteState,
    enforce_sprite_limits: bool,
) {
    buffers.pixels.fill(SpritePixel::default());

    let h_size = registers.horizontal_display_size;
    let sprite_display_area =
        SPRITE_H_DISPLAY_START..SPRITE_H_DISPLAY_START + h_size.active_display_pixels();

    let half_tiles_not_fetched = if state.pixels_disabled_during_hblank != 0 {
        state.pixels_disabled_during_hblank + 8
    } else {
        0
    };

    let interlacing_mode = registers.interlacing_mode;
    let sprite_scanline = interlacing_mode.sprite_display_top() + scanline;
    let cell_height = interlacing_mode.cell_height();

    // Apply max sprite pixel per scanline limit.
    //
    // If display was disabled during HBlank on the previous scanline, the number of sprite pixels
    // rendered is reduced roughly proportional to the number of pixels during which display was
    // disabled.
    // As above, this is an approximation; in actual hardware it depends on which VRAM access slots
    // were skipped because display was disabled
    let max_sprite_pixels_per_line =
        h_size.max_sprite_pixels_per_line().saturating_sub(4 * half_tiles_not_fetched);

    let mut line_pixels = 0;
    let mut dot_overflow = false;

    // Sprites with H position 0 mask all lower priority sprites on the same scanline...with
    // some quirks. There must be at least one sprite with H != 0 before the H=0 sprite, unless
    // there was a sprite pixel overflow on the previous scanline.
    let mut found_non_zero = state.dot_overflow_on_prev_line;

    'outer: for sprite in &buffers.sprites {
        if sprite.h_position == 0 && found_non_zero {
            // Sprite masking from H=0 sprite; no more sprites will display on this line
            break;
        } else if sprite.h_position != 0 {
            found_non_zero = true;
        }

        let v_size_cells: u16 = sprite.v_size_cells.into();
        let h_size_cells: u16 = sprite.h_size_cells.into();

        // The lowest 5 bits of difference between sprite V position and scanline are considered, regardless of whether
        // the sprite overlaps the current scanline.
        //
        // Sprite V position is not necessarily in range of the current line because V position can change between the
        // sprite scan and tile fetching; Titan Overdrive 2's textured cube depends on handling this correctly
        let sprite_row = sprite_scanline
            .wrapping_sub(sprite_y_position(sprite.v_position, interlacing_mode))
            & 0x1F;
        let sprite_row = if sprite.vertical_flip {
            (cell_height * v_size_cells - 1).wrapping_sub(sprite_row) & 0x1F
        } else {
            sprite_row
        };

        let sprite_width = 8 * h_size_cells;
        let sprite_right = sprite.h_position + sprite_width;
        for h_position in sprite.h_position..sprite_right {
            if !sprite_display_area.contains(&h_position) {
                continue;
            }

            let sprite_col = h_position - sprite.h_position;
            let sprite_col =
                if sprite.horizontal_flip { 8 * h_size_cells - 1 - sprite_col } else { sprite_col };

            let pattern_offset = (sprite_col / 8) * v_size_cells + sprite_row / cell_height;
            let color_id = read_pattern_generator(
                vram,
                PatternGeneratorArgs {
                    vertical_flip: false,
                    horizontal_flip: false,
                    pattern_generator: sprite.pattern_generator.wrapping_add(pattern_offset),
                    row: sprite_row % cell_height,
                    col: sprite_col % 8,
                    cell_height,
                },
            );

            let pixel = h_position - SPRITE_H_DISPLAY_START;
            if buffers.pixels[pixel as usize].color_id == 0 {
                // Transparent pixels are always overwritten, even if the current pixel is also transparent
                buffers.pixels[pixel as usize] =
                    SpritePixel { palette: sprite.palette, color_id, priority: sprite.priority };
            } else {
                // Sprite collision; two non-transparent sprite pixels in the same position
                state.collision = true;
            }

            line_pixels += 1;
            if line_pixels == max_sprite_pixels_per_line {
                // Hit sprite pixel per scanline limit
                state.collision = true;
                dot_overflow = true;
                if enforce_sprite_limits {
                    break 'outer;
                }
            }
        }
    }

    state.dot_overflow_on_prev_line = dot_overflow;
}

fn sprite_y_position(v_position: u16, interlacing_mode: InterlacingMode) -> u16 {
    // V position is 9 bits in progressive mode and interlaced mode 1, and 10 bits in
    // interlaced mode 2
    match interlacing_mode {
        InterlacingMode::Progressive | InterlacingMode::Interlaced => v_position & 0x1FF,
        InterlacingMode::InterlacedDouble => v_position & 0x3FF,
    }
}

pub struct RenderingArgs<'a> {
    pub frame_buffer: &'a mut FrameBuffer,
    pub sprite_buffers: &'a mut SpriteBuffers,
    pub sprite_state: &'a mut SpriteState,
    pub vram: &'a Vram,
    pub cram: &'a Cram,
    pub vsram: &'a Vsram,
    pub registers: &'a Registers,
    pub debug_register: DebugRegister,
    pub full_screen_v_scroll_a: u16,
    pub full_screen_v_scroll_b: u16,
    pub enforce_sprite_limits: bool,
    pub emulate_non_linear_dac: bool,
}

pub fn render_scanline(mut args: RenderingArgs<'_>, scanline: u16, starting_pixel: u16) {
    if !args.registers.display_enabled {
        if scanline < args.registers.vertical_display_size.active_scanlines() {
            clear_scanline(&mut args, scanline, starting_pixel);

            // Clear sprite pixel buffer in case display is enabled during active display
            args.sprite_buffers.pixels.fill(SpritePixel::default());
        }

        return;
    }

    let bg_color = colors::resolve_color(
        args.cram,
        args.registers.background_palette,
        args.registers.background_color_id,
    );

    if starting_pixel == 0 {
        render_sprite_pixels(
            scanline,
            args.vram,
            args.registers,
            args.sprite_buffers,
            args.sprite_state,
            args.enforce_sprite_limits,
        );
    }

    render_pixels_in_scanline(&mut args, scanline, starting_pixel, bg_color);
}

fn clear_scanline(args: &mut RenderingArgs<'_>, scanline: u16, starting_pixel: u16) {
    let screen_width = args.registers.horizontal_display_size.active_display_pixels().into();
    let bg_color = colors::resolve_color(
        args.cram,
        args.registers.background_palette,
        args.registers.background_color_id,
    );

    let row: u32 = scanline.into();
    let starting_col: u32 = starting_pixel.into();

    for pixel in starting_col..screen_width {
        set_in_frame_buffer(args, row, pixel, bg_color, ColorModifier::None);
    }
}

fn set_in_frame_buffer(
    args: &mut RenderingArgs<'_>,
    row: u32,
    col: u32,
    color: u16,
    modifier: ColorModifier,
) {
    let r = ((color >> 1) & 0x07) as u8;
    let g = ((color >> 5) & 0x07) as u8;
    let b = ((color >> 9) & 0x07) as u8;
    let rgb_color = colors::gen_to_rgb(r, g, b, modifier, args.emulate_non_linear_dac);

    let screen_width: u32 = args.registers.horizontal_display_size.active_display_pixels().into();
    args.frame_buffer[(row * screen_width + col) as usize] = rgb_color;
}

#[allow(clippy::identity_op)]
fn render_pixels_in_scanline(
    args: &mut RenderingArgs<'_>,
    scanline: u16,
    starting_pixel: u16,
    bg_color: u16,
) {
    let cell_height = args.registers.interlacing_mode.cell_height();
    let v_scroll_size = args.registers.vertical_scroll_size;
    let h_scroll_size = args.registers.horizontal_scroll_size;

    let (h_scroll_size_pixels, v_scroll_size_pixels) = match (h_scroll_size, v_scroll_size) {
        // An invalid H scroll size always produces 32x1 scroll planes
        (ScrollSize::Invalid, _) => (32 * 8, 1 * 8),
        // An invalid V scroll size with valid H scroll size functions as a size of 32
        (_, ScrollSize::Invalid) => (h_scroll_size.to_pixels(), 32 * 8),
        _ => (h_scroll_size.to_pixels(), v_scroll_size.to_pixels()),
    };

    let scroll_line_bit_mask = match args.registers.interlacing_mode {
        InterlacingMode::Progressive | InterlacingMode::Interlaced => v_scroll_size_pixels - 1,
        InterlacingMode::InterlacedDouble => ((v_scroll_size_pixels - 1) << 1) | 0x01,
    };

    let h_scroll_scanline = match args.registers.interlacing_mode {
        InterlacingMode::Progressive | InterlacingMode::Interlaced => scanline,
        InterlacingMode::InterlacedDouble => scanline / 2,
    };
    let (h_scroll_a, h_scroll_b) = read_h_scroll(
        args.vram,
        args.registers.h_scroll_table_base_addr,
        args.registers.horizontal_scroll_mode,
        h_scroll_scanline,
    );

    let mut scroll_a_nt_row = u16::MAX;
    let mut scroll_a_nt_col = u16::MAX;
    let mut scroll_a_nt_word = NameTableWord::default();

    let mut scroll_b_nt_row = u16::MAX;
    let mut scroll_b_nt_col = u16::MAX;
    let mut scroll_b_nt_word = NameTableWord::default();

    for pixel in starting_pixel..args.registers.horizontal_display_size.active_display_pixels() {
        let h_cell = pixel / 8;
        let (v_scroll_a, v_scroll_b) = read_v_scroll(args, h_cell);

        let scrolled_scanline_a = scanline.wrapping_add(v_scroll_a) & scroll_line_bit_mask;
        let scroll_a_v_cell = scrolled_scanline_a / cell_height;

        let scrolled_scanline_b = scanline.wrapping_add(v_scroll_b) & scroll_line_bit_mask;
        let scroll_b_v_cell = scrolled_scanline_b / cell_height;

        let scrolled_pixel_a = pixel.wrapping_sub(h_scroll_a) & (h_scroll_size_pixels - 1);
        let scroll_a_h_cell = scrolled_pixel_a / 8;

        let scrolled_pixel_b = pixel.wrapping_sub(h_scroll_b) & (h_scroll_size_pixels - 1);
        let scroll_b_h_cell = scrolled_pixel_b / 8;

        if scroll_a_v_cell != scroll_a_nt_row || scroll_a_h_cell != scroll_a_nt_col {
            scroll_a_nt_word = read_name_table_word(
                args.vram,
                args.registers.scroll_a_base_nt_addr,
                h_scroll_size.into(),
                scroll_a_v_cell,
                scroll_a_h_cell,
            );
            scroll_a_nt_row = scroll_a_v_cell;
            scroll_a_nt_col = scroll_a_h_cell;
        }

        if scroll_b_v_cell != scroll_b_nt_row || scroll_b_h_cell != scroll_b_nt_col {
            scroll_b_nt_word = read_name_table_word(
                args.vram,
                args.registers.scroll_b_base_nt_addr,
                h_scroll_size.into(),
                scroll_b_v_cell,
                scroll_b_h_cell,
            );
            scroll_b_nt_row = scroll_b_v_cell;
            scroll_b_nt_col = scroll_b_h_cell;
        }

        let scroll_a_color_id = read_pattern_generator(
            args.vram,
            PatternGeneratorArgs {
                vertical_flip: scroll_a_nt_word.vertical_flip,
                horizontal_flip: scroll_a_nt_word.horizontal_flip,
                pattern_generator: scroll_a_nt_word.pattern_generator,
                row: scrolled_scanline_a,
                col: scrolled_pixel_a,
                cell_height,
            },
        );
        let scroll_b_color_id = read_pattern_generator(
            args.vram,
            PatternGeneratorArgs {
                vertical_flip: scroll_b_nt_word.vertical_flip,
                horizontal_flip: scroll_b_nt_word.horizontal_flip,
                pattern_generator: scroll_b_nt_word.pattern_generator,
                row: scrolled_scanline_b,
                col: scrolled_pixel_b,
                cell_height,
            },
        );

        let in_window = args.registers.is_in_window(scanline, pixel);
        let (window_priority, window_palette, window_color_id) = if in_window {
            let v_cell = scanline / cell_height;
            let window_nt_word = read_name_table_word(
                args.vram,
                args.registers.window_base_nt_addr,
                args.registers.horizontal_display_size.window_width_cells(),
                v_cell,
                h_cell,
            );
            let window_color_id = read_pattern_generator(
                args.vram,
                PatternGeneratorArgs {
                    vertical_flip: window_nt_word.vertical_flip,
                    horizontal_flip: window_nt_word.horizontal_flip,
                    pattern_generator: window_nt_word.pattern_generator,
                    row: scanline,
                    col: pixel,
                    cell_height,
                },
            );
            (window_nt_word.priority, window_nt_word.palette, window_color_id)
        } else {
            (false, 0, 0)
        };

        let SpritePixel {
            palette: sprite_palette,
            color_id: sprite_color_id,
            priority: sprite_priority,
        } = args.sprite_buffers.pixels[pixel as usize];

        let (scroll_a_priority, scroll_a_palette, scroll_a_color_id) = if in_window {
            // Window replaces scroll A if this pixel is inside the window
            (window_priority, window_palette, window_color_id)
        } else {
            (scroll_a_nt_word.priority, scroll_a_nt_word.palette, scroll_a_color_id)
        };

        let (pixel_color, color_modifier) = determine_pixel_color(
            args.cram,
            args.debug_register,
            PixelColorArgs {
                sprite_priority,
                sprite_palette,
                sprite_color_id,
                scroll_a_priority,
                scroll_a_palette,
                scroll_a_color_id,
                scroll_b_priority: scroll_b_nt_word.priority,
                scroll_b_palette: scroll_b_nt_word.palette,
                scroll_b_color_id,
                bg_color,
                shadow_highlight_flag: args.registers.shadow_highlight_flag,
            },
        );

        set_in_frame_buffer(args, scanline.into(), pixel.into(), pixel_color, color_modifier);
    }
}

fn read_v_scroll(args: &RenderingArgs<'_>, h_cell: u16) -> (u16, u16) {
    let (v_scroll_a, v_scroll_b) = match args.registers.vertical_scroll_mode {
        VerticalScrollMode::FullScreen => {
            (args.full_screen_v_scroll_a, args.full_screen_v_scroll_b)
        }
        VerticalScrollMode::TwoCell => {
            let addr = 4 * (h_cell as usize / 2);
            let v_scroll_a = u16::from_be_bytes([args.vsram[addr], args.vsram[addr + 1]]);
            let v_scroll_b = u16::from_be_bytes([args.vsram[addr + 2], args.vsram[addr + 3]]);
            (v_scroll_a, v_scroll_b)
        }
    };

    let v_scroll_mask = args.registers.interlacing_mode.v_scroll_mask();
    (v_scroll_a & v_scroll_mask, v_scroll_b & v_scroll_mask)
}

fn read_h_scroll(
    vram: &Vram,
    h_scroll_table_addr: u16,
    h_scroll_mode: HorizontalScrollMode,
    scanline: u16,
) -> (u16, u16) {
    let h_scroll_addr = match h_scroll_mode {
        HorizontalScrollMode::FullScreen => h_scroll_table_addr,
        HorizontalScrollMode::Cell => h_scroll_table_addr.wrapping_add(32 * (scanline / 8)),
        HorizontalScrollMode::Line => h_scroll_table_addr.wrapping_add(4 * scanline),
        HorizontalScrollMode::Invalid => h_scroll_table_addr.wrapping_add(4 * (scanline & 0x7)),
    };

    let h_scroll_a =
        u16::from_be_bytes([vram[h_scroll_addr as usize], vram[(h_scroll_addr + 1) as usize]]);
    let h_scroll_b = u16::from_be_bytes([
        vram[(h_scroll_addr + 2) as usize],
        vram[(h_scroll_addr + 3) as usize],
    ]);

    (h_scroll_a & 0x03FF, h_scroll_b & 0x03FF)
}

#[derive(Debug, Clone, Copy, Default)]
struct NameTableWord {
    priority: bool,
    palette: u8,
    vertical_flip: bool,
    horizontal_flip: bool,
    pattern_generator: u16,
}

fn read_name_table_word(
    vram: &Vram,
    base_addr: u16,
    name_table_width: u16,
    row: u16,
    col: u16,
) -> NameTableWord {
    let row_addr = base_addr.wrapping_add(2 * row * name_table_width);
    let addr = row_addr.wrapping_add(2 * col);
    let word = u16::from_be_bytes([vram[addr as usize], vram[addr.wrapping_add(1) as usize]]);

    NameTableWord {
        priority: word.bit(15),
        palette: ((word >> 13) & 0x03) as u8,
        vertical_flip: word.bit(12),
        horizontal_flip: word.bit(11),
        pattern_generator: word & 0x07FF,
    }
}

#[derive(Debug, Clone)]
pub struct PatternGeneratorArgs {
    pub vertical_flip: bool,
    pub horizontal_flip: bool,
    pub pattern_generator: u16,
    pub row: u16,
    pub col: u16,
    pub cell_height: u16,
}

#[inline]
pub fn read_pattern_generator(
    vram: &Vram,
    PatternGeneratorArgs {
        vertical_flip,
        horizontal_flip,
        pattern_generator,
        row,
        col,
        cell_height,
    }: PatternGeneratorArgs,
) -> u8 {
    let cell_row =
        if vertical_flip { cell_height - 1 - (row % cell_height) } else { row % cell_height };
    let cell_col = if horizontal_flip { 7 - (col % 8) } else { col % 8 };

    let row_addr = (4 * cell_height).wrapping_mul(pattern_generator);
    let addr = (row_addr + 4 * cell_row + (cell_col >> 1)) as usize;
    (vram[addr] >> (4 - ((cell_col & 0x01) << 2))) & 0x0F
}

#[derive(Debug, Clone, Copy)]
struct UnresolvedColor {
    palette: u8,
    color_id: u8,
    is_sprite: bool,
}

struct PixelColorArgs {
    sprite_priority: bool,
    sprite_palette: u8,
    sprite_color_id: u8,
    scroll_a_priority: bool,
    scroll_a_palette: u8,
    scroll_a_color_id: u8,
    scroll_b_priority: bool,
    scroll_b_palette: u8,
    scroll_b_color_id: u8,
    bg_color: u16,
    shadow_highlight_flag: bool,
}

#[inline]
#[allow(clippy::unnested_or_patterns)]
fn determine_pixel_color(
    cram: &Cram,
    debug_register: DebugRegister,
    PixelColorArgs {
        sprite_priority,
        sprite_palette,
        sprite_color_id,
        scroll_a_priority,
        scroll_a_palette,
        scroll_a_color_id,
        scroll_b_priority,
        scroll_b_palette,
        scroll_b_color_id,
        bg_color,
        shadow_highlight_flag,
    }: PixelColorArgs,
) -> (u16, ColorModifier) {
    let sprite_cram_idx = (sprite_palette << 4) | sprite_color_id;
    let scroll_a_cram_idx = (scroll_a_palette << 4) | scroll_a_color_id;
    let scroll_b_cram_idx = (scroll_b_palette << 4) | scroll_b_color_id;

    if debug_register.display_disabled {
        let color = match debug_register.forced_plane {
            Plane::Background => bg_color,
            Plane::Sprite => cram[sprite_cram_idx as usize],
            Plane::ScrollA => cram[scroll_a_cram_idx as usize],
            Plane::ScrollB => cram[scroll_b_cram_idx as usize],
        };
        return (color, ColorModifier::None);
    };

    let mut modifier = if shadow_highlight_flag && !scroll_a_priority && !scroll_b_priority {
        // If shadow/highlight bit is set and all priority flags are 0, default modifier to shadow
        ColorModifier::Shadow
    } else {
        ColorModifier::None
    };

    let sprite =
        UnresolvedColor { palette: sprite_palette, color_id: sprite_color_id, is_sprite: true };
    let scroll_a = UnresolvedColor {
        palette: scroll_a_palette,
        color_id: scroll_a_color_id,
        is_sprite: false,
    };
    let scroll_b = UnresolvedColor {
        palette: scroll_b_palette,
        color_id: scroll_b_color_id,
        is_sprite: false,
    };
    let colors = match (sprite_priority, scroll_a_priority, scroll_b_priority) {
        (false, false, false) | (true, false, false) | (true, true, false) | (true, true, true) => {
            [sprite, scroll_a, scroll_b]
        }
        (false, true, false) => [scroll_a, sprite, scroll_b],
        (false, false, true) => [scroll_b, sprite, scroll_a],
        (true, false, true) => [sprite, scroll_b, scroll_a],
        (false, true, true) => [scroll_a, scroll_b, sprite],
    };

    for UnresolvedColor { palette, color_id, is_sprite } in colors {
        if color_id == 0 {
            // Pixel is transparent
            continue;
        }

        if shadow_highlight_flag && is_sprite && palette == 3 {
            if color_id == 14 {
                // Palette 3 + color 14 = highlight; sprite is transparent, underlying pixel is highlighted
                modifier += ColorModifier::Highlight;
                continue;
            } else if color_id == 15 {
                // Palette 3 + color 15 = shadow; sprite is transparent, underlying pixel is shadowed
                modifier = ColorModifier::Shadow;
                continue;
            }
        }

        let cram_idx_mask = match debug_register.forced_plane {
            Plane::Background => 0x3F,
            Plane::Sprite => sprite_cram_idx,
            Plane::ScrollA => scroll_a_cram_idx,
            Plane::ScrollB => scroll_b_cram_idx,
        };
        let cram_idx = ((palette << 4) | color_id) & cram_idx_mask;

        let color = cram[cram_idx as usize];
        // Sprite color id 14 is never shadowed/highlighted, and neither is a sprite with the priority
        // bit set
        let modifier = if is_sprite && (color_id == 14 || sprite_priority) {
            ColorModifier::None
        } else {
            modifier
        };
        return (color, modifier);
    }

    let fallback_color = match debug_register.forced_plane {
        Plane::Background => bg_color,
        Plane::Sprite => cram[sprite_cram_idx as usize],
        Plane::ScrollA => cram[scroll_a_cram_idx as usize],
        Plane::ScrollB => cram[scroll_b_cram_idx as usize],
    };

    (fallback_color, modifier)
}
