use crate::bus::PpuBus;
use crate::graphics;
use crate::ppu::ColorEmphasis;
use jgenesis_common::frontend::Color;
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PatternTable {
    #[default]
    Zero,
    One,
}

pub fn copy_nametables(pattern_table: PatternTable, bus: &mut PpuBus<'_>, out: &mut [Color]) {
    let backdrop_color = bus.get_palette_ram()[0] & 0x3F;

    // Dump the pattern tables and nametables into Vecs because this function is horrendously slow if it needs to do
    // a bus lookup for each nametable/pattern table byte
    let pattern_table = dump_pattern_table(pattern_table, bus);

    let mut nametables = vec![0; 0x1000];
    for (i, value) in nametables.iter_mut().enumerate() {
        *value = bus.read_address(0x2000 | (i as u16));
    }

    for nametable in 0..4 {
        for row in 0..240 {
            for col in 0..256 {
                let nametable_addr = 0x400 * nametable + 32 * (row / 8) + col / 8;
                let tile_number = nametables[nametable_addr as usize];

                let attributes_addr = 0x3C0 + 0x400 * nametable + 8 * (row / 32) + col / 32;
                let attributes_shift = 2 * (2 * ((row % 32) / 16) + ((col % 32) / 16));
                let palette = (nametables[attributes_addr as usize] >> attributes_shift) & 0x03;

                let pattern_table_addr = (u16::from(tile_number) << 4) | (row % 8);
                let pattern_table_shift = 7 - (col % 8);
                let color_0 =
                    (pattern_table[pattern_table_addr as usize] >> pattern_table_shift) & 0x01;
                let color_1 = (pattern_table[(pattern_table_addr + 8) as usize]
                    >> pattern_table_shift)
                    & 0x01;
                let color = (color_1 << 1) | color_0;

                let nes_color = if color != 0 {
                    let color_addr = (palette << 2) | color;
                    bus.get_palette_ram()[color_addr as usize] & 0x3F
                } else {
                    backdrop_color
                };

                let out_idx = u32::from(nametable & 0x02) * 256 * 240
                    + u32::from(nametable & 0x01) * 256
                    + u32::from(row) * 256 * 2
                    + u32::from(col);
                out[out_idx as usize] = graphics::nes_color_to_rgba(nes_color, ColorEmphasis::NONE);
            }
        }
    }
}

fn dump_pattern_table(pattern_table: PatternTable, bus: &mut PpuBus<'_>) -> Vec<u8> {
    let mut out = vec![0; 0x1000];
    dump_pattern_table_into(pattern_table, bus, &mut out);

    out
}

fn dump_pattern_table_into(pattern_table: PatternTable, bus: &mut PpuBus<'_>, out: &mut [u8]) {
    let pattern_table_addr = match pattern_table {
        PatternTable::Zero => 0x0000,
        PatternTable::One => 0x1000,
    };

    for (i, value) in out.iter_mut().enumerate() {
        *value = bus.read_address(pattern_table_addr | (i as u16));
    }
}

pub fn copy_oam(pattern_table: PatternTable, bus: &mut PpuBus<'_>, out: &mut [Color]) {
    let backdrop_color = bus.get_palette_ram()[0] & 0x3F;

    let mut pattern_tables = vec![0; 0x2000];
    dump_pattern_table_into(PatternTable::Zero, bus, &mut pattern_tables[..0x1000]);
    dump_pattern_table_into(PatternTable::One, bus, &mut pattern_tables[0x1000..]);

    let sprite_pattern_table_addr = match pattern_table {
        PatternTable::Zero => 0x0000,
        PatternTable::One => 0x1000,
    };

    let double_height_sprites = bus.get_ppu_registers().double_height_sprites();

    let oam = bus.get_oam();
    for sprite in 0_u16..64 {
        let tile_number = oam[(4 * sprite + 1) as usize];

        let attributes = oam[(4 * sprite + 2) as usize];
        let palette = 4 + (attributes & 0x03);
        let x_flip = attributes.bit(6);
        let y_flip = attributes.bit(7);

        let (tile_number, base_pattern_table_addr) = if double_height_sprites {
            (tile_number & !0x01, u16::from(tile_number & 0x01) << 12)
        } else {
            (tile_number, sprite_pattern_table_addr)
        };

        let rows = if double_height_sprites { 16 } else { 8 };
        for row in 0_u16..rows {
            for col in 0..8 {
                let row = if y_flip { rows - 1 - row } else { row };
                let col = if x_flip { 7 - col } else { col };

                let double_height_offset = if row >= 8 { 0x0010 } else { 0x0000 };

                let pattern_table_addr = base_pattern_table_addr
                    | (u16::from(tile_number) << 4)
                    | double_height_offset
                    | (row & 0x07);
                let pattern_table_shift = 7 - col;
                let color_0 =
                    (pattern_tables[pattern_table_addr as usize] >> pattern_table_shift) & 0x01;
                let color_1 = (pattern_tables[(pattern_table_addr + 8) as usize]
                    >> pattern_table_shift)
                    & 0x01;
                let color = (color_1 << 1) | color_0;

                let nes_color = if color != 0 {
                    let color_addr = (palette << 2) | color;
                    bus.get_palette_ram()[color_addr as usize] & 0x3F
                } else {
                    backdrop_color
                };

                let out_idx = (sprite / 8) * 64 * rows + (sprite % 8) * 8 + row * 64 + col;
                out[out_idx as usize] = graphics::nes_color_to_rgba(nes_color, ColorEmphasis::NONE);
            }
        }
    }
}

pub fn copy_palette_ram(bus: &PpuBus<'_>, out: &mut [Color]) {
    let palette_ram = bus.get_palette_ram();
    for (&nes_color, out_color) in palette_ram.iter().zip(out) {
        *out_color = graphics::nes_color_to_rgba(nes_color & 0x3F, ColorEmphasis::NONE);
    }
}
