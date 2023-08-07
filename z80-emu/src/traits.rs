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

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct InMemoryBus {
    pub(crate) memory: [u8; 0x10000],
    pub(crate) io_ports: [u8; 0x100],
    pub(crate) nmi: InterruptLine,
    pub(crate) int: InterruptLine,
}

#[cfg(test)]
impl InMemoryBus {
    pub(crate) fn new() -> Self {
        Self {
            memory: [0; 0x10000],
            io_ports: [0; 0x100],
            nmi: InterruptLine::High,
            int: InterruptLine::High,
        }
    }
}

#[cfg(test)]
impl BusInterface for InMemoryBus {
    fn read_memory(&mut self, address: u16) -> u8 {
        self.memory[address as usize]
    }

    fn write_memory(&mut self, address: u16, value: u8) {
        self.memory[address as usize] = value;
    }

    fn read_io(&mut self, address: u16) -> u8 {
        self.io_ports[(address & 0xFF) as usize]
    }

    fn write_io(&mut self, address: u16, value: u8) {
        self.io_ports[(address & 0xFF) as usize] = value;
    }

    fn nmi(&self) -> InterruptLine {
        self.nmi
    }

    fn int(&self) -> InterruptLine {
        self.int
    }
}
