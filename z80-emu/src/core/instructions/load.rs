use crate::core::instructions::{BlockMode, InstructionExecutor};
use crate::core::{IndexRegister, Register16, Register8};
use crate::traits::BusInterface;
use std::mem;

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
        debug_assert!(register == Register8::I || register == Register8::R);

        let value = register.read_from(self.registers);
        self.registers.a = value;

        self.registers
            .f
            .set_sign_from(value)
            .set_zero_from(value)
            .set_half_carry(false)
            .set_overflow(self.registers.iff2)
            .set_subtract(false);

        9
    }

    // LD I, A and LD R, A
    pub(super) fn ld_ir_a(&mut self, register: Register8) -> u32 {
        debug_assert!(register == Register8::I || register == Register8::R);

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

    pub(super) fn exchange_de_hl(&mut self) -> u32 {
        mem::swap(&mut self.registers.d, &mut self.registers.h);
        mem::swap(&mut self.registers.e, &mut self.registers.l);

        4
    }

    pub(super) fn exchange_af(&mut self) -> u32 {
        mem::swap(&mut self.registers.a, &mut self.registers.ap);
        mem::swap(&mut self.registers.f, &mut self.registers.fp);

        4
    }

    pub(super) fn exchange_bcdehl(&mut self) -> u32 {
        mem::swap(&mut self.registers.b, &mut self.registers.bp);
        mem::swap(&mut self.registers.c, &mut self.registers.cp);
        mem::swap(&mut self.registers.d, &mut self.registers.dp);
        mem::swap(&mut self.registers.e, &mut self.registers.ep);
        mem::swap(&mut self.registers.h, &mut self.registers.hp);
        mem::swap(&mut self.registers.l, &mut self.registers.lp);

        4
    }

    pub(super) fn exchange_stack_hl(&mut self, index: Option<IndexRegister>) -> u32 {
        let register = index.map_or(Register16::HL, IndexRegister::into);
        let register_value = register.read_from(self.registers);
        let stack_value = self.pop_stack();

        register.write_to(stack_value, self.registers);
        self.push_stack(register_value);

        19
    }

    pub(super) fn block_transfer(&mut self, mode: BlockMode, repeat: bool) -> u32 {
        let hl = Register16::HL.read_from(self.registers);
        let de = Register16::DE.read_from(self.registers);

        let value = self.bus.read_memory(hl);
        self.bus.write_memory(de, value);

        let bc = Register16::BC.read_from(self.registers);
        Register16::BC.write_to(bc.wrapping_sub(1), self.registers);

        Register16::HL.write_to(mode.apply(hl), self.registers);
        Register16::DE.write_to(mode.apply(de), self.registers);

        let should_repeat = repeat && bc == 1;
        if should_repeat {
            self.registers.pc = self.registers.pc.wrapping_sub(2);
        }

        self.registers
            .f
            .set_half_carry(false)
            .set_overflow(bc != 1)
            .set_subtract(false);

        if should_repeat {
            21
        } else {
            16
        }
    }
}
