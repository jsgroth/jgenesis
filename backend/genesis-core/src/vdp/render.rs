use crate::vdp::colors::ColorModifier;
use crate::vdp::registers::{
    HorizontalDisplaySize, HorizontalScrollMode, InterlacingMode, Registers, ScrollSize,
    VerticalScrollMode,
};
use crate::vdp::{
    colors, CachedSpriteData, Cram, FrameBuffer, SpriteBitSet, SpriteData, Vram, Vsram,
    MAX_SPRITES_PER_FRAME,
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
    pixels_disabled_during_scan: u16,
    pixels_disabled_during_tile_fetch: u16,
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
        self.pixels_disabled_during_scan = self.pixels_disabled_during_tile_fetch;
        self.pixels_disabled_during_tile_fetch = 0;

        self.display_enabled = display_enabled;
        self.display_enabled_pixel = h_display_size.active_display_pixels();
    }

    pub fn handle_display_enabled_write(&mut self, display_enabled: bool, pixel: u16) {
        if pixel < self.display_enabled_pixel {
            // Pre-HBlank write on the next scanline; ignore
            return;
        }

        if !self.display_enabled {
            self.pixels_disabled_during_tile_fetch += pixel - self.display_enabled_pixel;
        }

        self.display_enabled = display_enabled;
        self.display_enabled_pixel = pixel;
    }

    pub fn handle_line_end(&mut self, h_display_size: HorizontalDisplaySize) {
        if !self.display_enabled {
            self.pixels_disabled_during_tile_fetch +=
                h_display_size.pixels_including_hblank() - self.display_enabled_pixel;
        }
    }
}

pub struct RenderingArgs<'a> {
    pub frame_buffer: &'a mut FrameBuffer,
    pub sprite_buffer: &'a mut Vec<SpriteData>,
    pub sprite_bit_set: &'a mut SpriteBitSet,
    pub sprite_state: &'a mut SpriteState,
    pub vram: &'a Vram,
    pub cram: &'a Cram,
    pub vsram: &'a Vsram,
    pub registers: &'a Registers,
    pub cached_sprite_attributes: &'a [CachedSpriteData; MAX_SPRITES_PER_FRAME],
    pub full_screen_v_scroll_a: u16,
    pub full_screen_v_scroll_b: u16,
    pub enforce_sprite_limits: bool,
    pub emulate_non_linear_dac: bool,
}

pub fn render_scanline(mut args: RenderingArgs<'_>, scanline: u16, starting_pixel: u16) {
    if !args.registers.display_enabled {
        if scanline < args.registers.vertical_display_size.active_scanlines() {
            clear_scanline(&mut args, scanline, starting_pixel);

            // Clear sprite buffer in case display is enabled during active display
            args.sprite_buffer.clear();
        }

        return;
    }

    let bg_color = colors::resolve_color(
        args.cram,
        args.registers.background_palette,
        args.registers.background_color_id,
    );

    match args.registers.interlacing_mode {
        InterlacingMode::Progressive | InterlacingMode::Interlaced => {
            if starting_pixel == 0 {
                populate_sprite_buffer(&mut args, scanline);
            }

            render_pixels_in_scanline(&mut args, scanline, starting_pixel, bg_color);
        }
        InterlacingMode::InterlacedDouble => {
            // Render scanlines 2N and 2N+1 at the same time
            for scanline in [2 * scanline, 2 * scanline + 1] {
                populate_sprite_buffer(&mut args, scanline);

                render_pixels_in_scanline(&mut args, scanline, starting_pixel, bg_color);
            }
        }
    }
}

fn clear_scanline(args: &mut RenderingArgs<'_>, scanline: u16, starting_pixel: u16) {
    match args.registers.interlacing_mode {
        InterlacingMode::Progressive | InterlacingMode::Interlaced => {
            clear_frame_buffer_row(args, scanline.into(), starting_pixel.into());
        }
        InterlacingMode::InterlacedDouble => {
            clear_frame_buffer_row(args, (2 * scanline).into(), starting_pixel.into());
            clear_frame_buffer_row(args, (2 * scanline + 1).into(), starting_pixel.into());
        }
    }
}

