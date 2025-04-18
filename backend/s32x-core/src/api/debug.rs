use crate::api::Sega32XEmulator;
use jgenesis_common::frontend::{ViewableBytes, ViewableWordsBigEndian};

impl Sega32XEmulator {
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
    pub fn debug_sdram_view(&mut self) -> ViewableWordsBigEndian<'_> {
        ViewableWordsBigEndian(self.memory.medium_mut().s32x_bus.sdram.as_mut_slice())
    }

    #[must_use]
    pub fn debug_fb0_view(&mut self) -> ViewableWordsBigEndian<'_> {
        self.memory.medium_mut().s32x_bus.vdp.debug_fb0_view()
    }

    #[must_use]
    pub fn debug_fb1_view(&mut self) -> ViewableWordsBigEndian<'_> {
        self.memory.medium_mut().s32x_bus.vdp.debug_fb1_view()
    }

    #[must_use]
    pub fn debug_32x_cram_view(&mut self) -> ViewableWordsBigEndian<'_> {
        self.memory.medium_mut().s32x_bus.vdp.debug_cram_view()
    }
}
