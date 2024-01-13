use crate::vdp::registers::{HorizontalDisplaySize, InterlacingMode};
use crate::vdp::render::{PatternGeneratorArgs, RasterLine};
use crate::vdp::{render, CachedSpriteData, SpriteData, Vdp};
use bincode::{Decode, Encode};

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
pub struct SpritePixel {
    pub palette: u8,
    pub color_id: u8,
    pub priority: bool,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SpriteBuffers {
    pub scanned_ids: Vec<u8>,
    pub sprites: Vec<SpriteData>,
    pub last_tile_addresses: Box<[u16; 40]>,
    pub pixels: Box<[SpritePixel; 320]>,
}

impl SpriteBuffers {
    pub fn new() -> Self {
        Self {
            scanned_ids: Vec::with_capacity(20),
            sprites: Vec::with_capacity(20),
            last_tile_addresses: vec![0; 40].into_boxed_slice().try_into().unwrap(),
            pixels: vec![SpritePixel::default(); 320].into_boxed_slice().try_into().unwrap(),
        }
    }
}

impl Vdp {
    // Scan sprites (Phase 1 according to Overdrive 2 documentation).
    //
    // This should be called at the end of HBlank 2 scanlines before the line to be rendered, with sprite attributes latched
    // from around when HINT is generated. Actual hardware does sprite scanning in parallel with sprite pixel fetching for
    // the next scanline, but here we want to know if there were any pixels where display was disabled during HBlank.
    pub(super) fn scan_sprites(&mut self, scanline: u16) {
        let raster_line =
            RasterLine::from_scanline(scanline, &self.latched_registers, self.timing_mode);

        // In the vertical border, sprite scan only occurs for the scanline immediately following
        // active display (unless the vertical border was forgotten)
        if raster_line.in_v_border
            && !self.state.v_border_forgotten
            && scanline != self.registers.vertical_display_size.active_scanlines()
        {
            return;
        }

        match self.latched_registers.interlacing_mode {
            InterlacingMode::Progressive | InterlacingMode::Interlaced => {
                self.do_sprite_scan(raster_line, false);
                self.interlaced_sprite_buffers.scanned_ids.clear();
            }
            InterlacingMode::InterlacedDouble => {
                self.do_sprite_scan(raster_line.to_interlaced_even(), false);
                self.do_sprite_scan(raster_line.to_interlaced_odd(), true);
            }
        }
    }

