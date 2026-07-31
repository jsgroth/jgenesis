use crate::GenesisEmulator;
use crate::bus::PendingWrites;
use genesis_components::cartridge::Cartridge;
use genesis_components::vdp::Vdp;
use genesis_components::vdp::debug::VdpDebugState;
use genesis_components::ym2612::Ym2612;
use jgenesis_common::debug::{
    DebugBytesView, DebugMemoryView, DebugWordsView, EmptyDebugView, Endian,
};
use jgenesis_common::sync::SharedVarSender;
use m68000_emu::M68000;
use s32x_core::WhichCpu;
use s32x_core::api::Sega32X;
use s32x_core::api::debug::{S32XMemoryArea, Sega32XDebugState, Sega32XDebugView, Sega32XDebugger};
use segacd_core::api::SegaCd;
use segacd_core::api::debug::{
    SegaCdDebugState, SegaCdDebugView, SegaCdDebugger, SegaCdMemoryArea,
};
use sh2_emu::bus::OpSizeEnum;
use std::ops::Deref;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, SendError, Sender, TryRecvError};
use std::sync::{Arc, mpsc};
use std::{array, ptr};
use ti_sn76489::Sn76489;
use z80_emu::Z80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GenesisMemoryArea {
    CartridgeRom,
    WorkingRam,
    AudioRam,
    Vram,
    Cram,
    Vsram,
}

#[derive(Debug, Clone, Copy)]
pub enum DebugPendingWrite {
    Word { address: u32, value: u16 },
    Byte { address: u32, value: u8 },
}

#[derive(Debug, Clone)]
pub struct GenesisDebugState {
    pub sega_cd: Option<SegaCdDebugState>,
    pub sega_32x: Option<Sega32XDebugState>,
    pub m68k: M68000,
    pub z80: Z80,
    pub cartridge: Option<Cartridge>,
    pub working_ram: Box<[u16]>,
    pub audio_ram: Box<[u8]>,
    pub z80_bank_number: u32,
    pub pending_writes: Vec<DebugPendingWrite>,
    pub vdp: VdpDebugState,
    pub ym2612: Ym2612,
    pub psg: Sn76489,
}

impl GenesisDebugState {
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

pub struct GenesisDebugView<'m68k, 'z80, 'genesis, 'scd, 's32x, 'cartridge> {
    pub(crate) sega_cd: Option<SegaCdDebugView<'scd>>,
    pub(crate) sega_32x: Option<Sega32XDebugView<'s32x>>,
    pub(crate) m68k: &'m68k mut M68000,
    pub(crate) z80: &'z80 mut Z80,
    pub(crate) cartridge: Option<&'cartridge mut Cartridge>,
    pub(crate) working_ram: &'genesis mut [u16],
    pub(crate) audio_ram: &'genesis mut [u8],
    pub(crate) z80_bank_number: u32,
    pub(crate) pending_writes: &'genesis mut PendingWrites,
    pub(crate) vdp: &'genesis mut Vdp,
    pub(crate) ym2612: &'genesis mut Ym2612,
    pub(crate) psg: &'genesis mut Sn76489,
}

impl GenesisDebugView<'_, '_, '_, '_, '_, '_> {
    pub fn to_debug_state(&self) -> GenesisDebugState {
        GenesisDebugState {
            sega_cd: self.sega_cd.as_ref().map(SegaCdDebugView::to_debug_state),
            sega_32x: self.sega_32x.as_ref().map(Sega32XDebugView::to_debug_state),
            m68k: self.m68k.clone(),
            z80: self.z80.clone(),
            cartridge: self.cartridge.as_ref().map(|cartridge| cartridge.deref().clone()),
            working_ram: self.working_ram.to_vec().into_boxed_slice(),
            audio_ram: self.audio_ram.to_vec().into_boxed_slice(),
            z80_bank_number: self.z80_bank_number,
            pending_writes: self.pending_writes.to_debug_vec(),
            vdp: self.vdp.to_debug_state(),
            ym2612: self.ym2612.clone(),
            psg: self.psg.clone(),
        }
    }

    pub(crate) fn apply_memory_edit(
        &mut self,
        memory_area: GenesisMemoryArea,
        address: usize,
        value: u8,
    ) {
        match memory_area {
            GenesisMemoryArea::CartridgeRom => {
                if let Some(cartridge) = self.cartridge.as_mut() {
                    DebugWordsView(cartridge.debug_rom_view(), Endian::Big).write(address, value);
                }
            }
            GenesisMemoryArea::WorkingRam => {
                DebugWordsView(self.working_ram, Endian::Big).write(address, value);
            }
            GenesisMemoryArea::AudioRam => {
                DebugBytesView(self.audio_ram).write(address, value);
            }
            GenesisMemoryArea::Vram => {
                self.vdp.debug_vram_view().write(address, value);
            }
            GenesisMemoryArea::Cram => {
                self.vdp.debug_cram_view().write(address, value);
            }
            GenesisMemoryArea::Vsram => {
                self.vdp.debug_vsram_view().write(address, value);
            }
        }
    }

