use crate::GenesisEmulator;
use crate::memory::{Memory, PhysicalMedium};
use crate::vdp::{ColorModifier, Vdp};
use jgenesis_common::debug::{
    DebugBytesView, DebugMemoryView, DebugWordsView, EmptyDebugView, Endian,
};
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

pub struct GenesisDebugState {
    cartridge_rom: Option<Box<[u16]>>,
    working_ram: Box<[u16]>,
    audio_ram: Box<[u8]>,
    vdp: Vdp,
}

impl GenesisDebugState {
    pub fn new<Medium: PhysicalMedium>(memory: &Memory<Medium>, vdp: Vdp) -> Self {
        Self {
            cartridge_rom: memory.clone_cartridge_rom(),
            working_ram: memory.clone_working_ram(),
            audio_ram: memory.clone_audio_ram(),
            vdp,
        }
    }

    pub fn copy_cram(&self, out: &mut [Color], modifier: ColorModifier) {
        self.vdp.copy_cram(out, modifier);
    }

    pub fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        self.vdp.copy_vram(out, palette, row_len);
    }

    pub fn dump_vdp_registers(&self, callback: impl FnMut(&str, &[(&str, &str)])) {
        self.vdp.dump_registers(callback);
    }

    pub fn copy_h_scroll(&self, out: &mut [(u16, u16)]) {
        self.vdp.copy_h_scroll(out);
    }

    pub fn copy_sprite_attributes(
        &self,
        out: &mut [SpriteAttributeEntry],
    ) -> CopySpriteAttributesResult {
        self.vdp.copy_sprite_attributes(out)
    }

    #[must_use]
    pub fn memory_view(&mut self, memory_area: GenesisMemoryArea) -> Box<dyn DebugMemoryView + '_> {
        match memory_area {
            GenesisMemoryArea::CartridgeRom => match self.cartridge_rom.as_mut() {
                Some(cartridge_rom) => Box::new(DebugWordsView(cartridge_rom, Endian::Big)),
                None => Box::new(EmptyDebugView),
            },
            GenesisMemoryArea::WorkingRam => {
                Box::new(DebugWordsView(&mut self.working_ram, Endian::Big))
            }
            GenesisMemoryArea::AudioRam => Box::new(DebugBytesView(&mut self.audio_ram)),
            GenesisMemoryArea::Vram => Box::new(self.vdp.debug_vram_view()),
            GenesisMemoryArea::Cram => Box::new(self.vdp.debug_cram_view()),
            GenesisMemoryArea::Vsram => Box::new(self.vdp.debug_vsram_view()),
        }
    }
}

impl GenesisEmulator {
    #[must_use]
    pub fn to_debug_state(&self) -> GenesisDebugState {
        GenesisDebugState::new(&self.memory, self.vdp.clone())
    }
}
