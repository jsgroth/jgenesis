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

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct InterruptFlags(u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum PendingWrite {
    Ime(bool),
    Ie(u16),
    If(u16),
    SetFlag(InterruptType),
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct InterruptRegisters {
    ime: bool,
    enabled: InterruptFlags,
    flags: InterruptFlags,
    pending_writes: Vec<(u64, PendingWrite)>,
    halted: bool,
    stopped: bool,
}

impl InterruptRegisters {
    pub fn new() -> Self {
        Self {
            ime: false,
            enabled: InterruptFlags(0),
            flags: InterruptFlags(0),
            pending_writes: Vec::with_capacity(5),
            halted: false,
            stopped: false,
        }
    }

    fn push_pending_write(&mut self, write: PendingWrite, cycles: u64) {
        let i = self
            .pending_writes
            .iter()
            .position(|&(existing_cycles, _)| existing_cycles > cycles)
            .unwrap_or(self.pending_writes.len());
        self.pending_writes.insert(i, (cycles, write));
    }

    fn apply_pending_writes(&mut self, cycles: u64) {
        const IRQ_SYNCHRONIZER_DELAY: u64 = 3;

        while let Some(front) = self.pending_writes.first()
            && front.0 + IRQ_SYNCHRONIZER_DELAY <= cycles
        {
            let write = front.1;
            match write {
                PendingWrite::Ime(ime) => {
                    self.ime = ime;
                }
                PendingWrite::Ie(ie) => {
                    self.enabled.0 = ie;
                }
                PendingWrite::If(flags) => {
                    self.flags.0 &= !flags;
                }
                PendingWrite::SetFlag(interrupt_type) => {
                    self.flags.0 |= 1 << (interrupt_type as u8);
                }
            }
            self.pending_writes.remove(0);
        }
    }

    pub fn sync(&mut self, cycles: u64) {
        self.apply_pending_writes(cycles);
        self.halted &= self.enabled.0 & self.flags.0 == 0;
        self.stopped &= self.halted;
    }

    // $4000200: IE (Interrupts enabled)
    pub fn read_ie(&self) -> u16 {
        for &(_, write) in self.pending_writes.iter().rev() {
            if let PendingWrite::Ie(ie) = write {
                return ie;
            }
        }

        self.enabled.0
    }

    // $4000200: IE (Interrupts enabled)
    pub fn write_ie(&mut self, value: u16, cycles: u64) {
        self.push_pending_write(PendingWrite::Ie(value), cycles);

        log::debug!("IE write: {value:04X} (cycles {cycles})");
        for interrupt_type in InterruptType::ALL {
            log::debug!(
                "  {} IRQ enabled: {}",
                interrupt_type.name(),
                value.bit(interrupt_type as u8)
            );
        }
    }

    // $4000202: IF (Interrupt flags)
    pub fn read_if(&self) -> u16 {
        let mut flags = self.flags.0;

        for &(_, write) in &self.pending_writes {
            match write {
                PendingWrite::If(flags_write) => {
                    flags &= !flags_write;
                }
                PendingWrite::SetFlag(interrupt_type) => {
                    flags |= 1 << (interrupt_type as u8);
                }
                _ => {}
            }
        }

        flags
    }

    // $4000202: IF (Interrupt flags)
    pub fn write_if(&mut self, value: u16, cycles: u64) {
        self.push_pending_write(PendingWrite::If(value), cycles);

        log::debug!("IF write: {value:04X} (cycles {cycles})");
    }

    // $4000208: IME (Interrupt master enable)
    pub fn read_ime(&self) -> u16 {
        for &(_, write) in self.pending_writes.iter().rev() {
            if let PendingWrite::Ime(ime) = write {
                return ime.into();
            }
        }

        self.ime.into()
    }

    // $4000208: IME (Interrupt master enable)
    pub fn write_ime(&mut self, value: u16, cycles: u64) {
        self.push_pending_write(PendingWrite::Ime(value.bit(0)), cycles);

        log::debug!("IME write: {value:04X} (enabled = {}) (cycles {cycles})", value.bit(0));
    }

    pub fn set_flag(&mut self, interrupt_type: InterruptType, cycles: u64) {
        self.push_pending_write(PendingWrite::SetFlag(interrupt_type), cycles);

        log::debug!("{} interrupt flag set (cycles {cycles})", interrupt_type.name());
    }

    pub fn pending(&self) -> bool {
        self.ime && self.enabled.0 & self.flags.0 != 0
    }

    pub fn halt_cpu(&mut self, value: u8) {
        self.halted = true;
        self.stopped = value.bit(7);

        log::debug!("HALTCNT write ({value:02X}): CPU halted");
        log::debug!("  Hardware stopped: {}", self.stopped);
    }

    pub fn cpu_halted(&self) -> bool {
        self.halted
    }

    pub fn stopped(&self) -> bool {
        self.stopped
    }
}
