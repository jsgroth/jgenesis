use crate::core::instructions::{BlockMode, InstructionExecutor, sign_flag, zero_flag};
use crate::core::{Flags, IndexRegister, Register8, Register16};
use crate::traits::BusInterface;
use std::mem;

impl<B: BusInterface> InstructionExecutor<'_, '_, B> {
    pub(super) fn ld_r_r(&mut self, opcode: u8, index: Option<IndexRegister>) -> u32 {
        let write_target =
            super::parse_register_from_opcode(opcode >> 3, index).expect("invalid opcode");
        let read_target = super::parse_register_from_opcode(opcode, index).expect("invalid opcode");

        let value = read_target.read_from(&self.cpu.registers);
        write_target.write_to(value, &mut self.cpu.registers);

        4
    }

    pub(super) fn ld_r_immediate(&mut self, opcode: u8, index: Option<IndexRegister>) -> u32 {
        let write_target =
            super::parse_register_from_opcode(opcode >> 3, index).expect("invalid opcode)");
        let value = self.fetch_operand();

        write_target.write_to(value, &mut self.cpu.registers);

        7
    }

    pub(super) fn ld_r_hl(&mut self, opcode: u8, index: Option<IndexRegister>) -> u32 {
        let write_target =
            super::parse_register_from_opcode(opcode >> 3, None).expect("invalid opcode");
        let address = self.fetch_indirect_hl_address(index);
        let value = self.bus.read_memory(address);

        write_target.write_to(value, &mut self.cpu.registers);

        match index {
            Some(_) => 15,
            None => 7,
        }
    }

    pub(super) fn ld_hl_r(&mut self, opcode: u8, index: Option<IndexRegister>) -> u32 {
        let read_target = super::parse_register_from_opcode(opcode, None).expect("invalid opcode");
        let value = read_target.read_from(&self.cpu.registers);
        let address = self.fetch_indirect_hl_address(index);

        self.bus.write_memory(address, value);

        match index {
            Some(_) => 15,
            None => 7,
        }
    }

    pub(super) fn ld_hl_immediate(&mut self, index: Option<IndexRegister>) -> u32 {
        let address = self.fetch_indirect_hl_address(index);
        let value = self.fetch_operand();

        self.bus.write_memory(address, value);

        match index {
            Some(_) => 15,
            None => 10,
        }
    }

    pub(super) fn ld_a_indirect(&mut self, register: Register16) -> u32 {
        let address = register.read_from(&self.cpu.registers);
        let value = self.bus.read_memory(address);

        self.cpu.registers.a = value;

        7
    }

    pub(super) fn ld_a_direct(&mut self) -> u32 {
        let address = self.fetch_operand_u16();
        let value = self.bus.read_memory(address);

        self.cpu.registers.a = value;

        13
    }

    pub(super) fn ld_indirect_a(&mut self, register: Register16) -> u32 {
        let address = register.read_from(&self.cpu.registers);

        self.bus.write_memory(address, self.cpu.registers.a);

        7
    }

    pub(super) fn ld_direct_a(&mut self) -> u32 {
        let address = self.fetch_operand_u16();
        self.bus.write_memory(address, self.cpu.registers.a);

        13
    }

    // LD A, I and LD A, R
    pub(super) fn ld_a_ir(&mut self, register: Register8) -> u32 {
        debug_assert!(register == Register8::I || register == Register8::R);

        let value = register.read_from(&self.cpu.registers);
        self.cpu.registers.a = value;

        self.cpu.registers.f = Flags {
            sign: sign_flag(value),
            zero: zero_flag(value),
            half_carry: false,
            overflow: self.cpu.registers.iff2,
            subtract: false,
            ..self.cpu.registers.f
        };

        9
    }

    // LD I, A and LD R, A
    pub(super) fn ld_ir_a(&mut self, register: Register8) -> u32 {
        debug_assert!(register == Register8::I || register == Register8::R);

        register.write_to(self.cpu.registers.a, &mut self.cpu.registers);

        9
    }

