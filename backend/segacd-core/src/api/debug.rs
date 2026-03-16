use crate::api::SegaCdEmulator;
use crate::cddrive::cdc::Rchip;
use crate::memory::SegaCd;
use crate::memory::wordram::WordRam;
use crate::rf5c164::Rf5c164;
use genesis_core::api::debug::{
    BaseGenesisDebugView, GenesisDebugState, GenesisMemoryArea, PhysicalMediumDebugView,
};
use jgenesis_common::debug::{DebugBytesView, DebugMemoryView};
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SegaCdMemoryArea {
    BiosRom,
    PrgRam,
    WordRam,
    PcmRam,
    CdcRam,
}

#[derive(Debug, Clone, Copy)]
pub enum SegaCdDebugCommand {
    EditGenesisMemory(GenesisMemoryArea, usize, u8),
    EditSegaCdMemory(SegaCdMemoryArea, usize, u8),
}

#[derive(Debug, Clone)]
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

pub struct SegaCdMediumView<'a> {
    pub(crate) bios_rom: &'a mut [u8],
    pub(crate) prg_ram: &'a mut [u8],
    pub(crate) word_ram: &'a mut WordRam,
    pub(crate) cdc: &'a mut Rchip,
}

impl PhysicalMediumDebugView for SegaCdMediumView<'_> {}

pub struct SegaCdEmulatorDebugView<'a> {
    genesis: BaseGenesisDebugView<'a, SegaCdMediumView<'a>>,
    pcm: &'a mut Rf5c164,
}

impl SegaCdEmulatorDebugView<'_> {
    pub fn apply_genesis_memory_edit(
        &mut self,
        memory_area: GenesisMemoryArea,
        address: usize,
        value: u8,
    ) {
        self.genesis.apply_memory_edit(memory_area, address, value);
    }

    pub fn apply_scd_memory_edit(
        &mut self,
        memory_area: SegaCdMemoryArea,
        address: usize,
        value: u8,
    ) {
        match memory_area {
            SegaCdMemoryArea::BiosRom => {
                DebugBytesView(self.genesis.medium_view().bios_rom).write(address, value);
            }
            SegaCdMemoryArea::PrgRam => {
                DebugBytesView(self.genesis.medium_view().prg_ram).write(address, value);
            }
            SegaCdMemoryArea::WordRam => {
                self.genesis.medium_view().word_ram.debug_view().write(address, value);
            }
            SegaCdMemoryArea::PcmRam => {
                self.pcm.debug_ram_view().write(address, value);
            }
            SegaCdMemoryArea::CdcRam => {
                self.genesis.medium_view().cdc.debug_ram_view().write(address, value);
            }
        }
    }

    pub fn to_debug_state(&mut self) -> SegaCdDebugState {
        SegaCdDebugState {
            genesis: self.genesis.to_debug_state(),
            bios_rom: self.genesis.medium_view().bios_rom.to_vec().into_boxed_slice(),
            prg_ram: self.genesis.medium_view().prg_ram.to_vec().into_boxed_slice(),
            word_ram: self.genesis.medium_view().word_ram.clone(),
            pcm: self.pcm.clone(),
            cdc: self.genesis.medium_view().cdc.clone(),
        }
    }
}

impl SegaCdEmulator {
    #[must_use]
    pub fn to_debug_state(&self) -> SegaCdDebugState {
        let sega_cd = self.memory.medium();

        SegaCdDebugState {
            genesis: GenesisDebugState::new(&self.main_cpu, &self.z80, &self.memory, &self.vdp),
            bios_rom: sega_cd.bios().to_vec().into_boxed_slice(),
            prg_ram: sega_cd.clone_prg_ram(),
            word_ram: sega_cd.word_ram().clone(),
            pcm: self.pcm.clone(),
            cdc: sega_cd.clone_cdc(),
        }
    }

    #[must_use]
    pub fn as_debug_view(&mut self) -> SegaCdEmulatorDebugView<'_> {
        SegaCdEmulatorDebugView {
            genesis: BaseGenesisDebugView::new(
                &mut self.main_cpu,
                &mut self.z80,
                self.memory.as_debug_view(SegaCd::as_debug_view),
                &mut self.vdp,
            ),
            pcm: &mut self.pcm,
        }
    }
}

pub struct SegaCdDebugger {
    command_receiver: Receiver<SegaCdDebugCommand>,
}

impl SegaCdDebugger {
    #[must_use]
    pub fn new() -> (Self, Sender<SegaCdDebugCommand>) {
        let (command_sender, command_receiver) = mpsc::channel();

        (Self { command_receiver }, command_sender)
    }

    pub fn process_commands(&mut self, debug_view: &mut SegaCdEmulatorDebugView<'_>) {
        loop {
            match self.command_receiver.try_recv() {
                Ok(command) => match command {
                    SegaCdDebugCommand::EditGenesisMemory(memory_area, address, value) => {
                        debug_view.apply_genesis_memory_edit(memory_area, address, value);
                    }
                    SegaCdDebugCommand::EditSegaCdMemory(memory_area, address, value) => {
                        debug_view.apply_scd_memory_edit(memory_area, address, value);
                    }
                },
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // TODO clear breakpoint/break status; debugger window closed
                    break;
                }
            }
        }
    }
}
