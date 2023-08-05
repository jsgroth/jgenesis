pub trait AddressSpace {
    fn read(&mut self, address: u16) -> u8;

    fn write(&mut self, address: u16, value: u8);
}

pub trait IOPorts {
    fn read(&mut self, address: u16) -> u8;

    fn write(&mut self, address: u16, value: u8);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptLine {
    High,
    Low,
}

pub trait InterruptLines {
    fn nmi(&self) -> InterruptLine;

    fn int(&self) -> InterruptLine;
}
