use crate::api::PcEngineEmulator;
use jgenesis_common::debug::DebugMemoryView;
use jgenesis_proc_macros::EnumAll;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumAll)]
pub enum PceMemoryArea {
    HuCardRom,
    WorkingRam,
    CdRomRam,
    AdpcmRam,
    SuperSystemCardRam,
    ArcadeCardRam,
}

impl PceMemoryArea {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::HuCardRom => "Cartridge ROM",
            Self::WorkingRam => "Working RAM",
            Self::CdRomRam => "CD-ROM Working RAM",
            Self::AdpcmRam => "ADPCM RAM",
            Self::SuperSystemCardRam => "Super System Card RAM",
            Self::ArcadeCardRam => "Arcade Card RAM",
        }
    }
}

impl PcEngineEmulator {
    pub fn debug_memory_view(
        &mut self,
        memory_area: PceMemoryArea,
    ) -> Option<Box<dyn DebugMemoryView + '_>> {
        match memory_area {
            PceMemoryArea::HuCardRom => Some(Box::new(self.cartridge.debug_rom_view())),
            PceMemoryArea::WorkingRam => Some(Box::new(self.memory.debug_working_ram_view())),
            PceMemoryArea::CdRomRam => match self.cd.as_mut() {
                Some(cd) => Some(Box::new(cd.debug_working_ram_view())),
                None => None,
            },
            PceMemoryArea::AdpcmRam => match self.cd.as_mut() {
                Some(cd) => Some(Box::new(cd.debug_adpcm_ram_view())),
                None => None,
            },
            PceMemoryArea::SuperSystemCardRam => self.cartridge.debug_super_syscard_ram_view(),
            PceMemoryArea::ArcadeCardRam => self.cartridge.debug_arcade_ram_view(),
        }
    }
}
