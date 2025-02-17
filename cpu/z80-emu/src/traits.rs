#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "bincode", derive(bincode::Encode, bincode::Decode))]
pub enum InterruptLine {
    High,
    Low,
}

pub trait BusInterface {
    /// Read a byte from the given memory address.
    fn read_memory(&mut self, address: u16) -> u8;

    /// Write the given byte to the given memory address.
    fn write_memory(&mut self, address: u16, value: u8);

    /// Read a byte from the given I/O port. Most hardware only uses the lower 8 bits of the address.
    fn read_io(&mut self, address: u16) -> u8;

    /// Write the given byte to the given I/O port. Most hardware only uses the lower 8 bits of the address.
    fn write_io(&mut self, address: u16, value: u8);

    /// Poll the NMI interrupt line.
    fn nmi(&self) -> InterruptLine;

    /// Poll the INT interrupt line.
    fn int(&self) -> InterruptLine;

    /// Poll the BUSREQ line; setting this halts the Z80
    fn busreq(&self) -> bool;

    /// Poll the RESET line; setting this resets and halts the Z80
    fn reset(&self) -> bool;
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct InMemoryBus {
    pub(crate) memory: [u8; 0x10000],
    pub(crate) io_ports: [u8; 0x100],
    pub(crate) nmi: InterruptLine,
    pub(crate) int: InterruptLine,
    pub(crate) reset: bool,
}

#[cfg(test)]
impl InMemoryBus {
    pub(crate) fn new() -> Self {
        use std::array;

        Self {
            memory: array::from_fn(|_| 0),
            io_ports: array::from_fn(|_| 0),
            nmi: InterruptLine::High,
            int: InterruptLine::High,
            reset: false,
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

    fn busreq(&self) -> bool {
        false
    }

    fn reset(&self) -> bool {
        self.reset
    }
}
