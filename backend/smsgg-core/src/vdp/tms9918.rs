use crate::vdp::{Mode, Vdp};
use crate::{SmsGgHardware, vdp};
use jgenesis_common::frontend::Color;
use jgenesis_common::num::GetBit;
use tinyvec::ArrayVec;

const MAX_SPRITES_PER_LINE: usize = 4;

// From https://www.smspower.org/forums/8224-TMS9918ColorsForSMSVDP
// Used for TMS9918 modes on SMS
pub const TMS9918_COLOR_TO_SMS_COLOR: &[u8; 16] = &[
    0x00, // Transparent (Black)
    0x00, // Black
    0x08, // Medium green
    0x0C, // Light green
    0x10, // Dark blue
    0x30, // Light blue
    0x01, // Dark red
    0x3C, // Cyan
    0x02, // Medium red
    0x03, // Light red
    0x05, // Dark yellow
    0x0F, // Light yellow
    0x04, // Dark green
    0x33, // Magenta
    0x15, // Gray
    0x3F, // White
];

// From https://www.smspower.org/Development/Palette&num=2#SG1000SC3000
// Used for SG-1000
pub const TMS9918_COLOR_TO_RGB8: &[Color; 16] = &[
    Color::rgb(0x00, 0x00, 0x00), // Transparent
    Color::rgb(0x00, 0x00, 0x00), // Black
    Color::rgb(0x21, 0xC8, 0x42), // Medium green
    Color::rgb(0x5E, 0xDC, 0x78), // Light green
    Color::rgb(0x54, 0x55, 0xED), // Dark blue
    Color::rgb(0x7D, 0x76, 0xFC), // Light blue
    Color::rgb(0xD4, 0x52, 0x4D), // Dark red
    Color::rgb(0x42, 0xEB, 0xF5), // Cyan
    Color::rgb(0xFC, 0x55, 0x54), // Medium red
    Color::rgb(0xFF, 0x79, 0x78), // Light red
    Color::rgb(0xD4, 0xC1, 0x54), // Dark yellow
    Color::rgb(0xE6, 0xCE, 0x80), // Light yellow
    Color::rgb(0x21, 0xB0, 0x3B), // Dark green
    Color::rgb(0xC9, 0x5B, 0xBA), // Magenta
    Color::rgb(0xCC, 0xCC, 0xCC), // Gray
    Color::rgb(0xFF, 0xFF, 0xFF), // White
];

pub const TMS9918_NOOP_LOOKUP: &[u8; 16] = &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

