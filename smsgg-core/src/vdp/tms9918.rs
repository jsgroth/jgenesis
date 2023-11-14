use crate::vdp;
use crate::vdp::Vdp;
use arrayvec::ArrayVec;
use jgenesis_common::num::GetBit;

const MAX_SPRITES_PER_LINE: usize = 4;

// From https://www.smspower.org/forums/8224-TMS9918ColorsForSMSVDP
pub const TMS9918_COLOR_TO_SMS_COLOR: &[u8; 16] = &[
    0x00, // Transparent (Black)
    0x00, // Black
    0x08, // Green 0
    0x0C, // Green 2
    0x10, // Blue 0
    0x30, // Blue 1
    0x01, // Red 0
    0x3C, // Cyan
    0x02, // Red 1
    0x03, // Red 2
    0x05, // Yellow 0
    0x0F, // Yellow 1
    0x04, // Green 1
    0x33, // Pink
    0x15, // Gray
    0x3F, // White
];

#[derive(Debug, Clone, Copy)]
struct Graphics2SpriteData {
    y: u8,
    x: u8,
    name: u8,
    color: u8,
    early_clock: bool,
}

impl Vdp {
    pub(super) fn render_graphics_2_scanline(&mut self) {
        let scanline = self.scanline;
        let frame_buffer_row = self.frame_buffer_row();
        let backdrop_color = TMS9918_COLOR_TO_SMS_COLOR[self.registers.backdrop_color as usize];

        let base_name_table_addr = self.registers.base_name_table_address;
        let base_color_table_addr = self.registers.color_table_address & 0x2000;
        let base_pattern_generator = self.registers.pattern_generator_address & 0x2000;

        let nametable_row = scanline / 8;
        let line_name_table_addr = base_name_table_addr | (nametable_row * 32);

        // Pattern generator and color table are split into 3 blocks of 2048 bytes each: one for the
        // first 8 rows, one for the middle 8 rows, and one for the last 8 rows
        let table_offset = if nametable_row >= 16 {
            4096
        } else if nametable_row >= 8 {
            2048
        } else {
            0
        };

        let tile_row = scanline % 8;

        let large_sprites = self.registers.double_sprite_height;
        let magnify_sprites = self.registers.double_sprite_size;
        let sprite_size = 16 * u8::from(large_sprites) + 16 * u8::from(magnify_sprites);

        // Scan for sprites on this line
        let sprite_buffer = self.find_sprites_on_line(sprite_size);

        for nametable_col in 0..vdp::SCREEN_WIDTH / 8 {
            let name_table_entry = self.vram[(line_name_table_addr | nametable_col) as usize];

            let pattern_generator_addr =
                base_pattern_generator + table_offset + 8 * u16::from(name_table_entry) + tile_row;
            let pattern_generator_entry = self.vram[pattern_generator_addr as usize];

            let color_table_addr =
                base_color_table_addr + table_offset + 8 * u16::from(name_table_entry) + tile_row;
            let color_table_entry = self.vram[color_table_addr as usize];
            let bg_color_0 = color_table_entry & 0x0F;
            let bg_color_1 = color_table_entry >> 4;

            for tile_col in 0..8 {
                let pixel = 8 * nametable_col + u16::from(tile_col);

                let sprite_color = self.determine_graphics_2_sprite_color(
                    &sprite_buffer,
                    scanline,
                    pixel,
                    sprite_size,
                    large_sprites,
                    magnify_sprites,
                );

                let bg_color =
                    if pattern_generator_entry.bit(7 - tile_col) { bg_color_1 } else { bg_color_0 };

                let pixel_color = if sprite_color != 0 {
                    TMS9918_COLOR_TO_SMS_COLOR[sprite_color as usize]
                } else if bg_color != 0 {
                    TMS9918_COLOR_TO_SMS_COLOR[bg_color as usize]
                } else {
                    backdrop_color
                };
                self.frame_buffer.set(frame_buffer_row, pixel, pixel_color.into());
            }
        }
    }

    fn find_sprites_on_line(
        &mut self,
        sprite_size: u8,
    ) -> ArrayVec<Graphics2SpriteData, MAX_SPRITES_PER_LINE> {
        let scanline = self.scanline as u8;
        let base_sprite_table_addr = self.registers.base_sprite_table_address;

        let mut sprite_buffer = ArrayVec::<Graphics2SpriteData, 4>::new();
        for sprite_idx in 0..32 {
            // Add 1 because sprites with Y=0 should display starting on line 1
            let sprite_table_addr = base_sprite_table_addr + 4 * sprite_idx;
            let y = self.vram[sprite_table_addr as usize].wrapping_add(1);

            if y == 0xD0 {
                // Termination signal
                break;
            }

            // Sprites can wrap from below the bottom of the screen to the top
            let sprite_bottom = y.wrapping_add(sprite_size);
            if !((y < sprite_bottom && (y..sprite_bottom).contains(&scanline))
                || (y >= sprite_bottom && (scanline >= y || scanline < sprite_bottom)))
            {
                continue;
            }

            if sprite_buffer.len() == sprite_buffer.capacity() {
                self.registers.sprite_overflow = true;
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

    fn determine_graphics_2_sprite_color(
        &self,
        sprite_buffer: &[Graphics2SpriteData],
        scanline: u16,
        pixel: u16,
        sprite_size: u8,
        large_sprites: bool,
        magnify_sprites: bool,
    ) -> u8 {
        sprite_buffer
            .iter()
            .copied()
            .find_map(|sprite| {
                let sprite_x =
                    if sprite.early_clock { i16::from(sprite.x) - 32 } else { i16::from(sprite.x) };

                let sprite_right = sprite_x + i16::from(sprite_size);
                if !(sprite_x..sprite_right).contains(&(pixel as i16)) {
                    return None;
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
                let mut sprite_pattern_addr = self.registers.base_sprite_pattern_address
                    + 8 * u16::from(sprite.name & sprite_name_mask)
                    + u16::from(sprite_row % 8);
                if sprite_row >= 8 {
                    sprite_pattern_addr += 8;
                }
                if sprite_col >= 8 {
                    sprite_pattern_addr += 16;
                }

                let sprite_pattern = self.vram[sprite_pattern_addr as usize];

                sprite_pattern.bit(7 - (sprite_col % 8)).then_some(sprite.color)
            })
            .unwrap_or(0)
    }
}
