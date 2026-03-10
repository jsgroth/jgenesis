use crate::api::Sega32XEmulator;
use crate::pwm::PwmChip;
use crate::registers::SystemRegisters;
use crate::vdp::Vdp;
use genesis_core::api::debug::GenesisDebugState;
use jgenesis_common::debug::{DebugMemoryView, DebugWordsView, Endian};
use jgenesis_common::frontend::Color;
use sh2_emu::Sh2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum S32XMemoryArea {
    Sdram,
    MasterSh2Cache,
    SlaveSh2Cache,
    FrameBuffer0,
    FrameBuffer1,
    PaletteRam,
}

pub struct Sega32XDebugState {
    genesis: GenesisDebugState,
    sdram: Box<[u16]>,
    sh2_master: Sh2,
    sh2_slave: Sh2,
    system_registers: SystemRegisters,
    s32x_vdp: Vdp,
    pwm: PwmChip,
}

impl Sega32XDebugState {
    pub fn genesis(&mut self) -> &mut GenesisDebugState {
        &mut self.genesis
    }

    pub fn copy_palette(&mut self, out: &mut [Color]) {
        self.s32x_vdp.copy_palette(out);
    }

    pub fn dump_32x_system_registers(&self, callback: impl FnMut(&str, &[(&str, &str)])) {
        let h_interrupt_in_vblank = self.s32x_vdp.hen_bit();
        let h_interrupt_interval = self.s32x_vdp.h_interrupt_interval();

        self.system_registers.dump(h_interrupt_in_vblank, h_interrupt_interval, callback);
    }

    pub fn dump_32x_vdp_registers(&mut self, callback: impl FnMut(&str, &[(&str, &str)])) {
        self.s32x_vdp.dump_registers(callback);
    }

    pub fn dump_pwm_registers(&mut self, callback: impl FnMut(&str, &[(&str, &str)])) {
        self.pwm.dump_registers(callback);
    }

    #[must_use]
    pub fn s32x_memory_view(
        &mut self,
        memory_area: S32XMemoryArea,
    ) -> Box<dyn DebugMemoryView + '_> {
        match memory_area {
            S32XMemoryArea::Sdram => Box::new(DebugWordsView(&mut self.sdram, Endian::Big)),
            S32XMemoryArea::MasterSh2Cache => Box::new(self.sh2_master.debug_cache_view()),
            S32XMemoryArea::SlaveSh2Cache => Box::new(self.sh2_slave.debug_cache_view()),
            S32XMemoryArea::FrameBuffer0 => Box::new(self.s32x_vdp.debug_frame_buffer_view(0)),
            S32XMemoryArea::FrameBuffer1 => Box::new(self.s32x_vdp.debug_frame_buffer_view(1)),
            S32XMemoryArea::PaletteRam => Box::new(self.s32x_vdp.debug_palette_ram_view()),
        }
    }
}

impl Sega32XEmulator {
    #[must_use]
    pub fn to_debug_state(&self) -> Sega32XDebugState {
        let sega_32x = self.memory.medium();

        Sega32XDebugState {
            genesis: GenesisDebugState::new(&self.memory, self.vdp.clone()),
            sdram: sega_32x.s32x_bus.sdram.to_vec().into_boxed_slice(),
            sh2_master: sega_32x.clone_sh2_master(),
            sh2_slave: sega_32x.clone_sh2_slave(),
            system_registers: sega_32x.s32x_bus.registers.clone(),
            s32x_vdp: sega_32x.s32x_bus.vdp.clone(),
            pwm: sega_32x.s32x_bus.pwm.clone(),
        }
    }
}