    pub(crate) fn apply_scd_memory_edit(
        &mut self,
        memory_area: SegaCdMemoryArea,
        address: usize,
        value: u8,
    ) {
        let Some(sega_cd) = &mut self.sega_cd else { return };
        sega_cd.apply_memory_edit(memory_area, address, value);
    }

    pub(crate) fn apply_32x_memory_edit(
        &mut self,
        memory_area: S32XMemoryArea,
        address: usize,
        value: u8,
    ) {
        let Some(sega_32x) = &mut self.sega_32x else { return };
        sega_32x.apply_memory_edit(memory_area, address, value);
    }
}

impl GenesisEmulator {
    pub fn as_debug_view(&mut self) -> GenesisDebugView<'_, '_, '_, '_, '_, '_> {
        self.bus.as_debug_view(&mut self.m68k, &mut self.z80)
    }
}

pub struct GenesisComponents<'genesis> {
    pub(crate) working_ram: &'genesis mut [u16],
    pub(crate) audio_ram: &'genesis mut [u8],
    pub(crate) z80_bank_number: u32,
    pub(crate) pending_writes: &'genesis mut PendingWrites,
    pub(crate) vdp: &'genesis mut Vdp,
    pub(crate) ym2612: &'genesis mut Ym2612,
    pub(crate) psg: &'genesis mut Sn76489,
}

impl<'genesis> GenesisComponents<'genesis> {
    pub fn as_debug_view<'m68k, 'z80, 'scd, 's32x, 'cartridge>(
        &mut self,
        m68k: &'m68k mut M68000,
        z80: &'z80 mut Z80,
        sega_cd: Option<SegaCdDebugView<'scd>>,
        sega_32x: Option<Sega32XDebugView<'s32x>>,
        cartridge: Option<&'cartridge mut Cartridge>,
    ) -> GenesisDebugView<'m68k, 'z80, '_, 'scd, 's32x, 'cartridge> {
        GenesisDebugView {
            sega_cd,
            sega_32x,
            m68k,
            z80,
            cartridge,
            working_ram: self.working_ram,
            audio_ram: self.audio_ram,
            z80_bank_number: self.z80_bank_number,
            pending_writes: self.pending_writes,
            vdp: self.vdp,
            ym2612: self.ym2612,
            psg: self.psg,
        }
    }

    pub fn into_debug_view<'m68k, 'z80, 'scd, 's32x, 'cartridge>(
        self,
        m68k: &'m68k mut M68000,
        z80: &'z80 mut Z80,
        sega_cd: Option<SegaCdDebugView<'scd>>,
        sega_32x: Option<Sega32XDebugView<'s32x>>,
        cartridge: Option<&'cartridge mut Cartridge>,
    ) -> GenesisDebugView<'m68k, 'z80, 'genesis, 'scd, 's32x, 'cartridge> {
        GenesisDebugView {
            sega_cd,
            sega_32x,
            m68k,
            z80,
            cartridge,
            working_ram: self.working_ram,
            audio_ram: self.audio_ram,
            z80_bank_number: self.z80_bank_number,
            pending_writes: self.pending_writes,
            vdp: self.vdp,
            ym2612: self.ym2612,
            psg: self.psg,
        }
    }
}

// Macro so that it only borrows the needed GenesisBus fields
macro_rules! genesis_components {
    ($bus:expr) => {{
        let (working_ram, audio_ram) = $bus.memory.debug_ram_view();

        crate::api::debug::GenesisComponents {
            working_ram,
            audio_ram,
            z80_bank_number: $bus.z80_bank.value(),
            pending_writes: &mut $bus.pending_writes,
            vdp: &mut $bus.vdp,
            ym2612: &mut $bus.ym2612,
            psg: &mut $bus.psg,
        }
    }};
}