    pub(super) fn ld_dd_immediate(&mut self, opcode: u8, index: Option<IndexRegister>) -> u32 {
        let register = super::parse_dd_register(opcode, index);
        let value = self.fetch_operand_u16();

        register.write_to(value, &mut self.cpu.registers);

        10
    }

    pub(super) fn ld_hl_direct(&mut self, index: Option<IndexRegister>) -> u32 {
        let address = self.fetch_operand_u16();
        let value = self.read_memory_u16(address);

        let register = index.map_or(Register16::HL, IndexRegister::into);
        register.write_to(value, &mut self.cpu.registers);

        16
    }

    pub(super) fn ld_dd_direct(&mut self, opcode: u8) -> u32 {
        let register = super::parse_dd_register(opcode, None);
        let address = self.fetch_operand_u16();
        let value = self.read_memory_u16(address);

        register.write_to(value, &mut self.cpu.registers);

        20
    }

    pub(super) fn ld_direct_hl(&mut self, index: Option<IndexRegister>) -> u32 {
        let register = index.map_or(Register16::HL, IndexRegister::into);
        let value = register.read_from(&self.cpu.registers);
        let address = self.fetch_operand_u16();

        self.write_memory_u16(address, value);

        16
    }

    pub(super) fn ld_direct_dd(&mut self, opcode: u8) -> u32 {
        let register = super::parse_dd_register(opcode, None);
        let value = register.read_from(&self.cpu.registers);
        let address = self.fetch_operand_u16();

        self.write_memory_u16(address, value);

        20
    }

    pub(super) fn ld_sp_hl(&mut self, index: Option<IndexRegister>) -> u32 {
        let register = index.map_or(Register16::HL, IndexRegister::into);
        let value = register.read_from(&self.cpu.registers);

        self.cpu.registers.sp = value;

        6
    }

    pub(super) fn push_qq(&mut self, opcode: u8, index: Option<IndexRegister>) -> u32 {
        let register = super::parse_qq_register(opcode, index);
        let value = register.read_from(&self.cpu.registers);

        self.push_stack(value);

        11
    }

    pub(super) fn pop_qq(&mut self, opcode: u8, index: Option<IndexRegister>) -> u32 {
        let register = super::parse_qq_register(opcode, index);
        let value = self.pop_stack();

        register.write_to(value, &mut self.cpu.registers);

        10
    }

    pub(super) fn exchange_de_hl(&mut self) -> u32 {
        mem::swap(&mut self.cpu.registers.d, &mut self.cpu.registers.h);
        mem::swap(&mut self.cpu.registers.e, &mut self.cpu.registers.l);

        4
    }

    pub(super) fn exchange_af(&mut self) -> u32 {
        mem::swap(&mut self.cpu.registers.a, &mut self.cpu.registers.ap);
        mem::swap(&mut self.cpu.registers.f, &mut self.cpu.registers.fp);

        4
    }

    pub(super) fn exchange_bcdehl(&mut self) -> u32 {
        mem::swap(&mut self.cpu.registers.b, &mut self.cpu.registers.bp);
        mem::swap(&mut self.cpu.registers.c, &mut self.cpu.registers.cp);
        mem::swap(&mut self.cpu.registers.d, &mut self.cpu.registers.dp);
        mem::swap(&mut self.cpu.registers.e, &mut self.cpu.registers.ep);
        mem::swap(&mut self.cpu.registers.h, &mut self.cpu.registers.hp);
        mem::swap(&mut self.cpu.registers.l, &mut self.cpu.registers.lp);

        4
    }

    pub(super) fn exchange_stack_hl(&mut self, index: Option<IndexRegister>) -> u32 {
        let register = index.map_or(Register16::HL, IndexRegister::into);
        let register_value = register.read_from(&self.cpu.registers);
        let stack_value = self.pop_stack();

        register.write_to(stack_value, &mut self.cpu.registers);
        self.push_stack(register_value);

        19
    }

