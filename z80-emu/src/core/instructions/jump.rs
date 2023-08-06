use crate::core::instructions::InstructionExecutor;
use crate::core::{IndexRegister, Register16, Registers};
use crate::traits::BusInterface;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JumpCondition {
    NonZero,
    Zero,
    NoCarry,
    Carry,
    ParityOdd,
    ParityEven,
    Positive,
    Negative,
}

impl JumpCondition {
    fn from_opcode(opcode: u8) -> Self {
        match opcode & 0x38 {
            0x00 => Self::NonZero,
            0x08 => Self::Zero,
            0x10 => Self::NoCarry,
            0x18 => Self::Carry,
            0x20 => Self::ParityOdd,
            0x28 => Self::ParityEven,
            0x30 => Self::Positive,
            0x38 => Self::Negative,
            _ => unreachable!("value & 0x38 is always one of the above 8 values"),
        }
    }

    fn check(self, registers: &Registers) -> bool {
        match self {
            Self::NonZero => !registers.f.zero(),
            Self::Zero => registers.f.zero(),
            Self::NoCarry => !registers.f.carry(),
            Self::Carry => registers.f.carry(),
            Self::ParityOdd => !registers.f.overflow(),
            Self::ParityEven => registers.f.overflow(),
            Self::Positive => !registers.f.sign(),
            Self::Negative => registers.f.sign(),
        }
    }
}

macro_rules! impl_jr_op {
    ($name:ident) => {
        pub(super) fn $name(&mut self) -> u32 {
            let offset = self.fetch_operand() as i8;

            self.registers.pc = (i32::from(self.registers.pc) + i32::from(offset)) as u16;

            12
        }
    };
    ($name:ident, $flag:ident == $flag_value:expr) => {
        pub(super) fn $name(&mut self) -> u32 {
            let offset = self.fetch_operand() as i8;

            if self.registers.f.$flag() == $flag_value {
                self.registers.pc = (i32::from(self.registers.pc) + i32::from(offset)) as u16;
                12
            } else {
                7
            }
        }
    };
}

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    impl_jr_op!(jr_e);
    impl_jr_op!(jr_c_e, carry == true);
    impl_jr_op!(jr_nc_e, carry == false);
    impl_jr_op!(jr_z_e, zero == true);
    impl_jr_op!(jr_nz_e, zero == false);

    pub(super) fn jp_nn(&mut self) -> u32 {
        let address = self.fetch_operand_u16();
        self.registers.pc = address;

        10
    }

    pub(super) fn jp_cc_nn(&mut self, opcode: u8) -> u32 {
        let condition = JumpCondition::from_opcode(opcode);
        let address = self.fetch_operand_u16();

        if condition.check(self.registers) {
            self.registers.pc = address;
        }

        10
    }

    pub(super) fn jp_hl(&mut self, index: Option<IndexRegister>) -> u32 {
        let register = index.map_or(Register16::HL, IndexRegister::into);
        let address = register.read_from(self.registers);

        self.registers.pc = address;

        4
    }

    pub(super) fn djnz_e(&mut self) -> u32 {
        let offset = self.fetch_operand() as i8;

        let b = self.registers.b;
        self.registers.b = b.wrapping_sub(1);

        if b != 1 {
            self.registers.pc = (i32::from(self.registers.pc) + i32::from(offset)) as u16;
            13
        } else {
            8
        }
    }

    pub(super) fn call_nn(&mut self) -> u32 {
        let address = self.fetch_operand_u16();

        self.push_stack(self.registers.pc);
        self.registers.pc = address;

        17
    }

    pub(super) fn call_cc_nn(&mut self, opcode: u8) -> u32 {
        let condition = JumpCondition::from_opcode(opcode);
        let address = self.fetch_operand_u16();

        if condition.check(self.registers) {
            self.push_stack(self.registers.pc);
            self.registers.pc = address;

            17
        } else {
            10
        }
    }

    pub(super) fn ret(&mut self) -> u32 {
        self.registers.pc = self.pop_stack();

        10
    }

    pub(super) fn ret_cc(&mut self, opcode: u8) -> u32 {
        let condition = JumpCondition::from_opcode(opcode);

        if condition.check(self.registers) {
            self.registers.pc = self.pop_stack();
            11
        } else {
            5
        }
    }

    pub(super) fn reti(&mut self) -> u32 {
        self.registers.pc = self.pop_stack();

        14
    }

    pub(super) fn retn(&mut self) -> u32 {
        self.registers.pc = self.pop_stack();
        self.registers.iff1 = self.registers.iff2;

        14
    }

    pub(super) fn rst_p(&mut self, opcode: u8) -> u32 {
        let address = opcode & 0x38;

        self.push_stack(self.registers.pc);
        self.registers.pc = address.into();

        11
    }
}
