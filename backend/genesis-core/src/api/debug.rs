use crate::GenesisEmulator;
use crate::cartridge::Cartridge;
use crate::memory::{Memory, PhysicalMedium};
use crate::vdp::debug::VdpDebugState;
use crate::vdp::{ColorModifier, Vdp};
use jgenesis_common::debug::{
    DebugBytesView, DebugMemoryView, DebugWordsView, EmptyDebugView, Endian,
};
use jgenesis_common::frontend::Color;
use jgenesis_common::sync::SharedVarSender;
use jgenesis_proc_macros::EnumAll;
use m68000_emu::M68000;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, SendError, Sender, TryRecvError};
use std::sync::{Arc, mpsc};
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

#[derive(Debug, Clone)]
pub enum GenesisDebugCommand {
    EditMemory(GenesisMemoryArea, usize, u8),
    Update68kBreakpoints(Vec<M68000Breakpoint>),
    UpdateZ80Breakpoints(Vec<Z80Breakpoint>),
    BreakPause68k,
    BreakPauseZ80,
    BreakResume,
    BreakStep68k,
    BreakStepZ80,
}

#[derive(Debug, Clone)]
pub struct GenesisDebugState {
    m68k: M68000,
    z80: Z80,
    cartridge: Option<Cartridge>,
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
            cartridge: memory.clone_cartridge(),
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
    pub fn z80(&self) -> &Z80 {
        &self.z80
    }

    #[must_use]
    pub fn cartridge(&self) -> Option<&Cartridge> {
        self.cartridge.as_ref()
    }

    #[must_use]
    pub fn cartridge_rom(&self) -> Option<&[u16]> {
        self.cartridge.as_ref().map(Cartridge::debug_rom_view_shared)
    }

    #[must_use]
    pub fn working_ram(&self) -> &[u16] {
        self.working_ram.as_ref()
    }

