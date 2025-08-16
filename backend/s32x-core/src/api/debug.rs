use crate::api::Sega32XEmulator;
use genesis_core::api::debug::GenesisMemoryArea;
use genesis_core::vdp::ColorModifier;
use jgenesis_common::debug::{DebugMemoryView, DebugWordsView, Endian};
use jgenesis_common::frontend::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum S32XMemoryArea {
    Sdram,
    MasterSh2Cache,
    SlaveSh2Cache,
    FrameBuffer0,
    FrameBuffer1,
    PaletteRam,
}

pub struct Sega32XDebugView<'emu>(&'emu mut Sega32XEmulator);

impl Sega32XEmulator {
    #[must_use]
    pub fn debug(&mut self) -> Sega32XDebugView<'_> {
        Sega32XDebugView(self)
    }
}

impl<'emu> Sega32XDebugView<'emu> {
    pub fn copy_cram(&self, out: &mut [Color], modifier: ColorModifier) {
        self.0.vdp.copy_cram(out, modifier);
    }

    pub fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        self.0.vdp.copy_vram(out, palette, row_len);
    }

    pub fn copy_palette(&mut self, out: &mut [Color]) {
        self.0.memory.medium_mut().vdp().copy_palette(out);
    }

    pub fn dump_vdp_registers(&self, callback: impl FnMut(&str, &[(&str, &str)])) {
        self.0.vdp.dump_registers(callback);
    }

    pub fn dump_32x_system_registers(&self, callback: impl FnMut(&str, &[(&str, &str)])) {
        let vdp = &self.0.memory.medium().s32x_bus.vdp;
        let h_interrupt_in_vblank = vdp.hen_bit();
        let h_interrupt_interval = vdp.h_interrupt_interval();

        self.0.memory.medium().s32x_bus.registers.dump(
            h_interrupt_in_vblank,
            h_interrupt_interval,
            callback,
        );
    }

    pub fn dump_32x_vdp_registers(&mut self, callback: impl FnMut(&str, &[(&str, &str)])) {
        self.0.memory.medium_mut().vdp().dump_registers(callback);
    }

    pub fn dump_pwm_registers(&mut self, callback: impl FnMut(&str, &[(&str, &str)])) {
        self.0.memory.medium().s32x_bus.pwm.dump_registers(callback);
    }

    #[must_use]
    pub fn genesis_memory_view(
        self,
        memory_area: GenesisMemoryArea,
    ) -> Box<dyn DebugMemoryView + 'emu> {
        match memory_area {
            GenesisMemoryArea::CartridgeRom => {
                Box::new(self.0.memory.medium_mut().s32x_bus.cartridge.debug_rom_view())
            }
            GenesisMemoryArea::WorkingRam => Box::new(self.0.memory.debug_working_ram_view()),
            GenesisMemoryArea::AudioRam => Box::new(self.0.memory.debug_audio_ram_view()),
            GenesisMemoryArea::Vram => Box::new(self.0.vdp.debug_vram_view()),
            GenesisMemoryArea::Cram => Box::new(self.0.vdp.debug_cram_view()),
            GenesisMemoryArea::Vsram => Box::new(self.0.vdp.debug_vsram_view()),
        }
    }

    #[must_use]
    pub fn s32x_memory_view(self, memory_area: S32XMemoryArea) -> Box<dyn DebugMemoryView + 'emu> {
        match memory_area {
            S32XMemoryArea::Sdram => Box::new(DebugWordsView(
                self.0.memory.medium_mut().s32x_bus.sdram.as_mut_slice(),
                Endian::Big,
            )),
            S32XMemoryArea::MasterSh2Cache => {
                Box::new(self.0.memory.medium_mut().debug_master_sh2_cache())
            }
            S32XMemoryArea::SlaveSh2Cache => {
                Box::new(self.0.memory.medium_mut().debug_slave_sh2_cache())
            }
            S32XMemoryArea::FrameBuffer0 => {
                Box::new(self.0.memory.medium_mut().vdp().debug_frame_buffer_view(0))
            }
            S32XMemoryArea::FrameBuffer1 => {
                Box::new(self.0.memory.medium_mut().vdp().debug_frame_buffer_view(1))
            }
            S32XMemoryArea::PaletteRam => {
                Box::new(self.0.memory.medium_mut().vdp().debug_palette_ram_view())
            }
        }
    }
}
