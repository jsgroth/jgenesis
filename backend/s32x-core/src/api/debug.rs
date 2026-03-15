use crate::api::{Sega32XEmulator, Sega32XError};
use crate::core::Sega32X;
use crate::pwm::PwmChip;
use crate::registers::SystemRegisters;
use crate::vdp::Vdp;
use crate::vdp::debug::VdpDebugState;
use crate::{GenesisVdp, WhichCpu};
use bincode::{Decode, Encode};
use genesis_config::GenesisInputs;
use genesis_core::api::debug::{
    BaseGenesisDebugView, GenesisDebugState, GenesisMemoryArea, PhysicalMediumDebugView,
};
use jgenesis_common::debug::{DebugMemoryView, DebugWordsView, Endian};
use jgenesis_common::frontend::{
    AudioOutput, Color, InputPoller, Renderer, SaveWriter, TickResult,
};
use jgenesis_common::sync::SharedVarSender;
use sh2_emu::Sh2;
use sh2_emu::bus::OpSize;
use std::array;
use std::fmt::{Debug, Display};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, SendError, Sender, TryRecvError};
use std::sync::{Arc, mpsc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum S32XMemoryArea {
    Sdram,
    MasterSh2Cache,
    SlaveSh2Cache,
    FrameBuffer0,
    FrameBuffer1,
    PaletteRam,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sh2Breakpoint {
    pub start_address: u32,
    pub end_address: u32,
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct Sh2Breakpoints {
    read_byte: Vec<(u32, u32)>,
    read_word: Vec<(u32, u32)>,
    read_longword: Vec<(u32, u32)>,
    write_byte: Vec<(u32, u32)>,
    write_word: Vec<(u32, u32)>,
    write_longword: Vec<(u32, u32)>,
    execute: Vec<(u32, u32)>,
}

impl Sh2Breakpoints {
    #[must_use]
    pub fn new(breakpoints: &[Sh2Breakpoint]) -> Self {
        let mut read_byte = Vec::new();
        let mut read_word = Vec::new();
        let mut read_longword = Vec::new();
        let mut write_byte = Vec::new();
        let mut write_word = Vec::new();
        let mut write_longword = Vec::new();
        let mut execute = Vec::new();

        for &breakpoint in breakpoints {
            if breakpoint.read {
                read_byte.push((breakpoint.start_address, breakpoint.end_address));
                read_word.push((breakpoint.start_address & !1, breakpoint.end_address & !1));
                read_longword.push((breakpoint.start_address & !3, breakpoint.end_address & !3));
            }

            if breakpoint.write {
                write_byte.push((breakpoint.start_address, breakpoint.end_address));
                write_word.push((breakpoint.start_address & !1, breakpoint.end_address & !1));
                write_longword.push((breakpoint.start_address & !3, breakpoint.end_address & !4));
            }

            if breakpoint.execute {
                execute.push((breakpoint.start_address & !1, breakpoint.end_address & !1));
            }
        }

        Self {
            read_byte,
            read_word,
            read_longword,
            write_byte,
            write_word,
            write_longword,
            execute,
        }
    }

    #[must_use]
    pub fn none() -> Self {
        Self::new(&[])
    }

    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn should_break_read<const SIZE: u8>(&self, address: u32) -> bool {
        let addresses = match SIZE {
            OpSize::BYTE => &self.read_byte,
            OpSize::WORD => &self.read_word,
            OpSize::LONGWORD => &self.read_longword,
            _ => panic!("invalid size {SIZE}"),
        };
        addresses.iter().any(|&(start, end)| (start..=end).contains(&address))
    }

    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn should_break_write<const SIZE: u8>(&self, address: u32) -> bool {
        let addresses = match SIZE {
            OpSize::BYTE => &self.write_byte,
            OpSize::WORD => &self.write_word,
            OpSize::LONGWORD => &self.write_longword,
            _ => panic!("invalid size {SIZE}"),
        };
        addresses.iter().any(|&(start, end)| (start..=end).contains(&address))
    }

    pub fn should_break_execute(&self, address: u32) -> bool {
        self.execute.iter().any(|&(start, end)| (start..=end).contains(&address))
    }
}

#[derive(Debug, Clone)]
pub enum Sega32XDebugCommand {
    EditGenesisMemory(GenesisMemoryArea, usize, u8),
    Edit32XMemory(S32XMemoryArea, usize, u8),
    UpdateBreakpoints(WhichCpu, Vec<Sh2Breakpoint>),
    BreakResume,
    BreakPause(WhichCpu),
    BreakStep(WhichCpu),
}

#[derive(Debug, Clone)]
pub struct Sega32XDebugState {
    pub genesis: GenesisDebugState,
    pub sdram: Box<[u16]>,
    pub sh2_master: Sh2,
    pub sh2_slave: Sh2,
    system_registers: SystemRegisters,
    s32x_vdp: VdpDebugState,
    pwm: PwmChip,
}

impl Sega32XDebugState {
    pub fn genesis(&mut self) -> &mut GenesisDebugState {
        &mut self.genesis
    }

    pub fn sh2(&mut self, which: WhichCpu) -> &mut Sh2 {
        match which {
            WhichCpu::Master => &mut self.sh2_master,
            WhichCpu::Slave => &mut self.sh2_slave,
        }
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
    pub(crate) genesis: BaseGenesisDebugView<'a, Sega32XMediumView<'a>>,
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
            genesis: GenesisDebugState::new(&self.memory, &self.vdp),
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

    /// # Errors
    ///
    /// Propagates any errors returned by the emulator
    pub fn debug_tick<R, A, I, S>(
        &mut self,
        renderer: &mut R,
        audio_output: &mut A,
        input_poller: &mut I,
        save_writer: &mut S,
        debugger: &mut Sega32XDebugger,
    ) -> TickResult<Sega32XError<R::Err, A::Err, S::Err>>
    where
        R: Renderer,
        R::Err: Debug + Display + Send + Sync + 'static,
        A: AudioOutput,
        A::Err: Debug + Display + Send + Sync + 'static,
        I: InputPoller<GenesisInputs>,
        S: SaveWriter,
        S::Err: Debug + Display + Send + Sync + 'static,
    {
        self.tick_inner::<true, _, _, _, _>(
            renderer,
            audio_output,
            input_poller,
            save_writer,
            Some(debugger),
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Sh2BreakStatus {
    pub master: Option<u32>,
    pub slave: Option<u32>,
}

impl Sh2BreakStatus {
    #[must_use]
    pub fn get(&self, which: WhichCpu) -> Option<u32> {
        match which {
            WhichCpu::Master => self.master,
            WhichCpu::Slave => self.slave,
        }
    }
}

pub struct Sh2BreakStatusAtomic {
    pub breaking: [AtomicBool; 2],
    pub break_pc: [AtomicU32; 2],
}

impl Sh2BreakStatusAtomic {
    fn new() -> Self {
        Self {
            breaking: array::from_fn(|_| AtomicBool::new(false)),
            break_pc: array::from_fn(|_| AtomicU32::new(0)),
        }
    }
}

pub struct Sega32XDebugger {
    command_receiver: Receiver<Sega32XDebugCommand>,
    state_sender: SharedVarSender<Sega32XDebugState>,
    last_sh2_pc: [u32; 2],
    breakpoints: [Sh2Breakpoints; 2],
    break_status: Arc<Sh2BreakStatusAtomic>,
    break_step: Option<(WhichCpu, u32)>,
}

pub struct Sega32XDebuggerHandle {
    pub command_sender: Sender<Sega32XDebugCommand>,
    pub break_status: Arc<Sh2BreakStatusAtomic>,
}

impl Sega32XDebuggerHandle {
    /// # Errors
    ///
    /// Propagates any errors returned by the MPSC [`Sender`]
    pub fn send_command(
        &self,
        command: Sega32XDebugCommand,
    ) -> Result<(), SendError<Sega32XDebugCommand>> {
        self.command_sender.send(command)
    }

    fn take_break_status_one(&self, which: WhichCpu) -> Option<u32> {
        if self.break_status.breaking[which as usize].compare_exchange(
            true,
            false,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) != Ok(true)
        {
            return None;
        }

        let pc = self.break_status.break_pc[which as usize].load(Ordering::Acquire);
        Some(pc)
    }

    #[must_use]
    pub fn take_break_status(&self) -> Sh2BreakStatus {
        let master = self.take_break_status_one(WhichCpu::Master);
        let slave = self.take_break_status_one(WhichCpu::Slave);

        Sh2BreakStatus { master, slave }
    }
}

impl Sega32XDebugger {
    #[must_use]
    pub fn new(state_sender: SharedVarSender<Sega32XDebugState>) -> (Self, Sega32XDebuggerHandle) {
        let (command_sender, command_receiver) = mpsc::channel();
        let break_status = Arc::new(Sh2BreakStatusAtomic::new());

        let debugger = Self {
            command_receiver,
            state_sender,
            last_sh2_pc: array::from_fn(|_| 0),
            breakpoints: array::from_fn(|_| Sh2Breakpoints::none()),
            break_status: Arc::clone(&break_status),
            break_step: None,
        };

        let handle = Sega32XDebuggerHandle { command_sender, break_status };

        (debugger, handle)
    }

    pub fn process_commands(&mut self, debug_view: &mut Sega32XEmulatorDebugView<'_>) {
        loop {
            match self.command_receiver.try_recv() {
                Ok(command) => {
                    self.process_command(command, debug_view);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // TODO clear breakpoint/break status; debugger window closed
                    break;
                }
            }
        }
    }

    fn process_command(
        &mut self,
        command: Sega32XDebugCommand,
        debug_view: &mut Sega32XEmulatorDebugView<'_>,
    ) {
        match command {
            Sega32XDebugCommand::EditGenesisMemory(memory_area, address, value) => {
                debug_view.apply_genesis_memory_edit(memory_area, address, value);
            }
            Sega32XDebugCommand::Edit32XMemory(memory_area, address, value) => {
                debug_view.apply_32x_memory_edit(memory_area, address, value);
            }
            Sega32XDebugCommand::UpdateBreakpoints(which, breakpoints) => {
                self.breakpoints[which as usize] = Sh2Breakpoints::new(&breakpoints);
            }
            Sega32XDebugCommand::BreakPause(which) => {
                log::info!("Received pause command for {which:?}");
                self.break_step = Some((which, 1)); // Break at start of next instruction
            }
            Sega32XDebugCommand::BreakResume | Sega32XDebugCommand::BreakStep(_) => {}
        }
    }

    pub(crate) fn breakpoints(&self, which: WhichCpu) -> &Sh2Breakpoints {
        &self.breakpoints[which as usize]
    }

    pub(crate) fn with_genesis_ram<'a>(
        &'a mut self,
        working_ram: &'a mut [u16],
        audio_ram: &'a mut [u8],
    ) -> Sega32XDebuggerGenesisRam<'a> {
        Sega32XDebuggerGenesisRam { debugger: self, working_ram, audio_ram }
    }

    fn set_break_status(&self, which: WhichCpu) {
        let break_idx = which as usize;
        self.break_status.break_pc[break_idx].store(self.last_sh2_pc[break_idx], Ordering::Relaxed);
        self.break_status.breaking[break_idx].store(true, Ordering::Release);
    }

    pub(crate) fn handle_breakpoint(
        &mut self,
        which: WhichCpu,
        debug_view: &mut Sega32XEmulatorDebugView<'_>,
    ) {
        self.state_sender.update(debug_view.to_debug_state());
        self.set_break_status(which);

        self.break_step = None;

        loop {
            match self.command_receiver.recv() {
                Ok(Sega32XDebugCommand::BreakResume) => break,
                Ok(Sega32XDebugCommand::BreakStep(step_which)) => {
                    self.break_step = Some((step_which, 1 + u32::from(step_which != which)));
                    break;
                }
                Ok(command) => self.process_command(command, debug_view),
                Err(_) => {
                    // Debugger window was closed
                    self.breakpoints = array::from_fn(|_| Sh2Breakpoints::none());
                    break;
                }
            }
        }
    }

    pub(crate) fn should_break_on_step(&mut self, which: WhichCpu) -> bool {
        let Some((step_which, remaining)) = &mut self.break_step else { return false };

        if which != *step_which {
            return false;
        }

        *remaining -= 1;
        if *remaining == 0 {
            self.break_step = None;
            true
        } else {
            false
        }
    }

    pub(crate) fn update_sh2_pc(&mut self, which: WhichCpu, pc: u32) {
        self.last_sh2_pc[which as usize] = pc;
    }
}

pub(crate) struct Sega32XDebuggerGenesisRam<'a> {
    pub debugger: &'a mut Sega32XDebugger,
    pub working_ram: &'a mut [u16],
    pub audio_ram: &'a mut [u8],
}

impl Sega32XDebuggerGenesisRam<'_> {
    /// # Safety
    ///
    /// The caller must not touch the values referenced by `self` or `vdp` until after the returned
    /// [`Sega32XDebuggerGenesisRamRaw`] has been dropped.
    pub unsafe fn as_raw(&mut self, vdp: &mut GenesisVdp) -> Sega32XDebuggerGenesisRamRaw {
        Sega32XDebuggerGenesisRamRaw {
            debugger: self.debugger.into(),
            working_ram: self.working_ram.into(),
            audio_ram: self.audio_ram.into(),
            vdp: vdp.into(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Sega32XDebuggerGenesisRamRaw {
    pub debugger: NonNull<Sega32XDebugger>,
    pub working_ram: NonNull<[u16]>,
    pub audio_ram: NonNull<[u8]>,
    pub vdp: NonNull<GenesisVdp>,
}
