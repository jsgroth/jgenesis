use crate::traits::BusInterface;
use bincode::{Decode, Encode};
use jgenesis_traits::num::GetBit;

mod instructions;
pub mod traits;

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
pub struct StatusRegister {
    negative: bool,
    overflow: bool,
    direct_page: bool,
    break_flag: bool,
    half_carry: bool,
    interrupt_enabled: bool,
    zero: bool,
    carry: bool,
}

impl From<StatusRegister> for u8 {
    fn from(value: StatusRegister) -> Self {
        (u8::from(value.negative) << 7)
            | (u8::from(value.overflow) << 6)
            | (u8::from(value.direct_page) << 5)
            | (u8::from(value.break_flag) << 4)
            | (u8::from(value.half_carry) << 3)
            | (u8::from(value.interrupt_enabled) << 2)
            | (u8::from(value.zero) << 1)
            | u8::from(value.carry)
    }
}

impl From<u8> for StatusRegister {
    fn from(value: u8) -> Self {
        Self {
            negative: value.bit(7),
            overflow: value.bit(6),
            direct_page: value.bit(5),
            break_flag: value.bit(4),
            half_carry: value.bit(3),
            interrupt_enabled: value.bit(2),
            zero: value.bit(1),
            carry: value.bit(0),
        }
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct Registers {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    pub psw: StatusRegister,
}

impl Registers {
    fn ya(&self) -> u16 {
        u16::from_le_bytes([self.a, self.y])
    }

    fn set_ya(&mut self, ya: u16) {
        let [a, y] = ya.to_le_bytes();
        self.a = a;
        self.y = y;
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
struct State {
    opcode: u8,
    cycle: u8,
    stopped: bool,
    // Variables to store data between cycles
    t0: u8,
    t1: u8,
    t2: u8,
    t3: u8,
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct Spc700 {
    registers: Registers,
    state: State,
}

impl Spc700 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tick<B: BusInterface>(&mut self, bus: &mut B) {
        instructions::execute(self, bus);
    }

    fn final_cycle(&mut self) {
        self.state.cycle = 0;
    }

    fn direct_page_msb(&self) -> u8 {
        // 1 -> 0x01, 0 -> 0x00
        self.registers.psw.direct_page.into()
    }

    fn stack_pointer(&self) -> u16 {
        u16::from_le_bytes([self.registers.sp, 0x01])
    }

    pub fn is_mid_instruction(&self) -> bool {
        self.state.cycle != 0
    }

    pub fn registers(&self) -> &Registers {
        &self.registers
    }

    pub fn set_registers(&mut self, registers: Registers) {
        self.registers = registers;
    }
}
