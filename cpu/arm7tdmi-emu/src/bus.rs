#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryCycle {
    // Non-sequential
    N,
    // Sequential
    S,
}

pub trait BusInterface {
    fn read_byte(&mut self, address: u32, cycle: MemoryCycle) -> u8;

    fn read_halfword(&mut self, address: u32, cycle: MemoryCycle) -> u16;

    fn read_word(&mut self, address: u32, cycle: MemoryCycle) -> u32;

    fn write_byte(&mut self, address: u32, value: u8, cycle: MemoryCycle);

    fn write_halfword(&mut self, address: u32, value: u16, cycle: MemoryCycle);

    fn write_word(&mut self, address: u32, value: u32, cycle: MemoryCycle);

    fn irq(&self) -> bool;

    fn internal_cycles(&mut self, cycles: u32);
}
