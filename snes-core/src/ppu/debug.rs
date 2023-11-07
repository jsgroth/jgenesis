use crate::ppu::{convert_snes_color, Ppu};
use jgenesis_traits::frontend::Color;

impl Ppu {
    pub fn debug_cram(&self, out: &mut [Color]) {
        let brightness = self.registers.brightness;
        for (out_color, &snes_color) in out.iter_mut().zip(self.cgram.iter()) {
            *out_color = convert_snes_color(snes_color, brightness);
        }
    }
}
