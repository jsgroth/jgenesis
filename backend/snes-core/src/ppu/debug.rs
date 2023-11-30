use crate::ppu;
use crate::ppu::registers::{BgMode, BitsPerPixel, Registers, TileSize};
use crate::ppu::Ppu;
use jgenesis_common::frontend::Color;
use jgenesis_common::num::GetBit;

impl Ppu {
    pub fn copy_cgram(&self, out: &mut [Color]) {
        for (out_color, &cgram_color) in out.iter_mut().zip(self.cgram.as_ref()) {
            *out_color = ppu::convert_snes_color(cgram_color, ppu::MAX_BRIGHTNESS);
        }
    }

    #[allow(clippy::needless_range_loop)]
    pub fn copy_vram_2bpp(&self, out: &mut [Color], palette: u8, row_len: usize) {
        let mut registers = Registers::new();
        registers.bg_mode = BgMode::One;
        registers.bg_tile_size[0] = TileSize::Small;
        registers.bg_tile_base_address[0] = 0;

        for tile_number in 0..4096 {
            let tile = ppu::get_bg_tile(
                &self.vram,
                &registers,
                0,
                0,
                0,
                BitsPerPixel::Two,
                tile_number as u16,
                false,
                false,
            );
            let out_tile_idx = tile_number / row_len * row_len * 64 + (tile_number % row_len) * 8;

            for row in 0..8 {
                for col in 0..8 {
                    let out_idx = out_tile_idx + row * row_len * 8 + col;

                    let col_idx = 7 - col as u8;
                    let mut snes_color = 0;
                    snes_color |= u8::from(tile[row].bit(col_idx));
                    snes_color |= u8::from(tile[row].bit(col_idx + 8)) << 1;

                    let cgram_idx = (palette << 2) | snes_color;
                    let color = if snes_color != 0 { self.cgram[cgram_idx as usize] } else { 0 };
                    out[out_idx] = ppu::convert_snes_color(color, ppu::MAX_BRIGHTNESS);
                }
            }
        }
    }

    pub fn copy_vram_4bpp(&self, out: &mut [Color], palette: u8, row_len: usize) {
        let mut registers = Registers::new();
        registers.bg_mode = BgMode::One;
        registers.bg_tile_size[0] = TileSize::Small;
        registers.bg_tile_base_address[0] = 0;

        for tile_number in 0..2048 {
            let tile = ppu::get_bg_tile(
                &self.vram,
                &registers,
                0,
                0,
                0,
                BitsPerPixel::Four,
                tile_number as u16,
                false,
                false,
            );
            let out_tile_idx = tile_number / row_len * row_len * 64 + (tile_number % row_len) * 8;

            for row in 0..8 {
                for col in 0..8 {
                    let out_idx = out_tile_idx + row * row_len * 8 + col;

                    let col_idx = 7 - col as u8;
                    let mut snes_color = 0;
                    snes_color |= u8::from(tile[row].bit(col_idx));
                    snes_color |= u8::from(tile[row].bit(col_idx + 8)) << 1;
                    snes_color |= u8::from(tile[row + 8].bit(col_idx)) << 2;
                    snes_color |= u8::from(tile[row + 8].bit(col_idx + 8)) << 3;

                    let cgram_idx = (palette << 4) | snes_color;
                    let color = if snes_color != 0 { self.cgram[cgram_idx as usize] } else { 0 };
                    out[out_idx] = ppu::convert_snes_color(color, ppu::MAX_BRIGHTNESS);
                }
            }
        }
    }

    pub fn copy_vram_8bpp(&self, out: &mut [Color], row_len: usize) {
        let mut registers = Registers::new();
        registers.bg_mode = BgMode::One;
        registers.bg_tile_size[0] = TileSize::Small;
        registers.bg_tile_base_address[0] = 0;

        for tile_number in 0..1024 {
            let tile = ppu::get_bg_tile(
                &self.vram,
                &registers,
                0,
                0,
                0,
                BitsPerPixel::Eight,
                tile_number as u16,
                false,
                false,
            );
            let out_tile_idx = tile_number / row_len * row_len * 64 + (tile_number % row_len) * 8;

            for row in 0..8 {
                for col in 0..8 {
                    let out_idx = out_tile_idx + row * row_len * 8 + col;

                    let col_idx = 7 - col as u8;
                    let mut snes_color = 0;
                    snes_color |= u8::from(tile[row].bit(col_idx));
                    snes_color |= u8::from(tile[row].bit(col_idx + 8)) << 1;
                    snes_color |= u8::from(tile[row + 8].bit(col_idx)) << 2;
                    snes_color |= u8::from(tile[row + 8].bit(col_idx + 8)) << 3;
                    snes_color |= u8::from(tile[row + 16].bit(col_idx)) << 4;
                    snes_color |= u8::from(tile[row + 16].bit(col_idx + 8)) << 5;
                    snes_color |= u8::from(tile[row + 24].bit(col_idx)) << 6;
                    snes_color |= u8::from(tile[row + 24].bit(col_idx + 8)) << 7;

                    let color = self.cgram[snes_color as usize];
                    out[out_idx] = ppu::convert_snes_color(color, ppu::MAX_BRIGHTNESS);
                }
            }
        }
    }

    pub fn copy_vram_mode7(&self, out: &mut [Color], row_len: usize) {
        for tile_number in 0..256 {
            let out_tile_idx = tile_number / row_len * row_len * 64 + (tile_number % row_len) * 8;

            for row in 0..8 {
                for col in 0..8 {
                    let out_idx = out_tile_idx + row * row_len * 8 + col;

                    let vram_addr = tile_number * 64 + row * 8 + col;
                    let snes_color = self.vram[vram_addr] >> 8;
                    let color = self.cgram[snes_color as usize];
                    out[out_idx] = ppu::convert_snes_color(color, ppu::MAX_BRIGHTNESS);
                }
            }
        }
    }
}
