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
    BaseGenesisDebugView, GenesisDebugState, GenesisMemoryArea, M68000BreakStatus,
    M68000BreakStatusAtomic, M68000Breakpoint, M68000BreakpointManager, M68000Breakpoints,
    PhysicalMediumDebugView, Z80BreakStatus, Z80BreakStatusAtomic, Z80Breakpoint,
    Z80BreakpointManager, Z80Breakpoints,
};
use genesis_core::cartridge::Cartridge;
use genesis_core::ym2612::Ym2612;
use jgenesis_common::debug::{DebugMemoryView, DebugWordsView, Endian};
use jgenesis_common::frontend::{
    AudioOutput, Color, InputPoller, Renderer, SaveWriter, TickResult,
};
use jgenesis_common::sync::SharedVarSender;
use jgenesis_proc_macros::EnumAll;
use m68000_emu::M68000;
use sh2_emu::Sh2;
use sh2_emu::bus::OpSize;
use smsgg_core::psg::Sn76489;
use std::array;
use std::fmt::Debug;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, SendError, Sender, TryRecvError};
use std::sync::{Arc, mpsc};
use z80_emu::Z80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumAll)]
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
    UpdateSh2Breakpoints(WhichCpu, Vec<Sh2Breakpoint>),
    Update68kBreakpoints(Vec<M68000Breakpoint>),
    UpdateZ80Breakpoints(Vec<Z80Breakpoint>),
    BreakResume,
    BreakPauseSh2(WhichCpu),
    BreakStepSh2(WhichCpu),
    BreakPause68k,
    BreakStep68k,
    BreakPauseZ80,
    BreakStepZ80,
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
    #[must_use]
    pub fn genesis(&mut self) -> &mut GenesisDebugState {
        &mut self.genesis
    }

    #[must_use]
    pub fn sh2(&mut self, which: WhichCpu) -> &mut Sh2 {
        match which {
            WhichCpu::Master => &mut self.sh2_master,
            WhichCpu::Slave => &mut self.sh2_slave,
        }
    }

    #[must_use]
    pub fn m68k_rom_bank(&self) -> u8 {
        self.system_registers.m68k_rom_bank
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
    pub(crate) cartridge: &'a mut Cartridge,
    pub(crate) sdram: &'a mut [u16],
    pub(crate) sh2_master: &'a mut Sh2,
    pub(crate) sh2_slave: &'a mut Sh2,
    pub(crate) system_registers: &'a mut SystemRegisters,
    pub(crate) s32x_vdp: &'a mut Vdp,
    pub(crate) pwm: &'a mut PwmChip,
}

