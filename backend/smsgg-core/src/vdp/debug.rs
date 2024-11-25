use crate::vdp::{VRAM_SIZE, Vdp, convert_gg_color, convert_sms_color, get_color_id};

use jgenesis_common::frontend::Color;

impl Vdp {
    pub fn copy_cram(&self, out: &mut [Color]) {
        if self.registers.version.is_master_system() {
            for (out_color, &cram_byte) in out.iter_mut().zip(&self.color_ram[..32]) {
                *out_color = sms_color_to_rgb(cram_byte);
            }
        } else {
            // Game Gear
            let colors_iter = self
                .color_ram
                .chunks_exact(2)
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]));

            for (out_color, cram_color) in out.iter_mut().zip(colors_iter) {
                *out_color = gg_color_to_rgb(cram_color);
            }
        }
    }

    pub fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        for pattern in 0..VRAM_SIZE / 32 {
            let tile = &self.vram[32 * pattern..32 * (pattern + 1)];
            let base_idx = pattern / row_len * row_len * 64 + (pattern % row_len) * 8;

            for row in 0..8 {
                for col in 0..8 {
                    let row_offset = row * row_len * 8;
                    let out_idx = base_idx + row_offset + col;

                    let color_id = get_color_id(tile, row as u16, col as u16, false);
                    let color = self.read_color_ram_word((palette << 4) | color_id);

                    out[out_idx] = if self.registers.version.is_master_system() {
                        sms_color_to_rgb(color as u8)
                    } else {
                        gg_color_to_rgb(color)
                    };
                }
            }
        }
    }

    pub fn dump_registers(&self, mut callback: impl FnMut(u32, &[(&str, &str)])) {
        let mode_str = self.registers.mode.to_string();

        callback(0, &[
            ("Vertical scroll lock", bool_str(self.registers.vertical_scroll_lock)),
            ("Horizontal scroll lock", bool_str(self.registers.horizontal_scroll_lock)),
            ("Hide left column", bool_str(self.registers.hide_left_column)),
            ("Horizontal interrupt enabled", bool_str(self.registers.line_interrupt_enabled)),
            ("Shift sprites left", bool_str(self.registers.shift_sprites_left)),
            ("M3", if self.registers.mode_bits[3] { "1" } else { "0" }),
            ("M1", if self.registers.mode_bits[1] { "1" } else { "0" }),
            ("Mode", &mode_str),
        ]);

        callback(1, &[
            ("Display enabled", bool_str(self.registers.display_enabled)),
            ("Vertical interrupt enabled", bool_str(self.registers.frame_interrupt_enabled)),
            ("M0", if self.registers.mode_bits[0] { "1" } else { "0" }),
            ("M2", if self.registers.mode_bits[2] { "1" } else { "0" }),
            ("Mode", &mode_str),
            ("Double sprite height", bool_str(self.registers.double_sprite_height)),
            ("Double sprite size", bool_str(self.registers.double_sprite_size)),
        ]);

        callback(2, &[(
            "Nametable address",
            &format!("${:04X}", self.registers.base_name_table_address),
        )]);

        callback(3, &[(
            "Color table address (TMS9918)",
            &format!("${:04X}", self.registers.color_table_address),
        )]);

        callback(4, &[(
            "Pattern generator start address (TMS9918)",
            &format!("${:04X}", self.registers.pattern_generator_address),
        )]);

        callback(5, &[(
            "Sprite attribute table address",
            &format!("${:04X}", self.registers.base_sprite_table_address),
        )]);

        callback(6, &[(
            "Sprite pattern table address",
            &format!("${:04X}", self.registers.base_sprite_pattern_address),
        )]);

        callback(7, &[("Backdrop color ID", &self.registers.backdrop_color.to_string())]);

        callback(8, &[("Horizontal scroll", &self.registers.x_scroll.to_string())]);

        callback(9, &[("Vertical scroll", &self.registers.y_scroll.to_string())]);

        callback(10, &[(
            "HINT counter reload value",
            &self.registers.line_counter_reload_value.to_string(),
        )]);
    }
}

fn bool_str(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

fn sms_color_to_rgb(cram_byte: u8) -> Color {
    let r = convert_sms_color((cram_byte & 0x03).into());
    let g = convert_sms_color(((cram_byte >> 2) & 0x03).into());
    let b = convert_sms_color(((cram_byte >> 4) & 0x03).into());
    Color::rgb(r, g, b)
}

fn gg_color_to_rgb(gg_color: u16) -> Color {
    let r = convert_gg_color(gg_color & 0x0F);
    let g = convert_gg_color((gg_color >> 4) & 0x0F);
    let b = convert_gg_color((gg_color >> 8) & 0x0F);
    Color::rgb(r, g, b)
}