    #[must_use]
    pub fn audio_ram(&self) -> &[u8] {
        self.audio_ram.as_ref()
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
            GenesisMemoryArea::CartridgeRom => match self.cartridge.as_mut() {
                Some(cartridge) => {
                    Box::new(DebugWordsView(cartridge.debug_rom_view(), Endian::Big))
                }
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
    fn debug_cartridge(&mut self) -> Option<&mut Cartridge> {
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
    pub m68k: &'a mut M68000,
    pub z80: &'a mut Z80,
    pub memory: GenesisMemoryDebugView<'a, MediumView>,
    pub vdp: &'a mut Vdp,
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
                if let Some(cartridge) = self.memory.medium_view.debug_cartridge() {
                    DebugWordsView(cartridge.debug_rom_view(), Endian::Big).write(address, value);
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
            cartridge: self.memory.medium_view.debug_cartridge().map(|cartridge| cartridge.clone()),
            working_ram: self.memory.working_ram.to_vec().into_boxed_slice(),
            audio_ram: self.memory.audio_ram.to_vec().into_boxed_slice(),
            vdp: self.vdp.to_debug_state(),
        }
    }
}

pub struct CartridgeDebugView<'a> {
    pub(crate) cartridge: &'a mut Cartridge,
}

impl PhysicalMediumDebugView for CartridgeDebugView<'_> {
    fn debug_cartridge(&mut self) -> Option<&mut Cartridge> {
        Some(self.cartridge)
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
            memory: self.memory.as_debug_view(|cartridge| CartridgeDebugView { cartridge }),
            vdp: &mut self.vdp,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct M68000Breakpoint {
    pub start_address: u32,
    pub end_address: u32,
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

#[derive(Debug, Clone)]
pub struct M68000Breakpoints {
    read_byte: Vec<(u32, u32)>,
    read_word: Vec<(u32, u32)>,
    write_byte: Vec<(u32, u32)>,
    write_word: Vec<(u32, u32)>,
    execute: Vec<(u32, u32)>,
}

impl M68000Breakpoints {
    #[must_use]
    pub fn new(breakpoints: &[M68000Breakpoint]) -> Self {
        let mut read_byte = Vec::new();
        let mut read_word = Vec::new();
        let mut write_byte = Vec::new();
        let mut write_word = Vec::new();
        let mut execute = Vec::new();

        for &breakpoint in breakpoints {
            if breakpoint.read {
                read_byte.push((breakpoint.start_address, breakpoint.end_address));
                read_word.push((breakpoint.start_address & !1, breakpoint.end_address & !1));
            }

            if breakpoint.write {
                write_byte.push((breakpoint.start_address, breakpoint.end_address));
                write_word.push((breakpoint.start_address & !1, breakpoint.end_address & !1));
            }

            if breakpoint.execute {
                execute.push((breakpoint.start_address & !1, breakpoint.end_address & !1));
            }
        }

        Self { read_byte, read_word, write_byte, write_word, execute }
    }

    #[must_use]
    pub fn none() -> Self {
        Self::new(&[])
    }

    #[must_use]
    pub fn check_read<const WORD: bool>(&self, address: u32) -> bool {
        let ranges = if WORD { &self.read_word } else { &self.read_byte };
        ranges.iter().any(|&(start, end)| (start..=end).contains(&address))
    }

    #[must_use]
    pub fn check_write<const WORD: bool>(&self, address: u32) -> bool {
        let ranges = if WORD { &self.write_word } else { &self.write_byte };
        ranges.iter().any(|&(start, end)| (start..=end).contains(&address))
    }

    #[must_use]
    pub fn check_execute(&self, address: u32) -> bool {
        self.execute.iter().any(|&(start, end)| (start..=end).contains(&address))
    }
}

pub struct M68000BreakStatus {
    pub status: bool,
    pub pc: u32,
}

pub struct M68000BreakStatusAtomic {
    pub breaking: AtomicBool,
    pub pc: AtomicU32,
}

impl M68000BreakStatusAtomic {
    #[must_use]
    pub fn new() -> Self {
        Self { breaking: AtomicBool::new(false), pc: AtomicU32::new(0) }
    }

    #[must_use]
    pub fn take(&self) -> Option<M68000BreakStatus> {
        if self.breaking.compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed)
            != Ok(true)
        {
            return None;
        }

        let pc = self.pc.load(Ordering::Acquire);
        Some(M68000BreakStatus { status: true, pc })
    }

    pub fn set_breaking(&self, pc: u32) {
        self.pc.store(pc, Ordering::Relaxed);
        self.breaking.store(true, Ordering::Release);
    }
}

impl Default for M68000BreakStatusAtomic {
    fn default() -> Self {
        Self::new()
    }
}

pub struct M68000BreakpointManager {
    pub breakpoints: M68000Breakpoints,
    pub last_pc: u32,
    pub status: Arc<M68000BreakStatusAtomic>,
    pub step: Option<u32>,
}

impl M68000BreakpointManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            breakpoints: M68000Breakpoints::none(),
            last_pc: 0,
            status: Arc::new(M68000BreakStatusAtomic::new()),
            step: None,
        }
    }

    pub fn set_break_status(&self) {
        self.status.set_breaking(self.last_pc);
    }

    pub fn clear(&mut self) {
        self.breakpoints = M68000Breakpoints::none();
        self.step = None;
    }
}

impl Default for M68000BreakpointManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Z80Breakpoint {
    pub start_address: u16,
    pub end_address: u16,
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

pub struct Z80Breakpoints {
    read: Vec<(u16, u16)>,
    write: Vec<(u16, u16)>,
    execute: Vec<(u16, u16)>,
}

impl Z80Breakpoints {
    #[must_use]
    pub fn new(breakpoints: &[Z80Breakpoint]) -> Self {
        let mut read = Vec::new();
        let mut write = Vec::new();
        let mut execute = Vec::new();

        for &breakpoint in breakpoints {
            if breakpoint.read {
                read.push((breakpoint.start_address, breakpoint.end_address));
            }

            if breakpoint.write {
                write.push((breakpoint.start_address, breakpoint.end_address));
            }

            if breakpoint.execute {
                execute.push((breakpoint.start_address, breakpoint.end_address));
            }
        }

        Self { read, write, execute }
    }

    #[must_use]
    pub fn none() -> Self {
        Self::new(&[])
    }

    #[must_use]
    pub fn check_read(&self, address: u16) -> bool {
        self.read.iter().any(|&(start, end)| (start..=end).contains(&address))
    }

