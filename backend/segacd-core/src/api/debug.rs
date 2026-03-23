use crate::api::{SegaCdEmulator, SegaCdError};
use crate::cddrive::cdc::Rchip;
use crate::memory::wordram::WordRam;
use crate::memory::{ScdCpu, SegaCd};
use crate::rf5c164::Rf5c164;
use genesis_config::GenesisInputs;
use genesis_core::api::debug::{
    BaseGenesisDebugView, GenesisDebugState, GenesisMemoryArea, M68000BreakStatus,
    M68000BreakStatusAtomic, M68000Breakpoint, M68000BreakpointManager, M68000Breakpoints,
    PhysicalMediumDebugView, Z80BreakStatus, Z80BreakStatusAtomic, Z80Breakpoint,
    Z80BreakpointManager, Z80Breakpoints,
};
use genesis_core::memory::MainBus;
use genesis_core::memory::debug::{MainBus68kDebugger, MainBusZ80Debugger};
use genesis_core::vdp::Vdp;
use jgenesis_common::debug::{DebugBytesView, DebugMemoryView};
use jgenesis_common::frontend::{AudioOutput, InputPoller, Renderer, SaveWriter, TickResult};
use jgenesis_common::sync::SharedVarSender;
use m68000_emu::M68000;
use std::sync::mpsc::{Receiver, SendError, Sender, TryRecvError};
use std::sync::{Arc, mpsc};
use z80_emu::Z80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SegaCdMemoryArea {
    BiosRom,
    PrgRam,
    WordRam,
    PcmRam,
    CdcRam,
}

#[derive(Debug, Clone)]
pub enum SegaCdDebugCommand {
    EditGenesisMemory(GenesisMemoryArea, usize, u8),
    EditSegaCdMemory(SegaCdMemoryArea, usize, u8),
    UpdateMain68kBreakpoints(Vec<M68000Breakpoint>),
    UpdateSub68kBreakpoints(Vec<M68000Breakpoint>),
    UpdateZ80Breakpoints(Vec<Z80Breakpoint>),
    BreakResume,
    BreakPauseMain68k,
    BreakPauseSub68k,
    BreakPauseZ80,
    BreakStepMain68k,
    BreakStepSub68k,
    BreakStepZ80,
}

#[derive(Debug, Clone)]
pub struct SegaCdDebugState {
    pub genesis: GenesisDebugState,
    sub_cpu: M68000,
    bios_rom: Box<[u8]>,
    prg_ram: Box<[u8]>,
    word_ram: WordRam,
    pcm: Rf5c164,
    cdc: Rchip,
    prg_ram_bank: u8,
}

impl SegaCdDebugState {
    pub fn genesis(&mut self) -> &mut GenesisDebugState {
        &mut self.genesis
    }

    #[must_use]
    pub fn sub_cpu(&self) -> &M68000 {
        &self.sub_cpu
    }

    #[must_use]
    pub fn bios_rom(&self) -> &[u8] {
        &self.bios_rom
    }

    #[must_use]
    pub fn prg_ram(&self) -> &[u8] {
        &self.prg_ram
    }

    #[must_use]
    pub fn main_cpu_prg_ram_bank(&self) -> u8 {
        self.prg_ram_bank
    }

    #[must_use]
    pub fn word_ram(&self) -> &WordRam {
        &self.word_ram
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
    pub(crate) prg_ram_bank: u8,
}

impl PhysicalMediumDebugView for SegaCdMediumView<'_> {}

pub struct SegaCdEmulatorDebugView<'a> {
    pub(crate) genesis: BaseGenesisDebugView<'a, SegaCdMediumView<'a>>,
    pub(crate) sub_cpu: &'a mut M68000,
    pub(crate) pcm: &'a mut Rf5c164,
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
            sub_cpu: self.sub_cpu.clone(),
            bios_rom: self.genesis.medium_view().bios_rom.to_vec().into_boxed_slice(),
            prg_ram: self.genesis.medium_view().prg_ram.to_vec().into_boxed_slice(),
            word_ram: self.genesis.medium_view().word_ram.clone(),
            pcm: self.pcm.clone(),
            cdc: self.genesis.medium_view().cdc.clone(),
            prg_ram_bank: self.genesis.medium_view().prg_ram_bank,
        }
    }
}

