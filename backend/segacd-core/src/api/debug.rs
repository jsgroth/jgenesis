use crate::api::SegaCdEmulator;
use crate::cddrive::cdc::Rchip;
use crate::memory::wordram::WordRam;
use crate::rf5c164::Rf5c164;
use genesis_core::api::debug::GenesisDebugState;
use jgenesis_common::debug::{DebugBytesView, DebugMemoryView};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SegaCdMemoryArea {
    BiosRom,
    PrgRam,
    WordRam,
    PcmRam,
    CdcRam,
}

pub struct SegaCdDebugState {
    genesis: GenesisDebugState,
    bios_rom: Box<[u8]>,
    prg_ram: Box<[u8]>,
    word_ram: WordRam,
    pcm: Rf5c164,
    cdc: Rchip,
}

impl SegaCdDebugState {
    pub fn genesis(&mut self) -> &mut GenesisDebugState {
        &mut self.genesis
    }

    #[must_use]
    pub fn scd_memory_view(
        &mut self,
        memory_area: SegaCdMemoryArea,
    ) -> Box<dyn DebugMemoryView + '_> {
        match memory_area {
            SegaCdMemoryArea::BiosRom => Box::new(DebugBytesView(&mut self.bios_rom)),
            SegaCdMemoryArea::PrgRam => Box::new(DebugBytesView(&mut self.prg_ram)),
            SegaCdMemoryArea::WordRam => Box::new(self.word_ram.debug_view()),
            SegaCdMemoryArea::PcmRam => Box::new(self.pcm.debug_ram_view()),
            SegaCdMemoryArea::CdcRam => Box::new(self.cdc.debug_ram_view()),
        }
    }
}

impl SegaCdEmulator {
    #[must_use]
    pub fn to_debug_state(&self) -> SegaCdDebugState {
        let sega_cd = self.memory.medium();

        SegaCdDebugState {
            genesis: GenesisDebugState::new(&self.memory, self.vdp.clone()),
            bios_rom: sega_cd.bios().to_vec().into_boxed_slice(),
            prg_ram: sega_cd.clone_prg_ram(),
            word_ram: sega_cd.word_ram().clone(),
            pcm: self.pcm.clone(),
            cdc: sega_cd.clone_cdc(),
        }
    }
}