pub(crate) use genesis_components;
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone)]
pub enum GenesisDebugCommand {
    EditMemory(GenesisMemoryArea, usize, u8),
    EditSegaCdMemory(SegaCdMemoryArea, usize, u8),
    Edit32XMemory(S32XMemoryArea, usize, u8),
    BreakResume,
    BreakPause68k,
    BreakStep68k,
    BreakPauseZ80,
    BreakStepZ80,
    BreakPauseSub68k,
    BreakStepSub68k,
    BreakPauseSh2(WhichCpu),
    BreakStepSh2(WhichCpu),
    Update68kBreakpoints(M68000Breakpoints),
    UpdateZ80Breakpoints(Vec<Z80Breakpoint>),
    UpdateSub68kBreakpoints(M68000Breakpoints),
    UpdateSh2Breakpoints(WhichCpu, Sh2Breakpoints),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct M68000Breakpoint {
    pub start_address: u32,
    pub end_address: u32,
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct M68000Breakpoints {
    pub memory: Vec<M68000Breakpoint>,
    pub interrupt: Vec<u8>,
}

impl M68000Breakpoints {
    #[must_use]
    pub fn new() -> Self {
        Self { memory: vec![], interrupt: vec![] }
    }
}

impl Default for M68000Breakpoints {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct M68000BreakpointsParsed {
    read_byte: Vec<(u32, u32)>,
    read_word: Vec<(u32, u32)>,
    write_byte: Vec<(u32, u32)>,
    write_word: Vec<(u32, u32)>,
    execute: Vec<(u32, u32)>,
    interrupt_bitset: u8,
}

impl M68000BreakpointsParsed {
    #[must_use]
    pub fn new(breakpoints: &M68000Breakpoints) -> Self {
        let mut read_byte = Vec::new();
        let mut read_word = Vec::new();
        let mut write_byte = Vec::new();
        let mut write_word = Vec::new();
        let mut execute = Vec::new();

        for &breakpoint in &breakpoints.memory {
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

        let interrupt_bitset = breakpoints
            .interrupt
            .iter()
            .map(|&interrupt_level| 1 << (interrupt_level & 7))
            .fold(0, |a, b| a | b);

        Self { read_byte, read_word, write_byte, write_word, execute, interrupt_bitset }
    }

    #[must_use]
    pub fn none() -> Self {
        Self::new(&M68000Breakpoints::new())
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
    pub fn check_interrupt(&self, interrupt_level: u8) -> bool {
        self.interrupt_bitset.bit(interrupt_level & 7)
    }

    #[must_use]
    pub fn check_execute(&self, address: u32) -> bool {
        self.execute.iter().any(|&(start, end)| (start..=end).contains(&address))
    }
}

const PREV_PC_COUNT: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct M68000BreakStatus {
    pub breaking: bool,
    pub pc: u32,
    pub previous_pcs: [u32; PREV_PC_COUNT],
}

pub struct M68000BreakStatusAtomic {
    pub breaking: AtomicBool,
    pub pc: AtomicU32,
    pub previous_pcs: [AtomicU32; PREV_PC_COUNT],
}

impl M68000BreakStatusAtomic {
    #[must_use]
    pub fn new() -> Self {
        Self {
            breaking: AtomicBool::new(false),
            pc: AtomicU32::new(0),
            previous_pcs: array::from_fn(|_| AtomicU32::new(0)),
        }
    }

    #[must_use]
    pub fn get(&self) -> M68000BreakStatus {
        let breaking = self.breaking.load(Ordering::Acquire);
        let pc = self.pc.load(Ordering::Relaxed);
        let previous_pcs = array::from_fn(|i| self.previous_pcs[i].load(Ordering::Relaxed));

        M68000BreakStatus { breaking, pc, previous_pcs }
    }

    pub fn set_breaking(&self, pcs_rev_iter: impl Iterator<Item = u32>) {
        for (in_pc, out_pc) in pcs_rev_iter.zip(&self.previous_pcs) {
            out_pc.store(in_pc, Ordering::Relaxed);
        }
        self.pc.store(self.previous_pcs[0].load(Ordering::Relaxed), Ordering::Relaxed);

        self.breaking.store(true, Ordering::Release);
    }

    pub fn clear_breaking(&self) {
        self.breaking.store(false, Ordering::Release);
    }
}

impl Default for M68000BreakStatusAtomic {
    fn default() -> Self {
        Self::new()
    }
}

pub struct RingBuffer<T, const N: usize> {
    values: [T; N],
    write_ptr: usize,
}

impl<T: Copy + Default, const N: usize> RingBuffer<T, N> {
    #[must_use]
    pub fn new() -> Self {
        Self { values: array::from_fn(|_| T::default()), write_ptr: 0 }
    }

    pub fn write(&mut self, value: T) {
        self.values[self.write_ptr] = value;
        self.write_ptr = (self.write_ptr + 1) % N;
    }

    #[must_use]
    pub fn last(&self) -> T {
        let read_ptr = Self::ptr_minus_one(self.write_ptr);
        self.values[read_ptr]
    }

    pub fn reverse_iter(&self) -> impl Iterator<Item = T> {
        RingBufferIter { buffer: self, ptr: Self::ptr_minus_one(self.write_ptr), remaining: N }
    }

    fn ptr_minus_one(ptr: usize) -> usize {
        if ptr == 0 { N - 1 } else { ptr - 1 }
    }
}

impl<T: Copy + Default, const N: usize> Default for RingBuffer<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

struct RingBufferIter<'a, T, const N: usize> {
    buffer: &'a RingBuffer<T, N>,
    ptr: usize,
    remaining: usize,
}

impl<T: Copy + Default, const N: usize> Iterator for RingBufferIter<'_, T, N> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }

        let value = self.buffer.values[self.ptr];

        self.ptr = RingBuffer::<T, N>::ptr_minus_one(self.ptr);
        self.remaining -= 1;

        Some(value)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

pub struct M68000BreakpointManager {
    pub breakpoints: M68000BreakpointsParsed,
    pub last_pcs: RingBuffer<u32, PREV_PC_COUNT>,
    pub status: Arc<M68000BreakStatusAtomic>,
    pub step: Option<u32>,
}

impl M68000BreakpointManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            breakpoints: M68000BreakpointsParsed::none(),
            last_pcs: RingBuffer::new(),
            status: Arc::new(M68000BreakStatusAtomic::new()),
            step: None,
        }
    }

    pub fn set_break_status(&self) {
        self.status.set_breaking(self.last_pcs.reverse_iter());
    }

    pub fn clear_break_status(&self) {
        self.status.clear_breaking();
    }

    pub fn clear(&mut self) {
        self.breakpoints = M68000BreakpointsParsed::none();
        self.step = None;
    }

    #[must_use]
    pub fn check_read<const WORD: bool>(&self, address: u32) -> bool {
        self.breakpoints.check_read::<WORD>(address)
    }

    #[must_use]
    pub fn check_write<const WORD: bool>(&self, address: u32) -> bool {
        self.breakpoints.check_write::<WORD>(address)
    }

    #[must_use]
    pub fn check_interrupt(&self, interrupt_level: u8) -> bool {
        self.breakpoints.check_interrupt(interrupt_level)
    }

    #[must_use]
    pub fn update_pc_and_check_execute(&mut self, pc: u32) -> bool {
        self.last_pcs.write(pc);
        self.breakpoints.check_execute(pc)
    }

    #[must_use]
    pub fn check_break_step(&mut self) -> bool {
        check_break_step(&mut self.step)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Z80BreakStatus {
    pub breaking: bool,
    pub pc: u16,
    pub previous_pcs: [u16; PREV_PC_COUNT],
}

pub struct Z80BreakStatusAtomic {
    pub breaking: AtomicBool,
    pub pc: AtomicU16,
    pub previous_pcs: [AtomicU16; PREV_PC_COUNT],
}

impl Z80BreakStatusAtomic {
    #[must_use]
    pub fn new() -> Self {
        Self {
            breaking: AtomicBool::new(false),
            pc: AtomicU16::new(0),
            previous_pcs: array::from_fn(|_| AtomicU16::new(0)),
        }
    }

    #[must_use]
    pub fn get(&self) -> Z80BreakStatus {
        let breaking = self.breaking.load(Ordering::Acquire);
        let pc = self.pc.load(Ordering::Relaxed);
        let previous_pcs = array::from_fn(|i| self.previous_pcs[i].load(Ordering::Relaxed));

        Z80BreakStatus { breaking, pc, previous_pcs }
    }

    pub fn set_breaking(&self, pcs_rev_iter: impl Iterator<Item = u16>) {
        for (in_pc, out_pc) in pcs_rev_iter.zip(&self.previous_pcs) {
            out_pc.store(in_pc, Ordering::Relaxed);
        }
        self.pc.store(self.previous_pcs[0].load(Ordering::Relaxed), Ordering::Relaxed);

        self.breaking.store(true, Ordering::Release);
    }

    pub fn clear_breaking(&self) {
        self.breaking.store(false, Ordering::Release);
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
    pub previous_pcs: RingBuffer<u16, PREV_PC_COUNT>,
    pub step: Option<u32>,
}

impl Z80BreakpointManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            breakpoints: Z80Breakpoints::none(),
            status: Arc::new(Z80BreakStatusAtomic::new()),
            previous_pcs: RingBuffer::new(),
            step: None,
        }
    }

    pub fn set_break_status(&self) {
        self.status.set_breaking(self.previous_pcs.reverse_iter());
    }

    pub fn clear_break_status(&self) {
        self.status.clear_breaking();
    }

    pub fn clear(&mut self) {
        self.breakpoints = Z80Breakpoints::none();
        self.step = None;
    }

    #[must_use]
    pub fn check_read(&self, address: u16) -> bool {
        self.breakpoints.check_read(address)
    }

    #[must_use]
    pub fn check_write(&self, address: u16) -> bool {
        self.breakpoints.check_write(address)
    }

    #[must_use]
    pub fn update_pc_and_check_execute(&mut self, pc: u16) -> bool {
        self.previous_pcs.write(pc);
        self.breakpoints.check_execute(pc)
    }

    #[must_use]
    pub fn check_break_step(&mut self) -> bool {
        check_break_step(&mut self.step)
    }
}

impl Default for Z80BreakpointManager {
    fn default() -> Self {
        Self::new()
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
pub struct Sh2Breakpoint {
    pub start_address: u32,
    pub end_address: u32,
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sh2Breakpoints {
    pub memory: Vec<Sh2Breakpoint>,
    pub interrupt: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(crate) struct Sh2BreakpointsParsed {
    read_byte: Vec<(u32, u32)>,
    read_word: Vec<(u32, u32)>,
    read_longword: Vec<(u32, u32)>,
    write_byte: Vec<(u32, u32)>,
    write_word: Vec<(u32, u32)>,
    write_longword: Vec<(u32, u32)>,
    execute: Vec<(u32, u32)>,
    interrupt_bitset: u16,
}

impl Sh2BreakpointsParsed {
    #[must_use]
    pub fn new(breakpoints: &Sh2Breakpoints) -> Self {
        let mut read_byte = Vec::new();
        let mut read_word = Vec::new();
        let mut read_longword = Vec::new();
        let mut write_byte = Vec::new();
        let mut write_word = Vec::new();
        let mut write_longword = Vec::new();
        let mut execute = Vec::new();

        for &breakpoint in &breakpoints.memory {
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

        let interrupt_bitset =
            breakpoints.interrupt.iter().map(|&level| 1 << (level & 15)).fold(0, |a, b| a | b);

        Self {
            read_byte,
            read_word,
            read_longword,
            write_byte,
            write_word,
            write_longword,
            execute,
            interrupt_bitset,
        }
    }

    #[must_use]
    pub fn none() -> Self {
        Self::new(&Sh2Breakpoints::default())
    }

    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn should_break_read(&self, address: u32, size: OpSizeEnum) -> bool {
        let addresses = match size {
            OpSizeEnum::Byte => &self.read_byte,
            OpSizeEnum::Word => &self.read_word,
            OpSizeEnum::Longword => &self.read_longword,
        };
        addresses.iter().any(|&(start, end)| (start..=end).contains(&address))
    }

    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn should_break_write(&self, address: u32, size: OpSizeEnum) -> bool {
        let addresses = match size {
            OpSizeEnum::Byte => &self.write_byte,
            OpSizeEnum::Word => &self.write_word,
            OpSizeEnum::Longword => &self.write_longword,
        };
        addresses.iter().any(|&(start, end)| (start..=end).contains(&address))
    }

    #[must_use]
    pub fn should_break_execute(&self, address: u32) -> bool {
        self.execute.iter().any(|&(start, end)| (start..=end).contains(&address))
    }

    #[must_use]
    pub fn should_break_interrupt(&self, interrupt_level: u8) -> bool {
        self.interrupt_bitset.bit(interrupt_level & 15)
    }
}

struct Sh2BreakpointsManager {
    breakpoints: [Sh2BreakpointsParsed; 2],
    status: Arc<Sh2BreakStatusAtomic>,
    step: Option<(WhichCpu, u32)>,
}

impl Sh2BreakpointsManager {
    fn new() -> Self {
        Self {
            breakpoints: array::from_fn(|_| Sh2BreakpointsParsed::none()),
            status: Arc::new(Sh2BreakStatusAtomic::new()),
            step: None,
        }
    }

    fn clear(&mut self) {
        self.breakpoints.fill_with(Sh2BreakpointsParsed::none);
        self.step = None;
    }

    fn set_break_status(&mut self, which: WhichCpu) {
        self.status.breaking[which as usize].store(true, Ordering::Release);
    }

    fn clear_break_status(&mut self, which: WhichCpu) {
        self.status.breaking[which as usize].store(false, Ordering::Release);
    }

    fn update_pc_and_check_execute(&mut self, which: WhichCpu, pc: u32) -> bool {
        self.status.break_pc[which as usize].store(pc, Ordering::Relaxed);
        self.breakpoints[which as usize].should_break_execute(pc)
    }

    fn check_break_step(&mut self, which: WhichCpu) -> bool {
        let Some((step_which, step)) = &mut self.step else { return false };

        if *step_which != which {
            return false;
        }

        *step -= 1;
        if *step == 0 {
            self.step = None;
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GenesisCpu {
    M68k,
    Z80,
    Sub68k,
    Sh2(WhichCpu),
}

pub struct GenesisDebugger {
    command_receiver: Receiver<GenesisDebugCommand>,
    state_sender: SharedVarSender<GenesisDebugState>,
    m68k_breakpoints: M68000BreakpointManager,
    z80_breakpoints: Z80BreakpointManager,
    sub_68k_breakpoints: M68000BreakpointManager,
    sh2_breakpoints: Sh2BreakpointsManager,
}

pub struct GenesisDebuggerHandle {
    pub command_sender: Sender<GenesisDebugCommand>,
    pub m68k_break_status: Arc<M68000BreakStatusAtomic>,
    pub z80_break_status: Arc<Z80BreakStatusAtomic>,
    pub sub_cpu_break_status: Arc<M68000BreakStatusAtomic>,
    pub sh2_break_status: Arc<Sh2BreakStatusAtomic>,
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
            sub_68k_breakpoints: M68000BreakpointManager::new(),
            sh2_breakpoints: Sh2BreakpointsManager::new(),
        };

        let handle = GenesisDebuggerHandle {
            command_sender,
            m68k_break_status: Arc::clone(&debugger.m68k_breakpoints.status),
            z80_break_status: Arc::clone(&debugger.z80_breakpoints.status),
            sub_cpu_break_status: Arc::clone(&debugger.sub_68k_breakpoints.status),
            sh2_break_status: Arc::clone(&debugger.sh2_breakpoints.status),
        };

        (debugger, handle)
    }

    pub fn process_commands(&mut self, debug_view: &mut GenesisDebugView<'_, '_, '_, '_, '_, '_>) {
        loop {
            match self.command_receiver.try_recv() {
                Ok(command) => self.process_command(command, debug_view),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.m68k_breakpoints.clear();
                    self.z80_breakpoints.clear();
                    self.sub_68k_breakpoints.clear();
                    self.sh2_breakpoints.clear();
                    break;
                }
            }
        }
    }

    pub fn process_command(
        &mut self,
        command: GenesisDebugCommand,
        debug_view: &mut GenesisDebugView<'_, '_, '_, '_, '_, '_>,
    ) {
        match command {
            GenesisDebugCommand::EditMemory(memory_area, address, value) => {
                debug_view.apply_memory_edit(memory_area, address, value);
            }
            GenesisDebugCommand::EditSegaCdMemory(memory_area, address, value) => {
                debug_view.apply_scd_memory_edit(memory_area, address, value);
            }
            GenesisDebugCommand::Edit32XMemory(memory_area, address, value) => {
                debug_view.apply_32x_memory_edit(memory_area, address, value);
            }
            GenesisDebugCommand::Update68kBreakpoints(breakpoints) => {
                self.m68k_breakpoints.breakpoints = M68000BreakpointsParsed::new(&breakpoints);
            }
            GenesisDebugCommand::UpdateZ80Breakpoints(breakpoints) => {
                self.z80_breakpoints.breakpoints = Z80Breakpoints::new(&breakpoints);
            }
            GenesisDebugCommand::UpdateSub68kBreakpoints(breakpoints) => {
                self.sub_68k_breakpoints.breakpoints = M68000BreakpointsParsed::new(&breakpoints);
            }
            GenesisDebugCommand::UpdateSh2Breakpoints(which, breakpoints) => {
                self.sh2_breakpoints.breakpoints[which as usize] =
                    Sh2BreakpointsParsed::new(&breakpoints);
            }
            GenesisDebugCommand::BreakPause68k => {
                self.m68k_breakpoints.step = Some(1);
            }
            GenesisDebugCommand::BreakPauseZ80 => {
                self.z80_breakpoints.step = Some(1);
            }
            GenesisDebugCommand::BreakPauseSub68k => {
                self.sub_68k_breakpoints.step = Some(1);
            }
            GenesisDebugCommand::BreakPauseSh2(which) => {
                self.sh2_breakpoints.step = Some((which, 1));
            }
            GenesisDebugCommand::BreakResume
            | GenesisDebugCommand::BreakStep68k
            | GenesisDebugCommand::BreakStepZ80
            | GenesisDebugCommand::BreakStepSub68k
            | GenesisDebugCommand::BreakStepSh2(_) => {}
        }
    }

    pub(crate) fn handle_breakpoint(
        &mut self,
        which: GenesisCpu,
        debug_view: &mut GenesisDebugView<'_, '_, '_, '_, '_, '_>,
    ) {
        self.state_sender.update(debug_view.to_debug_state());

        match which {
            GenesisCpu::M68k => {
                self.m68k_breakpoints.set_break_status();
            }
            GenesisCpu::Z80 => {
                self.z80_breakpoints.set_break_status();
            }
            GenesisCpu::Sub68k => {
                self.sub_68k_breakpoints.set_break_status();
            }
            GenesisCpu::Sh2(which) => {
                self.sh2_breakpoints.set_break_status(which);
            }
        }

        self.m68k_breakpoints.step = None;
        self.z80_breakpoints.step = None;
        self.sub_68k_breakpoints.step = None;
        self.sh2_breakpoints.step = None;

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
                Ok(GenesisDebugCommand::BreakStepSub68k) => {
                    self.sub_68k_breakpoints.step =
                        Some(1 + u32::from(which != GenesisCpu::Sub68k));
                    break;
                }
                Ok(GenesisDebugCommand::BreakStepSh2(sh2_which)) => {
                    self.sh2_breakpoints.step =
                        Some((sh2_which, 1 + u32::from(which != GenesisCpu::Sh2(sh2_which))));
                    break;
                }
                Ok(command) => self.process_command(command, debug_view),
                Err(_) => {
                    // Debugger window closed
                    self.m68k_breakpoints.clear();
                    self.z80_breakpoints.clear();
                    self.sub_68k_breakpoints.clear();
                    self.sh2_breakpoints.clear();
                    break;
                }
            }
        }

        match which {
            GenesisCpu::M68k => {
                self.m68k_breakpoints.clear_break_status();
            }
            GenesisCpu::Z80 => {
                self.z80_breakpoints.clear_break_status();
            }
            GenesisCpu::Sub68k => {
                self.sub_68k_breakpoints.clear_break_status();
            }
            GenesisCpu::Sh2(which) => {
                self.sh2_breakpoints.clear_break_status(which);
            }
        }
    }

    pub(crate) fn m68k_breakpoints(&mut self) -> &mut M68000BreakpointManager {
        &mut self.m68k_breakpoints
    }

    pub(crate) fn z80_breakpoints(&mut self) -> &mut Z80BreakpointManager {
        &mut self.z80_breakpoints
    }

    pub(crate) fn with_cpus<'cpus>(
        &mut self,
        m68k: &'cpus mut M68000,
        z80: &'cpus mut Z80,
    ) -> GenesisDebuggerWithCpus<'_, 'cpus> {
        GenesisDebuggerWithCpus { debugger: self, m68k, z80 }
    }
}

impl GenesisDebuggerHandle {
    /// # Errors
    ///
    /// Returns an error if the debugger backend has been closed.
    pub fn send_command(
        &self,
        command: GenesisDebugCommand,
    ) -> Result<(), SendError<GenesisDebugCommand>> {
        self.command_sender.send(command)
    }

    #[must_use]
    pub fn m68k_break_status(&self) -> M68000BreakStatus {
        self.m68k_break_status.get()
    }

    #[must_use]
    pub fn z80_break_status(&self) -> Z80BreakStatus {
        self.z80_break_status.get()
    }

    #[must_use]
    pub fn sub_cpu_break_status(&self) -> M68000BreakStatus {
        self.sub_cpu_break_status.get()
    }

    #[must_use]
    pub fn sh2_break_status(&self) -> S32XSh2BreakStatus {
        let master = self.sh2_break_status_one(WhichCpu::Master);
        let slave = self.sh2_break_status_one(WhichCpu::Slave);

        S32XSh2BreakStatus { master, slave }
    }

    fn sh2_break_status_one(&self, which: WhichCpu) -> Sh2BreakStatus {
        let break_idx = which as usize;
        let breaking = self.sh2_break_status.breaking[break_idx].load(Ordering::Acquire);
        let pc = self.sh2_break_status.break_pc[break_idx].load(Ordering::Relaxed);
        Sh2BreakStatus { breaking, pc }
    }
}

pub(crate) struct GenesisDebuggerWithCpus<'debugger, 'cpus> {
    debugger: &'debugger mut GenesisDebugger,
    m68k: &'cpus mut M68000,
    z80: &'cpus mut Z80,
}

pub(crate) struct GenesisDebuggerForSegaCd<'debugger, 'genesis, 's32x> {
    debugger: &'debugger mut GenesisDebugger,
    m68k: &'debugger mut M68000,
    z80: &'debugger mut Z80,
    sega_32x: Option<&'s32x mut Sega32X>,
    genesis_components: GenesisComponents<'genesis>,
    cartridge: Option<&'genesis mut Cartridge>,
}

pub(crate) struct GenesisDebuggerFor32X {
    debugger: NonNull<GenesisDebugger>,
    m68k: NonNull<M68000>,
    z80: NonNull<Z80>,
    sega_cd: *mut SegaCd,
    working_ram: NonNull<[u16]>,
    audio_ram: NonNull<[u8]>,
    z80_bank_number: u32,
    pending_writes: NonNull<PendingWrites>,
    vdp: NonNull<Vdp>,
    ym2612: NonNull<Ym2612>,
    psg: NonNull<Sn76489>,
}

impl GenesisDebuggerWithCpus<'_, '_> {
    pub(crate) fn for_sega_cd<'genesis, 's32x>(
        &mut self,
        sega_32x: Option<&'s32x mut Sega32X>,
        genesis_components: GenesisComponents<'genesis>,
        cartridge: Option<&'genesis mut Cartridge>,
    ) -> GenesisDebuggerForSegaCd<'_, 'genesis, 's32x> {
        GenesisDebuggerForSegaCd {
            debugger: self.debugger,
            m68k: self.m68k,
            z80: self.z80,
            sega_32x,
            genesis_components,
            cartridge,
        }
    }

