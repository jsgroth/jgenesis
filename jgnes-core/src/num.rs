pub trait GetBit: Copy {
    fn bit(self, i: u8) -> bool;
}

impl GetBit for u8 {
    #[inline]
    fn bit(self, i: u8) -> bool {
        assert!(i < 8, "invalid u8 bit: {i}");

        self & (1 << i) != 0
    }
}

impl GetBit for u16 {
    #[inline]
    fn bit(self, i: u8) -> bool {
        assert!(i < 16, "invalid u16 bit: {i}");

        self & (1 << i) != 0
    }
}

impl GetBit for u32 {
    #[inline]
    fn bit(self, i: u8) -> bool {
        assert!(i < 32, "invalid u32 bit: {i}");

        self & (1 << i) != 0
    }
}
