use crate::GenesisEmulator;
use jgenesis_common::frontend::{ViewableBytes, ViewableWordsBigEndian};

impl GenesisEmulator {
    #[must_use]
    pub fn debug_working_ram_view(&mut self) -> ViewableBytes<'_> {
        self.memory.debug_main_ram_view()
    }

    #[must_use]
    pub fn debug_audio_ram_view(&mut self) -> ViewableBytes<'_> {
        self.memory.debug_audio_ram_view()
    }

    #[must_use]
    pub fn debug_vram_view(&mut self) -> ViewableBytes<'_> {
        self.vdp.debug_vram_view()
    }

    #[must_use]
    pub fn debug_cram_view(&mut self) -> ViewableWordsBigEndian<'_> {
        self.vdp.debug_cram_view()
    }

    #[must_use]
    pub fn debug_vsram_view(&mut self) -> ViewableBytes<'_> {
        self.vdp.debug_vsram_view()
    }
}