impl PhysicalMediumDebugView for Sega32XMediumView<'_> {
    fn debug_cartridge(&mut self) -> Option<&mut Cartridge> {
        Some(self.cartridge)
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
    pub fn as_debug_view(&mut self) -> Sega32XEmulatorDebugView<'_> {
        Sega32XEmulatorDebugView {
            genesis: BaseGenesisDebugView::new(
                &mut self.m68k,
                &mut self.z80,
                self.memory.as_debug_view(Sega32X::as_debug_view),
                &mut self.vdp,
                &mut self.ym2612,
                &mut self.psg,
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
        A: AudioOutput,
        I: InputPoller<GenesisInputs>,
        S: SaveWriter,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sh2BreakStatus {
    pub breaking: bool,
    pub pc: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct S32XSh2BreakStatus {
    pub master: Sh2BreakStatus,
    pub slave: Sh2BreakStatus,
}

impl S32XSh2BreakStatus {
    #[must_use]
    pub fn get(&self, which: WhichCpu) -> Sh2BreakStatus {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugWhichCpu {
    Sh2(WhichCpu),
    M68k,
    Z80,
}

impl DebugWhichCpu {
    fn sh2_which(self) -> Option<WhichCpu> {
        match self {
            Self::Sh2(which) => Some(which),
            Self::M68k | Self::Z80 => None,
        }
    }
}

pub struct Sega32XDebugger {
    command_receiver: Receiver<Sega32XDebugCommand>,
    state_sender: SharedVarSender<Sega32XDebugState>,
    last_sh2_pc: [u32; 2],
    sh2_breakpoints: [Sh2Breakpoints; 2],
    sh2_break_status: Arc<Sh2BreakStatusAtomic>,
    sh2_break_step: Option<(WhichCpu, u32)>,
    m68k_breakpoints: M68000BreakpointManager,
    z80_breakpoints: Z80BreakpointManager,
}

pub struct Sega32XDebuggerHandle {
    pub command_sender: Sender<Sega32XDebugCommand>,
    pub sh2_break_status: Arc<Sh2BreakStatusAtomic>,
    pub m68k_break_status: Arc<M68000BreakStatusAtomic>,
    pub z80_break_status: Arc<Z80BreakStatusAtomic>,
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

    fn sh2_break_status_one(&self, which: WhichCpu) -> Sh2BreakStatus {
        let break_idx = which as usize;
        let breaking = self.sh2_break_status.breaking[break_idx].load(Ordering::Acquire);
        let pc = self.sh2_break_status.break_pc[break_idx].load(Ordering::Relaxed);
        Sh2BreakStatus { breaking, pc }
    }

    #[must_use]
    pub fn sh2_break_status(&self) -> S32XSh2BreakStatus {
        let master = self.sh2_break_status_one(WhichCpu::Master);
        let slave = self.sh2_break_status_one(WhichCpu::Slave);

        S32XSh2BreakStatus { master, slave }
    }

    #[must_use]
    pub fn m68k_break_status(&self) -> M68000BreakStatus {
        self.m68k_break_status.get()
    }

    #[must_use]
    pub fn z80_break_status(&self) -> Z80BreakStatus {
        self.z80_break_status.get()
    }
}

impl Sega32XDebugger {
    #[must_use]
    pub fn new(state_sender: SharedVarSender<Sega32XDebugState>) -> (Self, Sega32XDebuggerHandle) {
        let (command_sender, command_receiver) = mpsc::channel();

        let debugger = Self {
            command_receiver,
            state_sender,
            last_sh2_pc: array::from_fn(|_| 0),
            sh2_breakpoints: array::from_fn(|_| Sh2Breakpoints::none()),
            sh2_break_status: Arc::new(Sh2BreakStatusAtomic::new()),
            sh2_break_step: None,
            m68k_breakpoints: M68000BreakpointManager::new(),
            z80_breakpoints: Z80BreakpointManager::new(),
        };

        let handle = Sega32XDebuggerHandle {
            command_sender,
            sh2_break_status: Arc::clone(&debugger.sh2_break_status),
            m68k_break_status: Arc::clone(&debugger.m68k_breakpoints.status),
            z80_break_status: Arc::clone(&debugger.z80_breakpoints.status),
        };

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
            Sega32XDebugCommand::UpdateSh2Breakpoints(which, breakpoints) => {
                self.sh2_breakpoints[which as usize] = Sh2Breakpoints::new(&breakpoints);
            }
            Sega32XDebugCommand::Update68kBreakpoints(breakpoints) => {
                self.m68k_breakpoints.breakpoints = M68000Breakpoints::new(&breakpoints);
            }
            Sega32XDebugCommand::UpdateZ80Breakpoints(breakpoints) => {
                self.z80_breakpoints.breakpoints = Z80Breakpoints::new(&breakpoints);
            }
            Sega32XDebugCommand::BreakPauseSh2(which) => {
                log::info!("Received pause command for {which:?}");
                self.sh2_break_step = Some((which, 1)); // Break at start of next instruction
            }
            Sega32XDebugCommand::BreakPause68k => {
                log::info!("Received pause command for 68000");
                self.m68k_breakpoints.step = Some(1);
            }
            Sega32XDebugCommand::BreakPauseZ80 => {
                log::info!("Received pause command for Z80");
                self.z80_breakpoints.step = Some(1);
            }
            Sega32XDebugCommand::BreakResume
            | Sega32XDebugCommand::BreakStepSh2(_)
            | Sega32XDebugCommand::BreakStep68k
            | Sega32XDebugCommand::BreakStepZ80 => {}
        }
    }

    pub(crate) fn sh2_breakpoints(&self, which: WhichCpu) -> &Sh2Breakpoints {
        &self.sh2_breakpoints[which as usize]
    }

    pub(crate) fn m68k_breakpoints(&mut self) -> &mut M68000BreakpointManager {
        &mut self.m68k_breakpoints
    }

    pub(crate) fn z80_breakpoints(&mut self) -> &mut Z80BreakpointManager {
        &mut self.z80_breakpoints
    }

    pub(crate) fn for_sh2<'a>(
        &'a mut self,
        m68k: &'a mut M68000,
        z80: &'a mut Z80,
        working_ram: &'a mut [u16],
        audio_ram: &'a mut [u8],
    ) -> Sega32XDebuggerForSh2<'a> {
        Sega32XDebuggerForSh2 { debugger: self, m68k, z80, working_ram, audio_ram }
    }

    pub(crate) fn for_68k<'slf, 'z80, 'ret>(
        &'slf mut self,
        z80: &'z80 mut Z80,
    ) -> Sega32XDebuggerFor68k<'ret>
    where
        'slf: 'ret,
        'z80: 'ret,
    {
        Sega32XDebuggerFor68k { debugger: self, z80 }
    }

    pub(crate) fn for_z80<'slf, 'm68k, 'ret>(
        &'slf mut self,
        m68k: &'m68k mut M68000,
    ) -> Sega32XDebuggerForZ80<'ret>
    where
        'slf: 'ret,
        'm68k: 'ret,
    {
        Sega32XDebuggerForZ80 { debugger: self, m68k }
    }

    fn set_sh2_break_status(&self, which: WhichCpu) {
        let break_idx = which as usize;
        self.sh2_break_status.break_pc[break_idx]
            .store(self.last_sh2_pc[break_idx], Ordering::Relaxed);
        self.sh2_break_status.breaking[break_idx].store(true, Ordering::Release);
    }

    fn clear_sh2_break_status(&self, which: WhichCpu) {
        self.sh2_break_status.breaking[which as usize].store(false, Ordering::Release);
    }

    fn set_68k_break_status(&self) {
        self.m68k_breakpoints.set_break_status();
    }

    fn set_z80_break_status(&self) {
        self.z80_breakpoints.set_break_status();
    }

    pub(crate) fn handle_breakpoint(
        &mut self,
        which: DebugWhichCpu,
        debug_view: &mut Sega32XEmulatorDebugView<'_>,
    ) {
        self.state_sender.update(debug_view.to_debug_state());

        match which {
            DebugWhichCpu::Sh2(which) => {
                self.set_sh2_break_status(which);
            }
            DebugWhichCpu::M68k => {
                self.set_68k_break_status();
            }
            DebugWhichCpu::Z80 => {
                self.set_z80_break_status();
            }
        }

        self.sh2_break_step = None;
        self.m68k_breakpoints.step = None;
        self.z80_breakpoints.step = None;

        loop {
            match self.command_receiver.recv() {
                Ok(Sega32XDebugCommand::BreakResume) => break,
                Ok(Sega32XDebugCommand::BreakStepSh2(step_which)) => {
                    self.sh2_break_step =
                        Some((step_which, 1 + u32::from(which.sh2_which() != Some(step_which))));
                    break;
                }
                Ok(Sega32XDebugCommand::BreakStep68k) => {
                    self.m68k_breakpoints.step = Some(1 + u32::from(which != DebugWhichCpu::M68k));
                    break;
                }
                Ok(Sega32XDebugCommand::BreakStepZ80) => {
                    self.z80_breakpoints.step = Some(1 + u32::from(which != DebugWhichCpu::Z80));
                    break;
                }
                Ok(command) => self.process_command(command, debug_view),
                Err(_) => {
                    // Debugger window was closed
                    self.sh2_breakpoints = array::from_fn(|_| Sh2Breakpoints::none());
                    self.sh2_break_step = None;
                    self.m68k_breakpoints.clear();
                    self.z80_breakpoints.clear();

                    break;
                }
            }
        }

        match which {
            DebugWhichCpu::Sh2(which) => {
                self.clear_sh2_break_status(which);
            }
            DebugWhichCpu::M68k => {
                self.m68k_breakpoints.clear_break_status();
            }
            DebugWhichCpu::Z80 => {
                self.z80_breakpoints.clear_break_status();
            }
        }
    }

    pub(crate) fn check_sh2_break_step(&mut self, which: WhichCpu) -> bool {
        let Some((step_which, remaining)) = &mut self.sh2_break_step else { return false };

        if which != *step_which {
            return false;
        }

        *remaining -= 1;
        if *remaining == 0 {
            self.sh2_break_step = None;
            true
        } else {
            false
        }
    }

    pub(crate) fn check_68k_break_step(&mut self) -> bool {
        check_break_step(&mut self.m68k_breakpoints.step)
    }

    pub(crate) fn check_z80_break_step(&mut self) -> bool {
        self.z80_breakpoints.check_break_step()
    }

    pub(crate) fn update_sh2_pc_and_check_execute(&mut self, which: WhichCpu, pc: u32) -> bool {
        self.last_sh2_pc[which as usize] = pc;
        self.sh2_breakpoints(which).should_break_execute(pc)
    }
}

fn check_break_step(step: &mut Option<u32>) -> bool {
    let Some(remaining) = step else { return false };

    *remaining -= 1;
    if *remaining == 0 {
        *step = None;
        true
    } else {
        false
    }
}

pub(crate) struct GenesisComponents<'a> {
    pub(crate) vdp: &'a mut GenesisVdp,
    pub(crate) ym2612: &'a mut Ym2612,
    pub(crate) psg: &'a mut Sn76489,
}

