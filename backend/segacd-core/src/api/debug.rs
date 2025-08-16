use crate::api::SegaCdEmulator;
use genesis_core::api::debug::GenesisMemoryArea;
use genesis_core::vdp::ColorModifier;
use jgenesis_common::debug::{DebugMemoryView, EmptyDebugView};
use jgenesis_common::frontend::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SegaCdMemoryArea {
    BiosRom,
    PrgRam,
    WordRam,
    PcmRam,
    CdcRam,
}

pub struct SegaCdDebugView<'emu>(&'emu mut SegaCdEmulator);

impl SegaCdEmulator {
    #[must_use]
    pub fn debug(&mut self) -> SegaCdDebugView<'_> {
        SegaCdDebugView(self)
    }
}

impl<'emu> SegaCdDebugView<'emu> {
    pub fn copy_cram(&self, out: &mut [Color], modifier: ColorModifier) {
        self.0.vdp.copy_cram(out, modifier);
    }

    pub fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        self.0.vdp.copy_vram(out, palette, row_len);
    }

    pub fn dump_vdp_registers(&self, callback: impl FnMut(&str, &[(&str, &str)])) {
        self.0.vdp.dump_registers(callback);
    }

    #[must_use]
    pub fn genesis_memory_view(
        self,
        memory_area: GenesisMemoryArea,
    ) -> Box<dyn DebugMemoryView + 'emu> {
        match memory_area {
            GenesisMemoryArea::CartridgeRom => Box::new(EmptyDebugView),
            GenesisMemoryArea::WorkingRam => Box::new(self.0.memory.debug_working_ram_view()),
            GenesisMemoryArea::AudioRam => Box::new(self.0.memory.debug_audio_ram_view()),
            GenesisMemoryArea::Vram => Box::new(self.0.vdp.debug_vram_view()),
            GenesisMemoryArea::Cram => Box::new(self.0.vdp.debug_cram_view()),
            GenesisMemoryArea::Vsram => Box::new(self.0.vdp.debug_vsram_view()),
        }
    }

    #[must_use]
    pub fn scd_memory_view(self, memory_area: SegaCdMemoryArea) -> Box<dyn DebugMemoryView + 'emu> {
        match memory_area {
            SegaCdMemoryArea::BiosRom => Box::new(self.0.memory.medium_mut().debug_bios_rom_view()),
            SegaCdMemoryArea::PrgRam => Box::new(self.0.memory.medium_mut().debug_prg_ram_view()),
            SegaCdMemoryArea::WordRam => Box::new(self.0.memory.medium_mut().debug_word_ram_view()),
            SegaCdMemoryArea::PcmRam => Box::new(self.0.pcm.debug_ram_view()),
            SegaCdMemoryArea::CdcRam => Box::new(self.0.memory.medium_mut().debug_cdc_ram_view()),
        }
    }
}
