use crate::core::instructions::{GetBit, InstructionExecutor};
use crate::core::{IndexRegister, Register16, Register8};
use crate::traits::BusInterface;

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    pub(super) fn ld_r_r(&mut self, opcode: u8, index: Option<IndexRegister>) -> u32 {
        let write_target =
            super::parse_register_from_opcode(opcode >> 3, index).expect("invalid opcode");
        let read_target = super::parse_register_from_opcode(opcode, index).expect("invalid opcode");

        let value = read_target.read_from(self.registers);
        write_target.write_to(value, self.registers);

        4
    }

    pub(super) fn ld_r_immediate(&mut self, opcode: u8, index: Option<IndexRegister>) -> u32 {
        let write_target =
            super::parse_register_from_opcode(opcode >> 3, index).expect("invalid opcode)");
        let value = self.fetch_operand();

        write_target.write_to(value, self.registers);

        7
    }

    pub(super) fn ld_r_hl(&mut self, opcode: u8, index: Option<IndexRegister>) -> u32 {
        let write_target =
            super::parse_register_from_opcode(opcode >> 3, None).expect("invalid opcode");
        let address = self.fetch_indirect_hl_address(index);
        let value = self.bus.read_memory(address);

        write_target.write_to(value, self.registers);

        match index {
            Some(_) => 15,
            None => 7,
        }
    }

    pub(super) fn ld_hl_r(&mut self, opcode: u8, index: Option<IndexRegister>) -> u32 {
        let read_target = super::parse_register_from_opcode(opcode, None).expect("invalid opcode");
        let value = read_target.read_from(self.registers);
        let address = self.fetch_indirect_hl_address(index);

        self.bus.write_memory(address, value);

        match index {
            Some(_) => 15,
            None => 7,
        }
    }

    pub(super) fn ld_hl_immediate(&mut self, index: Option<IndexRegister>) -> u32 {
        let value = self.fetch_operand();
        let address = self.fetch_indirect_hl_address(index);

        self.bus.write_memory(address, value);

        match index {
            Some(_) => 15,
            None => 10,
        }
    }

    pub(super) fn ld_a_indirect(&mut self, register: Register16) -> u32 {
        let address = register.read_from(self.registers);
        let value = self.bus.read_memory(address);

        self.registers.a = value;

        7
    }

    pub(super) fn ld_a_direct(&mut self) -> u32 {
        let address = self.fetch_operand_u16();
        let value = self.bus.read_memory(address);

        self.registers.a = value;

        13
    }

    pub(super) fn ld_indirect_a(&mut self, register: Register16) -> u32 {
        let address = register.read_from(self.registers);

        self.bus.write_memory(address, self.registers.a);

        7
    }

    pub(super) fn ld_direct_a(&mut self) -> u32 {
        let address = self.fetch_operand_u16();
        self.bus.write_memory(address, self.registers.a);

        13
    }

    // LD A, I and LD A, R
    pub(super) fn ld_a_ir(&mut self, register: Register8) -> u32 {
        let value = register.read_from(self.registers);
        self.registers.a = value;

        self.registers
            .f
            .set_sign(value.bit(7))
            .set_zero(value == 0)
            .set_half_carry(false)
            .set_overflow(self.registers.iff2)
            .set_subtract(false);

        9
    }

    // LD I, A and LD R, A
    pub(super) fn ld_ir_a(&mut self, register: Register8) -> u32 {
        register.write_to(self.registers.a, self.registers);

        9
    }

    pub(super) fn ld_dd_immediate(&mut self, opcode: u8, index: Option<IndexRegister>) -> u32 {
        let register = super::parse_dd_register(opcode, index);
        let value = self.fetch_operand_u16();

        register.write_to(value, self.registers);

        10
    }

    pub(super) fn ld_hl_direct(&mut self, index: Option<IndexRegister>) -> u32 {
        let address = self.fetch_operand_u16();
        let value = self.read_memory_u16(address);

        let register = index.map_or(Register16::HL, IndexRegister::into);
        register.write_to(value, self.registers);

        16
    }

    pub(super) fn ld_dd_direct(&mut self, opcode: u8) -> u32 {
        let register = super::parse_dd_register(opcode, None);
        let address = self.fetch_operand_u16();
        let value = self.read_memory_u16(address);

        register.write_to(value, self.registers);

        20
    }

    pub(super) fn ld_direct_hl(&mut self, index: Option<IndexRegister>) -> u32 {
        let register = index.map_or(Register16::HL, IndexRegister::into);
        let value = register.read_from(self.registers);
        let address = self.fetch_operand_u16();

        self.write_memory_u16(address, value);

        16
    }

    pub(super) fn ld_direct_dd(&mut self, opcode: u8) -> u32 {
        let register = super::parse_dd_register(opcode, None);
        let value = register.read_from(self.registers);
        let address = self.fetch_operand_u16();

        self.write_memory_u16(address, value);

        20
    }

    pub(super) fn ld_sp_hl(&mut self, index: Option<IndexRegister>) -> u32 {
        let register = index.map_or(Register16::HL, IndexRegister::into);
        let value = register.read_from(self.registers);

        self.registers.sp = value;

        6
    }

    pub(super) fn push_qq(&mut self, opcode: u8, index: Option<IndexRegister>) -> u32 {
        let register = super::parse_qq_register(opcode, index);
        let value = register.read_from(self.registers);

        self.push_stack(value);

        11
    }

    pub(super) fn pop_qq(&mut self, opcode: u8, index: Option<IndexRegister>) -> u32 {
        let register = super::parse_qq_register(opcode, index);
        let value = self.pop_stack();

        register.write_to(value, self.registers);

        10
    }
}
