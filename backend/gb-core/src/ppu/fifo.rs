use crate::ppu::registers::{CgbPaletteRam, Registers, TileDataArea};
use crate::ppu::{PpuFrameBuffer, SpriteData, Vram, MAX_SPRITES_PER_LINE, SCREEN_WIDTH};
use crate::HardwareMode;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::array;
use std::collections::VecDeque;

const MAX_FIFO_X: u8 = SCREEN_WIDTH as u8 + 8;

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct BgPixel {
    color: u8,
    palette: u8,
    high_priority: bool,
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct SpritePixel {
    color: u8,
    palette: u8,
    low_priority: bool,
    oam_index: u8,
}

impl SpritePixel {
    const TRANSPARENT: Self = Self { color: 0, palette: 0, low_priority: true, oam_index: 255 };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum BgLayer {
    Background,
    Window { window_line: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
struct RenderingBgTileFields {
    bg_layer: BgLayer,
    dots_remaining: u8,
    screen_x: u8,
    fetcher_x: u8,
    // Whether or not a sprite fetch was delayed by a BG fetch in the current BG tile
    sprite_fetch_delayed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum FifoState {
    // Fetching the offscreen first tile
    InitialBgFetch { dots_remaining: u8 },
    // Rendering a background tile
    RenderingBgTile(RenderingBgTileFields),
    // Fetching a sprite tile
    SpriteFetch { dots_remaining: u8, previous_bg_fields: RenderingBgTileFields },
    // Fetching the first window tile
    InitialWindowFetch { dots_remaining: u8, screen_x: u8, window_line: u8 },
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct PixelFifo {
    hardware_mode: HardwareMode,
    bg: VecDeque<BgPixel>,
    sprites: VecDeque<SpritePixel>,
    y: u8,
    window_y_triggered: bool,
    window_line_counter: u8,
    scanned_sprites: VecDeque<SpriteData>,
    state: FifoState,
}

impl PixelFifo {
    pub fn new(hardware_mode: HardwareMode) -> Self {
        Self {
            hardware_mode,
            bg: VecDeque::with_capacity(16),
            sprites: VecDeque::with_capacity(16),
            y: 0,
            window_y_triggered: false,
            window_line_counter: 0,
            scanned_sprites: VecDeque::with_capacity(MAX_SPRITES_PER_LINE),
            state: FifoState::InitialBgFetch { dots_remaining: 6 },
        }
    }

    pub fn reset_window_state(&mut self) {
        self.window_y_triggered = false;
        self.window_line_counter = 0;
    }

    pub fn start_new_line(&mut self, scanline: u8, registers: &Registers, sprites: &[SpriteData]) {
        self.bg.clear();
        self.sprites.clear();
        self.y = scanline;

        // Intentionally don't check whether the window is enabled here; doing so breaks certain test ROMs (e.g. fairylake.gb)
        if registers.window_y == scanline {
            self.window_y_triggered = true;
        }

        self.scanned_sprites.clear();
        self.scanned_sprites.extend(sprites);

        // Initial BG tile fetch always takes 6 cycles
        self.state = FifoState::InitialBgFetch { dots_remaining: 6 };

        // TODO check WY here
    }

    pub fn tick(
        &mut self,
        vram: &Vram,
        registers: &Registers,
        bg_palette_ram: &CgbPaletteRam,
        sprite_palette_ram: &CgbPaletteRam,
        frame_buffer: &mut PpuFrameBuffer,
    ) {
        match self.state {
            FifoState::InitialBgFetch { dots_remaining } => {
                self.handle_initial_bg_fetch(dots_remaining, vram, registers);
            }
            FifoState::RenderingBgTile(fields) => {
                self.handle_rendering_bg_tile(
                    fields,
                    vram,
                    registers,
                    bg_palette_ram,
                    sprite_palette_ram,
                    frame_buffer,
                );
            }
            FifoState::SpriteFetch { dots_remaining, previous_bg_fields } => {
                self.handle_sprite_fetch(dots_remaining, previous_bg_fields);
            }
            FifoState::InitialWindowFetch { dots_remaining, screen_x, window_line } => {
                self.handle_initial_window_fetch(dots_remaining, screen_x, window_line);
            }
        }
    }

    fn handle_initial_bg_fetch(&mut self, dots_remaining: u8, vram: &Vram, registers: &Registers) {
        if dots_remaining == 6 {
            // Do the initial tile fetch
            let bg_tile_row = fetch_bg_tile(self.hardware_mode, 0, self.y, vram, registers);
            for color in bg_tile_row.pixels {
                self.bg.push_back(BgPixel {
                    color,
                    palette: bg_tile_row.palette,
                    high_priority: bg_tile_row.high_priority,
                });
            }
        }

        if dots_remaining != 1 {
            self.state = FifoState::InitialBgFetch { dots_remaining: dots_remaining - 1 };
            return;
        }

        // TODO check if WX=0 here

        // The first 8 pixels are always discarded, and (SCX % 8) additional pixels are discarded to handle fine X scrolling
        // Simulate this by using fine X scroll to move the screen position backwards
        let fine_x_scroll = registers.bg_x_scroll % 8;
        self.state = FifoState::RenderingBgTile(RenderingBgTileFields {
            bg_layer: BgLayer::Background,
            dots_remaining: 8,
            screen_x: 0_u8.wrapping_sub(fine_x_scroll),
            fetcher_x: 0,
            sprite_fetch_delayed: false,
        });
    }

    fn handle_rendering_bg_tile(
        &mut self,
        mut fields: RenderingBgTileFields,
        vram: &Vram,
        registers: &Registers,
        bg_palette_ram: &CgbPaletteRam,
        sprite_palette_ram: &CgbPaletteRam,
        frame_buffer: &mut PpuFrameBuffer,
    ) {
        if self.scanned_sprites.front().is_some_and(|sprite| sprite.x == fields.screen_x) {
            // A sprite starts on this position. Go ahead and do the actual tile fetch immediately
            let sprite = self.scanned_sprites.pop_front().unwrap();

            // TODO GBC always fetches sprite tiles even when sprites are disabled
            if registers.sprites_enabled {
                self.fetch_next_sprite_tile(sprite, vram, registers);

                // Sprite fetches take at minimum 6 cycles, and the fetch may be delayed by an additional 1-5 cycles if it
                // needs to wait for a BG fetch to finish
                let sprite_fetch_cycles =
                    if !fields.sprite_fetch_delayed && (4..9).contains(&fields.dots_remaining) {
                        fields.sprite_fetch_delayed = true;
                        6 + fields.dots_remaining - 3
                    } else {
                        6
                    };

                log::trace!(
                    "Sprite encountered at X {}, delaying by {sprite_fetch_cycles} cycles",
                    fields.screen_x
                );

                // Subtract 1 to account for the current tick
                self.state = FifoState::SpriteFetch {
                    dots_remaining: sprite_fetch_cycles - 1,
                    previous_bg_fields: fields,
                };

                return;
            }

            // Sprites are disabled; pop all sprites at the current position
            while self.scanned_sprites.front().is_some_and(|sprite| sprite.x == fields.screen_x) {
                self.scanned_sprites.pop_front();
            }
        }

        if self.window_y_triggered
            && registers.window_enabled
            && registers.window_x.saturating_add(1) == fields.screen_x
            && fields.bg_layer == BgLayer::Background
        {
            log::trace!("Window started at X {}", fields.screen_x);

            // Window triggered; clear the BG FIFO and fetch window tile 0
            self.bg.clear();
            let first_window_tile =
                fetch_window_tile(self.hardware_mode, 0, self.window_line_counter, vram, registers);
            self.push_bg_tile_row(first_window_tile);

            self.state = FifoState::InitialWindowFetch {
                // Wait for 5 cycles instead of 6 to account for the current tick
                dots_remaining: 5,
                screen_x: fields.screen_x,
                window_line: self.window_line_counter,
            };
            self.window_line_counter += 1;

            return;
        }

        if fields.dots_remaining == 8 {
            log::trace!("Fetching BG tile at X {}", fields.screen_x);

            // Fetch next BG/window tile
            match fields.bg_layer {
                BgLayer::Background => {
                    let bg_tile_row = fetch_bg_tile(
                        self.hardware_mode,
                        fields.fetcher_x,
                        self.y,
                        vram,
                        registers,
                    );
                    self.push_bg_tile_row(bg_tile_row);
                }
                BgLayer::Window { window_line } => {
                    let window_tile_row = fetch_window_tile(
                        self.hardware_mode,
                        fields.fetcher_x,
                        window_line,
                        vram,
                        registers,
                    );
                    self.push_bg_tile_row(window_tile_row);
                }
            }

            fields.fetcher_x = fields.fetcher_x.wrapping_add(1);
        }

        let bg_pixel = self.bg.pop_front().expect("BG FIFO should never be empty while rendering");

        let mut sprite_pixel = self.sprites.pop_front().unwrap_or(SpritePixel::TRANSPARENT);
        if !registers.sprites_enabled {
            sprite_pixel = SpritePixel::TRANSPARENT;
        }

        if (8..MAX_FIFO_X).contains(&fields.screen_x) {
            let color = if sprite_pixel.color != 0
                && !(bg_pixel.color != 0
                    && registers.bg_enabled
                    && (sprite_pixel.low_priority || bg_pixel.high_priority))
            {
                match self.hardware_mode {
                    HardwareMode::Dmg => registers.sprite_palettes[sprite_pixel.palette as usize]
                        [sprite_pixel.color as usize]
                        .into(),
                    HardwareMode::Cgb => {
                        sprite_palette_ram.read_color(sprite_pixel.palette, sprite_pixel.color)
                    }
                }
            } else if registers.bg_enabled || self.hardware_mode == HardwareMode::Cgb {
                match self.hardware_mode {
                    HardwareMode::Dmg => registers.bg_palette[bg_pixel.color as usize].into(),
                    HardwareMode::Cgb => {
                        bg_palette_ram.read_color(bg_pixel.palette, bg_pixel.color)
                    }
                }
            } else {
                // In DMG mode, if BG is disabled and sprite pixel is transparent, always display white
                0
            };

            frame_buffer.set(self.y, fields.screen_x - 8, color);
        }
        fields.screen_x = fields.screen_x.wrapping_add(1);

        if fields.dots_remaining == 1 {
            fields.dots_remaining = 8;
            fields.sprite_fetch_delayed = false;
        } else {
            fields.dots_remaining -= 1;
        }

        self.state = FifoState::RenderingBgTile(fields);
    }

    fn push_bg_tile_row(&mut self, bg_tile_row: BgTileRow) {
        for color in bg_tile_row.pixels {
            self.bg.push_back(BgPixel {
                color,
                palette: bg_tile_row.palette,
                high_priority: bg_tile_row.high_priority,
            });
        }
    }

    fn fetch_next_sprite_tile(&mut self, sprite: SpriteData, vram: &Vram, registers: &Registers) {
        let sprite_tile = fetch_sprite_tile(sprite, self.y, vram, registers.double_height_sprites);

        while self.sprites.len() < 8 {
            self.sprites.push_back(SpritePixel::TRANSPARENT);
        }

        for (i, color) in sprite_tile.into_iter().enumerate() {
            if color == 0 {
                // Don't add transparent pixels to the FIFO
                continue;
            }

            // Replace any transparent pixels in the FIFO
            // If in CGB mode, also replace any non-transparent pixels from sprites with a higher OAM index
            if self.sprites[i].color == 0
                || (self.hardware_mode == HardwareMode::Cgb
                    && sprite.oam_index < self.sprites[i].oam_index)
            {
                self.sprites[i] = SpritePixel {
                    color,
                    palette: sprite.palette,
                    low_priority: sprite.low_priority,
                    oam_index: sprite.oam_index,
                };
            }
        }
    }

    fn handle_sprite_fetch(
        &mut self,
        dots_remaining: u8,
        previous_bg_fields: RenderingBgTileFields,
    ) {
        self.state = if dots_remaining == 1 {
            FifoState::RenderingBgTile(previous_bg_fields)
        } else {
            FifoState::SpriteFetch { dots_remaining: dots_remaining - 1, previous_bg_fields }
        };
    }

    fn handle_initial_window_fetch(&mut self, dots_remaining: u8, screen_x: u8, window_line: u8) {
        self.state = if dots_remaining == 1 {
            FifoState::RenderingBgTile(RenderingBgTileFields {
                bg_layer: BgLayer::Window { window_line },
                dots_remaining: 8,
                screen_x,
                // Start at tile 1 since tile 0 has already been fetched
                fetcher_x: 1,
                sprite_fetch_delayed: false,
            })
        } else {
            FifoState::InitialWindowFetch {
                dots_remaining: dots_remaining - 1,
                screen_x,
                window_line,
            }
        };
    }

    pub fn done_with_line(&self) -> bool {
        match self.state {
            FifoState::InitialBgFetch { .. }
            | FifoState::SpriteFetch { .. }
            | FifoState::InitialWindowFetch { .. } => false,
            FifoState::RenderingBgTile(fields) => fields.screen_x == MAX_FIFO_X,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct BgTileAttributes {
    high_priority: bool,
    vertical_flip: bool,
    horizontal_flip: bool,
    vram_bank: u8,
    palette: u8,
}

impl From<u8> for BgTileAttributes {
    fn from(value: u8) -> Self {
        Self {
            high_priority: value.bit(7),
            vertical_flip: value.bit(6),
            horizontal_flip: value.bit(5),
            vram_bank: value.bit(3).into(),
            palette: value & 0x07,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct BgTileRow {
    pixels: [u8; 8],
    palette: u8,
    high_priority: bool,
}

fn fetch_bg_tile(
    hardware_mode: HardwareMode,
    fetcher_x: u8,
    y: u8,
    vram: &Vram,
    registers: &Registers,
) -> BgTileRow {
    if hardware_mode == HardwareMode::Dmg && !registers.bg_enabled {
        // On DMG, all BG pixels are transparent if BG is disabled
        return BgTileRow { pixels: [0; 8], palette: 0, high_priority: false };
    }

    let coarse_x_scroll = registers.bg_x_scroll / 8;
    let tile_map_x: u16 = (fetcher_x.wrapping_add(coarse_x_scroll) % 32).into();

    let bg_y: u16 = y.wrapping_add(registers.bg_y_scroll).into();
    let tile_map_y = bg_y / 8;

    let tile_map_addr = registers.bg_tile_map_addr | (tile_map_y << 5) | tile_map_x;
    let tile_number = vram[tile_map_addr as usize];

    // No need to check for CGB here; the CPU can't write to VRAM bank 1 in DMG mode, and an attributes byte of 0 is
    // equivalent to DMG functionality
    let attributes_map_addr = 0x2000 | tile_map_addr;
    let attributes = BgTileAttributes::from(vram[attributes_map_addr as usize]);

    let bank_addr = u16::from(attributes.vram_bank) << 13;
    let tile_row = if attributes.vertical_flip { 7 - (bg_y % 8) } else { bg_y % 8 };
    let tile_addr =
        bank_addr | registers.bg_tile_data_area.tile_address(tile_number) | (tile_row << 1);
    let tile_data_lsb = vram[tile_addr as usize];
    let tile_data_msb = vram[(tile_addr + 1) as usize];

    let pixels = tile_data_to_pixels(tile_data_lsb, tile_data_msb, attributes.horizontal_flip);

    BgTileRow { pixels, palette: attributes.palette, high_priority: attributes.high_priority }
}

fn fetch_window_tile(
    hardware_mode: HardwareMode,
    fetcher_x: u8,
    window_line: u8,
    vram: &Vram,
    registers: &Registers,
) -> BgTileRow {
    if (hardware_mode == HardwareMode::Dmg && !registers.bg_enabled) || !registers.window_enabled {
        // All BG pixels are transparent if BG is disabled
        return BgTileRow { pixels: [0; 8], palette: 0, high_priority: false };
    }

    let tile_map_x: u16 = fetcher_x.into();
    let tile_map_y: u16 = (window_line / 8).into();

    let tile_map_addr = registers.window_tile_map_addr | (tile_map_y << 5) | tile_map_x;
    let tile_number = vram[tile_map_addr as usize];

    let attributes_addr = 0x2000 | tile_map_addr;
    let attributes = BgTileAttributes::from(vram[attributes_addr as usize]);

    let bank_addr = u16::from(attributes.vram_bank) << 13;
    let tile_row: u16 = (window_line % 8).into();
    let tile_addr =
        bank_addr | registers.bg_tile_data_area.tile_address(tile_number) | (tile_row << 1);
    let tile_data_lsb = vram[tile_addr as usize];
    let tile_data_msb = vram[(tile_addr + 1) as usize];

    let pixels = tile_data_to_pixels(tile_data_lsb, tile_data_msb, attributes.horizontal_flip);

    BgTileRow { pixels, palette: attributes.palette, high_priority: attributes.high_priority }
}

fn fetch_sprite_tile(
    sprite: SpriteData,
    y: u8,
    vram: &Vram,
    double_height_sprites: bool,
) -> [u8; 8] {
    let sprite_row = y.wrapping_sub(sprite.y.wrapping_add(16));

    let tile_number = if double_height_sprites {
        // In double height sprite mode, the lowest bit of tile number is ignored, and which tile gets used depends
        // on sprite row and vertical flip
        let base_tile_number = sprite.tile_number & !0x01;
        let lower_tile = sprite_row.bit(3) ^ sprite.vertical_flip;
        base_tile_number | u8::from(lower_tile)
    } else {
        sprite.tile_number
    };

    let tile_row = if sprite.vertical_flip { 7 - (sprite_row & 0x07) } else { sprite_row & 0x07 };

    let bank_addr = u16::from(sprite.vram_bank) << 13;
    let tile_addr =
        bank_addr | TileDataArea::SPRITES.tile_address(tile_number) | u16::from(tile_row << 1);
    let tile_data_lsb = vram[tile_addr as usize];
    let tile_data_msb = vram[(tile_addr + 1) as usize];

    tile_data_to_pixels(tile_data_lsb, tile_data_msb, sprite.horizontal_flip)
}

fn tile_data_to_pixels(tile_data_lsb: u8, tile_data_msb: u8, horizontal_flip: bool) -> [u8; 8] {
    if horizontal_flip {
        array::from_fn(|i| {
            u8::from(tile_data_lsb.bit(i as u8)) | (u8::from(tile_data_msb.bit(i as u8)) << 1)
        })
    } else {
        array::from_fn(|i| {
            let pixel_idx = 7 - i as u8;
            u8::from(tile_data_lsb.bit(pixel_idx)) | (u8::from(tile_data_msb.bit(pixel_idx)) << 1)
        })
    }
}
