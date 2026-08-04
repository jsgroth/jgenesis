use crate::WordRam;
use crate::api::SegaCd;
use crate::cddrive::cdc::Rchip;
use crate::rf5c164::Rf5c164;
use jgenesis_common::debug::{DebugMemoryView, DebugWordsView, Endian};
use jgenesis_proc_macros::EnumAll;
use m68000_emu::M68000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumAll)]
pub enum SegaCdMemoryArea {
    BiosRom,
    PrgRam,
    WordRam,
    PcmRam,
    CdcRam,
}

#[derive(Debug, Clone)]
pub struct SegaCdDebugState {
    sub_cpu: M68000,
    bios_rom: Box<[u16]>,
    prg_ram: Box<[u16]>,
    word_ram: WordRam,
    pcm: Rf5c164,
    cdc: Rchip,
    prg_ram_bank: u8,
}

impl SegaCdDebugState {
    #[must_use]
    pub fn sub_cpu(&self) -> &M68000 {
        &self.sub_cpu
    }

    #[must_use]
    pub fn bios_rom(&self) -> &[u16] {
        &self.bios_rom
    }

    #[must_use]
    pub fn prg_ram(&self) -> &[u16] {
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
            SegaCdMemoryArea::BiosRom => Box::new(DebugWordsView(&mut self.bios_rom, Endian::Big)),
            SegaCdMemoryArea::PrgRam => Box::new(DebugWordsView(&mut self.prg_ram, Endian::Big)),
            SegaCdMemoryArea::WordRam => Box::new(self.word_ram.debug_view()),
            SegaCdMemoryArea::PcmRam => Box::new(self.pcm.debug_ram_view()),
            SegaCdMemoryArea::CdcRam => Box::new(self.cdc.debug_ram_view()),
        }
    }
}

pub struct SegaCdDebugView<'scd> {
    pub(crate) sub_cpu: &'scd mut M68000,
    pub(crate) bios_rom: &'scd mut [u16],
    pub(crate) prg_ram: &'scd mut [u16],
    pub(crate) word_ram: &'scd mut WordRam,
    pub(crate) pcm: &'scd mut Rf5c164,
    pub(crate) cdc: &'scd mut Rchip,
    pub(crate) prg_ram_bank: u8,
}

impl SegaCdDebugView<'_> {
    #[must_use]
    pub fn to_debug_state(&self) -> SegaCdDebugState {
        SegaCdDebugState {
            sub_cpu: self.sub_cpu.clone(),
            bios_rom: self.bios_rom.to_vec().into_boxed_slice(),
            prg_ram: self.prg_ram.to_vec().into_boxed_slice(),
            word_ram: self.word_ram.clone(),
            pcm: self.pcm.clone(),
            cdc: self.cdc.clone(),
            prg_ram_bank: self.prg_ram_bank,
        }
    }

    pub fn apply_memory_edit(&mut self, memory_area: SegaCdMemoryArea, address: usize, value: u8) {
        match memory_area {
            SegaCdMemoryArea::BiosRom => {
                DebugWordsView(self.bios_rom, Endian::Big).write(address, value);
            }
            SegaCdMemoryArea::PrgRam => {
                DebugWordsView(self.prg_ram, Endian::Big).write(address, value);
            }
            SegaCdMemoryArea::WordRam => {
                self.word_ram.debug_view().write(address, value);
            }
            SegaCdMemoryArea::PcmRam => {
                self.pcm.debug_ram_view().write(address, value);
            }
            SegaCdMemoryArea::CdcRam => {
                self.cdc.debug_ram_view().write(address, value);
            }
        }
    }
}

impl SegaCd {
    pub fn as_debug_view(&mut self) -> SegaCdDebugView<'_> {
        self.bus.as_debug_view(&mut self.sub_cpu)
    }
}

pub trait SegaCdDebugger {
    fn check_sub_read_breakpoint<const WORD: bool>(&mut self, address: u32) -> bool;

    fn check_sub_write_breakpoint<const WORD: bool>(&mut self, address: u32, value: u16) -> bool;

    fn check_sub_execute_breakpoint(&mut self, pc: u32) -> bool;

    fn check_sub_interrupt_breakpoint(&mut self, interrupt_level: u8) -> bool;

    fn handle_sub_breakpoint(&mut self, debug_view: SegaCdDebugView<'_>);
}

pub struct DummySegaCdDebugger;

#[allow(unused_variables)]
impl SegaCdDebugger for DummySegaCdDebugger {
    #[inline(always)]
    fn check_sub_read_breakpoint<const WORD: bool>(&mut self, address: u32) -> bool {
        false
    }

    #[inline(always)]
    fn check_sub_write_breakpoint<const WORD: bool>(&mut self, address: u32, value: u16) -> bool {
        false
    }

    #[inline(always)]
    fn check_sub_execute_breakpoint(&mut self, pc: u32) -> bool {
        false
    }

    #[inline(always)]
    fn check_sub_interrupt_breakpoint(&mut self, interrupt_level: u8) -> bool {
        false
    }

    #[inline(always)]
    fn handle_sub_breakpoint(&mut self, debug_view: SegaCdDebugView<'_>) {}
}
