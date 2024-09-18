pub trait GetBit: Copy {
    fn bit(self, i: u8) -> bool;
}

macro_rules! impl_get_bit {
    ($t:ty, $limit:expr) => {
        impl GetBit for $t {
            #[inline]
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
impl_get_bit!(u64, 64);
impl_get_bit!(usize, usize::BITS as u8);

impl_get_bit!(i8, 8);
impl_get_bit!(i16, 16);
impl_get_bit!(i32, 32);
impl_get_bit!(i64, 64);

pub trait SignBit: Copy {
    fn sign_bit(self) -> bool;
}

macro_rules! impl_sign_bit {
    ($t:ty, $bit:expr) => {
        impl SignBit for $t {
            #[inline]
            fn sign_bit(self) -> bool {
                self.bit($bit)
            }
        }
    };
}

impl_sign_bit!(u8, 7);
impl_sign_bit!(u16, 15);
impl_sign_bit!(u32, 31);
impl_sign_bit!(u64, 63);

impl_sign_bit!(i8, 7);
impl_sign_bit!(i16, 15);
impl_sign_bit!(i32, 31);
impl_sign_bit!(i64, 63);

pub trait U16Ext {
    fn lsb(self) -> u8;

    fn msb(self) -> u8;

    fn set_lsb(&mut self, value: u8);

    fn set_msb(&mut self, value: u8);
}

impl U16Ext for u16 {
    #[inline(always)]
    fn lsb(self) -> u8 {
        self as u8
    }

    #[inline(always)]
    fn msb(self) -> u8 {
        (self >> 8) as u8
    }

    #[inline(always)]
    fn set_lsb(&mut self, value: u8) {
        *self = (*self & 0xFF00) | u16::from(value);
    }

    #[inline(always)]
    fn set_msb(&mut self, value: u8) {
        *self = (*self & 0x00FF) | (u16::from(value) << 8);
    }
}

pub trait U24Ext {
    fn low_byte(self) -> u8;

    fn mid_byte(self) -> u8;

    fn high_byte(self) -> u8;

    fn set_low_byte(&mut self, value: u8);

    fn set_mid_byte(&mut self, value: u8);

    fn set_high_byte(&mut self, value: u8);
}

macro_rules! impl_u24_ext {
    ($t:ty) => {
        impl U24Ext for $t {
            #[inline(always)]
            fn low_byte(self) -> u8 {
                self as u8
            }

            #[inline(always)]
            fn mid_byte(self) -> u8 {
                (self >> 8) as u8
            }

            #[inline(always)]
            fn high_byte(self) -> u8 {
                (self >> 16) as u8
            }

            #[inline(always)]
            fn set_low_byte(&mut self, value: u8) {
                *self = (*self & 0xFFFF00) | <$t>::from(value);
            }

            #[inline(always)]
            fn set_mid_byte(&mut self, value: u8) {
                *self = (*self & 0xFF00FF) | (<$t>::from(value) << 8);
            }

            #[inline(always)]
            fn set_high_byte(&mut self, value: u8) {
                *self = (*self & 0x00FFFF) | (<$t>::from(value) << 16);
            }
        }
    };
}

impl_u24_ext!(u32);
impl_u24_ext!(i32);
