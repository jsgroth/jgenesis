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

    fn acknowledge_interrupt(&mut self);
}

pub(crate) trait GetBit: Copy {
    fn bit(self, i: u8) -> bool;
}

macro_rules! impl_get_bit {
    ($t:ty, $limit:expr) => {
        impl GetBit for $t {
            fn bit(self, i: u8) -> bool {
                assert!(i < $limit);
                self & (1 << i) != 0
            }
        }
    };
}

impl_get_bit!(u8, 8);
impl_get_bit!(u16, 16);
impl_get_bit!(u32, 32);

impl_get_bit!(i8, 8);
impl_get_bit!(i16, 16);
impl_get_bit!(i32, 32);

pub(crate) trait SignBit: Copy {
    fn sign_bit(self) -> bool;
}

macro_rules! impl_sign_bit {
    ($t:ty, $bit:expr) => {
        impl SignBit for $t {
            fn sign_bit(self) -> bool {
                self.bit($bit)
            }
        }
    };
}

impl_sign_bit!(u8, 7);
impl_sign_bit!(u16, 15);
impl_sign_bit!(u32, 31);

impl_sign_bit!(i8, 7);
impl_sign_bit!(i16, 15);
impl_sign_bit!(i32, 31);
