use crate::api::Sega32XEmulator;
use crate::core::Sega32X;
use crate::pwm::PwmChip;
use crate::registers::SystemRegisters;
use crate::vdp::Vdp;
use crate::vdp::debug::VdpDebugState;
use genesis_core::api::debug::{
    BaseGenesisDebugView, GenesisDebugState, GenesisMemoryArea, PhysicalMediumDebugView,
};
use jgenesis_common::debug::{DebugMemoryView, DebugWordsView, Endian};
use jgenesis_common::frontend::Color;
use sh2_emu::Sh2;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum S32XMemoryArea {
    Sdram,
    MasterSh2Cache,
    SlaveSh2Cache,
    FrameBuffer0,
    FrameBuffer1,
    PaletteRam,
}

#[derive(Debug, Clone, Copy)]
pub enum Sega32XDebugCommand {
    EditGenesisMemory(GenesisMemoryArea, usize, u8),
    Edit32XMemory(S32XMemoryArea, usize, u8),
}

#[derive(Debug, Clone)]
pub struct Sega32XDebugState {
    genesis: GenesisDebugState,
    sdram: Box<[u16]>,
    sh2_master: Sh2,
    sh2_slave: Sh2,
    system_registers: SystemRegisters,
    s32x_vdp: VdpDebugState,
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

pub struct Sega32XMediumView<'a> {
    pub(crate) cartridge_rom: &'a mut [u16],
    pub(crate) sdram: &'a mut [u16],
    pub(crate) sh2_master: &'a mut Sh2,
    pub(crate) sh2_slave: &'a mut Sh2,
    pub(crate) system_registers: &'a mut SystemRegisters,
    pub(crate) s32x_vdp: &'a mut Vdp,
    pub(crate) pwm: &'a mut PwmChip,
}

impl PhysicalMediumDebugView for Sega32XMediumView<'_> {
    fn debug_cartridge_rom(&mut self) -> Option<&mut [u16]> {
        Some(self.cartridge_rom)
    }
}

pub struct Sega32XEmulatorDebugView<'a> {
    genesis: BaseGenesisDebugView<'a, Sega32XMediumView<'a>>,
}

impl Sega32XEmulatorDebugView<'_> {
    pub fn apply_genesis_memory_edit(
        &mut self,
        memory_area: GenesisMemoryArea,
        address: usize,
        value: u8,
    ) {
        self.genesis.apply_memory_edit(memory_area, address, value);
    }

    pub fn apply_32x_memory_edit(
        &mut self,
        memory_area: S32XMemoryArea,
        address: usize,
        value: u8,
    ) {
        match memory_area {
            S32XMemoryArea::Sdram => {
                DebugWordsView(self.genesis.medium_view().sdram, Endian::Big).write(address, value);
            }
            S32XMemoryArea::MasterSh2Cache => {
                self.genesis.medium_view().sh2_master.debug_cache_view().write(address, value);
            }
            S32XMemoryArea::SlaveSh2Cache => {
                self.genesis.medium_view().sh2_slave.debug_cache_view().write(address, value);
            }
            S32XMemoryArea::FrameBuffer0 => {
                self.genesis
                    .medium_view()
                    .s32x_vdp
                    .debug_frame_buffer_view(0)
                    .write(address, value);
            }
            S32XMemoryArea::FrameBuffer1 => {
                self.genesis
                    .medium_view()
                    .s32x_vdp
                    .debug_frame_buffer_view(1)
                    .write(address, value);
            }
            S32XMemoryArea::PaletteRam => {
                self.genesis.medium_view().s32x_vdp.debug_palette_ram_view().write(address, value);
            }
        }
    }

    pub fn to_debug_state(&mut self) -> Sega32XDebugState {
        Sega32XDebugState {
            genesis: self.genesis.to_debug_state(),
            sdram: self.genesis.medium_view().sdram.to_vec().into_boxed_slice(),
            sh2_master: self.genesis.medium_view().sh2_master.clone(),
            sh2_slave: self.genesis.medium_view().sh2_slave.clone(),
            system_registers: self.genesis.medium_view().system_registers.clone(),
            s32x_vdp: self.genesis.medium_view().s32x_vdp.to_debug_state(),
            pwm: self.genesis.medium_view().pwm.clone(),
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
            s32x_vdp: sega_32x.s32x_bus.vdp.to_debug_state(),
            pwm: sega_32x.s32x_bus.pwm.clone(),
        }
    }

    #[must_use]
    pub fn as_debug_view(&mut self) -> Sega32XEmulatorDebugView<'_> {
        Sega32XEmulatorDebugView {
            genesis: BaseGenesisDebugView::new(
                self.memory.as_debug_view(Sega32X::as_debug_view),
                &mut self.vdp,
            ),
        }
    }
}

pub struct Sega32XDebugger {
    command_receiver: Receiver<Sega32XDebugCommand>,
}

impl Sega32XDebugger {
    #[must_use]
    pub fn new() -> (Self, Sender<Sega32XDebugCommand>) {
        let (command_sender, command_receiver) = mpsc::channel();

        (Self { command_receiver }, command_sender)
    }

    pub fn process_commands(&mut self, debug_view: &mut Sega32XEmulatorDebugView<'_>) {
        loop {
            match self.command_receiver.try_recv() {
                Ok(command) => match command {
                    Sega32XDebugCommand::EditGenesisMemory(memory_area, address, value) => {
                        debug_view.apply_genesis_memory_edit(memory_area, address, value);
                    }
                    Sega32XDebugCommand::Edit32XMemory(memory_area, address, value) => {
                        debug_view.apply_32x_memory_edit(memory_area, address, value);
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