    pub(crate) unsafe fn for_32x(
        &mut self,
        sega_cd: Option<&mut SegaCd>,
        genesis_components: GenesisComponents<'_>,
    ) -> GenesisDebuggerFor32X {
        GenesisDebuggerFor32X {
            debugger: self.debugger.into(),
            m68k: self.m68k.into(),
            z80: self.z80.into(),
            sega_cd: sega_cd.map(ptr::from_mut).unwrap_or(ptr::null_mut()),
            working_ram: genesis_components.working_ram.into(),
            audio_ram: genesis_components.audio_ram.into(),
            z80_bank_number: genesis_components.z80_bank_number,
            pending_writes: genesis_components.pending_writes.into(),
            vdp: genesis_components.vdp.into(),
            ym2612: genesis_components.ym2612.into(),
            psg: genesis_components.psg.into(),
        }
    }
}

impl SegaCdDebugger for GenesisDebuggerForSegaCd<'_, '_, '_> {
    fn check_sub_read_breakpoint<const WORD: bool>(&mut self, address: u32) -> bool {
        self.debugger.sub_68k_breakpoints.check_read::<WORD>(address)
    }

    fn check_sub_write_breakpoint<const WORD: bool>(&mut self, address: u32, _value: u16) -> bool {
        self.debugger.sub_68k_breakpoints.check_write::<WORD>(address)
    }

