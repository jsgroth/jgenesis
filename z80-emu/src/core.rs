use crate::core::instructions::ExecuteResult;
use crate::traits::{BusInterface, InterruptLine};

mod instructions;

trait GetBit: Copy {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Flags {
    sign: bool,
    zero: bool,
    y: bool,
    half_carry: bool,
    x: bool,
    overflow: bool,
    subtract: bool,
    carry: bool,
}

impl From<Flags> for u8 {
    fn from(value: Flags) -> Self {
        (u8::from(value.sign) << 7)
            | (u8::from(value.zero) << 6)
            | (u8::from(value.y) << 5)
            | (u8::from(value.half_carry) << 4)
            | (u8::from(value.x) << 3)
            | (u8::from(value.overflow) << 2)
            | (u8::from(value.subtract) << 1)
            | u8::from(value.carry)
    }
}

impl From<u8> for Flags {
    fn from(value: u8) -> Self {
        Self {
            sign: value & 0x80 != 0,
            zero: value & 0x40 != 0,
            y: value & 0x20 != 0,
            half_carry: value & 0x10 != 0,
            x: value & 0x08 != 0,
            overflow: value & 0x04 != 0,
            subtract: value & 0x02 != 0,
            carry: value & 0x01 != 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptMode {
    Mode0,
    Mode1,
    Mode2,
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
    interrupt_mode: InterruptMode,
    interrupt_delay: bool,
    last_nmi: InterruptLine,
    halted: bool,
}

impl Registers {
    pub fn new() -> Self {
        Self {
            a: 0xFF,
            f: 0xFF.into(),
            b: 0xFF,
            c: 0xFF,
            d: 0xFF,
            e: 0xFF,
            h: 0xFF,
            l: 0xFF,
            ap: 0xFF,
            fp: 0xFF.into(),
            bp: 0xFF,
            cp: 0xFF,
            dp: 0xFF,
            ep: 0xFF,
            hp: 0xFF,
            lp: 0xFF,
            i: 0xFF,
            r: 0xFF,
            ix: 0xFFFF,
            iy: 0xFFFF,
            sp: 0xFFFF,
            pc: 0x0000,
            iff1: false,
            iff2: false,
            interrupt_mode: InterruptMode::Mode0,
            interrupt_delay: false,
            last_nmi: InterruptLine::High,
            halted: false,
        }
    }
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
    IXHigh,
    IXLow,
    IYHigh,
    IYLow,
}

impl Register8 {
    fn read_from(self, registers: &Registers) -> u8 {
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
            Self::IXHigh => (registers.ix >> 8) as u8,
            Self::IXLow => registers.ix as u8,
            Self::IYHigh => (registers.iy >> 8) as u8,
            Self::IYLow => registers.iy as u8,
        }
    }

    fn write_to(self, value: u8, registers: &mut Registers) {
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
            Self::IXHigh => {
                registers.ix = (registers.ix & 0x00FF) | (u16::from(value) << 8);
            }
            Self::IXLow => {
                registers.ix = (registers.ix & 0xFF00) | u16::from(value);
            }
            Self::IYHigh => {
                registers.iy = (registers.iy & 0x00FF) | (u16::from(value) << 8);
            }
            Self::IYLow => {
                registers.iy = (registers.iy & 0xFF00) | u16::from(value);
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
    fn read_from(self, registers: &Registers) -> u16 {
        match self {
            Self::AF => u16::from_be_bytes([registers.a, registers.f.into()]),
            Self::BC => u16::from_be_bytes([registers.b, registers.c]),
            Self::DE => u16::from_be_bytes([registers.d, registers.e]),
            Self::HL => u16::from_be_bytes([registers.h, registers.l]),
            Self::IX => registers.ix,
            Self::IY => registers.iy,
            Self::SP => registers.sp,
        }
    }

    fn write_to(self, value: u16, registers: &mut Registers) {
        match self {
            Self::AF => {
                let [a, f] = value.to_be_bytes();
                registers.a = a;
                registers.f = f.into();
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
    fn read_from(self, registers: &Registers) -> u16 {
        match self {
            Self::IX => registers.ix,
            Self::IY => registers.iy,
        }
    }

    fn high_byte(self) -> Register8 {
        match self {
            Self::IX => Register8::IXHigh,
            Self::IY => Register8::IYHigh,
        }
    }

    fn low_byte(self) -> Register8 {
        match self {
            Self::IX => Register8::IXLow,
            Self::IY => Register8::IYLow,
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

#[derive(Debug, Clone)]
pub struct Z80 {
    registers: Registers,
}

impl Z80 {
    #[must_use]
    pub fn new() -> Self {
        Self {
            registers: Registers::new(),
        }
    }

    #[must_use]
    pub fn pc(&self) -> u16 {
        self.registers.pc
    }

    pub fn set_pc(&mut self, pc: u16) {
        self.registers.pc = pc;
    }

    pub fn execute_instruction<B: BusInterface>(&mut self, bus: &mut B) -> u32 {
        let ExecuteResult { t_cycles } = instructions::execute(&mut self.registers, bus);

        // TODO interrupts

        t_cycles
    }
}

impl Default for Z80 {
    fn default() -> Self {
        Self::new()
    }
}