    #[must_use]
    pub fn check_write(&self, address: u16) -> bool {
        self.write.iter().any(|&(start, end)| (start..=end).contains(&address))
    }

    #[must_use]
    pub fn check_execute(&self, address: u16) -> bool {
        self.execute.iter().any(|&(start, end)| (start..=end).contains(&address))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Z80BreakStatus {
    pub breaking: bool,
    pub pc: u16,
}

pub struct Z80BreakStatusAtomic {
    pub breaking: AtomicBool,
    pub pc: AtomicU16,
}

impl Z80BreakStatusAtomic {
    #[must_use]
    pub fn new() -> Self {
        Self { breaking: AtomicBool::new(false), pc: AtomicU16::new(0) }
    }

    #[must_use]
    pub fn take(&self) -> Option<Z80BreakStatus> {
        if self.breaking.compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed)
            != Ok(true)
        {
            return None;
        }

        let pc = self.pc.load(Ordering::Acquire);
        Some(Z80BreakStatus { breaking: true, pc })
    }

    pub fn set_breaking(&self, pc: u16) {
        self.pc.store(pc, Ordering::Relaxed);
        self.breaking.store(true, Ordering::Release);
    }
}

impl Default for Z80BreakStatusAtomic {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Z80BreakpointManager {
    pub breakpoints: Z80Breakpoints,
    pub status: Arc<Z80BreakStatusAtomic>,
    pub last_pc: u16,
    pub step: Option<u32>,
}

impl Z80BreakpointManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            breakpoints: Z80Breakpoints::none(),
            status: Arc::new(Z80BreakStatusAtomic::new()),
            last_pc: 0,
            step: None,
        }
    }

    pub fn set_break_status(&self) {
        self.status.set_breaking(self.last_pc);
    }

    pub fn clear(&mut self) {
        self.breakpoints = Z80Breakpoints::none();
        self.step = None;
    }
}

impl Default for Z80BreakpointManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenesisCpu {
    M68k,
    Z80,
}

pub struct GenesisDebugger {
    command_receiver: Receiver<GenesisDebugCommand>,
    state_sender: SharedVarSender<GenesisDebugState>,
    m68k_breakpoints: M68000BreakpointManager,
    z80_breakpoints: Z80BreakpointManager,
}

pub struct GenesisDebuggerHandle {
    pub command_sender: Sender<GenesisDebugCommand>,
    pub m68k_break_status: Arc<M68000BreakStatusAtomic>,
    pub z80_break_status: Arc<Z80BreakStatusAtomic>,
}

impl GenesisDebugger {
    #[must_use]
    pub fn new(state_sender: SharedVarSender<GenesisDebugState>) -> (Self, GenesisDebuggerHandle) {
        let (command_sender, command_receiver) = mpsc::channel();

        let debugger = Self {
            command_receiver,
            state_sender,
            m68k_breakpoints: M68000BreakpointManager::new(),
            z80_breakpoints: Z80BreakpointManager::new(),
        };

        let handle = GenesisDebuggerHandle {
            command_sender,
            m68k_break_status: Arc::clone(&debugger.m68k_breakpoints.status),
            z80_break_status: Arc::clone(&debugger.z80_breakpoints.status),
        };

        (debugger, handle)
    }

    #[must_use]
    pub fn m68k_breakpoints(&self) -> &M68000Breakpoints {
        &self.m68k_breakpoints.breakpoints
    }

    #[must_use]
    pub fn z80_breakpoints(&self) -> &Z80Breakpoints {
        &self.z80_breakpoints.breakpoints
    }

    #[must_use]
    pub fn check_68k_break_step(&mut self) -> bool {
        check_break_step(&mut self.m68k_breakpoints.step)
    }

    #[must_use]
    pub fn check_z80_break_step(&mut self) -> bool {
        check_break_step(&mut self.z80_breakpoints.step)
    }

    pub fn update_68k_pc(&mut self, address: u32) {
        self.m68k_breakpoints.last_pc = address;
    }

    pub fn update_z80_pc(&mut self, address: u16) {
        self.z80_breakpoints.last_pc = address;
    }

