mod arithmetic;
mod bits;
mod control;
mod io;
mod jump;
mod load;
mod mnemonics;

use crate::core::{IndexRegister, InterruptMode, Register8, Register16, Registers};
use crate::traits::{BusInterface, InterruptLine};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockMode {
    Increment,
    Decrement,
}

impl BlockMode {
    fn apply(self, value: u16) -> u16 {
        match self {
            Self::Increment => value.wrapping_add(1),
            Self::Decrement => value.wrapping_sub(1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InterruptType {
    Nmi,
    Int,
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

fn sign_flag(value: u8) -> bool {
    value.bit(7)
}

fn zero_flag(value: u8) -> bool {
    value == 0
}

fn parity_flag(value: u8) -> bool {
    value.count_ones() % 2 == 0
}

#[derive(Debug, Clone, Copy)]
struct ParseResult {
    opcode: u8,
    index_prefix: Option<IndexRegister>,
    index_fetch_t_cycles: u32,
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

    fn check_pending_interrupt(&self) -> Option<InterruptType> {
        if self.registers.interrupt_delay {
            None
        } else if self.bus.nmi() == InterruptLine::Low
            && self.registers.last_nmi == InterruptLine::High
        {
            Some(InterruptType::Nmi)
        } else if self.registers.iff1 && self.bus.int() == InterruptLine::Low {
            Some(InterruptType::Int)
        } else {
            None
        }
    }

    fn interrupt_service_routine(&mut self, interrupt_type: InterruptType) -> u32 {
        log::trace!("Executing interrupt service routine for interrupt type {interrupt_type:?}");

        self.registers.halted = false;

        match interrupt_type {
            InterruptType::Nmi => {
                self.push_stack(self.registers.pc);
                self.registers.pc = 0x0066;
                self.registers.iff1 = false;

                11
            }
            InterruptType::Int => {
                self.registers.iff1 = false;
                self.registers.iff2 = false;

                #[allow(unreachable_code)]
                match self.registers.interrupt_mode {
                    // Modes 0 and 1 don't actually work the same way in actual hardware, but for
                    // the purposes of emulating these consoles they do.
                    // Mode 1 (used exclusively by the overwhelming majority of games) is defined to
                    // always execute RST $38.
                    // Mode 0 reads an opcode from the data bus, but on these consoles, the Z80
                    // will always read $FF (RST $38) if it handles an interrupt while in mode 0.
                    // Some games depend on this mode 0 behavior, e.g. Blaster Master 2
                    InterruptMode::Mode0 | InterruptMode::Mode1 => {
                        self.push_stack(self.registers.pc);
                        self.registers.pc = 0x0038;

                        13
                    }
                    InterruptMode::Mode2 => {
                        log::error!("Interrupt mode 2 is not implemented; treating as mode 1");

                        self.push_stack(self.registers.pc);
                        self.registers.pc = 0x0038;

                        19
                    }
                }
            }
        }
    }

    fn execute_cb_prefix(&mut self, index: Option<IndexRegister>) -> u32 {
        // For DD+CB and FD+CB instructions, the index offset comes before the last opcode byte
        let index_with_offset = match index {
            Some(index) => {
                let offset = self.fetch_operand() as i8;
                Some((index, offset))
            }
            None => None,
        };

        let opcode2 = self.fetch_operand();

        log::trace!("CB prefix opcode: {opcode2:02X}");

        match opcode2 {
            0x00..=0x05 | 0x07 => self.rlc_r(opcode2, index_with_offset),
            0x06 => self.rlc_hl(index_with_offset),
            0x08..=0x0D | 0x0F => self.rrc_r(opcode2, index_with_offset),
            0x0E => self.rrc_hl(index_with_offset),
            0x10..=0x15 | 0x17 => self.rl_r(opcode2, index_with_offset),
            0x16 => self.rl_hl(index_with_offset),
            0x18..=0x1D | 0x1F => self.rr_r(opcode2, index_with_offset),
            0x1E => self.rr_hl(index_with_offset),
            0x20..=0x25 | 0x27 => self.sla_r(opcode2, index_with_offset),
            0x26 => self.sla_hl(index_with_offset),
            0x28..=0x2D | 0x2F => self.sra_r(opcode2, index_with_offset),
            0x2E => self.sra_hl(index_with_offset),
            0x30..=0x35 | 0x37 => self.sll_r(opcode2, index_with_offset),
            0x36 => self.sll_hl(index_with_offset),
            0x38..=0x3D | 0x3F => self.srl_r(opcode2, index_with_offset),
            0x3E => self.srl_hl(index_with_offset),
            0x40..=0x7F => {
                if opcode2 & 0x07 == 0x06 {
                    self.bit_b_hl(opcode2, index_with_offset)
                } else {
                    self.bit_b_r(opcode2)
                }
            }
            0x80..=0xBF => {
                if opcode2 & 0x07 == 0x06 {
                    self.res_b_hl(opcode2, index_with_offset)
                } else {
                    self.res_b_r(opcode2, index_with_offset)
                }
            }
            0xC0..=0xFF => {
                if opcode2 & 0x07 == 0x06 {
                    self.set_b_hl(opcode2, index_with_offset)
                } else {
                    self.set_b_r(opcode2, index_with_offset)
                }
            }
        }
    }

    fn execute_ed_prefix(&mut self) -> u32 {
        let opcode2 = self.fetch_operand();

        match opcode2 {
            0x40 | 0x48 | 0x50 | 0x58 | 0x60 | 0x68 | 0x70 | 0x78 => self.in_r_c(opcode2),
            0x41 | 0x49 | 0x51 | 0x59 | 0x61 | 0x69 | 0x71 | 0x79 => self.out_c_r(opcode2),
            0x42 | 0x52 | 0x62 | 0x72 => self.sbc_hl_ss(opcode2),
            0x43 | 0x53 | 0x63 | 0x73 => self.ld_direct_dd(opcode2),
            0x44 => self.neg(),
            0x45 | 0x4D => self.reti_retn(),
            0x46 => self.im(InterruptMode::Mode0),
            0x47 => self.ld_ir_a(Register8::I),
            0x4A | 0x5A | 0x6A | 0x7A => self.adc_hl_ss(opcode2),
            0x4B | 0x5B | 0x6B | 0x7B => self.ld_dd_direct(opcode2),
            0x4F => self.ld_ir_a(Register8::R),
            0x56 => self.im(InterruptMode::Mode1),
            0x57 => self.ld_a_ir(Register8::I),
            0x5E => self.im(InterruptMode::Mode2),
            0x5F => self.ld_a_ir(Register8::R),
            0x67 => self.rrd(),
            0x6F => self.rld(),
            0xA0 => self.block_transfer(BlockMode::Increment, false),
            0xA1 => self.compare_block(BlockMode::Increment, false),
            0xA2 => self.in_block(BlockMode::Increment, false),
            0xA3 => self.out_block(BlockMode::Increment, false),
            0xA8 => self.block_transfer(BlockMode::Decrement, false),
            0xA9 => self.compare_block(BlockMode::Decrement, false),
            0xAA => self.in_block(BlockMode::Decrement, false),
            0xAB => self.out_block(BlockMode::Decrement, false),
            0xB0 => self.block_transfer(BlockMode::Increment, true),
            0xB1 => self.compare_block(BlockMode::Increment, true),
            0xB2 => self.in_block(BlockMode::Increment, true),
            0xB3 => self.out_block(BlockMode::Increment, true),
            0xB8 => self.block_transfer(BlockMode::Decrement, true),
            0xB9 => self.compare_block(BlockMode::Decrement, true),
            0xBA => self.in_block(BlockMode::Decrement, true),
            0xBB => self.out_block(BlockMode::Decrement, true),
            _ => {
                // Treat unused opcodes as NOPs
                control::nop()
            }
        }
    }

    fn execute(mut self) -> u32 {
        self.registers.r = (self.registers.r.wrapping_add(1) & 0x7F) | (self.registers.r & 0x80);

        let interrupt_type = self.check_pending_interrupt();

        self.registers.interrupt_delay = false;
        self.registers.last_nmi = self.bus.nmi();

        if let Some(interrupt_type) = interrupt_type {
            return self.interrupt_service_routine(interrupt_type);
        }

        if self.registers.halted {
            return control::nop();
        }

        let ParseResult { opcode, index_prefix: index, index_fetch_t_cycles } = self.parse_opcode();

        log::trace!(
            "PC={:04X}, opcode={opcode:02X} ({}), index={index:?}, a={:02X}, next={:02X} {:02X}, sp={:04X}, a={:02X}, f={:02X}, b={:02X}, c={:02X}, d={:02X}, e={:02X}, h={:02X}, l={:02X}, ix={:04X}, iy={:04X}, iff1={}",
            self.registers.pc.wrapping_sub(1),
            mnemonics::for_opcode(opcode, self.bus.read_memory(self.registers.pc)),
            self.registers.a,
            self.bus.read_memory(self.registers.pc),
            self.bus.read_memory(self.registers.pc.wrapping_add(1)),
            self.registers.sp,
            self.registers.a,
            u8::from(self.registers.f),
            self.registers.b,
            self.registers.c,
            self.registers.d,
            self.registers.e,
            self.registers.h,
            self.registers.l,
            self.registers.ix,
            self.registers.iy,
            self.registers.iff1
        );

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
            0x07 => self.rlca(),
            0x08 => self.exchange_af(),
            0x09 | 0x19 | 0x29 | 0x39 => self.add_hl_ss(opcode, index),
            0x0A => self.ld_a_indirect(Register16::BC),
            0x0B | 0x1B | 0x2B | 0x3B => self.dec_ss(opcode, index),
            0x0F => self.rrca(),
            0x10 => self.djnz_e(),
            0x12 => self.ld_indirect_a(Register16::DE),
            0x17 => self.rla(),
            0x18 => self.jr_e(),
            0x1A => self.ld_a_indirect(Register16::DE),
            0x1F => self.rra(),
            0x20 => self.jr_nz_e(),
            0x22 => self.ld_direct_hl(index),
            0x27 => self.daa(),
            0x28 => self.jr_z_e(),
            0x2A => self.ld_hl_direct(index),
            0x2F => self.cpl(),
            0x30 => self.jr_nc_e(),
            0x32 => self.ld_direct_a(),
            0x37 => self.scf(),
            0x38 => self.jr_c_e(),
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
            0xC0 | 0xC8 | 0xD0 | 0xD8 | 0xE0 | 0xE8 | 0xF0 | 0xF8 => self.ret_cc(opcode),
            0xC1 | 0xD1 | 0xE1 | 0xF1 => self.pop_qq(opcode, index),
            0xC2 | 0xCA | 0xD2 | 0xDA | 0xE2 | 0xEA | 0xF2 | 0xFA => self.jp_cc_nn(opcode),
            0xC3 => self.jp_nn(),
            0xC4 | 0xCC | 0xD4 | 0xDC | 0xE4 | 0xEC | 0xF4 | 0xFC => self.call_cc_nn(opcode),
            0xC5 | 0xD5 | 0xE5 | 0xF5 => self.push_qq(opcode, index),
            0xC6 => self.add_a_immediate(false),
            0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => self.rst_p(opcode),
            0xC9 => self.ret(),
            0xCB => self.execute_cb_prefix(index),
            0xCD => self.call_nn(),
            0xCE => self.add_a_immediate(true),
            0xD3 => self.out_n_a(),
            0xD6 => self.sub_a_immediate(false),
            0xD9 => self.exchange_bcdehl(),
            0xDB => self.in_a_n(),
            0xDE => self.sub_a_immediate(true),
            0xE3 => self.exchange_stack_hl(index),
            0xE6 => self.and_a_immediate(),
            0xE9 => self.jp_hl(index),
            0xEB => self.exchange_de_hl(),
            0xED => self.execute_ed_prefix(),
            0xEE => self.xor_a_immediate(),
            0xF3 => self.di(),
            0xF6 => self.or_a_immediate(),
            0xF9 => self.ld_sp_hl(index),
            0xFB => self.ei(),
            0xFE => self.cp_a_immediate(),
            0xDD | 0xFD => unreachable!("DD/FD prefixes have already been removed"),
        };

        index_fetch_t_cycles + instruction_t_cycles
    }
}

pub fn execute<B: BusInterface>(registers: &mut Registers, bus: &mut B) -> u32 {
    InstructionExecutor::new(registers, bus).execute()
}
