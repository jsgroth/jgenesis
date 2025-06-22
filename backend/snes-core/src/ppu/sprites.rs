use crate::ppu::registers::{BitsPerPixel, ObjPriorityMode, TileSize};
use crate::ppu::{
    MAX_SPRITE_TILES_PER_LINE, MAX_SPRITES_PER_LINE, Pixel, Ppu, VRAM_ADDRESS_MASK,
    line_overlaps_sprite,
};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::cmp;

pub const SPRITE_EVALUATION_END_DOT: u16 = 256;

// 340 - 270 = 70
// Tile limit is 34 (68/2), but give 2 extra dots so that it's possible for a 35th tile to trigger
// the sprite time overflow check
pub const SPRITE_FETCH_START_DOT: u16 = 270;
pub const SPRITE_FETCH_END_DOT: u16 = 340;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum SpriteState {
    Blank,
    Evaluation { oam_idx: u8 },
    Idle { oam_idx: u8 },
    TileFetch { oam_buffer_idx: u8, tile_idx: u8 },
}

impl SpriteState {
    pub fn oam_idx(self, scanned_oam_idxs: &[u8]) -> u8 {
        match self {
            Self::Evaluation { oam_idx } | Self::Idle { oam_idx } => oam_idx,
            Self::TileFetch { oam_buffer_idx, .. } => scanned_oam_idxs[oam_buffer_idx as usize],
            Self::Blank => 0,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SpriteTileData {
    pub x: u16,
    pub palette: u8,
    pub priority: u8,
    pub colors: [u8; 8],
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SpriteProcessor {
    pub state: SpriteState,
    pub line: u16,
    pub interlaced_odd_frame: bool,
    pub dot: u16,
    pub scanned_oam_idxs: Vec<u8>,
    pub fetched_tiles: Vec<SpriteTileData>,
    pub fetched_tiles_deinterlace: Vec<SpriteTileData>,
    pub last_fetched_oam_idx: u8,
}

impl SpriteProcessor {
    pub fn new() -> Self {
        Self {
            state: SpriteState::Blank,
            line: 0,
            interlaced_odd_frame: false,
            dot: 0,
            scanned_oam_idxs: Vec::with_capacity(MAX_SPRITES_PER_LINE),
            fetched_tiles: Vec::with_capacity(MAX_SPRITE_TILES_PER_LINE),
            fetched_tiles_deinterlace: Vec::with_capacity(MAX_SPRITE_TILES_PER_LINE),
            last_fetched_oam_idx: 0,
        }
    }
}

impl Ppu {
    pub(super) fn sprites_start_new_line(&mut self, scanline: u16, interlaced_odd_frame: bool) {
        self.sprites.line = scanline;
        self.sprites.interlaced_odd_frame = interlaced_odd_frame;
        self.sprites.dot = 0;

        self.sprites.state = if self.in_active_display(scanline) {
            // If priority rotate mode is set, start iteration at the current OAM address instead of 0
            self.sprites.scanned_oam_idxs.clear();
            let start_oam_idx = match self.registers.obj_priority_mode {
                ObjPriorityMode::Normal => 0,
                ObjPriorityMode::Rotate => (self.registers.oam_address >> 1) & 0x7F,
            };

            log::trace!(
                "Beginning sprite evaluation for line {scanline} at OAM idx {start_oam_idx}"
            );

            SpriteState::Evaluation { oam_idx: start_oam_idx as u8 }
        } else {
            // Sprite evaluation not performed in VBlank or forced blanking
            SpriteState::Blank
        };
    }

    pub(super) fn progress_sprite_evaluation(&mut self, dot: u16) {
        let SpriteState::Evaluation { oam_idx } = self.sprites.state else { return };

        let dot = cmp::min(dot, SPRITE_EVALUATION_END_DOT);

        // Sprite evaluation takes 2 dots per OAM entry
        debug_assert!(self.sprites.dot <= dot);
        let num_sprites_to_scan = dot / 2 - self.sprites.dot / 2;

        log::trace!(
            "Progressing sprite evaluation on line {} to dot {dot}, scanning up to {num_sprites_to_scan} sprites",
            self.sprites.line
        );

        let new_oam_idx = self.progress_oam_scan(self.sprites.line, oam_idx, num_sprites_to_scan);

        self.sprites.dot = dot;
        self.sprites.state = if dot == SPRITE_EVALUATION_END_DOT
            || self.sprites.scanned_oam_idxs.len() == MAX_SPRITES_PER_LINE
        {
            SpriteState::Idle {
                oam_idx: if self.sprites.scanned_oam_idxs.is_empty() {
                    // If no sprites were scanned in range for this line, mid-scanline OAM writes
                    // should go to the sprite for the last fetched tile.
                    // Uniracers depends on this for correct rendering in Vs. mode
                    self.sprites.last_fetched_oam_idx
                } else {
                    0
                },
            }
        } else {
            SpriteState::Evaluation { oam_idx: new_oam_idx }
        };
    }

    #[must_use]
    fn progress_oam_scan(
        &mut self,
        scanline: u16,
        start_oam_idx: u8,
        num_sprites_to_scan: u16,
    ) -> u8 {
        const OAM_IDX_MASK: u8 = 0x7F;

        let (small_width, small_height, large_width, large_height) = {
            let (small_width, mut small_height) = self.registers.obj_tile_size.small_size();
            let (large_width, mut large_height) = self.registers.obj_tile_size.large_size();

            if self.registers.interlaced && self.registers.pseudo_obj_hi_res {
                // If smaller OBJs are enabled, pretend sprites are half-size vertically for the OAM scan
                small_height >>= 1;
                large_height >>= 1;
            }

            (small_width, small_height, large_width, large_height)
        };

        let mut oam_idx = start_oam_idx;
        for _ in 0..num_sprites_to_scan {
            let oam_low_addr = (oam_idx << 1) as usize;
            let [x_lsb, y] = self.oam_low[oam_low_addr].to_le_bytes();

            let oam_high_addr = (oam_idx >> 2) as usize;
            let oam_high_shift = 2 * (oam_idx & 3);
            let oam_high_bits = self.oam_high[oam_high_addr] >> oam_high_shift;

            let x_msb = oam_high_bits.bit(0);
            let size = if oam_high_bits.bit(1) { TileSize::Large } else { TileSize::Small };

            let (sprite_width, sprite_height) = match size {
                TileSize::Small => (small_width, small_height),
                TileSize::Large => (large_width, large_height),
            };

            if !line_overlaps_sprite(y, sprite_height, scanline) {
                oam_idx = (oam_idx + 1) & OAM_IDX_MASK;
                continue;
            }

            // Only sprites with pixels in the range [0, 256) are scanned into the sprite buffer
            let x = u16::from_le_bytes([x_lsb, u8::from(x_msb)]);
            if x >= 256 && x + sprite_width <= 512 {
                oam_idx = (oam_idx + 1) & OAM_IDX_MASK;
                continue;
            }

            if self.sprites.scanned_oam_idxs.len() == MAX_SPRITES_PER_LINE {
                self.registers.sprite_overflow = true;
                log::debug!("Hit 32 sprites per line limit on line {scanline}");
                return (oam_idx + 1) & OAM_IDX_MASK;
            }

            self.sprites.scanned_oam_idxs.push(oam_idx);
            oam_idx = (oam_idx + 1) & OAM_IDX_MASK;
        }

        oam_idx
    }

    pub(super) fn begin_sprite_tile_fetch(&mut self) {
        if self.vblank_flag() {
            // No sprite tile fetching during VBlank
            self.sprites.state = SpriteState::Blank;
            return;
        }

        // Explicitly do not check whether forced blanking is enabled.
        // If forced blanking is enabled, let the tile fetch proceed as normal, but make any fetched
        // sprites invisible (all pixels transparent).

        self.sprites.dot = SPRITE_FETCH_START_DOT;
        self.sprites.fetched_tiles.clear();

        if self.sprites.scanned_oam_idxs.is_empty() {
            // No tiles to fetch
            log::trace!("No sprites scanned during evaluation for line {}", self.sprites.line);
            self.sprites.state = SpriteState::Idle { oam_idx: self.sprites.last_fetched_oam_idx };
            return;
        }

        log::trace!(
            "Beginning sprite tile fetch for line {}, scanned {} sprites",
            self.sprites.line,
            self.sprites.scanned_oam_idxs.len()
        );

        // Tiles are fetched for sprites in reverse order (games depend on this, e.g. Final Fantasy 6)
        // Tiles within a sprite are processed left-to-right
        self.sprites.state = SpriteState::TileFetch {
            oam_buffer_idx: (self.sprites.scanned_oam_idxs.len() - 1) as u8,
            tile_idx: 0,
        };
    }

    pub(super) fn progress_sprite_tile_fetch(&mut self, dot: u16) {
        if !matches!(self.sprites.state, SpriteState::TileFetch { .. }) {
            return;
        }

        let end_dot = cmp::min(dot, SPRITE_FETCH_END_DOT);
        if self.sprites.dot >= end_dot {
            return;
        }

        // Tiles are fetched at a rate of 2 dots per tile
        let num_tiles_to_fetch = end_dot / 2 - self.sprites.dot / 2;

        log::trace!(
            "Progressing sprite tile fetch for line {} to dot {dot}, fetching up to {num_tiles_to_fetch} tiles",
            self.sprites.line
        );

        self.sprites.state = self.fetch_sprite_tiles(
            self.sprites.line,
            end_dot,
            self.sprites.interlaced_odd_frame,
            num_tiles_to_fetch,
        );
        self.sprites.dot = end_dot;
    }

    pub(super) fn sprites_finish_line(&mut self) {
        self.progress_sprite_tile_fetch(SPRITE_FETCH_END_DOT);

        log::trace!(
            "Rendering {} sprite tiles to line buffer for line {}",
            self.sprites.fetched_tiles.len(),
            self.sprites.line
        );

        self.render_sprite_tiles();

        if self.deinterlace
            && self.state.v_hi_res_frame
            && self.registers.pseudo_obj_hi_res
            && !self.sprites.scanned_oam_idxs.is_empty()
        {
            log::trace!("Fetching extra line of sprite tiles for deinterlaced rendering");

            self.sprites.fetched_tiles.clear();
            self.sprites.state = SpriteState::TileFetch {
                oam_buffer_idx: (self.sprites.scanned_oam_idxs.len() - 1) as u8,
                tile_idx: 0,
            };
            self.sprites.state = self.fetch_sprite_tiles(
                self.sprites.line,
                SPRITE_FETCH_END_DOT,
                !self.sprites.interlaced_odd_frame,
                MAX_SPRITE_TILES_PER_LINE as u16,
            );
        }
    }

    #[must_use]
    fn fetch_sprite_tiles(
        &mut self,
        scanline: u16,
        dot: u16,
        interlaced_odd_line: bool,
        num_tiles_to_fetch: u16,
    ) -> SpriteState {
        let SpriteState::TileFetch { mut oam_buffer_idx, mut tile_idx } = self.sprites.state else {
            return self.sprites.state;
        };

        let (small_width, small_height) = self.registers.obj_tile_size.small_size();
        let (large_width, large_height) = self.registers.obj_tile_size.large_size();

        let mut tiles_fetched = 0;
        while tiles_fetched < num_tiles_to_fetch {
            let oam_idx = self.sprites.scanned_oam_idxs[oam_buffer_idx as usize];

            let oam_low_addr = usize::from(oam_idx) << 1;
            let [x_lsb, y] = self.oam_low[oam_low_addr].to_le_bytes();

            let [tile_number_lsb, attributes] = self.oam_low[oam_low_addr + 1].to_le_bytes();

            let oam_high_addr = usize::from(oam_idx >> 2);
            let oam_high_shift = 2 * (oam_idx & 3);
            let oam_high_bits = self.oam_high[oam_high_addr] >> oam_high_shift;

            let base_tile_number =
                u16::from_le_bytes([tile_number_lsb, u8::from(attributes.bit(0))]);
            let palette = (attributes >> 1) & 0x07;
            let priority = (attributes >> 4) & 0x03;
            let x_flip = attributes.bit(6);
            let y_flip = attributes.bit(7);

            let x_msb = oam_high_bits.bit(0);
            let size = if oam_high_bits.bit(1) { TileSize::Large } else { TileSize::Small };

            let x = u16::from_le_bytes([x_lsb, x_msb.into()]);

            let (sprite_width, sprite_height) = match size {
                TileSize::Small => (small_width, small_height),
                TileSize::Large => (large_width, large_height),
            };

            if !line_overlaps_sprite(y, sprite_height, scanline) {
                // Can happen if Y coordinate changes between OAM scan and tile fetch
                if oam_buffer_idx == 0 {
                    // Fetched all tiles for this line
                    return SpriteState::Idle { oam_idx: self.sprites.last_fetched_oam_idx };
                }

                oam_buffer_idx -= 1;
                tile_idx = 0;
                continue;
            }

            let mut sprite_line = if y_flip {
                sprite_height as u8
                    - 1
                    - ((scanline as u8).wrapping_sub(y) & ((sprite_height - 1) as u8))
            } else {
                (scanline as u8).wrapping_sub(y) & ((sprite_height - 1) as u8)
            };

            // Adjust sprite line if smaller OBJs are enabled
            // Smaller OBJs affect how the line within the sprite is determined, but not where the
            // sprite is positioned onscreen
            if self.registers.interlaced && self.registers.pseudo_obj_hi_res {
                sprite_line = (sprite_line << 1) | u8::from(interlaced_odd_line ^ y_flip);
            }

            let tile_y_offset: u16 = (sprite_line / 8).into();

            let num_sprite_tiles = (sprite_width / 8) as u8;
            while tile_idx < num_sprite_tiles && tiles_fetched < num_tiles_to_fetch {
                let tile_x_offset: u16 = tile_idx.into();
                let x = if x_flip {
                    x + (sprite_width - 8) - 8 * tile_x_offset
                } else {
                    x + 8 * tile_x_offset
                };

                if x >= 256 && x + 8 < 512 {
                    // Sprite tile is entirely offscreen; don't fetch
                    tile_idx += 1;
                    continue;
                }

                if self.sprites.fetched_tiles.len() == MAX_SPRITE_TILES_PER_LINE {
                    // Sprite time overflow
                    self.registers.sprite_pixel_overflow = true;
                    log::debug!("Hit 34 sprite tiles per line limit on line {scanline}");
                    return SpriteState::Idle { oam_idx: self.sprites.last_fetched_oam_idx };
                }

                // Unlike BG tiles in 16x16 mode, overflows in large OBJ tiles do not carry to the next nibble
                let mut tile_number = base_tile_number;
                tile_number =
                    (tile_number & !0xF) | (tile_number.wrapping_add(tile_x_offset) & 0xF);
                tile_number =
                    (tile_number & !0xF0) | (tile_number.wrapping_add(tile_y_offset << 4) & 0xF0);

                let tile_size_words = BitsPerPixel::OBJ.tile_size_words();
                let tile_base_addr = self.registers.obj_tile_base_address
                    + u16::from(tile_number.bit(8))
                        * (256 * tile_size_words + self.registers.obj_tile_gap_size);
                let tile_addr = ((tile_base_addr + (tile_number & 0x00FF) * tile_size_words)
                    & VRAM_ADDRESS_MASK) as usize;

                let tile_data = &self.vram[tile_addr..tile_addr + tile_size_words as usize];

                let tile_row: u16 = (sprite_line % 8).into();

                let mut colors = [0_u8; 8];
                for tile_col in 0..8 {
                    let bit_index = (7 - tile_col) as u8;

                    let mut color = 0_u8;
                    for i in 0..2 {
                        let tile_word = tile_data[(tile_row + 8 * i) as usize];
                        color |= u8::from(tile_word.bit(bit_index)) << (2 * i);
                        color |= u8::from(tile_word.bit(bit_index + 8)) << (2 * i + 1);
                    }

                    colors[if x_flip { 7 - tile_col } else { tile_col }] = color;
                }

                self.sprites.fetched_tiles.push(if !self.registers.forced_blanking {
                    SpriteTileData { x, palette, priority, colors }
                } else {
                    SpriteTileData { x: 0, palette: 0, priority: 0, colors: [0; 8] }
                });
                self.sprites.last_fetched_oam_idx = oam_idx;

                tile_idx += 1;
                tiles_fetched += 1;
            }

            if tile_idx == num_sprite_tiles {
                if oam_buffer_idx == 0 {
                    // Fetched all sprite tiles
                    return SpriteState::Idle { oam_idx: self.sprites.last_fetched_oam_idx };
                }

                oam_buffer_idx -= 1;
                tile_idx = 0;
            }
        }

        if dot >= SPRITE_FETCH_END_DOT {
            SpriteState::Idle { oam_idx: self.sprites.last_fetched_oam_idx }
        } else {
            SpriteState::TileFetch { oam_buffer_idx, tile_idx }
        }
    }

    pub(super) fn render_sprite_tiles(&mut self) {
        if self.vblank_flag()
            || !(self.registers.main_obj_enabled || self.registers.sub_obj_enabled)
        {
            return;
        }

        self.buffers.obj_pixels.fill(Pixel::TRANSPARENT);
        for tile in &self.sprites.fetched_tiles {
            for dx in 0..8 {
                let x = (tile.x + dx) & 0x1FF;
                if x >= 256 {
                    continue;
                }

                let pixel_color = tile.colors[dx as usize];
                if pixel_color == 0 {
                    // Transparent
                    continue;
                }

                self.buffers.obj_pixels[x as usize] =
                    Pixel { palette: tile.palette, color: pixel_color, priority: tile.priority };
            }
        }
    }

    pub(super) fn progress_for_mid_scanline_write(&mut self, dot: u16) {
        log::debug!(
            "Progressing sprite state to dot {dot} on line {} for active display OAMDATA/INIDISP write",
            self.sprites.line
        );

        match self.sprites.state {
            SpriteState::Evaluation { .. } => {
                self.progress_sprite_evaluation(dot);
            }
            SpriteState::TileFetch { .. } => {
                self.progress_sprite_tile_fetch(dot);
            }
            _ => {}
        }
    }

    pub(super) fn sprites_forced_blanking_change(&mut self, new_forced_blanking: bool) {
        if self.vblank_flag() {
            // Changing forced blanking during VBlank has no effect on sprite state
            return;
        }

        log::debug!(
            "Handling sprite state update for mid-scanline forced blanking change to {new_forced_blanking} on line {}",
            self.sprites.line
        );

        let dot = (self.state.scanline_master_cycles / 4) as u16;
        self.progress_for_mid_scanline_write(dot);
        self.sprites.dot = dot;

        if new_forced_blanking {
            // Reset OAM address based on current sprite evaluation/fetching state
            self.registers.oam_address =
                (self.sprites.state.oam_idx(&self.sprites.scanned_oam_idxs) << 1).into();
            if !matches!(self.sprites.state, SpriteState::TileFetch { .. }) {
                self.sprites.state = SpriteState::Blank;
            }
            return;
        }

        let oam_idx = ((self.registers.oam_address >> 1) & 0x7F) as u8;
        match dot {
            0..=255 => {
                // Disabling forced blanking at H<256 causes the PPU to resume sprite evaluation from
                // the current OAM address
                self.sprites.state = SpriteState::Evaluation { oam_idx };
            }
            270..=u16::MAX => {
                // Fetching tiles; don't update state
            }
            _ => {
                self.sprites.state = SpriteState::Idle { oam_idx };
            }
        }
    }
}
