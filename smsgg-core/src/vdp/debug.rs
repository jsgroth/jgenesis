use crate::vdp::{convert_gg_color, convert_sms_color, get_color_id, Vdp};
use jgenesis_traits::frontend;
use jgenesis_traits::frontend::Color;

impl Vdp {
    pub fn debug_cram(&self, out: &mut [Color]) {
        if self.registers.version.is_master_system() {
            for (color, cram_byte) in out.iter_mut().zip(self.color_ram[..32].iter().copied()) {
                *color = sms_color_to_rgb(cram_byte);
            }
        } else {
            for (color, chunk) in out.iter_mut().zip(self.color_ram.chunks_exact(2)) {
                let &[lsb, msb] = chunk else { unreachable!("chunks_exact(2)") };
                let gg_color = u16::from_le_bytes([lsb, msb]);
                *color = gg_color_to_rgb(gg_color);
            }
        }
    }

    pub fn debug_vram(&self, out: &mut [Color], palette: u8) {
        let row_len = frontend::VRAM_DEBUG_ROW_LEN as usize;
        for (i, color_mut) in out.iter_mut().enumerate() {
            let pattern = row_len * (i / row_len / (8 * 8)) + (i % (row_len * 8)) / 8;
            let row = (i / (row_len * 8)) % 8;
            let col = i % 8;

            let tile = &self.vram[32 * pattern..32 * (pattern + 1)];
            let color_id = get_color_id(tile, row as u16, col as u16, false);
            let color = self.read_color_ram_word(16 * palette + color_id);

            *color_mut = if self.registers.version.is_master_system() {
                sms_color_to_rgb(color as u8)
            } else {
                gg_color_to_rgb(color)
            };
        }
    }
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
