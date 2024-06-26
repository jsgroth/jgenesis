pub trait BusInterface {
    // Addresses are 32-bit internally but the 68000 only has a 24-bit address bus
    const ADDRESS_MASK: u32 = 0x00FF_FFFF;

    fn read_byte(&mut self, address: u32) -> u8;

    fn read_word(&mut self, address: u32) -> u16;

    fn write_byte(&mut self, address: u32, value: u8);

    fn write_word(&mut self, address: u32, value: u16);

    fn read_long_word(&mut self, address: u32) -> u32 {
        let high_word = self.read_word(address);
        let low_word = self.read_word(address.wrapping_add(2));

        (u32::from(high_word) << 16) | u32::from(low_word)
    }

    fn write_long_word(&mut self, address: u32, value: u32) {
        let high_word = (value >> 16) as u16;
        let low_word = value as u16;

        self.write_word(address, high_word);
        self.write_word(address.wrapping_add(2), low_word);
    }

    // Auto-vectored interrupt level; should be between 0 and 7, with 0 indicating no interrupt
    fn interrupt_level(&self) -> u8;

    fn acknowledge_interrupt(&mut self, interrupt_level: u8);

    fn halt(&self) -> bool;

    fn reset(&self) -> bool;
}