    fn check_sub_execute_breakpoint(&mut self, pc: u32) -> bool {
        let execute = self.debugger.sub_68k_breakpoints.update_pc_and_check_execute(pc);
        let step = self.debugger.sub_68k_breakpoints.check_break_step();

        execute || step
    }

    fn check_sub_interrupt_breakpoint(&mut self, interrupt_level: u8) -> bool {
        self.debugger.sub_68k_breakpoints.check_interrupt(interrupt_level)
    }

    fn handle_sub_breakpoint(&mut self, debug_view: SegaCdDebugView<'_>) {
        let (s32x_debug_view, s32x_cartridge) =
            match self.sega_32x.as_mut().map(|sega_32x| sega_32x.as_debug_view()) {
                Some((debug_view, cartridge)) => (Some(debug_view), cartridge),
                None => (None, None),
            };
        let cartridge = s32x_cartridge.or(self.cartridge.as_deref_mut());

        let mut genesis_debug_view = self.genesis_components.as_debug_view(
            self.m68k,
            self.z80,
            Some(debug_view),
            s32x_debug_view,
            cartridge,
        );
        self.debugger.handle_breakpoint(GenesisCpu::Sub68k, &mut genesis_debug_view);
    }
}

impl Sega32XDebugger for GenesisDebuggerFor32X {
    fn check_sh2_read_breakpoint(
        &mut self,
        which: WhichCpu,
        address: u32,
        size: OpSizeEnum,
    ) -> bool {
        unsafe {
            self.debugger.as_mut().sh2_breakpoints.breakpoints[which as usize]
                .should_break_read(address, size)
        }
    }

