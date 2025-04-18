use crate::api::SegaCdEmulator;
use jgenesis_common::frontend::{ViewableBytes, ViewableWordsBigEndian};

impl SegaCdEmulator {
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

    #[must_use]
    pub fn debug_prg_ram_view(&mut self) -> ViewableBytes<'_> {
        self.memory.medium_mut().debug_prg_ram_view()
    }

    #[must_use]
    pub fn debug_word_ram_view(&mut self) -> ViewableBytes<'_> {
        self.memory.medium_mut().word_ram_mut().debug_view()
    }

    #[must_use]
    pub fn debug_cdc_ram_view(&mut self) -> ViewableBytes<'_> {
        self.memory.medium_mut().debug_cdc_ram_view()
    }

    #[must_use]
    pub fn debug_pcm_ram_view(&mut self) -> ViewableBytes<'_> {
        self.pcm.debug_ram_view()
    }
}
