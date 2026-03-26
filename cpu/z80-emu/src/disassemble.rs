use crate::Z80;
use std::borrow::Cow;
use std::cmp::Ordering;
use std::mem;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexRegister {
    Ix,
    Iy,
}

impl IndexRegister {
    fn label(self) -> &'static str {
        match self {
            Self::Ix => "ix",
            Self::Iy => "iy",
        }
    }

    fn indirect_with_offset(self, offset: i8) -> String {
        match offset.cmp(&0) {
            Ordering::Equal => match self {
                Self::Ix => "(ix)".into(),
                Self::Iy => "(iy)".into(),
            },
            Ordering::Greater => match self {
                Self::Ix => format!("(ix+{offset})"),
                Self::Iy => format!("(iy+{offset})"),
            },
            Ordering::Less => match self {
                Self::Ix => format!("(ix{offset})"),
                Self::Iy => format!("(iy{offset})"),
            },
        }
    }

    fn high_label(self) -> &'static str {
        match self {
            Self::Ix => "ixh",
            Self::Iy => "iyh",
        }
    }

    fn low_label(self) -> &'static str {
        match self {
            Self::Ix => "ixl",
            Self::Iy => "iyl",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IndexWithOffset(IndexRegister, i8);

impl IndexWithOffset {
    fn indirect_with_offset(self) -> String {
        self.0.indirect_with_offset(self.1)
    }
}

struct ByteReader<F> {
    pc: u16,
    reader: F,
    opcodes: Vec<u8>,
}

impl<F: FnMut() -> u8> ByteReader<F> {
    fn read_byte(&mut self) -> u8 {
        let opcode = (self.reader)();

        self.pc = self.pc.wrapping_add(1);
        self.opcodes.push(opcode);

        opcode
    }

    fn read_word(&mut self) -> u16 {
        let lsb = self.read_byte();
        let msb = self.read_byte();
        u16::from_le_bytes([lsb, msb])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndirectRegister {
    Bc,
    De,
    Hl,
    Sp,
    Ix { displacement: i8 },
    Iy { displacement: i8 },
}

impl IndirectRegister {
    fn from_index(index: IndexRegister, displacement: i8) -> Self {
        match index {
            IndexRegister::Ix => Self::Ix { displacement },
            IndexRegister::Iy => Self::Iy { displacement },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryAccess {
    Absolute(u16),
    Indirect(IndirectRegister),
}

impl MemoryAccess {
    #[must_use]
    pub fn resolve_address(self, cpu: &Z80) -> u16 {
        match self {
            Self::Absolute(address) => address,
            Self::Indirect(register) => {
                let registers = cpu.registers();
                match register {
                    IndirectRegister::Bc => u16::from_be_bytes([registers.b, registers.c]),
                    IndirectRegister::De => u16::from_be_bytes([registers.d, registers.e]),
                    IndirectRegister::Hl => u16::from_be_bytes([registers.h, registers.l]),
                    IndirectRegister::Sp => registers.sp,
                    IndirectRegister::Ix { displacement } => {
                        registers.ix.wrapping_add_signed(displacement.into())
                    }
                    IndirectRegister::Iy { displacement } => {
                        registers.iy.wrapping_add_signed(displacement.into())
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct DisassembledInstruction {
    pub opcodes: Vec<u8>,
    pub text: String,
    pub memory_access: Option<MemoryAccess>,
}

impl DisassembledInstruction {
    #[must_use]
    pub fn new() -> Self {
        Self { opcodes: Vec::new(), text: String::new(), memory_access: None }
    }

    fn clear(&mut self) {
        self.opcodes.clear();
        self.text.clear();
        self.memory_access = None;
    }

    #[must_use]
    fn parse_memory_access(&self, index: Option<IndexRegister>) -> Option<MemoryAccess> {
        let i = (0..self.opcodes.len())
            .find(|&i| self.opcodes[i] != 0xDD && self.opcodes[i] != 0xFD)?;

        let opcode = self.opcodes[i];
        match opcode {
            0x02 | 0x0A => {
                // ld (bc), a
                // ld a, (bc)
                Some(MemoryAccess::Indirect(IndirectRegister::Bc))
            }
            0x12 | 0x1A => {
                // ld (de), a
                // ld a, (de)
                Some(MemoryAccess::Indirect(IndirectRegister::De))
            }
            0x22 | 0x2A | 0x32 | 0x3A => {
                // ld (nn), hl
                // ld hl, (nn)
                // ld (nn), a
                // ld a, (nn)
                let lsb = self.opcodes.get(i + 1).copied()?;
                let msb = self.opcodes.get(i + 2).copied()?;
                Some(MemoryAccess::Absolute(u16::from_le_bytes([lsb, msb])))
            }
            0x34..=0x36 | 0x86 | 0x8E | 0x96 | 0x9E | 0xA6 | 0xAE | 0xB6 | 0xBE => {
                // inc (hl)
                // dec (hl)
                // ld (hl), nn
                // add a, (hl)
                // adc a, (hl)
                // sub (hl)
                // sbc a, (hl)
                // and (hl)
                // xor (hl)
                // or (hl)
                // cp (hl)
                let register = index.map_or(IndirectRegister::Hl, |index| {
                    let displacement = self.opcodes.get(i + 1).copied().unwrap_or(0) as i8;
                    IndirectRegister::from_index(index, displacement)
                });
                Some(MemoryAccess::Indirect(register))
            }
            0x40..=0x75 | 0x77..=0x7F => {
                // ld r, (hl)
                // ld (hl), r
                if opcode & 7 != 6 && (opcode >> 3) & 7 != 6 {
                    return None;
                }
                let register = index.map_or(IndirectRegister::Hl, |index| {
                    let displacement = self.opcodes.get(i + 1).copied().unwrap_or(0) as i8;
                    IndirectRegister::from_index(index, displacement)
                });
                Some(MemoryAccess::Indirect(register))
            }
            0xE3 => {
                // ex (sp), hl
                Some(MemoryAccess::Indirect(IndirectRegister::Sp))
            }
            0xCB => self.parse_memory_access_cb(i + 1, index),
            0xED => self.parse_memory_access_ed(i + 1),
            _ => None,
        }
    }

    fn parse_memory_access_cb(
        &self,
        i: usize,
        index: Option<IndexRegister>,
    ) -> Option<MemoryAccess> {
        if let Some(index) = index {
            // All index-prefixed CB-prefixed instructions perform the same memory access
            let displacement = self.opcodes.get(i).copied()? as i8;
            return Some(MemoryAccess::Indirect(IndirectRegister::from_index(index, displacement)));
        }

        let opcode = self.opcodes.get(i).copied()?;
        if opcode & 7 == 6 {
            return Some(MemoryAccess::Indirect(IndirectRegister::Hl));
        }

        None
    }

    fn parse_memory_access_ed(&self, i: usize) -> Option<MemoryAccess> {
        let opcode = self.opcodes.get(i).copied()?;

        match opcode {
            0x43 | 0x4B | 0x53 | 0x5B | 0x63 | 0x6B | 0x73 | 0x7B => {
                // ld (nn), rr
                // ld rr, (nn)
                let lsb = self.opcodes.get(i + 1).copied()?;
                let msb = self.opcodes.get(i + 2).copied()?;
                Some(MemoryAccess::Absolute(u16::from_le_bytes([lsb, msb])))
            }
            0xA0 | 0xA1 | 0xA8 | 0xA9 | 0xB0 | 0xB1 | 0xB8 | 0xB9 => {
                // ldi[r], cpi[r], ldd[r], cpd[r]
                Some(MemoryAccess::Indirect(IndirectRegister::Hl))
            }
            _ => None,
        }
    }
}

impl Default for DisassembledInstruction {
    fn default() -> Self {
        Self::new()
    }
}

pub fn disassemble_into(out: &mut DisassembledInstruction, pc: u16, reader: impl FnMut() -> u8) {
    out.clear();
    let mut reader = ByteReader { pc, reader, opcodes: mem::take(&mut out.opcodes) };

    let mut index: Option<IndexRegister> = None;

    let opcode = loop {
        match reader.read_byte() {
            0xDD => index = Some(IndexRegister::Ix),
            0xFD => index = Some(IndexRegister::Iy),
            opcode => break opcode,
        }
    };

    out.text = match opcode {
        0x00 => "nop".into(),
        0x01 | 0x11 | 0x21 | 0x31 => ld_dd_immediate(opcode, index, &mut reader),
        0x02 => "ld (bc), a".into(),
        0x03 | 0x13 | 0x23 | 0x33 => inc_ss(opcode, index),
        0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x34 | 0x3C => inc_r(opcode, index, &mut reader),
        0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x35 | 0x3D => dec_r(opcode, index, &mut reader),
        0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x36 | 0x3E => {
            ld_r_immediate(opcode, index, &mut reader)
        }
        0x07 => "rlca".into(),
        0x08 => "ex af, af'".into(),
        0x09 | 0x19 | 0x29 | 0x39 => add_hl_ss(opcode, index),
        0x0A => "ld a, (bc)".into(),
        0x0B | 0x1B | 0x2B | 0x3B => dec_ss(opcode, index),
        0x0F => "rrca".into(),
        0x10 => djnz(&mut reader),
        0x12 => "ld (de), a".into(),
        0x17 => "rla".into(),
        0x18 => jr(&mut reader),
        0x1A => "ld a, (de)".into(),
        0x1F => "rra".into(),
        0x20 | 0x28 | 0x30 | 0x38 => jr_cc(opcode, &mut reader),
        0x22 => ld_direct_hl(index, &mut reader),
        0x27 => "daa".into(),
        0x2A => ld_hl_direct(index, &mut reader),
        0x2F => "cpl".into(),
        0x32 => ld_direct_a(&mut reader),
        0x37 => "scf".into(),
        0x3A => ld_a_direct(&mut reader),
        0x3F => "ccf".into(),
        0x40..=0x75 | 0x77..=0x7F => ld_r_r(opcode, index, &mut reader),
        0x76 => "halt".into(),
        0x80..=0x87 => add_a_r(opcode, index, &mut reader),
        0x88..=0x8F => adc_a_r(opcode, index, &mut reader),
        0x90..=0x97 => sub_a_r(opcode, index, &mut reader),
        0x98..=0x9F => sbc_a_r(opcode, index, &mut reader),
        0xA0..=0xA7 => and_a_r(opcode, index, &mut reader),
        0xA8..=0xAF => xor_a_r(opcode, index, &mut reader),
        0xB0..=0xB7 => or_a_r(opcode, index, &mut reader),
        0xB8..=0xBF => cp_a_r(opcode, index, &mut reader),
        0xC0 | 0xC8 | 0xD0 | 0xD8 | 0xE0 | 0xE8 | 0xF0 | 0xF8 => ret_cc(opcode),
        0xC1 | 0xD1 | 0xE1 | 0xF1 => pop_qq(opcode, index),
        0xC2 | 0xCA | 0xD2 | 0xDA | 0xE2 | 0xEA | 0xF2 | 0xFA => jp_cc_nn(opcode, &mut reader),
        0xC3 => jp_nn(&mut reader),
        0xC4 | 0xCC | 0xD4 | 0xDC | 0xE4 | 0xEC | 0xF4 | 0xFC => call_cc_nn(opcode, &mut reader),
        0xC5 | 0xD5 | 0xE5 | 0xF5 => push_qq(opcode, index),
        0xC6 => add_a_immediate(&mut reader),
        0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => rst(opcode),
        0xC9 => "ret".into(),
        0xCD => call_nn(&mut reader),
        0xCE => adc_a_immediate(&mut reader),
        0xD3 => out_n_a(&mut reader),
        0xD6 => sub_a_immediate(&mut reader),
        0xD9 => "exx".into(),
        0xDB => in_a_n(&mut reader),
        0xDE => sbc_a_immediate(&mut reader),
        0xE3 => ex_stack_hl(index),
        0xE6 => and_a_immediate(&mut reader),
        0xE9 => jp_hl(index),
        0xEB => "ex de, hl".into(),
        0xEE => xor_a_immediate(&mut reader),
        0xF3 => "di".into(),
        0xF6 => or_a_immediate(&mut reader),
        0xF9 => ld_sp_hl(index),
        0xFB => "ei".into(),
        0xFE => cp_a_immediate(&mut reader),
        0xCB => cb_prefix(index, &mut reader),
        0xED => ed_prefix(&mut reader),
        0xDD | 0xFD => unreachable!("would still be in above index register loop"),
    };

    out.opcodes = mem::take(&mut reader.opcodes);
    out.memory_access = out.parse_memory_access(index);
}

fn cb_prefix(index: Option<IndexRegister>, reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    // For DD+CB and FD+CB instructions, the index offset comes before the last opcode byte
    let index = index.map(|index| {
        let offset = reader.read_byte() as i8;
        IndexWithOffset(index, offset)
    });

    let opcode = reader.read_byte();

    match opcode {
        0x00..=0x07 => shift_rotate_op("rlc", opcode, index),
        0x08..=0x0F => shift_rotate_op("rrc", opcode, index),
        0x10..=0x17 => shift_rotate_op("rl", opcode, index),
        0x18..=0x1F => shift_rotate_op("rr", opcode, index),
        0x20..=0x27 => shift_rotate_op("sla", opcode, index),
        0x28..=0x2F => shift_rotate_op("sra", opcode, index),
        0x30..=0x37 => shift_rotate_op("sll", opcode, index),
        0x38..=0x3F => shift_rotate_op("srl", opcode, index),
        0x40..=0x7F => bit_op("bit", opcode, index),
        0x80..=0xBF => bit_op("res", opcode, index),
        0xC0..=0xFF => bit_op("set", opcode, index),
    }
}

fn ed_prefix(reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let opcode = reader.read_byte();

    match opcode {
        0x40 | 0x48 | 0x50 | 0x58 | 0x60 | 0x68 | 0x70 | 0x78 => in_r_c(opcode),
        0x41 | 0x49 | 0x51 | 0x59 | 0x61 | 0x69 | 0x71 | 0x79 => out_c_r(opcode),
        0x42 | 0x52 | 0x62 | 0x72 => sbc_hl_ss(opcode),
        0x43 | 0x53 | 0x63 | 0x73 => ld_direct_dd(opcode, reader),
        0x44 => "neg".into(),
        0x45 => "retn".into(),
        0x46 => "im 0".into(),
        0x47 => "ld i, a".into(),
        0x4A | 0x5A | 0x6A | 0x7A => adc_hl_ss(opcode),
        0x4B | 0x5B | 0x6B | 0x7B => ld_dd_direct(opcode, reader),
        0x4F => "ld r, a".into(),
        0x56 => "im 1".into(),
        0x57 => "ld a, i".into(),
        0x5E => "im 2".into(),
        0x5F => "ld a, r".into(),
        0x67 => "rrd".into(),
        0x6F => "rld".into(),
        0xA0 => "ldi".into(),
        0xA1 => "cpi".into(),
        0xA2 => "ini".into(),
        0xA3 => "outi".into(),
        0xA8 => "ldd".into(),
        0xA9 => "cpd".into(),
        0xAA => "ind".into(),
        0xAB => "outd".into(),
        0xB0 => "ldir".into(),
        0xB1 => "cpir".into(),
        0xB2 => "inir".into(),
        0xB3 => "otir".into(),
        0xB8 => "lddr".into(),
        0xB9 => "cpdr".into(),
        0xBA => "indr".into(),
        0xBB => "otdr".into(),
        _ => "illegal".into(),
    }
}

fn r_register(
    opcode: u8,
    index: Option<IndexRegister>,
    reader: &mut ByteReader<impl FnMut() -> u8>,
) -> Cow<'static, str> {
    match opcode & 7 {
        0 => "b".into(),
        1 => "c".into(),
        2 => "d".into(),
        3 => "e".into(),
        4 => index.map_or("h", IndexRegister::high_label).into(),
        5 => index.map_or("l", IndexRegister::low_label).into(),
        6 => index
            .map_or("(hl)".into(), |index| {
                let offset = reader.read_byte() as i8;
                index.indirect_with_offset(offset)
            })
            .into(),
        7 => "a".into(),
        _ => unreachable!("value & 7 is always <= 7"),
    }
}

fn dd_register(opcode: u8, index: Option<IndexRegister>) -> &'static str {
    match (opcode >> 4) & 3 {
        0 => "bc",
        1 => "de",
        2 => index.map_or("hl", IndexRegister::label),
        3 => "sp",
        _ => unreachable!("value & 3 is always <= 3"),
    }
}

fn qq_register(opcode: u8, index: Option<IndexRegister>) -> &'static str {
    match (opcode >> 4) & 3 {
        0 => "bc",
        1 => "de",
        2 => index.map_or("hl", IndexRegister::label),
        3 => "af",
        _ => unreachable!("value & 3 is always <= 3"),
    }
}

fn ld_dd_immediate(
    opcode: u8,
    index: Option<IndexRegister>,
    reader: &mut ByteReader<impl FnMut() -> u8>,
) -> String {
    let register = dd_register(opcode, index);
    let immediate = reader.read_word();
    format!("ld {register}, #0x{immediate:04X}")
}

fn inc_ss(opcode: u8, index: Option<IndexRegister>) -> String {
    let register = dd_register(opcode, index);
    format!("inc {register}")
}

fn dec_ss(opcode: u8, index: Option<IndexRegister>) -> String {
    let register = dd_register(opcode, index);
    format!("dec {register}")
}

fn inc_r(
    opcode: u8,
    index: Option<IndexRegister>,
    reader: &mut ByteReader<impl FnMut() -> u8>,
) -> String {
    let register = r_register(opcode >> 3, index, reader);
    format!("inc {register}")
}

fn dec_r(
    opcode: u8,
    index: Option<IndexRegister>,
    reader: &mut ByteReader<impl FnMut() -> u8>,
) -> String {
    let register = r_register(opcode >> 3, index, reader);
    format!("dec {register}")
}

fn ld_r_immediate(
    opcode: u8,
    index: Option<IndexRegister>,
    reader: &mut ByteReader<impl FnMut() -> u8>,
) -> String {
    let register = r_register(opcode >> 3, index, reader);
    let immediate = reader.read_byte();
    format!("ld {register}, #0x{immediate:02X}")
}

fn add_hl_ss(opcode: u8, index: Option<IndexRegister>) -> String {
    let dest = index.map_or("hl", IndexRegister::label);
    let source = dd_register(opcode, index);
    format!("add {dest}, {source}")
}

fn djnz(reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let offset = reader.read_byte() as i8;
    let address = reader.pc.wrapping_add_signed(offset.into());
    format!("djnz ${address:04X}")
}

fn jr(reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let offset = reader.read_byte() as i8;
    let address = reader.pc.wrapping_add_signed(offset.into());
    format!("jr ${address:04X}")
}

fn jr_cc(opcode: u8, reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let offset = reader.read_byte() as i8;
    let address = reader.pc.wrapping_add_signed(offset.into());
    let condition = match (opcode >> 3) & 3 {
        0 => "nz",
        1 => "z",
        2 => "nc",
        3 => "c",
        _ => unreachable!("value & 3 is always <= 3"),
    };
    format!("jr {condition}, ${address:04X}")
}

fn ld_direct_hl(
    index: Option<IndexRegister>,
    reader: &mut ByteReader<impl FnMut() -> u8>,
) -> String {
    let address = reader.read_word();
    let register = index.map_or("hl", IndexRegister::label);
    format!("ld (${address:04X}), {register}")
}

fn ld_hl_direct(
    index: Option<IndexRegister>,
    reader: &mut ByteReader<impl FnMut() -> u8>,
) -> String {
    let address = reader.read_word();
    let register = index.map_or("hl", IndexRegister::label);
    format!("ld {register}, (${address:04X})")
}

fn ld_direct_a(reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let address = reader.read_word();
    format!("ld (${address:04X}), a")
}

fn ld_a_direct(reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let address = reader.read_word();
    format!("ld a, (${address:04X})")
}

fn ld_r_r(
    opcode: u8,
    index: Option<IndexRegister>,
    reader: &mut ByteReader<impl FnMut() -> u8>,
) -> String {
    let dest_index = if (opcode & 7) != 6 { index } else { None };
    let dest = r_register(opcode >> 3, dest_index, reader);

    let source_index = if ((opcode >> 3) & 7) != 6 { index } else { None };
    let source = r_register(opcode, source_index, reader);

    format!("ld {dest}, {source}")
}

fn add_a_r(
    opcode: u8,
    index: Option<IndexRegister>,
    reader: &mut ByteReader<impl FnMut() -> u8>,
) -> String {
    let source = r_register(opcode, index, reader);
    format!("add a, {source}")
}

fn adc_a_r(
    opcode: u8,
    index: Option<IndexRegister>,
    reader: &mut ByteReader<impl FnMut() -> u8>,
) -> String {
    let source = r_register(opcode, index, reader);
    format!("adc a, {source}")
}

fn sub_a_r(
    opcode: u8,
    index: Option<IndexRegister>,
    reader: &mut ByteReader<impl FnMut() -> u8>,
) -> String {
    let source = r_register(opcode, index, reader);
    format!("sub {source}")
}

fn sbc_a_r(
    opcode: u8,
    index: Option<IndexRegister>,
    reader: &mut ByteReader<impl FnMut() -> u8>,
) -> String {
    let source = r_register(opcode, index, reader);
    format!("sbc a, {source}")
}

fn and_a_r(
    opcode: u8,
    index: Option<IndexRegister>,
    reader: &mut ByteReader<impl FnMut() -> u8>,
) -> String {
    let source = r_register(opcode, index, reader);
    format!("and {source}")
}

fn xor_a_r(
    opcode: u8,
    index: Option<IndexRegister>,
    reader: &mut ByteReader<impl FnMut() -> u8>,
) -> String {
    let source = r_register(opcode, index, reader);
    format!("xor {source}")
}

fn or_a_r(
    opcode: u8,
    index: Option<IndexRegister>,
    reader: &mut ByteReader<impl FnMut() -> u8>,
) -> String {
    let source = r_register(opcode, index, reader);
    format!("or {source}")
}

fn cp_a_r(
    opcode: u8,
    index: Option<IndexRegister>,
    reader: &mut ByteReader<impl FnMut() -> u8>,
) -> String {
    let source = r_register(opcode, index, reader);
    format!("cp {source}")
}

fn jump_condition(opcode: u8) -> &'static str {
    match (opcode >> 3) & 7 {
        0 => "nz",
        1 => "z",
        2 => "nc",
        3 => "c",
        4 => "po",
        5 => "pe",
        6 => "p",
        7 => "m",
        _ => unreachable!("value & 7 is <= 7"),
    }
}

fn ret_cc(opcode: u8) -> String {
    let condition = jump_condition(opcode);
    format!("ret {condition}")
}

fn pop_qq(opcode: u8, index: Option<IndexRegister>) -> String {
    let register = qq_register(opcode, index);
    format!("pop {register}")
}

fn push_qq(opcode: u8, index: Option<IndexRegister>) -> String {
    let register = qq_register(opcode, index);
    format!("push {register}")
}

fn jp_nn(reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let address = reader.read_word();
    format!("jp ${address:04X}")
}

fn call_nn(reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let address = reader.read_word();
    format!("call ${address:04X}")
}

fn jp_cc_nn(opcode: u8, reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let condition = jump_condition(opcode);
    let address = reader.read_word();
    format!("jp {condition}, ${address:04X}")
}

fn call_cc_nn(opcode: u8, reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let condition = jump_condition(opcode);
    let address = reader.read_word();
    format!("call {condition}, ${address:04X}")
}

fn add_a_immediate(reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let operand = reader.read_byte();
    format!("add a, #0x{operand:02X}")
}

fn adc_a_immediate(reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let operand = reader.read_byte();
    format!("adc a, #0x{operand:02X}")
}

fn sub_a_immediate(reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let operand = reader.read_byte();
    format!("sub #0x{operand:02X}")
}

fn sbc_a_immediate(reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let operand = reader.read_byte();
    format!("sbc a, #0x{operand:02X}")
}

fn and_a_immediate(reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let operand = reader.read_byte();
    format!("and #0x{operand:02X}")
}

fn xor_a_immediate(reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let operand = reader.read_byte();
    format!("xor #0x{operand:02X}")
}

fn or_a_immediate(reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let operand = reader.read_byte();
    format!("or #0x{operand:02X}")
}

fn cp_a_immediate(reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let operand = reader.read_byte();
    format!("cp #0x{operand:02X}")
}

fn rst(opcode: u8) -> String {
    let address = opcode & 0x38;
    format!("rst ${address:02X}")
}

fn out_n_a(reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let address = reader.read_byte();
    format!("out (${address:02X}), a")
}

fn in_a_n(reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let address = reader.read_byte();
    format!("in a, (${address:02X})")
}

fn ex_stack_hl(index: Option<IndexRegister>) -> String {
    let source = index.map_or("hl", IndexRegister::label);
    format!("ex (sp), {source}")
}

fn jp_hl(index: Option<IndexRegister>) -> String {
    let register = index.map_or("hl", IndexRegister::label);
    format!("jp ({register})")
}

fn ld_sp_hl(index: Option<IndexRegister>) -> String {
    let register = index.map_or("hl", IndexRegister::label);
    format!("ld sp, {register}")
}

fn bit_register(opcode: u8) -> &'static str {
    match opcode & 7 {
        0 => "b",
        1 => "c",
        2 => "d",
        3 => "e",
        4 => "h",
        5 => "l",
        6 => "(hl)",
        7 => "a",
        _ => unreachable!("value & 7 is always <= 7"),
    }
}

fn shift_rotate_op(name: &str, opcode: u8, index: Option<IndexWithOffset>) -> String {
    let register =
        index.map_or_else(|| bit_register(opcode).into(), IndexWithOffset::indirect_with_offset);
    format!("{name} {register}")
}

fn bit_op(name: &str, opcode: u8, index: Option<IndexWithOffset>) -> String {
    let bit = (opcode >> 3) & 7;
    let register =
        index.map_or_else(|| bit_register(opcode).into(), IndexWithOffset::indirect_with_offset);
    format!("{name} {bit}, {register}")
}

fn r_register_no_hl(opcode: u8) -> Option<&'static str> {
    match (opcode >> 3) & 7 {
        0 => Some("b"),
        1 => Some("c"),
        2 => Some("d"),
        3 => Some("e"),
        4 => Some("h"),
        5 => Some("l"),
        6 => None,
        7 => Some("a"),
        _ => unreachable!("value & 7 is always <= 7"),
    }
}

fn in_r_c(opcode: u8) -> String {
    let Some(register) = r_register_no_hl(opcode) else { return "illegal".into() };
    format!("in {register}, (c)")
}

fn out_c_r(opcode: u8) -> String {
    let Some(register) = r_register_no_hl(opcode) else { return "illegal".into() };
    format!("out (c), {register}")
}

fn sbc_hl_ss(opcode: u8) -> String {
    let register = dd_register(opcode, None);
    format!("sbc hl, {register}")
}

fn adc_hl_ss(opcode: u8) -> String {
    let register = dd_register(opcode, None);
    format!("adc hl, {register}")
}

fn ld_direct_dd(opcode: u8, reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let address = reader.read_word();
    let register = dd_register(opcode, None);
    format!("ld (${address:04X}), {register}")
}

fn ld_dd_direct(opcode: u8, reader: &mut ByteReader<impl FnMut() -> u8>) -> String {
    let address = reader.read_word();
    let register = dd_register(opcode, None);
    format!("ld {register}, (${address:04X})")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iter_reader(iterable: impl IntoIterator<Item = u8>) -> impl FnMut() -> u8 {
        let mut iter = iterable.into_iter();
        move || iter.next().expect("ran out of opcodes")
    }

    #[test]
    fn ld_r_r_index() {
        let mut out = DisassembledInstruction::new();

        disassemble_into(&mut out, 0, iter_reader([0x66]));
        assert_eq!(out.text.as_str(), "ld h, (hl)");

        disassemble_into(&mut out, 0, iter_reader([0xDD, 0x66, 0x05]));
        assert_eq!(out.text.as_str(), "ld h, (ix+5)");

        disassemble_into(&mut out, 0, iter_reader([0x75]));
        assert_eq!(out.text.as_str(), "ld (hl), l");

        disassemble_into(&mut out, 0, iter_reader([0xFD, 0x75, 0xFD]));
        assert_eq!(out.text.as_str(), "ld (iy-3), l");

        disassemble_into(&mut out, 0, iter_reader([0x76]));
        assert_eq!(out.text.as_str(), "halt");

        disassemble_into(&mut out, 0, iter_reader([0xDD, 0x76]));
        assert_eq!(out.text.as_str(), "halt");
    }

    #[test]
    fn other_register_operands() {
        let mut out = DisassembledInstruction::new();

        disassemble_into(&mut out, 0, iter_reader([0x0C]));
        assert_eq!(out.text.as_str(), "inc c");

        disassemble_into(&mut out, 0, iter_reader([0xA2]));
        assert_eq!(out.text.as_str(), "and d");

        disassemble_into(&mut out, 0, iter_reader([0x2D]));
        assert_eq!(out.text.as_str(), "dec l");

        disassemble_into(&mut out, 0, iter_reader([0xFD, 0x36, 0x00, 0x34]));
        assert_eq!(out.text.as_str(), "ld (iy), #0x34");
    }
}
