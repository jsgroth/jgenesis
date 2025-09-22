//! GBA interrupt registers

use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::EnumAll;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumAll, Encode, Decode)]
pub enum InterruptType {
    VBlank = 0,
    HBlank = 1,
    VCounter = 2,
    Timer0 = 3,
    Timer1 = 4,
    Timer2 = 5,
    Timer3 = 6,
    Serial = 7,
    Dma0 = 8,
    Dma1 = 9,
    Dma2 = 10,
    Dma3 = 11,
    Keypad = 12,
    GamePak = 13,
}

impl InterruptType {
    pub const DMA: [Self; 4] = [Self::Dma0, Self::Dma1, Self::Dma2, Self::Dma3];

    pub const TIMER: [Self; 4] = [Self::Timer0, Self::Timer1, Self::Timer2, Self::Timer3];

    fn bit_mask(self) -> u16 {
        1 << (self as u8)
    }

    fn name(self) -> &'static str {
        match self {
            Self::VBlank => "VBlank",
            Self::HBlank => "HBlank",
            Self::VCounter => "V counter match",
            Self::Timer0 => "Timer 0 overflow",
            Self::Timer1 => "Timer 1 overflow",
            Self::Timer2 => "Timer 2 overflow",
            Self::Timer3 => "Timer 3 overflow",
            Self::Serial => "Serial",
            Self::Dma0 => "DMA 0",
            Self::Dma1 => "DMA 1",
            Self::Dma2 => "DMA 2",
            Self::Dma3 => "DMA 3",
            Self::Keypad => "Keypad",
            Self::GamePak => "Game Pak",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum PendingWrite {
    Ime(bool),
    Ie(u16),
    If(u16),
    SetFlag(InterruptType),
}

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct IrqFlags {
    ime: bool,
    enabled: u16,
    flags: u16,
    pending: bool,
}

impl IrqFlags {
    fn apply_write(&mut self, write: PendingWrite) {
        match write {
            PendingWrite::Ime(ime) => self.ime = ime,
            PendingWrite::Ie(enabled) => self.enabled = enabled,
            PendingWrite::If(flags) => {
                // Writing 1 to a bit clears that flag
                self.flags &= !flags;
            }
            PendingWrite::SetFlag(interrupt) => {
                self.flags |= interrupt.bit_mask();
            }
        }

        self.pending = self.ime && self.enabled & self.flags != 0;
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct InterruptRegisters {
    irq: IrqFlags,
    irq_synchronized: IrqFlags,
    irq_pending: bool,
    halted: bool,
    stopped: bool,
    pending_writes: Vec<(PendingWrite, u64)>,
}

impl InterruptRegisters {
    pub fn new() -> Self {
        Self {
            irq: IrqFlags::default(),
            irq_synchronized: IrqFlags::default(),
            irq_pending: false,
            halted: false,
            stopped: false,
            pending_writes: Vec::with_capacity(10),
        }
    }

    pub fn cpu_bus_cycle(&mut self, cycles: u64) {
        self.apply_pending_writes(cycles);
        self.irq_pending = self.irq_synchronized.pending;

        self.halted &= self.irq.enabled & self.irq.flags == 0;
        self.stopped &= self.halted;
    }

    pub fn irq(&self) -> bool {
        self.irq_synchronized.pending
    }

    pub fn write_ime(&mut self, value: u16, cycles: u64) {
        self.push_pending_write(PendingWrite::Ime(value.bit(0)), cycles);
        log::trace!("IME write: {value:04X} (enabled = {}) (cycles = {cycles})", value.bit(0));
    }

    pub fn read_ime(&mut self, cycles: u64) -> u16 {
        self.apply_pending_writes(cycles);
        self.irq.ime.into()
    }

    pub fn write_ie(&mut self, value: u16, cycles: u64) {
        self.push_pending_write(PendingWrite::Ie(value), cycles);
        log::trace!("IE write: {value:04X} (cycles = {cycles})");
    }

    pub fn read_ie(&mut self, cycles: u64) -> u16 {
        self.apply_pending_writes(cycles);
        self.irq.enabled
    }

    pub fn write_if(&mut self, value: u16, cycles: u64) {
        // Clearing interrupt flags via IF write affects any flag sets that occur on the same cycle
        self.pending_writes.retain(|&(write, write_cycles)| {
            let PendingWrite::SetFlag(interrupt) = write else { return true };
            !(cycles == write_cycles && value & interrupt.bit_mask() != 0)
        });

        self.push_pending_write(PendingWrite::If(value), cycles);
        log::trace!("IF write: {value:04X} (cycles = {cycles})");
    }

    pub fn read_if(&mut self, cycles: u64) -> u16 {
        self.apply_pending_writes(cycles);
        self.irq.flags
    }

    pub fn set_flag(&mut self, interrupt: InterruptType, cycles: u64) {
        // Setting interrupt flag has no affect if the flag is cleared via IF write on the same cycle
        let interrupt_bit = interrupt.bit_mask();
        if self.pending_writes.iter().any(|&(write, write_cycles)| {
            let PendingWrite::If(flags) = write else { return false };
            cycles == write_cycles && flags & interrupt_bit != 0
        }) {
            log::trace!(
                "Skipping {} interrupt flag set due to pending IF write (cycles = {cycles})",
                interrupt.name()
            );
            return;
        }

        self.push_pending_write(PendingWrite::SetFlag(interrupt), cycles);
        log::trace!("{} interrupt flag set (cycles = {cycles})", interrupt.name());
    }

    pub fn write_haltcnt(&mut self, value: u8) {
        self.halted = true;
        self.stopped = value.bit(7);

        log::trace!("HALTCNT write: {value:02X}, CPU is halted");
        log::trace!("  Stopped: {}", self.stopped);
    }

    pub fn cpu_halted(&self) -> bool {
        self.halted
    }

    pub fn stopped(&self) -> bool {
        if !self.stopped {
            return false;
        }

        if !self.pending_writes.iter().any(|&(write, _)| matches!(write, PendingWrite::SetFlag(_)))
        {
            return true;
        }

        let mut clone = self.clone();
        clone.apply_pending_writes(u64::MAX);
        clone.irq.enabled & clone.irq.flags == 0
    }

    fn push_pending_write(&mut self, write: PendingWrite, cycles: u64) {
        let i = self
            .pending_writes
            .iter()
            .position(|&(_, other_cycles)| other_cycles > cycles)
            .unwrap_or(self.pending_writes.len());
        self.pending_writes.insert(i, (write, cycles));
    }

    #[allow(clippy::int_plus_one)]
    fn apply_pending_writes(&mut self, cycles: u64) {
        if self.pending_writes.is_empty() {
            return;
        }

        for &(write, write_cycles) in &self.pending_writes {
            // 1-cycle delay before writes and flag sets are visible for reads
            if cycles >= write_cycles + 1 {
                self.irq.apply_write(write);
            }

            // 2-cycle delay before writes affect the IRQ synchronizer
            if cycles >= write_cycles + 2 {
                self.irq_synchronized.apply_write(write);
            }
        }

        self.pending_writes.retain(|&(_, write_cycles)| cycles < write_cycles + 2);
    }
}