pub fn color_table(hardware: SmsGgHardware) -> &'static [u8; 16] {
    match hardware {
        SmsGgHardware::MasterSystem | SmsGgHardware::GameGear => TMS9918_COLOR_TO_SMS_COLOR,
        SmsGgHardware::Sg1000 => TMS9918_NOOP_LOOKUP, // VDP-to-RGB8 code will convert to actual color
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Graphics2SpriteData {
    y: u8,
    x: u8,
    name: u8,
    color: u8,
    early_clock: bool,
}

impl Vdp {
    pub(super) fn render_text_scanline(&mut self, scanline: u16) {
        let tms9918_color_table = color_table(self.registers.version.hardware());

        let frame_buffer_row = self.frame_buffer_row(scanline);

        let text_colors = [
            tms9918_color_table[self.registers.backdrop_color as usize],
            tms9918_color_table[self.registers.text_mode_color_1 as usize],
        ];

        let base_name_table_addr = self.registers.base_name_table_address;
        let base_pattern_generator = self.registers.pattern_generator_address;

        let nametable_row = scanline / 8;
        let line_name_table_addr = base_name_table_addr + nametable_row * 40;

        let pattern_y_offset = scanline % 8;

        for text_pattern in 0..40 {
            let name_table_byte = self.vram[(line_name_table_addr + text_pattern) as usize];

            let pattern_generator_addr =
                base_pattern_generator | (8 * u16::from(name_table_byte)) | pattern_y_offset;
            let pattern_byte = self.vram[pattern_generator_addr as usize];

            for pattern_y in 0..6 {
                let pixel = 6 * text_pattern + u16::from(pattern_y);
                let pixel_color = text_colors[usize::from(pattern_byte.bit(7 - pattern_y))];
                self.frame_buffer.set(frame_buffer_row, pixel, pixel_color.into());
            }
        }

        for pixel in 40 * 6..vdp::SCREEN_WIDTH {
            self.frame_buffer.set(frame_buffer_row, pixel, text_colors[0].into());
        }
    }

    pub(super) fn render_graphics_12_scanline(&mut self, scanline: u16, mode: Mode) {
        let graphics2 = mode == Mode::GraphicsII;

        let tms9918_color_table = color_table(self.registers.version.hardware());

        let frame_buffer_row = self.frame_buffer_row(scanline);
        let backdrop_color = tms9918_color_table[self.registers.backdrop_color as usize];

        let base_name_table_addr = self.registers.base_name_table_address;
        let base_color_table_addr = if graphics2 {
            self.registers.color_table_address & 0x2000
        } else {
            self.registers.color_table_address
        };
        let base_pattern_generator = if graphics2 {
            self.registers.pattern_generator_address & 0x2000
        } else {
            self.registers.pattern_generator_address
        };

        let nametable_row = scanline / 8;
        let line_name_table_addr = base_name_table_addr | (nametable_row * 32);

        // In Graphics II mode, pattern generator and color table are split into 3 blocks of 2048
        // bytes each: one for the first 8 rows, one for the middle 8 rows, and one for the last 8 rows
        let table_offset = if !graphics2 {
            0
        } else if nametable_row >= 16 {
            4096
        } else if nametable_row >= 8 {
            2048
        } else {
            0
        };

        let tile_row = scanline % 8;

        // Scan for sprites on this line
        let sprite_buffer = self.find_sprites_on_line(scanline as u8);

        for nametable_col in 0..vdp::SCREEN_WIDTH / 8 {
            let name_table_entry = self.vram[(line_name_table_addr | nametable_col) as usize];

            let pattern_generator_addr =
                base_pattern_generator + table_offset + 8 * u16::from(name_table_entry) + tile_row;
            let pattern_generator_entry = self.vram[pattern_generator_addr as usize];

            let color_table_addr = if graphics2 {
                base_color_table_addr + table_offset + 8 * u16::from(name_table_entry) + tile_row
            } else {
                base_color_table_addr + u16::from(name_table_entry / 8)
            };
            let color_table_entry = self.vram[color_table_addr as usize];
            let bg_color_0 = color_table_entry & 0x0F;
            let bg_color_1 = color_table_entry >> 4;

            for tile_col in 0..8 {
                let pixel = 8 * nametable_col + u16::from(tile_col);

                let sprite_color =
                    self.resolve_tms9918_sprite_color(&sprite_buffer, scanline, pixel);

                let bg_color =
                    if pattern_generator_entry.bit(7 - tile_col) { bg_color_1 } else { bg_color_0 };

                let pixel_color = if sprite_color != 0 {
                    tms9918_color_table[sprite_color as usize]
                } else if bg_color != 0 {
                    tms9918_color_table[bg_color as usize]
                } else {
                    backdrop_color
                };
                self.frame_buffer.set(frame_buffer_row, pixel, pixel_color.into());
            }
        }
    }

    pub(super) fn render_multicolor_scanline(&mut self, scanline: u16) {
        let tms9918_color_table = color_table(self.registers.version.hardware());

        let backdrop_color = tms9918_color_table[self.registers.backdrop_color as usize];
        let frame_buffer_row = self.frame_buffer_row(scanline);

        let base_name_table_addr = self.registers.base_name_table_address;

        let nametable_row = scanline / 8;
        let line_name_table_addr = base_name_table_addr | (nametable_row * 32);

        let base_pattern_generator_addr = self.registers.pattern_generator_address;
        let pattern_y_offset = (scanline / 4) % 8;

        let sprite_buffer = self.find_sprites_on_line(scanline as u8);

        for nametable_col in 0..vdp::SCREEN_WIDTH / 8 {
            let name_table_entry = self.vram[(line_name_table_addr | nametable_col) as usize];

            let pattern_generator_addr =
                base_pattern_generator_addr | (8 * u16::from(name_table_entry));
            let pattern_byte = self.vram[(pattern_generator_addr | pattern_y_offset) as usize];

            let first_color = pattern_byte >> 4;
            let second_color = pattern_byte & 0xF;

            for x in 0..8 {
                let pixel = 8 * nametable_col + x;
                let sprite_color =
                    self.resolve_tms9918_sprite_color(&sprite_buffer, scanline, pixel);

                let pixel_color = if sprite_color != 0 {
                    tms9918_color_table[sprite_color as usize]
                } else if x < 4 && first_color != 0 {
                    tms9918_color_table[first_color as usize]
                } else if x >= 4 && second_color != 0 {
                    tms9918_color_table[second_color as usize]
                } else {
                    backdrop_color
                };

                self.frame_buffer.set(frame_buffer_row, pixel, pixel_color.into());
            }
        }
    }

    fn find_sprites_on_line(
        &mut self,
        scanline: u8,
    ) -> ArrayVec<[Graphics2SpriteData; MAX_SPRITES_PER_LINE]> {
        let base_sprite_table_addr = self.registers.latched_sprite.base_sprite_table_address;

        let large_sprites = self.registers.latched_sprite.double_sprite_height;
        let magnify_sprites = self.registers.latched_sprite.double_sprite_size;
        let sprite_size = 8 << (u8::from(large_sprites) + u8::from(magnify_sprites));

        let mut sprite_buffer = ArrayVec::new();
        for sprite_idx in 0..32 {
            let sprite_table_addr = base_sprite_table_addr + 4 * sprite_idx;
            let y = self.vram[sprite_table_addr as usize];

            if y == 0xD0 {
                // Termination signal
                break;
            }

            // Sprites can wrap from below the bottom of the screen to the top
            let sprite_bottom = y.wrapping_add(sprite_size);
            let sprite_in_y_range = if y < sprite_bottom {
                (y..sprite_bottom).contains(&scanline)
            } else {
                scanline >= y || scanline < sprite_bottom
            };
            if !sprite_in_y_range {
                continue;
            }

            if sprite_buffer.len() == sprite_buffer.capacity() {
                self.registers.sprite_overflow = true;
                self.registers.tms9918_5th_sprite = sprite_idx as u8;
                break;
            }

            let x = self.vram[(sprite_table_addr + 1) as usize];
            let name = self.vram[(sprite_table_addr + 2) as usize];
            let attributes = self.vram[(sprite_table_addr + 3) as usize];
            let color = attributes & 0x0F;
            let early_clock = attributes.bit(7);

            if color == 0 {
                // Transparent
                continue;
            }

            sprite_buffer.push(Graphics2SpriteData { y, x, name, color, early_clock });
        }

        sprite_buffer
    }

    fn resolve_tms9918_sprite_color(
        &mut self,
        sprite_buffer: &[Graphics2SpriteData],
        scanline: u16,
        pixel: u16,
    ) -> u8 {
        let large_sprites = self.registers.latched_sprite.double_sprite_height;
        let magnify_sprites = self.registers.latched_sprite.double_sprite_size;
        let sprite_size = 8 << (u8::from(large_sprites) + u8::from(magnify_sprites));

        let mut found_color: Option<u8> = None;

        for &sprite in sprite_buffer {
            let sprite_x =
                if sprite.early_clock { i16::from(sprite.x) - 32 } else { i16::from(sprite.x) };

            let sprite_right = sprite_x + sprite_size;
            if !(sprite_x..sprite_right).contains(&(pixel as i16)) {
                continue;
            }

            let mut sprite_row = (scanline as u8).wrapping_sub(sprite.y);
            let mut sprite_col = (pixel as i16 - sprite_x) as u8;
            if magnify_sprites {
                // Magnifying sprites simply blows up the sprite to 2x size in each dimension
                sprite_row >>= 1;
                sprite_col >>= 1;
            }

            // Mask out the lowest 2 bits of sprite name when using 16x16 sprites
            let sprite_name_mask = if large_sprites { !0x03 } else { !0x00 };
            let mut sprite_pattern_addr = self.registers.latched_sprite.base_sprite_pattern_address
                + 8 * u16::from(sprite.name & sprite_name_mask)
                + u16::from(sprite_row % 8);
            if sprite_row >= 8 {
                sprite_pattern_addr += 8;
            }
            if sprite_col >= 8 {
                sprite_pattern_addr += 16;
            }

            let sprite_pattern = self.vram[sprite_pattern_addr as usize];

            if !sprite_pattern.bit(7 - (sprite_col % 8)) {
                continue;
            }

            if let Some(found_color) = found_color {
                self.registers.sprite_collision = true;
                return found_color;
            }

            found_color = Some(sprite.color);
        }

        found_color.unwrap_or(0)
    }
}
