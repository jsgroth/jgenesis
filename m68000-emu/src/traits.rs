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
}

pub(crate) trait GetBit: Copy {
    fn bit(self, i: u8) -> bool;
}

impl GetBit for u8 {
    fn bit(self, i: u8) -> bool {
        assert!(i < 8);
        self & (1 << i) != 0
    }
}

impl GetBit for u16 {
    fn bit(self, i: u8) -> bool {
        assert!(i < 16);
        self & (1 << i) != 0
    }
}

impl GetBit for u32 {
    fn bit(self, i: u8) -> bool {
        assert!(i < 32);
        self & (1 << i) != 0
    }
}

pub(crate) trait SignBit: Copy {
    fn sign_bit(self) -> bool;
}

impl SignBit for u8 {
    fn sign_bit(self) -> bool {
        self.bit(7)
    }
}

impl SignBit for u16 {
    fn sign_bit(self) -> bool {
        self.bit(15)
    }
}

impl SignBit for u32 {
    fn sign_bit(self) -> bool {
        self.bit(31)
    }
}