    pub(super) fn block_transfer(&mut self, mode: BlockMode, repeat: bool) -> u32 {
        let hl = Register16::HL.read_from(&self.cpu.registers);
        let de = Register16::DE.read_from(&self.cpu.registers);

        let value = self.bus.read_memory(hl);
        self.bus.write_memory(de, value);

        let bc = Register16::BC.read_from(&self.cpu.registers);
        Register16::BC.write_to(bc.wrapping_sub(1), &mut self.cpu.registers);

        Register16::HL.write_to(mode.apply(hl), &mut self.cpu.registers);
        Register16::DE.write_to(mode.apply(de), &mut self.cpu.registers);

        let should_repeat = repeat && bc != 1;
        if should_repeat {
            self.cpu.registers.pc = self.cpu.registers.pc.wrapping_sub(2);
        }

        self.cpu.registers.f =
            Flags { half_carry: false, overflow: bc != 1, subtract: false, ..self.cpu.registers.f };

        if should_repeat { 21 } else { 16 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Z80;
    use crate::traits::InMemoryBus;

    const REAL_REGISTERS: &[Register8] = &[
        Register8::B,
        Register8::C,
        Register8::D,
        Register8::E,
        Register8::H,
        Register8::L,
        Register8::A,
    ];

    fn register_opcode_bits(register: Register8) -> u8 {
        use Register8::*;

        match register {
            B => 0x00,
            C => 0x01,
            D => 0x02,
            E => 0x03,
            H => 0x04,
            L => 0x05,
            A => 0x07,
            _ => panic!("unexpected register: {register:?}"),
        }
    }

    #[test]
    fn load_r_r() {
        let mut cpu = Z80::new();
        let mut bus = InMemoryBus::new();

        for &r in REAL_REGISTERS {
            for &rp in REAL_REGISTERS {
                let opcode = 0x40 | (register_opcode_bits(r) << 3) | register_opcode_bits(rp);

                r.write_to(rand::random(), &mut cpu.registers);
                let value = rand::random();
                rp.write_to(value, &mut cpu.registers);

                InstructionExecutor::new(&mut cpu, &mut bus).ld_r_r(opcode, None);

                assert_eq!(r.read_from(&cpu.registers), value, "LD {r:?}, {rp:?}");
                assert_eq!(rp.read_from(&cpu.registers), value, "LD {r:?}, {rp:?}");
            }
        }
    }

    #[test]
    fn load_r_hl() {
        let mut cpu = Z80::new();
        let mut bus = InMemoryBus::new();

        for &r in REAL_REGISTERS {
            let opcode = 0x46 | (register_opcode_bits(r) << 3);

            let address = rand::random();
            let value = rand::random();

            bus.write_memory(address, value);
            r.write_to(rand::random(), &mut cpu.registers);
            Register16::HL.write_to(address, &mut cpu.registers);

            InstructionExecutor::new(&mut cpu, &mut bus).ld_r_hl(opcode, None);

            assert_eq!(r.read_from(&cpu.registers), value, "LD {r:?}, (HL)");
            assert_eq!(bus.read_memory(address), value, "LD {r:?}, (HL)");
        }
    }

    #[test]
    fn load_r_immediate() {
        let mut cpu = Z80::new();
        let mut bus = InMemoryBus::new();

        for &r in REAL_REGISTERS {
            let opcode = 0x06 | (register_opcode_bits(r) << 3);

            let value = rand::random();
            r.write_to(rand::random(), &mut cpu.registers);

            let pc = rand::random();
            cpu.registers.pc = pc;
            bus.write_memory(cpu.registers.pc, value);

            InstructionExecutor::new(&mut cpu, &mut bus).ld_r_immediate(opcode, None);

            assert_eq!(cpu.registers.pc, pc.wrapping_add(1), "LD {r:?}, n");
            assert_eq!(r.read_from(&cpu.registers), value, "LD {r:?}, n");
        }
    }

    #[test]
    fn load_hl_r() {
        let mut cpu = Z80::new();
        let mut bus = InMemoryBus::new();

        for &r in REAL_REGISTERS {
            let opcode = 0x70 | register_opcode_bits(r);

            let address = rand::random();
            let value = rand::random();

            Register16::HL.write_to(address, &mut cpu.registers);
            r.write_to(value, &mut cpu.registers);
            let address = Register16::HL.read_from(&cpu.registers);

            bus.write_memory(address, rand::random());

            InstructionExecutor::new(&mut cpu, &mut bus).ld_hl_r(opcode, None);

            assert_eq!(r.read_from(&cpu.registers), value, "LD (HL), {r:?}");
            assert_eq!(bus.read_memory(address), value, "LD (HL), {r:?}");
        }
    }

    #[test]
    fn load_hl_immediate() {
        let mut cpu = Z80::new();
        let mut bus = InMemoryBus::new();

        let address = rand::random();
        let value = rand::random();

        Register16::HL.write_to(address, &mut cpu.registers);

        bus.write_memory(address, rand::random());

        let pc = rand::random();
        cpu.registers.pc = pc;
        bus.write_memory(pc, value);

        InstructionExecutor::new(&mut cpu, &mut bus).ld_hl_immediate(None);

        assert_eq!(cpu.registers.pc, pc.wrapping_add(1), "LD (HL), n");
        assert_eq!(bus.read_memory(address), value, "LD (HL), n");
    }

    #[test]
    fn load_a_indirect() {
        let mut cpu = Z80::new();
        let mut bus = InMemoryBus::new();

        for r16 in [Register16::BC, Register16::DE] {
            let address = rand::random();
            let value = rand::random();

            r16.write_to(address, &mut cpu.registers);
            bus.write_memory(address, value);

            cpu.registers.a = rand::random();

            InstructionExecutor::new(&mut cpu, &mut bus).ld_a_indirect(r16);

            assert_eq!(cpu.registers.a, value, "LD A, ({r16:?})");
            assert_eq!(bus.read_memory(address), value, "LD A, ({r16:?})");
        }
    }

    #[test]
    fn load_a_direct() {
        let mut cpu = Z80::new();
        let mut bus = InMemoryBus::new();

        let address: u16 = rand::random();
        let value = rand::random();
        let pc = rand::random();

        let [address_lsb, address_msb] = address.to_le_bytes();

        cpu.registers.pc = pc;
        bus.write_memory(pc, address_lsb);
        bus.write_memory(pc.wrapping_add(1), address_msb);
        bus.write_memory(address, value);

        cpu.registers.a = rand::random();

        InstructionExecutor::new(&mut cpu, &mut bus).ld_a_direct();

        assert_eq!(cpu.registers.pc, pc.wrapping_add(2), "LD A, (nn)");
        assert_eq!(cpu.registers.a, value, "LD A, (nn)");
    }

    #[test]
    fn load_indirect_a() {
        let mut cpu = Z80::new();
        let mut bus = InMemoryBus::new();

        for r16 in [Register16::BC, Register16::DE] {
            let address = rand::random();
            let value = rand::random();

            r16.write_to(address, &mut cpu.registers);
            cpu.registers.a = value;

            bus.write_memory(address, rand::random());

            InstructionExecutor::new(&mut cpu, &mut bus).ld_indirect_a(r16);

            assert_eq!(bus.read_memory(address), value, "LD ({r16:?}), A");
            assert_eq!(cpu.registers.a, value, "LD ({r16:?}), A");
        }
    }

    #[test]
    fn load_direct_a() {
        let mut cpu = Z80::new();
        let mut bus = InMemoryBus::new();

        let address: u16 = rand::random();
        let [address_lsb, address_msb] = address.to_le_bytes();
        let value = rand::random();
        let pc = rand::random();

        cpu.registers.pc = pc;
        cpu.registers.a = value;
        bus.write_memory(pc, address_lsb);
        bus.write_memory(pc.wrapping_add(1), address_msb);

        bus.write_memory(address, rand::random());

        InstructionExecutor::new(&mut cpu, &mut bus).ld_direct_a();

        assert_eq!(cpu.registers.pc, pc.wrapping_add(2), "LD (nn), A");
        assert_eq!(bus.read_memory(address), value, "LD (nn), A");
        assert_eq!(cpu.registers.a, value, "LD (nn), A");
    }
}
