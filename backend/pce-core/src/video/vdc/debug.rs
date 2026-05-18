use crate::video::palette::PcePalette;
use crate::video::vce::Vce;
use crate::video::vdc::Vdc;
use jgenesis_common::frontend::Color;

impl Vdc {
    pub fn dump_vram(
        &self,
        palette: u16,
        out: &mut [[Color; 64]],
        vce: &Vce,
        pce_palette: &PcePalette,
    ) {
        for (tile_number, out_tile) in out[..2048].iter_mut().enumerate() {
            let tile_addr = 16 * tile_number;
            let tile_data = &self.vram[tile_addr..tile_addr + 16];

            for tile_row in 0..8 {
                let cg0 = tile_data[tile_row];
                let cg1 = tile_data[tile_row + 8];

                for tile_col in 0..8 {
                    let shift = 7 - tile_col;
                    let color_idx = ((cg0 >> shift) & 1)
                        | (((cg0 >> (shift + 8)) & 1) << 1)
                        | (((cg1 >> shift) & 1) << 2)
                        | (((cg1 >> (shift + 8)) & 1) << 3);

                    let vce_color = vce.read_color((palette << 4) | color_idx);
                    let (r, g, b) = pce_palette[(vce_color & 0x1FF) as usize];
                    out_tile[8 * tile_row + tile_col] = Color::rgb(r, g, b);
                }
            }
        }
    }
}
