mod load;

use crate::core::{IndexRegister, Register16, Register8, Registers};
use crate::traits::BusInterface;

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

fn parse_register_from_opcode(opcode: u8, index: Option<IndexRegister>) -> Option<Register8> {
    match opcode & 0x07 {
        0x00 => Some(Register8::B),
        0x01 => Some(Register8::C),
        0x02 => Some(Register8::D),
        0x03 => Some(Register8::E),
        0x04 => Some(index.map_or(Register8::H, IndexRegister::high_byte)),
        0x05 => Some(index.map_or(Register8::L, IndexRegister::low_byte)),
        0x06 => None,
        0x07 => Some(Register8::A),
        _ => unreachable!("value & 0x07 is always <= 0x07"),
    }
}

fn parse_dd_register(opcode: u8, index: Option<IndexRegister>) -> Register16 {
    match opcode & 0x30 {
        0x00 => Register16::BC,
        0x10 => Register16::DE,
        0x20 => index.map_or(Register16::HL, IndexRegister::into),
        0x30 => Register16::SP,
        _ => unreachable!("value & 0x30 is always 0x00/0x10/0x20/0x30"),
    }
}

fn parse_qq_register(opcode: u8, index: Option<IndexRegister>) -> Register16 {
    match opcode & 0x30 {
        0x00 => Register16::BC,
        0x10 => Register16::DE,
        0x20 => index.map_or(Register16::HL, IndexRegister::into),
        0x30 => Register16::AF,
        _ => unreachable!("value & 0x30 is always 0x00/0x10/0x20/0x30"),
    }
}

#[derive(Debug, Clone, Copy)]
struct ParseResult {
    opcode: u8,
    index_prefix: Option<IndexRegister>,
    index_fetch_t_cycles: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct ExecuteResult {
    pub t_cycles: u32,
}

struct InstructionExecutor<'registers, 'bus, B> {
    registers: &'registers mut Registers,
    bus: &'bus mut B,
}

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    fn new(registers: &'registers mut Registers, bus: &'bus mut B) -> Self {
        Self { registers, bus }
    }

    fn fetch_operand(&mut self) -> u8 {
        let operand = self.bus.read_memory(self.registers.pc);
        self.registers.pc = self.registers.pc.wrapping_add(1);
        operand
    }

    fn fetch_operand_u16(&mut self) -> u16 {
        let lsb = self.fetch_operand();
        let msb = self.fetch_operand();
        u16::from_le_bytes([lsb, msb])
    }

    fn parse_opcode(&mut self) -> ParseResult {
        let mut index = None;
        let mut t_cycles = 0;
        loop {
            let opcode = self.fetch_operand();
            match opcode {
                0xDD => {
                    index = Some(IndexRegister::IX);
                    t_cycles += 4;
                }
                0xFD => {
                    index = Some(IndexRegister::IY);
                    t_cycles += 4;
                }
                _ => {
                    return ParseResult {
                        opcode,
                        index_prefix: index,
                        index_fetch_t_cycles: t_cycles,
                    };
                }
            }
        }
    }

    // Replaces (HL) with (IX+d) or (IY+d) if index register is Some
    fn fetch_indirect_hl_address(&mut self, index: Option<IndexRegister>) -> u16 {
        match index {
            Some(index) => {
                let index = index.read_from(self.registers);
                let offset = self.fetch_operand() as i8;
                (i32::from(index) + i32::from(offset)) as u16
            }
            None => Register16::HL.read_from(self.registers),
        }
    }

    fn read_memory_u16(&mut self, address: u16) -> u16 {
        let lsb = self.bus.read_memory(address);
        let msb = self.bus.read_memory(address.wrapping_add(1));
        u16::from_le_bytes([lsb, msb])
    }

    fn write_memory_u16(&mut self, address: u16, value: u16) {
        let [lsb, msb] = value.to_le_bytes();
        self.bus.write_memory(address, lsb);
        self.bus.write_memory(address.wrapping_add(1), msb);
    }

