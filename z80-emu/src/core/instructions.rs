mod arithmetic;
mod control;
mod load;

use crate::core::instructions::load::BlockMode;
use crate::core::{IndexRegister, InterruptMode, Register16, Register8, Registers};
use crate::traits::BusInterface;

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
            0x42 | 0x52 | 0x62 | 0x72 => self.sbc_hl_ss(opcode2),
            0x43 | 0x53 | 0x63 | 0x73 => self.ld_direct_dd(opcode2),
            0x44 => self.neg(),
            0x46 => self.im(InterruptMode::Mode0),
            0x47 => self.ld_ir_a(Register8::I),
            0x4A | 0x5A | 0x6A | 0x7A => self.adc_hl_ss(opcode2),
            0x4B | 0x5B | 0x6B | 0x7B => self.ld_dd_direct(opcode2),
            0x4F => self.ld_ir_a(Register8::R),
            0x56 => self.im(InterruptMode::Mode1),
            0x57 => self.ld_a_ir(Register8::I),
            0x5E => self.im(InterruptMode::Mode2),
            0x5F => self.ld_a_ir(Register8::R),
            0xA0 => self.block_transfer(BlockMode::Increment, false),
            0xA8 => self.block_transfer(BlockMode::Decrement, false),
            0xB0 => self.block_transfer(BlockMode::Increment, true),
            0xB8 => self.block_transfer(BlockMode::Decrement, true),
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
            0x00 => control::nop(),
            0x01 | 0x11 | 0x21 | 0x31 => self.ld_dd_immediate(opcode, index),
            0x02 => self.ld_indirect_a(Register16::BC),
            0x03 | 0x13 | 0x23 | 0x33 => self.inc_ss(opcode, index),
            0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x3C => self.inc_r(opcode, index),
            0x34 => self.inc_hl(index),
            0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x3D => self.dec_r(opcode, index),
            0x35 => self.dec_hl(index),
            0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x3E => self.ld_r_immediate(opcode, index),
            0x36 => self.ld_hl_immediate(index),
            0x08 => self.exchange_af(),
            0x09 | 0x19 | 0x29 | 0x39 => self.add_hl_ss(opcode, index),
            0x0A => self.ld_a_indirect(Register16::BC),
            0x0B | 0x1B | 0x2B | 0x3B => self.dec_ss(opcode, index),
            0x12 => self.ld_indirect_a(Register16::DE),
            0x1A => self.ld_a_indirect(Register16::DE),
            0x22 => self.ld_direct_hl(index),
            0x27 => self.daa(),
            0x2A => self.ld_hl_direct(index),
            0x2F => self.cpl(),
            0x32 => self.ld_direct_a(),
            0x37 => self.scf(),
            0x3A => self.ld_a_direct(),
            0x3F => self.ccf(),
            0x40 | 0x41 | 0x42 | 0x43 | 0x44 | 0x45 | 0x47 | 0x48 | 0x49 | 0x4A | 0x4B | 0x4C
            | 0x4D | 0x4F | 0x50 | 0x51 | 0x52 | 0x53 | 0x54 | 0x55 | 0x57 | 0x58 | 0x59 | 0x5A
            | 0x5B | 0x5C | 0x5D | 0x5F | 0x60 | 0x61 | 0x62 | 0x63 | 0x64 | 0x65 | 0x67 | 0x68
            | 0x69 | 0x6A | 0x6B | 0x6C | 0x6D | 0x6F | 0x78 | 0x79 | 0x7A | 0x7B | 0x7C | 0x7D
            | 0x7F => self.ld_r_r(opcode, index),
            0x46 | 0x4E | 0x56 | 0x5E | 0x66 | 0x6E | 0x7E => self.ld_r_hl(opcode, index),
            0x70 | 0x71 | 0x72 | 0x73 | 0x74 | 0x75 | 0x77 => self.ld_hl_r(opcode, index),
            0x76 => self.halt(),
            0x80 | 0x81 | 0x82 | 0x83 | 0x84 | 0x85 | 0x87 => self.add_a_r(opcode, index, false),
            0x86 => self.add_a_hl(index, false),
            0x88 | 0x89 | 0x8A | 0x8B | 0x8C | 0x8D | 0x8F => self.add_a_r(opcode, index, true),
            0x8E => self.add_a_hl(index, true),
            0x90 | 0x91 | 0x92 | 0x93 | 0x94 | 0x95 | 0x97 => self.sub_a_r(opcode, index, false),
            0x96 => self.sub_a_hl(index, false),
            0x98 | 0x99 | 0x9A | 0x9B | 0x9C | 0x9D | 0x9F => self.sub_a_r(opcode, index, true),
            0x9E => self.sub_a_hl(index, true),
            0xA0 | 0xA1 | 0xA2 | 0xA3 | 0xA4 | 0xA5 | 0xA7 => self.and_a_r(opcode, index),
            0xA6 => self.and_a_hl(index),
            0xA8 | 0xA9 | 0xAA | 0xAB | 0xAC | 0xAD | 0xAF => self.xor_a_r(opcode, index),
            0xAE => self.xor_a_hl(index),
            0xB0 | 0xB1 | 0xB2 | 0xB3 | 0xB4 | 0xB5 | 0xB7 => self.or_a_r(opcode, index),
            0xB6 => self.or_a_hl(index),
            0xB8 | 0xB9 | 0xBA | 0xBB | 0xBC | 0xBD | 0xBF => self.cp_a_r(opcode, index),
            0xBE => self.cp_a_hl(index),
            0xC1 | 0xD1 | 0xE1 | 0xF1 => self.pop_qq(opcode, index),
            0xC5 | 0xD5 | 0xE5 | 0xF5 => self.push_qq(opcode, index),
            0xC6 => self.add_a_immediate(false),
            0xCE => self.add_a_immediate(true),
            0xD6 => self.sub_a_immediate(false),
            0xD9 => self.exchange_bcdehl(),
            0xDE => self.sub_a_immediate(true),
            0xE3 => self.exchange_stack_hl(index),
            0xE6 => self.and_a_immediate(),
            0xEB => self.exchange_de_hl(),
            0xED => self.execute_ed_prefix(),
            0xEE => self.xor_a_immediate(),
            0xF3 => self.di(),
            0xF6 => self.or_a_immediate(),
            0xF9 => self.ld_sp_hl(index),
            0xFB => self.ei(),
            0xFE => self.cp_a_immediate(),
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
