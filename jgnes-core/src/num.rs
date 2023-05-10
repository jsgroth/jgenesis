pub trait GetBit: Copy {
    fn bit(self, i: u8) -> bool;
}

impl GetBit for u8 {
    fn bit(self, i: u8) -> bool {
        match i {
            0 => self & 0x01 != 0,
            1 => self & 0x02 != 0,
            2 => self & 0x04 != 0,
            3 => self & 0x08 != 0,
            4 => self & 0x10 != 0,
            5 => self & 0x20 != 0,
            6 => self & 0x40 != 0,
            7 => self & 0x80 != 0,
            _ => panic!("invalid u8 bit: {i}"),
        }
    }
}

impl GetBit for u16 {
    fn bit(self, i: u8) -> bool {
        match i {
            0 => self & 0x0001 != 0,
            1 => self & 0x0002 != 0,
            2 => self & 0x0004 != 0,
            3 => self & 0x0008 != 0,
            4 => self & 0x0010 != 0,
            5 => self & 0x0020 != 0,
            6 => self & 0x0040 != 0,
            7 => self & 0x0080 != 0,
            8 => self & 0x0100 != 0,
            9 => self & 0x0200 != 0,
            10 => self & 0x0400 != 0,
            11 => self & 0x0800 != 0,
            12 => self & 0x1000 != 0,
            13 => self & 0x2000 != 0,
            14 => self & 0x4000 != 0,
            15 => self & 0x8000 != 0,
            _ => panic!("invalid u16 bit: {i}"),
        }
    }
}