impl<'a> GenesisComponents<'a> {
    pub fn new(vdp: &'a mut GenesisVdp, ym2612: &'a mut Ym2612, psg: &'a mut Sn76489) -> Self {
        Self { vdp, ym2612, psg }
    }

    pub fn reborrow<'slf, 'ret>(&'slf mut self) -> GenesisComponents<'ret>
    where
        'slf: 'ret,
        'a: 'ret,
    {
        GenesisComponents { vdp: self.vdp, ym2612: self.ym2612, psg: self.psg }
    }
}

pub(crate) struct Sega32XDebuggerForSh2<'a> {
    pub debugger: &'a mut Sega32XDebugger,
    pub m68k: &'a mut M68000,
    pub z80: &'a mut Z80,
    pub working_ram: &'a mut [u16],
    pub audio_ram: &'a mut [u8],
}

impl Sega32XDebuggerForSh2<'_> {
    /// # Safety
    ///
    /// The caller must not touch the values referenced until after the returned
    /// [`Sega32XDebuggerForSh2Raw`] has been dropped.
    pub unsafe fn as_raw(&mut self, components: GenesisComponents<'_>) -> Sega32XDebuggerForSh2Raw {
        Sega32XDebuggerForSh2Raw {
            debugger: self.debugger.into(),
            m68k: self.m68k.into(),
            z80: self.z80.into(),
            working_ram: self.working_ram.into(),
            audio_ram: self.audio_ram.into(),
            vdp: components.vdp.into(),
            ym2612: components.ym2612.into(),
            psg: components.psg.into(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Sega32XDebuggerForSh2Raw {
    pub debugger: NonNull<Sega32XDebugger>,
    pub m68k: NonNull<M68000>,
    pub z80: NonNull<Z80>,
    pub working_ram: NonNull<[u16]>,
    pub audio_ram: NonNull<[u8]>,
    pub vdp: NonNull<GenesisVdp>,
    pub ym2612: NonNull<Ym2612>,
    pub psg: NonNull<Sn76489>,
}

pub(crate) struct Sega32XDebuggerFor68k<'a> {
    pub debugger: &'a mut Sega32XDebugger,
    pub z80: &'a mut Z80,
}

pub(crate) struct Sega32XDebuggerForZ80<'a> {
    pub debugger: &'a mut Sega32XDebugger,
    pub m68k: &'a mut M68000,
}
