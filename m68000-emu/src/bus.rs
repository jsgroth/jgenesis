use crate::traits::BusInterface;

pub struct InMemoryBus {
    memory: Vec<u8>,
}

impl InMemoryBus {
    #[must_use]
    pub fn new() -> Self {
        Self {
            memory: vec![0; 0x0100_0000],
        }
    }
}

impl BusInterface for InMemoryBus {
    fn read_memory(&mut self, address: u32) -> u8 {
        self.memory[(address & Self::ADDRESS_MASK) as usize]
    }

    fn write_memory(&mut self, address: u32, value: u8) {
        self.memory[(address & Self::ADDRESS_MASK) as usize] = value;
    }
}

impl Default for InMemoryBus {
    fn default() -> Self {
        Self::new()
    }
}
