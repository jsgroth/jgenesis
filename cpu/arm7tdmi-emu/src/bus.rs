use bincode::{Decode, Encode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum MemoryCycle {
    N, // Non-sequential
    S, // Sequential
}

pub struct OpSize;

impl OpSize {
    pub const BYTE: u8 = 0;
    pub const HALFWORD: u8 = 1;
    pub const WORD: u8 = 2;

    #[must_use]
    pub const fn display(size: u8) -> &'static str {
        match size {
            Self::BYTE => "Byte",
            Self::HALFWORD => "Halfword",
            Self::WORD => "Word",
            _ => "(invalid)",
        }
    }
}

pub trait BusInterface {
    /// 8-bit / 16-bit / 32-bit read instruction
    fn read<const SIZE: u8>(&mut self, address: u32, cycle: MemoryCycle) -> u32;

    /// 16-bit (Thumb) / 32-bit (ARM) opcode fetch
    /// Implementations can override this to adjust behavior based on PC location or whether a read
    /// is for instruction fetch
    fn fetch_opcode<const SIZE: u8>(&mut self, address: u32, cycle: MemoryCycle) -> u32 {
        self.read::<SIZE>(address, cycle)
    }

    /// 8-bit / 16-bit / 32-bit write instruction
    fn write<const SIZE: u8>(&mut self, address: u32, value: u32, cycle: MemoryCycle);

    /// 8-bit read instruction
    fn read_byte(&mut self, address: u32, cycle: MemoryCycle) -> u8 {
        self.read::<{ OpSize::BYTE }>(address, cycle) as u8
    }

    /// 16-bit read instruction
    fn read_halfword(&mut self, address: u32, cycle: MemoryCycle) -> u16 {
        self.read::<{ OpSize::HALFWORD }>(address, cycle) as u16
    }

    /// 32-bit read instruction
    fn read_word(&mut self, address: u32, cycle: MemoryCycle) -> u32 {
        self.read::<{ OpSize::WORD }>(address, cycle)
    }

    /// Opcode fetch in Thumb mode
    /// Implementations can override this to adjust behavior based on PC location
    fn fetch_opcode_halfword(&mut self, address: u32, cycle: MemoryCycle) -> u16 {
        self.fetch_opcode::<{ OpSize::HALFWORD }>(address, cycle) as u16
    }

    /// Opcode fetch in ARM mode
    /// Implementations can override this to adjust behavior based on PC location
    fn fetch_opcode_word(&mut self, address: u32, cycle: MemoryCycle) -> u32 {
        self.fetch_opcode::<{ OpSize::WORD }>(address, cycle)
    }

    /// 8-bit write instruction
    fn write_byte(&mut self, address: u32, value: u8, cycle: MemoryCycle) {
        self.write::<{ OpSize::BYTE }>(address, value.into(), cycle);
    }

    /// 16-bit write instruction
    fn write_halfword(&mut self, address: u32, value: u16, cycle: MemoryCycle) {
        self.write::<{ OpSize::HALFWORD }>(address, value.into(), cycle);
    }

    /// 32-bit write instruction
    fn write_word(&mut self, address: u32, value: u32, cycle: MemoryCycle) {
        self.write::<{ OpSize::WORD }>(address, value, cycle);
    }

    /// IRQ line
    fn irq(&self) -> bool;

    /// Called when instructions have internal cycles
    fn internal_cycles(&mut self, cycles: u32);

    /// Lock the bus (used by SWAP instruction)
    fn lock(&mut self) {}

    /// Unlock the bus (used by SWAP instruction)
    fn unlock(&mut self) {}
}
