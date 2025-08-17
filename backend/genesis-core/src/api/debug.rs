use crate::GenesisEmulator;
use crate::vdp::ColorModifier;
use jgenesis_common::debug::DebugMemoryView;
use jgenesis_common::frontend::Color;

#[derive(Debug, Clone, Copy, Default)]
pub struct SpriteAttributeEntry {
    pub tile_number: u16,
    pub x: u16,
    pub y: u16,
    pub h_cells: u8,
    pub v_cells: u8,
    pub palette: u8,
    pub priority: bool,
    pub h_flip: bool,
    pub v_flip: bool,
    pub link: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct CopySpriteAttributesResult {
    pub sprite_table_len: u16,
    pub top_left_x: u16,
    pub top_left_y: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GenesisMemoryArea {
    CartridgeRom,
    WorkingRam,
    AudioRam,
    Vram,
    Cram,
    Vsram,
}

pub struct GenesisDebugView<'emu>(&'emu mut GenesisEmulator);

impl GenesisEmulator {
    #[must_use]
    pub fn debug(&mut self) -> GenesisDebugView<'_> {
        GenesisDebugView(self)
    }
}

impl<'emu> GenesisDebugView<'emu> {
    pub fn copy_cram(&self, out: &mut [Color], modifier: ColorModifier) {
        self.0.vdp.copy_cram(out, modifier);
    }

    pub fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        self.0.vdp.copy_vram(out, palette, row_len);
    }

    pub fn dump_vdp_registers(&self, callback: impl FnMut(&str, &[(&str, &str)])) {
        self.0.vdp.dump_registers(callback);
    }

    pub fn copy_h_scroll(&self, out: &mut [(u16, u16)]) {
        self.0.vdp.copy_h_scroll(out);
    }

    pub fn copy_sprite_attributes(
        &self,
        out: &mut [SpriteAttributeEntry],
    ) -> CopySpriteAttributesResult {
        self.0.vdp.copy_sprite_attributes(out)
    }

    #[must_use]
    pub fn memory_view(self, memory_area: GenesisMemoryArea) -> Box<dyn DebugMemoryView + 'emu> {
        match memory_area {
            GenesisMemoryArea::CartridgeRom => {
                Box::new(self.0.memory.medium_mut().debug_rom_view())
            }
            GenesisMemoryArea::WorkingRam => Box::new(self.0.memory.debug_working_ram_view()),
            GenesisMemoryArea::AudioRam => Box::new(self.0.memory.debug_audio_ram_view()),
            GenesisMemoryArea::Vram => Box::new(self.0.vdp.debug_vram_view()),
            GenesisMemoryArea::Cram => Box::new(self.0.vdp.debug_cram_view()),
            GenesisMemoryArea::Vsram => Box::new(self.0.vdp.debug_vsram_view()),
        }
    }
}