fn clear_frame_buffer_row(args: &mut RenderingArgs<'_>, row: u32, starting_col: u32) {
    let screen_width = args.registers.horizontal_display_size.active_display_pixels().into();
    let bg_color = colors::resolve_color(
        args.cram,
        args.registers.background_palette,
        args.registers.background_color_id,
    );

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

// TODO optimize this to do fewer passes for sorting/filtering
fn populate_sprite_buffer(args: &mut RenderingArgs<'_>, scanline: u16) {
    args.sprite_buffer.clear();

    // Populate buffer from the sprite attribute table
    let h_size = args.registers.horizontal_display_size;
    let sprite_table_addr = args.registers.masked_sprite_attribute_table_addr();

    // Sprite 0 is always populated
    let sprite_0 = SpriteData::create(
        args.cached_sprite_attributes[0],
        &args.vram[sprite_table_addr as usize + 4..sprite_table_addr as usize + 8],
    );
    let mut sprite_idx: u16 = sprite_0.link_data.into();
    args.sprite_buffer.push(sprite_0);

    // If display was disabled during part of HBlank on the scanline before the previous scanline,
    // the number of sprites scanned for the current scanline is reduced roughly by the number of
    // pixels that display was disabled for.
    // Actual hardware doesn't work exactly this way (it depends on exactly which VRAM access slots
    // display was disabled during), but this approximation works well enough for Mickey Mania's
    // 3D stages and Titan Overdrive's "your emulator suxx" screen
    let sprites_not_scanned = if args.sprite_state.pixels_disabled_during_scan != 0 {
        // Not sure exactly why, but adding ~8 here is necessary to fully remove the "your emulator
        // suxx" text from Titan Overdrive's 512-color screen
        args.sprite_state.pixels_disabled_during_scan + 8
    } else {
        0
    };
    let max_sprites_to_scan = h_size.sprite_table_len().saturating_sub(sprites_not_scanned);
    for _ in 0..max_sprites_to_scan {
        if sprite_idx == 0 || sprite_idx >= max_sprites_to_scan {
            break;
        }

        let sprite_addr = sprite_table_addr.wrapping_add(8 * sprite_idx) as usize;
        let sprite = SpriteData::create(
            args.cached_sprite_attributes[sprite_idx as usize],
            &args.vram[sprite_addr + 4..sprite_addr + 8],
        );
        sprite_idx = sprite.link_data.into();
        args.sprite_buffer.push(sprite);
    }

    // Remove sprites that don't fall on this scanline
    let interlacing_mode = args.registers.interlacing_mode;
    let sprite_scanline = interlacing_mode.sprite_display_top() + scanline;
    let cell_height = interlacing_mode.cell_height();
    args.sprite_buffer.retain(|sprite| {
        let sprite_top = sprite.v_position(interlacing_mode);
        let sprite_bottom = sprite_top + cell_height * u16::from(sprite.v_size_cells);
        (sprite_top..sprite_bottom).contains(&sprite_scanline)
    });

    // Apply max sprite per scanline limit
    let max_sprites_per_line = h_size.max_sprites_per_line() as usize;
    if args.sprite_buffer.len() > max_sprites_per_line {
        if args.enforce_sprite_limits {
            args.sprite_buffer.truncate(max_sprites_per_line);
        }
        args.sprite_state.overflow = true;
    }

    // Apply max sprite pixel per scanline limit.
    //
    // If display was disabled during HBlank on the previous scanline, the number of sprite pixels
    // rendered is reduced roughly proportional to the number of pixels during which display was
    // disabled.
    // As above, this is an approximation; in actual hardware it depends on which VRAM access slots
    // were skipped because display was disabled
    let max_sprite_pixels_per_line =
        h_size.max_sprite_pixels_per_line().saturating_sub(sprites_not_scanned * 4);
    let mut line_pixels = 0;
    let mut dot_overflow = false;
    for i in 0..args.sprite_buffer.len() {
        let sprite_pixels = 8 * u16::from(args.sprite_buffer[i].h_size_cells);
        line_pixels += sprite_pixels;
        if line_pixels > max_sprite_pixels_per_line {
            if args.enforce_sprite_limits {
                let overflow_pixels = line_pixels - max_sprite_pixels_per_line;
                args.sprite_buffer[i].partial_width = Some(sprite_pixels - overflow_pixels);

                args.sprite_buffer.truncate(i + 1);
            }

            args.sprite_state.overflow = true;
            dot_overflow = true;
            break;
        }
    }

    // Sprites with H position 0 mask all lower priority sprites on the same scanline...with
    // some quirks. There must be at least one sprite with H != 0 before the H=0 sprite, unless
    // there was a sprite pixel overflow on the previous scanline.
    let mut found_non_zero = args.sprite_state.dot_overflow_on_prev_line;
    for i in 0..args.sprite_buffer.len() {
        if args.sprite_buffer[i].h_position != 0 {
            found_non_zero = true;
            continue;
        }

        if args.sprite_buffer[i].h_position == 0 && found_non_zero {
            args.sprite_buffer.truncate(i);
            break;
        }
    }
    args.sprite_state.dot_overflow_on_prev_line = dot_overflow;

    // Fill in bit set
    args.sprite_bit_set.clear();
    for sprite in &*args.sprite_buffer {
        for x in sprite.h_position..sprite.h_position + 8 * u16::from(sprite.h_size_cells) {
            let pixel = x.wrapping_sub(SPRITE_H_DISPLAY_START);
            if pixel < SpriteBitSet::LEN {
                args.sprite_bit_set.set(pixel);
            }
        }
    }
}

fn find_first_overlapping_sprite<'sprites>(
    sprite_buffer: &'sprites [SpriteData],
    sprite_bit_set: &SpriteBitSet,
    sprite_state: &mut SpriteState,
    vram: &Vram,
    registers: &Registers,
    scanline: u16,
    pixel: u16,
) -> Option<(&'sprites SpriteData, u8)> {
    if !sprite_bit_set.get(pixel) {
        return None;
    }

    let interlacing_mode = registers.interlacing_mode;
    let sprite_display_top = interlacing_mode.sprite_display_top();
    let cell_height = interlacing_mode.cell_height();

    let sprite_pixel = SPRITE_H_DISPLAY_START + pixel;

    let mut found_sprite: Option<(&SpriteData, u8)> = None;
    for sprite in sprite_buffer {
        let sprite_width = sprite.partial_width.unwrap_or(8 * u16::from(sprite.h_size_cells));
        let sprite_right = sprite.h_position + sprite_width;
        if !(sprite.h_position..sprite_right).contains(&sprite_pixel) {
            continue;
        }

        let v_size_cells: u16 = sprite.v_size_cells.into();
        let h_size_cells: u16 = sprite.h_size_cells.into();

        let sprite_row = sprite_display_top + scanline - sprite.v_position(interlacing_mode);
        let sprite_row = if sprite.vertical_flip {
            cell_height * v_size_cells - 1 - sprite_row
        } else {
            sprite_row
        };

        let sprite_col = sprite_pixel - sprite.h_position;
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
        if color_id == 0 {
            // Sprite pixel is transparent
            continue;
        }

        match found_sprite {
            Some(_) => {
                sprite_state.collision = true;
                break;
            }
            None => {
                found_sprite = Some((sprite, color_id));
                if sprite_state.collision {
                    // No point in continuing to check sprites if the collision flag is
                    // already set
                    break;
                }
            }
        }
    }

    found_sprite
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

        let (sprite_priority, sprite_palette, sprite_color_id) = find_first_overlapping_sprite(
            args.sprite_buffer,
            args.sprite_bit_set,
            args.sprite_state,
            args.vram,
            args.registers,
            scanline,
            pixel,
        )
        .map_or((false, 0, 0), |(sprite, color_id)| (sprite.priority, sprite.palette, color_id));

        let (scroll_a_priority, scroll_a_palette, scroll_a_color_id) = if in_window {
            // Window replaces scroll A if this pixel is inside the window
            (window_priority, window_palette, window_color_id)
        } else {
            (scroll_a_nt_word.priority, scroll_a_nt_word.palette, scroll_a_color_id)
        };

        let (pixel_color, color_modifier) = determine_pixel_color(
            args.cram,
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
    let (h_scroll_a, h_scroll_b) = match h_scroll_mode {
        HorizontalScrollMode::FullScreen => {
            let h_scroll_a = u16::from_be_bytes([
                vram[h_scroll_table_addr as usize],
                vram[h_scroll_table_addr.wrapping_add(1) as usize],
            ]);
            let h_scroll_b = u16::from_be_bytes([
                vram[h_scroll_table_addr.wrapping_add(2) as usize],
                vram[h_scroll_table_addr.wrapping_add(3) as usize],
            ]);
            (h_scroll_a, h_scroll_b)
        }
        HorizontalScrollMode::Cell => {
            let v_cell = scanline / 8;
            let addr = h_scroll_table_addr.wrapping_add(32 * v_cell);
            let h_scroll_a =
                u16::from_be_bytes([vram[addr as usize], vram[addr.wrapping_add(1) as usize]]);
            let h_scroll_b = u16::from_be_bytes([
                vram[addr.wrapping_add(2) as usize],
                vram[addr.wrapping_add(3) as usize],
            ]);
            (h_scroll_a, h_scroll_b)
        }
        HorizontalScrollMode::Line => {
            let addr = h_scroll_table_addr.wrapping_add(4 * scanline);
            let h_scroll_a =
                u16::from_be_bytes([vram[addr as usize], vram[addr.wrapping_add(1) as usize]]);
            let h_scroll_b = u16::from_be_bytes([
                vram[addr.wrapping_add(2) as usize],
                vram[addr.wrapping_add(3) as usize],
            ]);
            (h_scroll_a, h_scroll_b)
        }
    };

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

        let color = cram[((palette << 4) | color_id) as usize];
        // Sprite color id 14 is never shadowed/highlighted, and neither is a sprite with the priority
        // bit set
        let modifier = if is_sprite && (color_id == 14 || sprite_priority) {
            ColorModifier::None
        } else {
            modifier
        };
        return (color, modifier);
    }

    (bg_color, modifier)
}