    fn do_sprite_scan(&mut self, raster_line: RasterLine, use_interlaced_buffers: bool) {
        let buffers = if use_interlaced_buffers {
            &mut self.interlaced_sprite_buffers
        } else {
            &mut self.sprite_buffers
        };

        buffers.scanned_ids.clear();

        let h_size = self.latched_registers.horizontal_display_size;

        // If display was disabled during part of HBlank on the scanline before the previous scanline,
        // the number of sprites scanned for the current scanline is reduced roughly by the number of
        // pixels that display was disabled for.
        // Actual hardware doesn't work exactly this way (it depends on exactly which VRAM access slots
        // display was disabled during), but this approximation works well enough for Mickey Mania's
        // 3D stages and Titan Overdrive's "your emulator suxx" screen
        let sprites_not_scanned = if self.sprite_state.pixels_disabled_during_hblank != 0 {
            // Not sure exactly why, but adding ~8 here is necessary to fully remove the "your emulator
            // suxx" text from Titan Overdrive's 512-color screen
            self.sprite_state.pixels_disabled_during_hblank + 8
        } else {
            0
        };
        let max_sprites_to_scan = h_size.sprite_table_len().saturating_sub(sprites_not_scanned);

        let interlacing_mode = self.latched_registers.interlacing_mode;
        let sprite_scanline = (interlacing_mode.sprite_display_top() + raster_line.line)
            & interlacing_mode.sprite_display_mask();
        let cell_height = interlacing_mode.cell_height();

        let max_sprites_per_line = h_size.max_sprites_per_line() as usize;

        // Sprite 0 is always populated
        let mut sprite_idx = 0_u16;
        for _ in 0..max_sprites_to_scan {
            let CachedSpriteData { v_position, v_size_cells, link_data, .. } =
                self.latched_sprite_attributes[sprite_idx as usize];

            // Check if sprite falls on this scanline
            let sprite_top = sprite_y_position(v_position, interlacing_mode);
            let sprite_bottom = sprite_top + cell_height * u16::from(v_size_cells);
            if (sprite_top..sprite_bottom).contains(&sprite_scanline) {
                // Check if sprite-per-scanline limit has been hit
                if buffers.scanned_ids.len() == max_sprites_per_line {
                    self.sprite_state.overflow = true;
                    if self.config.enforce_sprite_limits {
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
    pub(super) fn fetch_sprite_attributes(&mut self) {
        self.do_sprite_attribute_fetch(false);

        match self.latched_registers.interlacing_mode {
            InterlacingMode::Progressive | InterlacingMode::Interlaced => {
                self.interlaced_sprite_buffers.sprites.clear();
            }
            InterlacingMode::InterlacedDouble => {
                self.do_sprite_attribute_fetch(true);
            }
        }
    }

    fn do_sprite_attribute_fetch(&mut self, use_interlaced_buffers: bool) {
        let buffers = if use_interlaced_buffers {
            &mut self.interlaced_sprite_buffers
        } else {
            &mut self.sprite_buffers
        };

        buffers.sprites.clear();

        let sprite_table_addr = self.registers.masked_sprite_attribute_table_addr();

        for &sprite_idx in &buffers.scanned_ids {
            let sprite_addr = sprite_table_addr.wrapping_add(8 * u16::from(sprite_idx)) as usize;
            let sprite = SpriteData::create(
                self.cached_sprite_attributes[sprite_idx as usize],
                &self.vram[sprite_addr + 4..sprite_addr + 8],
            );
            buffers.sprites.push(sprite);
        }
    }

    // Fetch and render sprite pixels into the line buffer (Phase 3 in the Overdrive 2 documentation). Uses the sprite
    // attributes that were fetched from VRAM during Phase 2.
    //
    // Similar to Phase 1, in actual hardware this occurs throughout HBlank using latched registers. Here, it should be
    // called at the end of HBlank so that we know how many pixels the display was disabled during HBlank.
    pub(super) fn render_sprite_pixels(
        &mut self,
        raster_line: RasterLine,
        use_interlaced_buffers: bool,
    ) {
        let buffers = if use_interlaced_buffers {
            &mut self.interlaced_sprite_buffers
        } else {
            &mut self.sprite_buffers
        };

        buffers.pixels.fill(SpritePixel::default());

        let h_size = self.latched_registers.horizontal_display_size;
        let sprite_display_area =
            SPRITE_H_DISPLAY_START..SPRITE_H_DISPLAY_START + h_size.active_display_pixels();

        let half_tiles_not_fetched = if self.sprite_state.pixels_disabled_during_hblank != 0 {
            self.sprite_state.pixels_disabled_during_hblank + 8
        } else {
            0
        };

        let interlacing_mode = self.latched_registers.interlacing_mode;
        let sprite_scanline = interlacing_mode.sprite_display_top() + raster_line.line;
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
        let mut tiles_fetched = 0;
        let mut dot_overflow = false;

        // Sprites with H position 0 mask all lower priority sprites on the same scanline...with
        // some quirks. There must be at least one sprite with H != 0 before the H=0 sprite, unless
        // there was a sprite pixel overflow on the previous scanline.
        let mut found_non_zero = self.sprite_state.dot_overflow_on_prev_line;

        for sprite in &buffers.sprites {
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

            // Record what VRAM addresses were accessed during sprite tile fetching; this is needed for rendering the
            // borders in Titan Overdrive 2
            for h_cell in 0..h_size_cells {
                let pattern_offset = h_cell * v_size_cells + sprite_row / cell_height;
                let pattern_generator = sprite.pattern_generator.wrapping_add(pattern_offset);
                let cell_addr = (4 * cell_height).wrapping_mul(pattern_generator);
                let row_addr = cell_addr + 4 * (sprite_row % cell_height);

                buffers.last_tile_addresses[tiles_fetched] = row_addr;
                tiles_fetched += 1;

                if tiles_fetched == buffers.last_tile_addresses.len() {
                    // Hit the 40 tile / 320 pixel limit
                    break;
                }
            }

            let sprite_width = 8 * h_size_cells;
            let sprite_right = sprite.h_position + sprite_width;
            for h_position in sprite.h_position..sprite_right {
                line_pixels += 1;
                if line_pixels > max_sprite_pixels_per_line {
                    break;
                }

                if !sprite_display_area.contains(&h_position) {
                    continue;
                }

                let sprite_col = h_position - sprite.h_position;
                let sprite_col = if sprite.horizontal_flip {
                    8 * h_size_cells - 1 - sprite_col
                } else {
                    sprite_col
                };

                let pattern_offset = (sprite_col / 8) * v_size_cells + sprite_row / cell_height;
                let color_id = render::read_pattern_generator(
                    &self.vram,
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
                    buffers.pixels[pixel as usize] = SpritePixel {
                        palette: sprite.palette,
                        color_id,
                        priority: sprite.priority,
                    };
                } else {
                    // Sprite collision; two non-transparent sprite pixels in the same position
                    self.sprite_state.collision = true;
                }
            }

            if line_pixels >= max_sprite_pixels_per_line {
                self.sprite_state.overflow = true;
                dot_overflow = true;
                break;
            }
        }

        self.sprite_state.dot_overflow_on_prev_line = dot_overflow;
    }
}

fn sprite_y_position(v_position: u16, interlacing_mode: InterlacingMode) -> u16 {
    // V position is 9 bits in progressive mode and interlaced mode 1, and 10 bits in
    // interlaced mode 2
    match interlacing_mode {
        InterlacingMode::Progressive | InterlacingMode::Interlaced => v_position & 0x1FF,
        InterlacingMode::InterlacedDouble => v_position & 0x3FF,
    }
}