impl SegaCdEmulator {
    #[must_use]
    pub fn as_debug_view(&mut self) -> SegaCdEmulatorDebugView<'_> {
        SegaCdEmulatorDebugView {
            genesis: BaseGenesisDebugView::new(
                &mut self.main_cpu,
                &mut self.z80,
                self.memory.as_debug_view(SegaCd::as_debug_view),
                &mut self.vdp,
            ),
            sub_cpu: &mut self.sub_cpu,
            pcm: &mut self.pcm,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BreakWhichCpu {
    Main,
    Sub,
    Z80,
}

pub struct SegaCdDebugger {
    command_receiver: Receiver<SegaCdDebugCommand>,
    state_sender: SharedVarSender<SegaCdDebugState>,
    main_cpu_breakpoints: M68000BreakpointManager,
    sub_cpu_breakpoints: M68000BreakpointManager,
    z80_breakpoints: Z80BreakpointManager,
}

#[derive(Clone)]
pub struct SegaCdDebuggerHandle {
    command_sender: Sender<SegaCdDebugCommand>,
    main_cpu_break_status: Arc<M68000BreakStatusAtomic>,
    sub_cpu_break_status: Arc<M68000BreakStatusAtomic>,
    z80_break_status: Arc<Z80BreakStatusAtomic>,
}

impl SegaCdDebugger {
    #[must_use]
    pub fn new(state_sender: SharedVarSender<SegaCdDebugState>) -> (Self, SegaCdDebuggerHandle) {
        let (command_sender, command_receiver) = mpsc::channel();

        let debugger = Self {
            command_receiver,
            state_sender,
            main_cpu_breakpoints: M68000BreakpointManager::new(),
            sub_cpu_breakpoints: M68000BreakpointManager::new(),
            z80_breakpoints: Z80BreakpointManager::new(),
        };

        let handle = SegaCdDebuggerHandle {
            command_sender,
            main_cpu_break_status: Arc::clone(&debugger.main_cpu_breakpoints.status),
            sub_cpu_break_status: Arc::clone(&debugger.sub_cpu_breakpoints.status),
            z80_break_status: Arc::clone(&debugger.z80_breakpoints.status),
        };

        (debugger, handle)
    }

    pub fn process_commands(&mut self, debug_view: &mut SegaCdEmulatorDebugView<'_>) {
        loop {
            match self.command_receiver.try_recv() {
                Ok(command) => self.process_command(command, debug_view),
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
        command: SegaCdDebugCommand,
        debug_view: &mut SegaCdEmulatorDebugView<'_>,
    ) {
        match command {
            SegaCdDebugCommand::EditGenesisMemory(memory_area, address, value) => {
                debug_view.apply_genesis_memory_edit(memory_area, address, value);
            }
            SegaCdDebugCommand::EditSegaCdMemory(memory_area, address, value) => {
                debug_view.apply_scd_memory_edit(memory_area, address, value);
            }
            SegaCdDebugCommand::UpdateMain68kBreakpoints(breakpoints) => {
                self.main_cpu_breakpoints.breakpoints = M68000Breakpoints::new(&breakpoints);
            }
            SegaCdDebugCommand::UpdateSub68kBreakpoints(breakpoints) => {
                self.sub_cpu_breakpoints.breakpoints = M68000Breakpoints::new(&breakpoints);
            }
            SegaCdDebugCommand::UpdateZ80Breakpoints(breakpoints) => {
                self.z80_breakpoints.breakpoints = Z80Breakpoints::new(&breakpoints);
            }
            SegaCdDebugCommand::BreakPauseMain68k => {
                self.main_cpu_breakpoints.step = Some(1);
            }
            SegaCdDebugCommand::BreakPauseSub68k => {
                self.sub_cpu_breakpoints.step = Some(1);
            }
            SegaCdDebugCommand::BreakPauseZ80 => {
                self.z80_breakpoints.step = Some(1);
            }
            SegaCdDebugCommand::BreakResume
            | SegaCdDebugCommand::BreakStepMain68k
            | SegaCdDebugCommand::BreakStepSub68k
            | SegaCdDebugCommand::BreakStepZ80 => {}
        }
    }

    pub(crate) fn handle_breakpoint(
        &mut self,
        which: BreakWhichCpu,
        debug_view: &mut SegaCdEmulatorDebugView<'_>,
    ) {
        self.state_sender.update(debug_view.to_debug_state());

        match which {
            BreakWhichCpu::Main => self.main_cpu_breakpoints.set_break_status(),
            BreakWhichCpu::Sub => self.sub_cpu_breakpoints.set_break_status(),
            BreakWhichCpu::Z80 => self.z80_breakpoints.set_break_status(),
        }

        self.main_cpu_breakpoints.step = None;
        self.sub_cpu_breakpoints.step = None;
        self.z80_breakpoints.step = None;

        loop {
            match self.command_receiver.recv() {
                Ok(SegaCdDebugCommand::BreakResume) => break,
                Ok(SegaCdDebugCommand::BreakStepMain68k) => {
                    self.main_cpu_breakpoints.step =
                        Some(1 + u32::from(which != BreakWhichCpu::Main));
                    break;
                }
                Ok(SegaCdDebugCommand::BreakStepSub68k) => {
                    self.sub_cpu_breakpoints.step =
                        Some(1 + u32::from(which != BreakWhichCpu::Sub));
                    break;
                }
                Ok(SegaCdDebugCommand::BreakStepZ80) => {
                    self.z80_breakpoints.step = Some(1 + u32::from(which != BreakWhichCpu::Z80));
                    break;
                }
                Ok(command) => self.process_command(command, debug_view),
                Err(_) => {
                    // Debugger window was closed
                    self.main_cpu_breakpoints.clear();
                    self.sub_cpu_breakpoints.clear();
                    self.z80_breakpoints.clear();
                    break;
                }
            }
        }

        match which {
            BreakWhichCpu::Main => self.main_cpu_breakpoints.clear_break_status(),
            BreakWhichCpu::Sub => self.sub_cpu_breakpoints.clear_break_status(),
            BreakWhichCpu::Z80 => self.z80_breakpoints.clear_break_status(),
        }
    }

    pub(crate) fn m68k_breakpoints(&mut self, which: ScdCpu) -> &mut M68000BreakpointManager {
        match which {
            ScdCpu::Main => &mut self.main_cpu_breakpoints,
            ScdCpu::Sub => &mut self.sub_cpu_breakpoints,
        }
    }

    pub(crate) fn check_sub_break_step(&mut self) -> bool {
        self.sub_cpu_breakpoints.check_break_step()
    }

    pub(crate) fn for_main_cpu<'slf, 'emu, 'ret>(
        &'slf mut self,
        sub_cpu: &'emu mut M68000,
        z80: &'emu mut Z80,
        pcm: &'emu mut Rf5c164,
    ) -> SegaCdDebuggerForMainCpu<'ret>
    where
        'slf: 'ret,
        'emu: 'ret,
    {
        SegaCdDebuggerForMainCpu { debugger: self, sub_cpu, z80, pcm }
    }

    pub(crate) fn for_sub_cpu<'slf, 'emu, 'ret>(
        &'slf mut self,
        main_cpu: &'emu mut M68000,
        z80: &'emu mut Z80,
        vdp: &'emu mut Vdp,
    ) -> SegaCdDebuggerForSubCpu<'ret>
    where
        'slf: 'ret,
        'emu: 'ret,
    {
        SegaCdDebuggerForSubCpu { debugger: self, main_cpu, z80, vdp }
    }

    pub(crate) fn for_z80<'slf, 'emu, 'ret>(
        &'slf mut self,
        main_cpu: &'emu mut M68000,
        sub_cpu: &'emu mut M68000,
        pcm: &'emu mut Rf5c164,
    ) -> SegaCdDebuggerForZ80<'ret>
    where
        'slf: 'ret,
        'emu: 'ret,
    {
        SegaCdDebuggerForZ80 { debugger: self, main_cpu, sub_cpu, pcm }
    }
}

impl SegaCdDebuggerHandle {
    /// # Errors
    ///
    /// Propagates any errors from the underlying MPSC [`Sender`].
    pub fn send_command(
        &self,
        command: SegaCdDebugCommand,
    ) -> Result<(), SendError<SegaCdDebugCommand>> {
        self.command_sender.send(command)
    }

