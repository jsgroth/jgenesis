use crate::traits::BusInterface;

pub struct InMemoryBus {
    memory: Vec<u8>,
}

impl InMemoryBus {
    #[must_use]
    pub fn new() -> Self {
        Self { memory: vec![0; 0x0100_0000] }
    }
}

impl BusInterface for InMemoryBus {
    fn read_byte(&mut self, address: u32) -> u8 {
        self.memory[(address & Self::ADDRESS_MASK) as usize]
    }

    fn read_word(&mut self, address: u32) -> u16 {
        let address = (address & Self::ADDRESS_MASK) as usize;
        u16::from_be_bytes([self.memory[address], self.memory[address.wrapping_add(1)]])
    }

    fn write_byte(&mut self, address: u32, value: u8) {
        self.memory[(address & Self::ADDRESS_MASK) as usize] = value;
    }

    fn write_word(&mut self, address: u32, value: u16) {
        let [msb, lsb] = value.to_be_bytes();
        let address = (address & Self::ADDRESS_MASK) as usize;

        self.memory[address] = msb;
        self.memory[address.wrapping_add(1)] = lsb;
    }

    fn interrupt_level(&self) -> u8 {
        0
    }

    fn acknowledge_interrupt(&mut self) {}

    fn halt(&self) -> bool {
        false
    }

    fn reset(&self) -> bool {
        false
    }
}

impl Default for InMemoryBus {
    fn default() -> Self {
        Self::new()
    }
}