    fn push_stack(&mut self, value: u16) {
        let [lsb, msb] = value.to_le_bytes();

        self.registers.sp = self.registers.sp.wrapping_sub(1);
        self.bus.write_memory(self.registers.sp, msb);
        self.registers.sp = self.registers.sp.wrapping_sub(1);
        self.bus.write_memory(self.registers.sp, lsb);
    }

    fn pop_stack(&mut self) -> u16 {
        let lsb = self.bus.read_memory(self.registers.sp);
        self.registers.sp = self.registers.sp.wrapping_add(1);
        let msb = self.bus.read_memory(self.registers.sp);
        self.registers.sp = self.registers.sp.wrapping_add(1);

        u16::from_le_bytes([lsb, msb])
    }

    fn execute_ed_prefix(&mut self) -> u32 {
        let opcode2 = self.fetch_operand();

        match opcode2 {
            0x43 | 0x53 | 0x63 | 0x73 => self.ld_direct_dd(opcode2),
            0x47 => self.ld_ir_a(Register8::I),
            0x4B | 0x5B | 0x6B | 0x7B => self.ld_dd_direct(opcode2),
            0x4F => self.ld_ir_a(Register8::R),
            0x57 => self.ld_a_ir(Register8::I),
            0x5F => self.ld_a_ir(Register8::R),
            _ => todo!(),
        }
    }

    fn execute(mut self) -> ExecuteResult {
        let ParseResult {
            opcode,
            index_prefix: index,
            index_fetch_t_cycles,
        } = self.parse_opcode();

        let instruction_t_cycles = match opcode {
            0x01 | 0x11 | 0x21 | 0x31 => self.ld_dd_immediate(opcode, index),
            0x02 => self.ld_indirect_a(Register16::BC),
            0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x3E => self.ld_r_immediate(opcode, index),
            0x0A => self.ld_a_indirect(Register16::BC),
            0x12 => self.ld_indirect_a(Register16::DE),
            0x1A => self.ld_a_indirect(Register16::DE),
            0x22 => self.ld_direct_hl(index),
            0x2A => self.ld_hl_direct(index),
            0x32 => self.ld_direct_a(),
            0x36 => self.ld_hl_immediate(index),
            0x3A => self.ld_a_direct(),
            0x40 | 0x41 | 0x42 | 0x43 | 0x44 | 0x45 | 0x47 | 0x48 | 0x49 | 0x4A | 0x4B | 0x4C
            | 0x4D | 0x4F | 0x50 | 0x51 | 0x52 | 0x53 | 0x54 | 0x55 | 0x57 | 0x58 | 0x59 | 0x5A
            | 0x5B | 0x5C | 0x5D | 0x5F | 0x60 | 0x61 | 0x62 | 0x63 | 0x64 | 0x65 | 0x67 | 0x68
            | 0x69 | 0x6A | 0x6B | 0x6C | 0x6D | 0x6F | 0x78 | 0x79 | 0x7A | 0x7B | 0x7C | 0x7D
            | 0x7F => self.ld_r_r(opcode, index),
            0x46 | 0x4E | 0x56 | 0x5E | 0x66 | 0x6E | 0x7E => self.ld_r_hl(opcode, index),
            0x70 | 0x71 | 0x72 | 0x73 | 0x74 | 0x75 | 0x77 => self.ld_hl_r(opcode, index),
            0xC1 | 0xD1 | 0xE1 | 0xF1 => self.pop_qq(opcode, index),
            0xC5 | 0xD5 | 0xE5 | 0xF5 => self.push_qq(opcode, index),
            0xED => self.execute_ed_prefix(),
            0xF9 => self.ld_sp_hl(index),
            0xDD | 0xFD => unreachable!("DD/FD prefixes have already been removed"),
            _ => todo!(),
        };

        ExecuteResult {
            t_cycles: index_fetch_t_cycles + instruction_t_cycles,
        }
    }
}

pub fn execute<B: BusInterface>(registers: &mut Registers, bus: &mut B) -> ExecuteResult {
    InstructionExecutor::new(registers, bus).execute()
}
