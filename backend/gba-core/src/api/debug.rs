use crate::api::GameBoyAdvanceEmulator;
use jgenesis_common::debug::DebugMemoryView;
use jgenesis_common::frontend::Color;
use jgenesis_proc_macros::EnumAll;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumAll)]
pub enum GbaMemoryArea {
    CartridgeRom,
    Ewram,
    Iwram,
    Vram,
    PaletteRam,
    Oam,
    BiosRom,
}

impl GbaMemoryArea {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::CartridgeRom => "Cartridge ROM",
            Self::Ewram => "EWRAM",
            Self::Iwram => "IWRAM",
            Self::Vram => "VRAM",
            Self::PaletteRam => "Palette RAM",
            Self::Oam => "OAM",
            Self::BiosRom => "BIOS ROM",
        }
    }
}

pub struct GbaDebugView<'emu>(&'emu mut GameBoyAdvanceEmulator);

impl GameBoyAdvanceEmulator {
    #[must_use]
    pub fn debug(&mut self) -> GbaDebugView<'_> {
        GbaDebugView(self)
    }
}

impl<'emu> GbaDebugView<'emu> {
    #[must_use]
    pub fn memory_view(self, area: GbaMemoryArea) -> Box<dyn DebugMemoryView + 'emu> {
        match area {
            GbaMemoryArea::CartridgeRom => Box::new(self.0.bus.cartridge.debug_rom_view()),
            GbaMemoryArea::Ewram => Box::new(self.0.bus.memory.debug_ewram_view()),
            GbaMemoryArea::Iwram => Box::new(self.0.bus.memory.debug_iwram_view()),
            GbaMemoryArea::Vram => Box::new(self.0.bus.ppu.debug_vram_view()),
            GbaMemoryArea::PaletteRam => Box::new(self.0.bus.ppu.debug_palette_view()),
            GbaMemoryArea::Oam => Box::new(self.0.bus.ppu.debug_oam_view()),
            GbaMemoryArea::BiosRom => Box::new(self.0.bus.memory.debug_bios_view()),
        }
    }
    
    pub fn copy_palette_ram(&self, out: &mut [Color]) {
        self.0.bus.ppu.copy_palette_ram(out);
    }
}
