use crate::ppu::{gba_color_to_rgb8, Ppu};
use jgenesis_common::debug::{DebugBytesView, DebugMemoryView, DebugWordsView, Endian};
use jgenesis_common::frontend::Color;

impl Ppu {
    pub fn debug_vram_view(&mut self) -> impl DebugMemoryView {
        DebugBytesView(self.vram.as_mut_slice())
    }

    pub fn debug_palette_view(&mut self) -> impl DebugMemoryView {
        DebugWordsView(self.palette_ram.as_mut_slice(), Endian::Little)
    }

    pub fn debug_oam_view(&mut self) -> impl DebugMemoryView {
        DebugWordsView(self.oam.as_mut_slice(), Endian::Little)
    }

    pub fn copy_palette_ram(&self, out: &mut [Color]) {
        for (i, &color) in self.palette_ram.iter().enumerate() {
            out[i] = gba_color_to_rgb8(color);
        }
    }
}
