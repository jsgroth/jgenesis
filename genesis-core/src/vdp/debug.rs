use crate::vdp::{
    gen_color_to_rgb, read_pattern_generator, resolve_color, ColorModifier, PatternGeneratorArgs,
    Vdp,
};
use jgenesis_traits::frontend;
use jgenesis_traits::frontend::Color;

impl Vdp {
    pub fn debug_cram(&self, out: &mut [Color]) {
        for (color, chunk) in out.iter_mut().zip(self.cram.chunks_exact(2)) {
            let &[msb, lsb] = chunk else { unreachable!("chunks_exact(2)") };
            let gen_color = u16::from_be_bytes([msb, lsb]);
            *color = parse_gen_color(gen_color);
        }
    }

    pub fn debug_vram(&self, out: &mut [Color], palette: u8) {
        let row_len = frontend::VRAM_DEBUG_ROW_LEN as usize;
        for (i, color_mut) in out.iter_mut().enumerate() {
            let pattern_generator = row_len * (i / row_len / (8 * 8)) + (i % (row_len * 8)) / 8;
            let row = (i / (row_len * 8)) % 8;
            let col = i % 8;

            let color_id = read_pattern_generator(
                &self.vram,
                PatternGeneratorArgs {
                    vertical_flip: false,
                    horizontal_flip: false,
                    pattern_generator: pattern_generator as u16,
                    row: row as u16,
                    col: col as u16,
                    cell_height: 8,
                },
            );
            let color = resolve_color(&self.cram, palette, color_id);
            *color_mut = parse_gen_color(color);
        }
    }
}

fn parse_gen_color(gen_color: u16) -> Color {
    let r = ((gen_color >> 1) & 0x07) as u8;
    let g = ((gen_color >> 5) & 0x07) as u8;
    let b = ((gen_color >> 9) & 0x07) as u8;
    gen_color_to_rgb(r, g, b, ColorModifier::None)
}
