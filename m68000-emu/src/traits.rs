pub trait BusInterface {
    // Addresses are 32-bit internally but the 68000 only has a 24-bit address bus
    const ADDRESS_MASK: u32 = 0x00FF_FFFF;

    fn read_memory(&mut self, address: u32) -> u8;

    fn write_memory(&mut self, address: u32, value: u8);

    fn read_word(&mut self, address: u32) -> u16 {
        u16::from_be_bytes([
            self.read_memory(address),
            self.read_memory(address.wrapping_add(1)),
        ])
    }

    fn write_word(&mut self, address: u32, value: u16) {
        let [msb, lsb] = value.to_be_bytes();
        self.write_memory(address, msb);
        self.write_memory(address.wrapping_add(1), lsb);
    }

    fn read_long_word(&mut self, address: u32) -> u32 {
        u32::from_be_bytes([
            self.read_memory(address),
            self.read_memory(address.wrapping_add(1)),
            self.read_memory(address.wrapping_add(2)),
            self.read_memory(address.wrapping_add(3)),
        ])
    }

    fn write_long_word(&mut self, address: u32, value: u32) {
        let bytes = value.to_be_bytes();
        for i in 0..4 {
            self.write_memory(address.wrapping_add(i), bytes[i as usize]);
        }
    }
}

#[cfg(test)]
pub(crate) struct InMemoryBus {
    memory: Vec<u8>,
}

#[cfg(test)]
impl InMemoryBus {
    pub(crate) fn new() -> Self {
        Self {
            memory: vec![0; 0x0100_0000],
        }
    }
}

#[cfg(test)]
impl BusInterface for InMemoryBus {
    fn read_memory(&mut self, address: u32) -> u8 {
        self.memory[(address & Self::ADDRESS_MASK) as usize]
    }

    fn write_memory(&mut self, address: u32, value: u8) {
        self.memory[(address & Self::ADDRESS_MASK) as usize] = value;
    }
}
