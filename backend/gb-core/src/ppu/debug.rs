use crate::api::BackgroundTileMap;
use crate::ppu::fifo::BgTileAttributes;
use crate::ppu::registers::{CgbPaletteRam, TileDataArea};
use crate::ppu::{registers, Ppu};
use crate::{graphics, HardwareMode};
use jgenesis_common::frontend::Color;
use jgenesis_common::num::GetBit;

impl Ppu {
    pub fn is_using_double_height_sprites(&self) -> bool {
        self.registers.double_height_sprites
    }

    pub fn copy_background(&self, tile_map: BackgroundTileMap, out: &mut [Color]) {
        let tile_map_addr = match tile_map {
            BackgroundTileMap::Zero => registers::TILE_MAP_AREA_0,
            BackgroundTileMap::One => registers::TILE_MAP_AREA_1,
        };

        let tile_data_area = self.registers.bg_tile_data_area;

        for tile_map_row in 0..32 {
            for tile_map_col in 0..32 {
                let tile_number =
                    self.vram[(tile_map_addr | (tile_map_row * 32 + tile_map_col)) as usize];

                let attributes = match self.hardware_mode {
                    HardwareMode::Dmg => 0x00,
                    HardwareMode::Cgb => {
                        self.vram
                            [(0x2000 | tile_map_addr | (tile_map_row * 32 + tile_map_col)) as usize]
                    }
                };
                let attributes: BgTileAttributes = attributes.into();

                let vram_bank_bit = u16::from(attributes.vram_bank) << 13;
                let tile_addr = (vram_bank_bit | tile_data_area.tile_address(tile_number)) as usize;
                let tile_data = &self.vram[tile_addr..tile_addr + 16];

                for tile_row in 0..8 {
                    let pixel_addr =
                        if attributes.vertical_flip { 2 * (7 - tile_row) } else { 2 * tile_row };

                    let pixel_row_lsb = tile_data[pixel_addr as usize];
                    let pixel_row_msb = tile_data[(pixel_addr + 1) as usize];

                    for tile_col in 0_u16..8 {
                        let pixel_idx =
                            (if attributes.horizontal_flip { tile_col } else { 7 - tile_col })
                                as u8;
                        let color_id = u8::from(pixel_row_lsb.bit(pixel_idx))
                            | (u8::from(pixel_row_msb.bit(pixel_idx)) << 1);

                        let color = match self.hardware_mode {
                            HardwareMode::Dmg => {
                                resolve_dmg_color(self.registers.bg_palette[color_id as usize])
                            }
                            HardwareMode::Cgb => resolve_cgb_color(
                                &self.bg_palette_ram,
                                attributes.palette,
                                color_id,
                            ),
                        };

                        let out_idx =
                            tile_map_row * 8 * 256 + tile_row * 256 + tile_map_col * 8 + tile_col;
                        out[out_idx as usize] = color;
                    }
                }
            }
        }
    }

    pub fn copy_sprites(&self, out: &mut [Color]) {
        let double_height_sprites = self.registers.double_height_sprites;
        let sprite_height = if double_height_sprites { 16 } else { 8 };

        for oam_idx in 0..40 {
            let oam_addr = 4 * oam_idx;

            let mut tile_number = self.oam[(oam_addr + 2) as usize];
            let attributes = self.oam[(oam_addr + 3) as usize];

            if double_height_sprites {
                tile_number &= !1;
            }

            let vertical_flip = attributes.bit(6);
            let horizontal_flip = attributes.bit(5);
            let dmg_palette: u8 = attributes.bit(4).into();
            let vram_bank = match self.hardware_mode {
                HardwareMode::Dmg => 0,
                HardwareMode::Cgb => u16::from(attributes.bit(3)),
            };
            let cgb_palette = attributes & 0x07;

            let vram_bank_bit = vram_bank << 13;
            let tile_addr = vram_bank_bit | TileDataArea::One.tile_address(tile_number);

            for tile_row in 0..sprite_height {
                let row_addr = if vertical_flip {
                    tile_addr + 2 * (sprite_height - 1 - tile_row)
                } else {
                    tile_addr + 2 * tile_row
                };

                let tile_data_lsb = self.vram[row_addr as usize];
                let tile_data_msb = self.vram[(row_addr + 1) as usize];

                for tile_col in 0_u16..8 {
                    let pixel_idx = (if horizontal_flip { tile_col } else { 7 - tile_col }) as u8;
                    let color_id = u8::from(tile_data_lsb.bit(pixel_idx))
                        | (u8::from(tile_data_msb.bit(pixel_idx)) << 1);

                    let color = match self.hardware_mode {
                        HardwareMode::Dmg => resolve_dmg_color(
                            self.registers.sprite_palettes[dmg_palette as usize][color_id as usize],
                        ),
                        HardwareMode::Cgb => {
                            resolve_cgb_color(&self.sprite_palette_ram, cgb_palette, color_id)
                        }
                    };

                    let out_idx = (oam_idx / 8) * sprite_height * 8 * 8
                        + tile_row * 8 * 8
                        + (oam_idx % 8) * 8
                        + tile_col;
                    out[out_idx as usize] = color;
                }
            }
        }
    }

    pub fn copy_palettes(&self, out: &mut [Color]) {
        match self.hardware_mode {
            HardwareMode::Dmg => self.copy_palettes_dmg(out),
            HardwareMode::Cgb => self.copy_palettes_cgb(out),
        }
    }

    fn copy_palettes_dmg(&self, out: &mut [Color]) {
        for (bg_color_id, dmg_color) in self.registers.bg_palette.into_iter().enumerate() {
            let color = resolve_dmg_color(dmg_color);
            out[bg_color_id] = color;
        }

        for (palette_id, sprite_palette) in self.registers.sprite_palettes.into_iter().enumerate() {
            for (sprite_color_id, dmg_color) in sprite_palette.into_iter().enumerate() {
                let color = resolve_dmg_color(dmg_color);
                out[4 * (palette_id + 1) + sprite_color_id] = color;
            }
        }
    }

    fn copy_palettes_cgb(&self, out: &mut [Color]) {
        for bg_palette in 0..8 {
            for bg_color_id in 0..4 {
                out[(4 * bg_palette + bg_color_id) as usize] =
                    resolve_cgb_color(&self.bg_palette_ram, bg_palette, bg_color_id);
            }
        }

        for obj_palette in 0..8 {
            for obj_color_id in 0..4 {
                out[(32 + 4 * obj_palette + obj_color_id) as usize] =
                    resolve_cgb_color(&self.sprite_palette_ram, obj_palette, obj_color_id);
            }
        }
    }
}

fn resolve_dmg_color(dmg_color: u8) -> Color {
    let [r, g, b] = graphics::GB_COLOR_TO_RGB_GREEN_TINT[dmg_color as usize];
    Color::rgb(r, g, b)
}

fn resolve_cgb_color(palette_ram: &CgbPaletteRam, palette: u8, color_id: u8) -> Color {
    let color = u16::from_le_bytes([
        palette_ram[(2 * (4 * palette + color_id)) as usize],
        palette_ram[(2 * (4 * palette + color_id) + 1) as usize],
    ]);
    let (r, g, b) = graphics::parse_cgb_color(color);
    Color::rgb(
        graphics::RGB_5_TO_8[r as usize],
        graphics::RGB_5_TO_8[g as usize],
        graphics::RGB_5_TO_8[b as usize],
    )
}