    pub fn process_commands(&mut self, debug_view: &mut GenesisEmulatorDebugView<'_>) {
        loop {
            match self.command_receiver.try_recv() {
                Ok(command) => self.process_command(command, debug_view),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.m68k_breakpoints.clear();
                    break;
                }
            }
        }
    }

    pub fn process_command(
        &mut self,
        command: GenesisDebugCommand,
        debug_view: &mut GenesisEmulatorDebugView<'_>,
    ) {
        match command {
            GenesisDebugCommand::EditMemory(memory_area, address, value) => {
                debug_view.apply_memory_edit(memory_area, address, value);
            }
            GenesisDebugCommand::Update68kBreakpoints(breakpoints) => {
                self.m68k_breakpoints.breakpoints = M68000Breakpoints::new(&breakpoints);
            }
            GenesisDebugCommand::UpdateZ80Breakpoints(breakpoints) => {
                self.z80_breakpoints.breakpoints = Z80Breakpoints::new(&breakpoints);
            }
            GenesisDebugCommand::BreakPause68k => {
                self.m68k_breakpoints.step = Some(1);
            }
            GenesisDebugCommand::BreakPauseZ80 => {
                self.z80_breakpoints.step = Some(1);
            }
            GenesisDebugCommand::BreakResume
            | GenesisDebugCommand::BreakStep68k
            | GenesisDebugCommand::BreakStepZ80 => {}
        }
    }

    pub fn handle_breakpoint(
        &mut self,
        which: GenesisCpu,
        debug_view: &mut GenesisEmulatorDebugView<'_>,
    ) {
        self.state_sender.update(debug_view.to_debug_state());

        match which {
            GenesisCpu::M68k => {
                self.m68k_breakpoints.set_break_status();
            }
            GenesisCpu::Z80 => {
                self.z80_breakpoints.set_break_status();
            }
        }

        self.m68k_breakpoints.step = None;
        self.z80_breakpoints.step = None;

        loop {
            match self.command_receiver.recv() {
                Ok(GenesisDebugCommand::BreakResume) => break,
                Ok(GenesisDebugCommand::BreakStep68k) => {
                    self.m68k_breakpoints.step = Some(1 + u32::from(which != GenesisCpu::M68k));
                    break;
                }
                Ok(GenesisDebugCommand::BreakStepZ80) => {
                    self.z80_breakpoints.step = Some(1 + u32::from(which != GenesisCpu::Z80));
                    break;
                }
                Ok(command) => self.process_command(command, debug_view),
                Err(_) => {
                    // Debugger window closed
                    self.m68k_breakpoints.clear();
                    self.z80_breakpoints.clear();
                    break;
                }
            }
        }
    }

    pub fn for_68k<'slf, 'z80, 'ret>(
        &'slf mut self,
        z80: &'z80 mut Z80,
    ) -> GenesisDebuggerFor68k<'ret>
    where
        'slf: 'ret,
        'z80: 'ret,
    {
        GenesisDebuggerFor68k { debugger: self, z80 }
    }

    pub fn for_z80<'slf, 'm68k, 'ret>(
        &'slf mut self,
        m68k: &'m68k mut M68000,
    ) -> GenesisDebuggerForZ80<'ret>
    where
        'slf: 'ret,
        'm68k: 'ret,
    {
        GenesisDebuggerForZ80 { debugger: self, m68k }
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

impl GenesisDebuggerHandle {
    /// # Errors
    ///
    /// Propagates any errors from the underlying MPSC [`Sender`]
    pub fn send_command(
        &self,
        command: GenesisDebugCommand,
    ) -> Result<(), SendError<GenesisDebugCommand>> {
        self.command_sender.send(command)
    }

    #[must_use]
    pub fn take_68k_break_status(&self) -> Option<M68000BreakStatus> {
        self.m68k_break_status.take()
    }

    #[must_use]
    pub fn take_z80_break_status(&self) -> Option<Z80BreakStatus> {
        self.z80_break_status.take()
    }
}

pub struct GenesisDebuggerFor68k<'a> {
    pub debugger: &'a mut GenesisDebugger,
    pub z80: &'a mut Z80,
}

pub struct GenesisDebuggerForZ80<'a> {
    pub debugger: &'a mut GenesisDebugger,
    pub m68k: &'a mut M68000,
}
