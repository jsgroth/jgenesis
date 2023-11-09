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

impl_get_bit!(i8, 8);
impl_get_bit!(i16, 16);
impl_get_bit!(i32, 32);

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

impl_sign_bit!(i8, 7);
impl_sign_bit!(i16, 15);
impl_sign_bit!(i32, 31);