    #[must_use]
    pub fn main_cpu_break_status(&self) -> M68000BreakStatus {
        self.main_cpu_break_status.get()
    }

    #[must_use]
    pub fn sub_cpu_break_status(&self) -> M68000BreakStatus {
        self.sub_cpu_break_status.get()
    }

    #[must_use]
    pub fn z80_break_status(&self) -> Z80BreakStatus {
        self.z80_break_status.get()
    }
}

impl SegaCdEmulator {
    /// # Errors
    ///
    /// Propagates any errors encountered while rendering, pushing audio samples, or writing save files.
    pub fn debug_tick<R, A, I, S>(
        &mut self,
        renderer: &mut R,
        audio_output: &mut A,
        input_poller: &mut I,
        save_writer: &mut S,
        debugger: &mut SegaCdDebugger,
    ) -> TickResult<SegaCdError<R::Err, A::Err, S::Err>>
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

pub(crate) struct SegaCdDebuggerForMainCpu<'a> {
    debugger: &'a mut SegaCdDebugger,
    sub_cpu: &'a mut M68000,
    z80: &'a mut Z80,
    pcm: &'a mut Rf5c164,
}

impl MainBus68kDebugger<SegaCd> for SegaCdDebuggerForMainCpu<'_> {
    fn check_read_breakpoint<const WORD: bool>(&mut self, address: u32) -> bool {
        self.debugger.main_cpu_breakpoints.check_read::<WORD>(address)
    }