    fn check_sh2_write_breakpoint(
        &mut self,
        which: WhichCpu,
        address: u32,
        _value: u32,
        size: OpSizeEnum,
    ) -> bool {
        unsafe {
            self.debugger.as_mut().sh2_breakpoints.breakpoints[which as usize]
                .should_break_write(address, size)
        }
    }

    fn check_sh2_execute_breakpoint(&mut self, which: WhichCpu, pc: u32, _opcode: u16) -> bool {
        unsafe {
            let sh2_breakpoints = &mut self.debugger.as_mut().sh2_breakpoints;

            let execute = sh2_breakpoints.update_pc_and_check_execute(which, pc);
            let step = sh2_breakpoints.check_break_step(which);

            execute || step
        }
    }

    fn check_sh2_interrupt_breakpoint(&mut self, which: WhichCpu, interrupt_level: u8) -> bool {
        unsafe {
            self.debugger.as_mut().sh2_breakpoints.breakpoints[which as usize]
                .should_break_interrupt(interrupt_level)
        }
    }

    fn handle_sh2_breakpoint(
        &mut self,
        which: WhichCpu,
        cartridge: Option<&mut Cartridge>,
        debug_view: Sega32XDebugView<'_>,
    ) {
        unsafe {
            let mut genesis_debug_view = GenesisDebugView {
                sega_cd: self.sega_cd.as_mut().map(SegaCd::as_debug_view),
                sega_32x: Some(debug_view),
                m68k: self.m68k.as_mut(),
                z80: self.z80.as_mut(),
                cartridge,
                working_ram: self.working_ram.as_mut(),
                audio_ram: self.audio_ram.as_mut(),
                z80_bank_number: self.z80_bank_number,
                pending_writes: self.pending_writes.as_mut(),
                vdp: self.vdp.as_mut(),
                ym2612: self.ym2612.as_mut(),
                psg: self.psg.as_mut(),
            };
            self.debugger
                .as_mut()
                .handle_breakpoint(GenesisCpu::Sh2(which), &mut genesis_debug_view);
        }
    }
}
