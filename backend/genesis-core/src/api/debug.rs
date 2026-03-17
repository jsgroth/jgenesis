use crate::GenesisEmulator;
use crate::memory::{Memory, PhysicalMedium};
use crate::vdp::debug::VdpDebugState;
use crate::vdp::{ColorModifier, Vdp};
use jgenesis_common::debug::{
    DebugBytesView, DebugMemoryView, DebugWordsView, EmptyDebugView, Endian,
};
use jgenesis_common::frontend::Color;
use jgenesis_proc_macros::EnumAll;
use m68000_emu::M68000;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use z80_emu::Z80;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumAll)]
pub enum GenesisMemoryArea {
    CartridgeRom,
    WorkingRam,
    AudioRam,
    Vram,
    Cram,
    Vsram,
}

#[derive(Debug, Clone, Copy)]
pub enum GenesisDebugCommand {
    EditMemory(GenesisMemoryArea, usize, u8),
}

#[derive(Debug, Clone)]
pub struct GenesisDebugState {
    m68k: M68000,
    z80: Z80,
    cartridge_rom: Option<Box<[u16]>>,
    working_ram: Box<[u16]>,
    audio_ram: Box<[u8]>,
    vdp: VdpDebugState,
}

impl GenesisDebugState {
    pub fn new<Medium: PhysicalMedium>(
        m68k: &M68000,
        z80: &Z80,
        memory: &Memory<Medium>,
        vdp: &Vdp,
    ) -> Self {
        Self {
            m68k: m68k.clone(),
            z80: z80.clone(),
            cartridge_rom: memory.clone_cartridge_rom(),
            working_ram: memory.clone_working_ram(),
            audio_ram: memory.clone_audio_ram(),
            vdp: vdp.to_debug_state(),
        }
    }

    #[must_use]
    pub fn m68k(&self) -> &M68000 {
        &self.m68k
    }

    #[must_use]
    pub fn z80(&mut self) -> &Z80 {
        &self.z80
    }

    #[must_use]
    pub fn cartridge_rom(&self) -> Option<&[u16]> {
        self.cartridge_rom.as_deref()
    }

    #[must_use]
    pub fn working_ram(&self) -> &[u16] {
        self.working_ram.as_ref()
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

pub trait PhysicalMediumDebugView {
    fn debug_cartridge_rom(&mut self) -> Option<&mut [u16]> {
        None
    }
}

pub struct GenesisMemoryDebugView<'a, MediumView> {
    pub medium_view: MediumView,
    pub working_ram: &'a mut [u16],
    pub audio_ram: &'a mut [u8],
}

impl<MediumView: PhysicalMediumDebugView> GenesisMemoryDebugView<'_, MediumView> {
    pub fn medium_view(&mut self) -> &mut MediumView {
        &mut self.medium_view
    }
}

pub struct BaseGenesisDebugView<'a, MediumView> {
    m68k: &'a mut M68000,
    z80: &'a mut Z80,
    memory: GenesisMemoryDebugView<'a, MediumView>,
    vdp: &'a mut Vdp,
}

impl<'a, MediumView: PhysicalMediumDebugView> BaseGenesisDebugView<'a, MediumView> {
    pub fn new(
        m68k: &'a mut M68000,
        z80: &'a mut Z80,
        memory: GenesisMemoryDebugView<'a, MediumView>,
        vdp: &'a mut Vdp,
    ) -> Self {
        Self { m68k, z80, memory, vdp }
    }

    pub fn memory(&mut self) -> &mut GenesisMemoryDebugView<'a, MediumView> {
        &mut self.memory
    }

    pub fn medium_view(&mut self) -> &mut MediumView {
        &mut self.memory.medium_view
    }

    pub fn apply_memory_edit(&mut self, memory_area: GenesisMemoryArea, address: usize, value: u8) {
        match memory_area {
            GenesisMemoryArea::CartridgeRom => {
                if let Some(rom) = self.memory.medium_view.debug_cartridge_rom() {
                    DebugWordsView(rom, Endian::Big).write(address, value);
                }
            }
            GenesisMemoryArea::WorkingRam => {
                DebugWordsView(self.memory.working_ram, Endian::Big).write(address, value);
            }
            GenesisMemoryArea::AudioRam => {
                DebugBytesView(self.memory.audio_ram).write(address, value);
            }
            GenesisMemoryArea::Vram => self.vdp.debug_vram_view().write(address, value),
            GenesisMemoryArea::Cram => self.vdp.debug_cram_view().write(address, value),
            GenesisMemoryArea::Vsram => self.vdp.debug_vsram_view().write(address, value),
        }
    }

    pub fn to_debug_state(&mut self) -> GenesisDebugState {
        GenesisDebugState {
            m68k: self.m68k.clone(),
            z80: self.z80.clone(),
            cartridge_rom: self
                .memory
                .medium_view
                .debug_cartridge_rom()
                .map(|rom| rom.to_vec().into_boxed_slice()),
            working_ram: self.memory.working_ram.to_vec().into_boxed_slice(),
            audio_ram: self.memory.audio_ram.to_vec().into_boxed_slice(),
            vdp: self.vdp.to_debug_state(),
        }
    }
}

pub struct CartridgeDebugView<'a> {
    pub(crate) rom: &'a mut [u16],
}

impl PhysicalMediumDebugView for CartridgeDebugView<'_> {
    fn debug_cartridge_rom(&mut self) -> Option<&mut [u16]> {
        Some(self.rom)
    }
}

pub type GenesisEmulatorDebugView<'a> = BaseGenesisDebugView<'a, CartridgeDebugView<'a>>;

impl GenesisEmulator {
    #[must_use]
    pub fn to_debug_state(&self) -> GenesisDebugState {
        GenesisDebugState::new(&self.m68k, &self.z80, &self.memory, &self.vdp)
    }

    #[must_use]
    pub fn as_debug_view(&mut self) -> GenesisEmulatorDebugView<'_> {
        GenesisEmulatorDebugView {
            m68k: &mut self.m68k,
            z80: &mut self.z80,
            memory: self
                .memory
                .as_debug_view(|cartridge| CartridgeDebugView { rom: cartridge.debug_rom_view() }),
            vdp: &mut self.vdp,
        }
    }
}

pub struct GenesisDebugger {
    command_receiver: Receiver<GenesisDebugCommand>,
}

impl GenesisDebugger {
    #[must_use]
    pub fn new() -> (Self, Sender<GenesisDebugCommand>) {
        let (command_sender, command_receiver) = mpsc::channel();

        (Self { command_receiver }, command_sender)
    }

    pub fn process_commands(&mut self, debug_view: &mut GenesisEmulatorDebugView<'_>) {
        loop {
            match self.command_receiver.try_recv() {
                Ok(command) => match command {
                    GenesisDebugCommand::EditMemory(memory_area, address, value) => {
                        debug_view.apply_memory_edit(memory_area, address, value);
                    }
                },
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // TODO debugger window was closed; clear breakpoints and break status
                    break;
                }
            }
        }
    }
}
