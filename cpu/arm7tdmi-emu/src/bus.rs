#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryCycle {
    // Non-sequential
    N,
    // Sequential
    S,
}

pub trait BusInterface {
    /// 8-bit read instruction
    fn read_byte(&mut self, address: u32, cycle: MemoryCycle) -> u8;

    /// 16-bit read instruction
    fn read_halfword(&mut self, address: u32, cycle: MemoryCycle) -> u16;

    /// 32-bit read instruction
    fn read_word(&mut self, address: u32, cycle: MemoryCycle) -> u32;

    /// Opcode fetch in Thumb mode
    /// Implementations can override this to adjust behavior based on PC location
    fn fetch_opcode_halfword(&mut self, address: u32, cycle: MemoryCycle) -> u16 {
        self.read_halfword(address, cycle)
    }

    /// Opcode fetch in ARM mode
    /// Implementations can override this to adjust behavior based on PC location
    fn fetch_opcode_word(&mut self, address: u32, cycle: MemoryCycle) -> u32 {
        self.read_word(address, cycle)
    }

    /// 8-bit write instruction
    fn write_byte(&mut self, address: u32, value: u8, cycle: MemoryCycle);

    /// 16-bit write instruction
    fn write_halfword(&mut self, address: u32, value: u16, cycle: MemoryCycle);

    /// 32-bit write instruction
    fn write_word(&mut self, address: u32, value: u32, cycle: MemoryCycle);

    /// IRQ line
    fn irq(&self) -> bool;

    /// Called when instructions have internal cycles
    fn internal_cycles(&mut self, cycles: u32);
}
