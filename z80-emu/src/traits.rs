#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptLine {
    High,
    Low,
}

pub trait BusInterface {
    fn read_memory(&mut self, address: u16) -> u8;

    fn write_memory(&mut self, address: u16, value: u8);

    fn read_io(&mut self, address: u16) -> u8;

    fn write_io(&mut self, address: u16, value: u8);

    fn nmi(&self) -> InterruptLine;

    fn int(&self) -> InterruptLine;
}
