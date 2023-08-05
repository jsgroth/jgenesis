mod instructions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Flags(u8);

impl Flags {
    const SIGN_BIT: u8 = 1 << 7;
    const ZERO_BIT: u8 = 1 << 6;
    const HALF_CARRY_BIT: u8 = 1 << 4;
    const OVERFLOW_BIT: u8 = 1 << 2;
    const SUBTRACT_BIT: u8 = 1 << 1;
    const CARRY_BIT: u8 = 1 << 0;

    fn sign(self) -> bool {
        self.0 & Self::SIGN_BIT != 0
    }

    fn set_sign(&mut self, sign: bool) -> &mut Self {
        if sign {
            self.0 |= Self::SIGN_BIT;
        } else {
            self.0 &= !Self::SIGN_BIT;
        }
        self
    }

    fn zero(self) -> bool {
        self.0 & Self::ZERO_BIT != 0
    }

    fn set_zero(&mut self, zero: bool) -> &mut Self {
        if zero {
            self.0 |= Self::ZERO_BIT;
        } else {
            self.0 &= !Self::ZERO_BIT;
        }
        self
    }

    fn half_carry(self) -> bool {
        self.0 & Self::ZERO_BIT != 0
    }

    fn set_half_carry(&mut self, half_carry: bool) -> &mut Self {
        if half_carry {
            self.0 |= Self::HALF_CARRY_BIT;
        } else {
            self.0 &= !Self::HALF_CARRY_BIT;
        }
        self
    }

    fn overflow(self) -> bool {
        self.0 & Self::OVERFLOW_BIT != 0
    }

    fn set_overflow(&mut self, overflow: bool) -> &mut Self {
        if overflow {
            self.0 |= Self::OVERFLOW_BIT;
        } else {
            self.0 &= !Self::OVERFLOW_BIT;
        }
        self
    }

    fn subtract(self) -> bool {
        self.0 & Self::SUBTRACT_BIT != 0
    }

    fn set_subtract(&mut self, subtract: bool) -> &mut Self {
        if subtract {
            self.0 |= Self::SUBTRACT_BIT;
        } else {
            self.0 &= !Self::SUBTRACT_BIT;
        }
        self
    }

    fn carry(self) -> bool {
        self.0 & Self::CARRY_BIT != 0
    }

    fn set_carry(&mut self, carry: bool) -> &mut Self {
        if carry {
            self.0 |= Self::CARRY_BIT;
        } else {
            self.0 &= !Self::CARRY_BIT;
        }
        self
    }

    fn to_byte(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct Registers {
    a: u8,
    f: Flags,
    b: u8,
    c: u8,
    d: u8,
    e: u8,
    h: u8,
    l: u8,
    ap: u8,
    fp: Flags,
    bp: u8,
    cp: u8,
    dp: u8,
    ep: u8,
    hp: u8,
    lp: u8,
    i: u8,
    r: u8,
    ix: u16,
    iy: u16,
    sp: u16,
    pc: u16,
    iff1: bool,
    iff2: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Register8 {
    A,
    B,
    C,
    D,
    E,
    H,
    L,
    I,
    R,
}

impl Register8 {
    fn read(self, registers: &Registers) -> u8 {
        match self {
            Self::A => registers.a,
            Self::B => registers.b,
            Self::C => registers.c,
            Self::D => registers.d,
            Self::E => registers.e,
            Self::H => registers.h,
            Self::L => registers.l,
            Self::I => registers.i,
            Self::R => registers.r,
        }
    }

    fn write(self, registers: &mut Registers, value: u8) {
        match self {
            Self::A => {
                registers.a = value;
            }
            Self::B => {
                registers.b = value;
            }
            Self::C => {
                registers.c = value;
            }
            Self::D => {
                registers.d = value;
            }
            Self::E => {
                registers.e = value;
            }
            Self::H => {
                registers.h = value;
            }
            Self::L => {
                registers.l = value;
            }
            Self::I => {
                registers.i = value;
            }
            Self::R => {
                registers.r = value;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Register16 {
    AF,
    BC,
    DE,
    HL,
    IX,
    IY,
    SP,
}

impl Register16 {
    fn read(self, registers: &Registers) -> u16 {
        match self {
            Self::AF => u16::from_be_bytes([registers.a, registers.f.to_byte()]),
            Self::BC => u16::from_be_bytes([registers.b, registers.c]),
            Self::DE => u16::from_be_bytes([registers.d, registers.e]),
            Self::HL => u16::from_be_bytes([registers.h, registers.l]),
            Self::IX => registers.ix,
            Self::IY => registers.iy,
            Self::SP => registers.sp,
        }
    }

    fn write(self, registers: &mut Registers, value: u16) {
        match self {
            Self::AF => {
                let [a, f] = value.to_be_bytes();
                registers.a = a;
                registers.f = Flags(f);
            }
            Self::BC => {
                let [b, c] = value.to_be_bytes();
                registers.b = b;
                registers.c = c;
            }
            Self::DE => {
                let [d, e] = value.to_be_bytes();
                registers.d = d;
                registers.e = e;
            }
            Self::HL => {
                let [h, l] = value.to_be_bytes();
                registers.h = h;
                registers.l = l;
            }
            Self::IX => {
                registers.ix = value;
            }
            Self::IY => {
                registers.iy = value;
            }
            Self::SP => {
                registers.sp = value;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexRegister {
    IX,
    IY,
}

impl IndexRegister {
    fn read(self, registers: &Registers) -> u16 {
        match self {
            Self::IX => registers.ix,
            Self::IY => registers.iy,
        }
    }
}

impl From<IndexRegister> for Register16 {
    fn from(value: IndexRegister) -> Self {
        match value {
            IndexRegister::IX => Register16::IX,
            IndexRegister::IY => Register16::IY,
        }
    }
}
