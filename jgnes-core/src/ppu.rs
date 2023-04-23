use crate::bus::PpuBus;

pub const SCREEN_WIDTH: u16 = 256;
pub const SCREEN_HEIGHT: u16 = 240;
pub const VISIBLE_SCREEN_HEIGHT: u16 = 224;

const FIRST_VBLANK_SCANLINE: u16 = 241;
const PRE_RENDER_SCANLINE: u16 = 261;
const DOTS_PER_SCANLINE: u16 = 341;

pub type FrameBuffer = [[u8; SCREEN_WIDTH as usize]; SCREEN_HEIGHT as usize];

pub struct PpuState {
    scanline: u16,
    dot: u16,
    frame_buffer: FrameBuffer,
}

impl PpuState {
    pub fn new() -> Self {
        Self {
            scanline: PRE_RENDER_SCANLINE,
            dot: 0,
            frame_buffer: [[0; SCREEN_WIDTH as usize]; SCREEN_HEIGHT as usize],
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct SpriteData {
    oam_index: u8,
    y_pos: u8,
    tile_index: u8,
    attributes: u8,
    x_pos: u8,
}

pub fn tick(state: &mut PpuState, bus: &mut PpuBus<'_>) {
    match state.scanline {
        scanline @ 0..=239 => {
            // Visible scanlines
            if state.dot == 0 {
                render_scanline(scanline, state, bus);
            }
        }
        240 | 242..=260 => {
            // Post-render scanline / 2+ VBlank scanlines, PPU idles
        }
        241 => {
            // First VBlank scanline
            if state.dot == 1 {
                log::trace!("PPU: Setting VBlank flag");
                bus.get_ppu_registers_mut().set_vblank_flag(true);
            }
        }
        261 => {
            // Pre-render scanline
            if state.dot == 1 {
                log::trace!("PPU: Clearing flags in pre-render scanline");
                let ppu_registers = bus.get_ppu_registers_mut();
                ppu_registers.set_vblank_flag(false);
                ppu_registers.set_sprite_0_hit(false);
                ppu_registers.set_sprite_overflow(false);
            }
        }
        _ => panic!("invalid scanline: {}", state.scanline),
    }

    state.dot += 1;
    if state.dot == DOTS_PER_SCANLINE {
        state.scanline += 1;
        state.dot = 0;

        if state.scanline == PRE_RENDER_SCANLINE + 1 {
            state.scanline = 0;
        }
    }
}

fn render_scanline(scanline: u16, state: &mut PpuState, bus: &mut PpuBus<'_>) {
    log::trace!("PPU: Rendering scanline {scanline}");

    let scanline = scanline as u8;

    let ppu_registers = bus.get_ppu_registers();
    let bg_enabled = ppu_registers.bg_enabled();
    let sprites_enabled = ppu_registers.sprites_enabled();
    let double_height_sprites = ppu_registers.double_height_sprites();
    let sprite_height = if double_height_sprites { 16 } else { 8 };
    let bg_pattern_table_address = ppu_registers.bg_pattern_table_address();
    let sprite_pattern_table_address = ppu_registers.sprite_pattern_table_address();
    let base_nametable_address = ppu_registers.base_nametable_address();

    let scroll_x = ppu_registers.scroll_x();
    let scroll_y = ppu_registers.scroll_y();

    let backdrop_color = bus.get_palette_ram()[0] & 0x3F;

    if !bg_enabled && !sprites_enabled {
        for value in state.frame_buffer[scanline as usize].iter_mut() {
            *value = backdrop_color;
        }
        return;
    }

    let mut sprites = Vec::new();

    if scanline > 0 && sprites_enabled {
        let oam = bus.get_oam();
        for (oam_index, chunk) in oam.chunks_exact(4).enumerate() {
            let &[y_pos, tile_index, attributes, x_pos] = chunk
            else {
                panic!("all OAM iteration chunks should be size 4");
            };

            if (y_pos..y_pos.saturating_add(sprite_height)).contains(&scanline) {
                sprites.push(SpriteData {
                    oam_index: oam_index as u8,
                    y_pos,
                    tile_index,
                    attributes,
                    x_pos,
                });
                if sprites.len() == 8 {
                    break;
                }
            }
        }
    }

    let mut nametable_address = base_nametable_address;

    let mut bg_y = u16::from(scroll_y) + u16::from(scanline);
    while bg_y >= SCREEN_HEIGHT {
        bg_y -= SCREEN_HEIGHT;
        nametable_address = 0x2000 + ((nametable_address + 0x0800) & 0x0F00);
    }

    let bg_tile_y = bg_y / 8;

    let mut bg_x = u16::from(scroll_x);

    for pixel in 0..SCREEN_WIDTH {
        let bg_tile_x = bg_x / 8;
        let nametable_index = 30 * bg_tile_y + bg_tile_x;
        let attribute_index = 0x03C0 + nametable_index / 16;

        let bg_tile_index = bus.read_address(nametable_address + nametable_index);
        let bg_attributes = bus.read_address(nametable_address + attribute_index);

        let bg_fine_x = (bg_x % 8) as u8;
        let bg_fine_y = (bg_y % 8) as u8;

        let bg_palette_index = match (bg_fine_x, bg_fine_y) {
            (x, y) if x < 4 && y < 4 => bg_attributes & 0x03,
            (x, y) if x >= 4 && y < 4 => (bg_attributes >> 2) & 0x03,
            (x, y) if x < 4 && y >= 4 => (bg_attributes >> 4) & 0x03,
            (x, y) if x >= 4 && y >= 4 => (bg_attributes >> 6) & 0x03,
            _ => panic!("match arm guards should be exhaustive"),
        };

        let bg_tile_data_0 = bus.read_address(
            bg_pattern_table_address + 16 * u16::from(bg_tile_index) + u16::from(bg_fine_y),
        );
        let bg_tile_data_1 = bus.read_address(
            bg_pattern_table_address + 16 * u16::from(bg_tile_index) + u16::from(bg_fine_y) + 1,
        );

        let bg_color_id = get_color_id(bg_tile_data_0, bg_tile_data_1, bg_fine_x);
        let bg_color_id = if bg_enabled { bg_color_id } else { 0 };

        let sprite = first_opaque_sprite_pixel(
            &sprites,
            bus,
            scanline,
            pixel as u8,
            sprite_pattern_table_address,
            double_height_sprites,
        );

        if let Some((SpriteData { oam_index: 0, .. }, _)) = sprite {
            if bg_color_id != 0 {
                bus.get_ppu_registers_mut().set_sprite_0_hit(true);
            }
        }

        let (sprite_color_id, sprite_bg_priority, sprite_palette_index) = match sprite {
            Some((sprite, color_id)) => (
                color_id,
                sprite.attributes & 0x20 != 0,
                sprite.attributes & 0x03,
            ),
            None => (0, true, 0),
        };

        let palette_ram = bus.get_palette_ram();
        let pixel_color = if sprite_color_id != 0 && (bg_color_id == 0 || !sprite_bg_priority) {
            palette_ram[(0x10 + 4 * sprite_palette_index + sprite_color_id) as usize] & 0x3F
        } else if bg_color_id != 0 {
            palette_ram[(4 * bg_palette_index + bg_color_id) as usize] & 0x3F
        } else {
            backdrop_color
        };

        state.frame_buffer[scanline as usize][pixel as usize] = pixel_color;

        bg_x += 1;
        if bg_x == SCREEN_WIDTH {
            bg_x = 0;
            nametable_address =
                (nametable_address & 0x2800) + ((nametable_address + 0x0400) & 0x0700);
        }
    }
}

fn get_color_id(tile_data_0: u8, tile_data_1: u8, fine_x: u8) -> u8 {
    assert!(fine_x < 8, "fine_x must be less than 8: {fine_x}");

    (((tile_data_0 & (1 << (7 - fine_x))) >> (7 - fine_x)) << 1)
        | ((tile_data_1 & (1 << (7 - fine_x))) >> (7 - fine_x))
}

fn first_opaque_sprite_pixel(
    sprites: &[SpriteData],
    bus: &mut PpuBus<'_>,
    scanline: u8,
    pixel: u8,
    sprite_pattern_table_address: u16,
    double_height_sprites: bool,
) -> Option<(SpriteData, u8)> {
    sprites.iter().find_map(|&sprite| {
        if !(sprite.x_pos..sprite.x_pos.saturating_add(8)).contains(&pixel) {
            return None;
        }

        let tile_index = u16::from(sprite.tile_index);
        let pattern_table_address = if double_height_sprites {
            0x1000 * (tile_index & 0x01)
                + (tile_index & 0xFE)
                + u16::from(scanline - sprite.y_pos >= 8)
        } else {
            sprite_pattern_table_address + tile_index
        };

        let tile_data_0 = bus.read_address(pattern_table_address);
        let tile_data_1 = bus.read_address(pattern_table_address + 1);

        let fine_x = (pixel - sprite.x_pos) % 8;
        let fine_y = (scanline - sprite.y_pos) % 8;

        let color_id = get_color_id(tile_data_0, tile_data_1, fine_x);

        (color_id != 0).then_some((sprite, color_id))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_id() {
        assert_eq!(0, get_color_id(0, 0, 0));

        assert_eq!(2, get_color_id(0x80, 0, 0));
        assert_eq!(1, get_color_id(0, 0x80, 0));
        assert_eq!(3, get_color_id(0x80, 0x80, 0));

        assert_eq!(0, get_color_id(0x80, 0x80, 1));

        assert_eq!(3, get_color_id(0x10, 0x10, 3));

        assert_eq!(3, get_color_id(0x01, 0x01, 7));
    }
}