    fn check_write_breakpoint<const WORD: bool>(&mut self, address: u32) -> bool {
        self.debugger.main_cpu_breakpoints.check_write::<WORD>(address)
    }

    fn check_execute_breakpoint(&mut self, pc: u32) -> bool {
        self.debugger.main_cpu_breakpoints.update_pc_and_check_execute(pc)
    }

    fn check_break_step(&mut self) -> bool {
        self.debugger.main_cpu_breakpoints.check_break_step()
    }

    fn handle_breakpoint<const REFRESH_INTERVAL: u32>(
        &mut self,
        cpu: &mut M68000,
        bus: &mut MainBus<'_, SegaCd, REFRESH_INTERVAL>,
    ) {
        let mut debug_view = SegaCdEmulatorDebugView {
            genesis: BaseGenesisDebugView {
                m68k: cpu,
                z80: self.z80,
                memory: bus.memory.as_debug_view(SegaCd::as_debug_view),
                vdp: bus.vdp,
            },
            sub_cpu: self.sub_cpu,
            pcm: self.pcm,
        };
        self.debugger.handle_breakpoint(BreakWhichCpu::Main, &mut debug_view);
    }
}

pub(crate) struct SegaCdDebuggerForZ80<'a> {
    debugger: &'a mut SegaCdDebugger,
    main_cpu: &'a mut M68000,
    sub_cpu: &'a mut M68000,
    pcm: &'a mut Rf5c164,
}

impl MainBusZ80Debugger<SegaCd> for SegaCdDebuggerForZ80<'_> {
    fn check_read_breakpoint(&mut self, address: u16) -> bool {
        self.debugger.z80_breakpoints.check_read(address)
    }

    fn check_write_breakpoint(&mut self, address: u16) -> bool {
        self.debugger.z80_breakpoints.check_write(address)
    }

    fn check_execute_breakpoint(&mut self, pc: u16) -> bool {
        self.debugger.z80_breakpoints.update_pc_and_check_execute(pc)
    }

    fn check_break_step(&mut self) -> bool {
        self.debugger.z80_breakpoints.check_break_step()
    }

    fn handle_breakpoint<const REFRESH_INTERVAL: u32>(
        &mut self,
        cpu: &mut Z80,
        bus: &mut MainBus<'_, SegaCd, REFRESH_INTERVAL>,
    ) {
        let mut debug_view = SegaCdEmulatorDebugView {
            genesis: BaseGenesisDebugView {
                m68k: self.main_cpu,
                z80: cpu,
                memory: bus.memory.as_debug_view(SegaCd::as_debug_view),
                vdp: bus.vdp,
            },
            sub_cpu: self.sub_cpu,
            pcm: self.pcm,
        };
        self.debugger.handle_breakpoint(BreakWhichCpu::Z80, &mut debug_view);
    }
}

pub struct SegaCdDebuggerForSubCpu<'a> {
    pub(crate) debugger: &'a mut SegaCdDebugger,
    pub(crate) main_cpu: &'a mut M68000,
    pub(crate) z80: &'a mut Z80,
    pub(crate) vdp: &'a mut Vdp,
}
