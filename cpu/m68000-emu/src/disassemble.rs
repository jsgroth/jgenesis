use crate::M68000;
use crate::core::instructions::{
    BranchCondition, Direction, Instruction, ShiftCount, ShiftDirection, UspDirection,
};
use crate::core::{
    AddressRegister, AddressingMode, DataRegister, IndexRegister, IndexSize, OpSize,
};
use jgenesis_common::num::GetBit;
use std::borrow::Cow;
use std::mem;

const DATA_REGISTERS: &[&str; 8] = &["d0", "d1", "d2", "d3", "d4", "d5", "d6", "d7"];
const ADDRESS_REGISTERS: &[&str; 8] = &["a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7"];
const ADDRESS_INDIRECT: &[&str; 8] =
    &["(a0)", "(a1)", "(a2)", "(a3)", "(a4)", "(a5)", "(a6)", "(a7)"];
const ADDRESS_POSTINC: &[&str; 8] =
    &["(a0)+", "(a1)+", "(a2)+", "(a3)+", "(a4)+", "(a5)+", "(a6)+", "(a7)+"];
const ADDRESS_PREDEC: &[&str; 8] =
    &["-(a0)", "-(a1)", "-(a2)", "-(a3)", "-(a4)", "-(a5)", "-(a6)", "-(a7)"];

fn size_str(size: OpSize) -> &'static str {
    match size {
        OpSize::Byte => "b",
        OpSize::Word => "w",
        OpSize::LongWord => "l",
    }
}

fn condition_suffix(condition: BranchCondition) -> &'static str {
    match condition {
        BranchCondition::True => "t",
        BranchCondition::False => "f",
        BranchCondition::Higher => "hi",
        BranchCondition::LowerOrSame => "ls",
        BranchCondition::CarryClear => "cc",
        BranchCondition::CarrySet => "cs",
        BranchCondition::NotEqual => "ne",
        BranchCondition::Equal => "eq",
        BranchCondition::OverflowClear => "vc",
        BranchCondition::OverflowSet => "vs",
        BranchCondition::Plus => "pl",
        BranchCondition::Minus => "mi",
        BranchCondition::GreaterOrEqual => "ge",
        BranchCondition::LessThan => "lt",
        BranchCondition::GreaterThan => "gt",
        BranchCondition::LessOrEqual => "le",
    }
}

fn direction_str(direction: ShiftDirection) -> &'static str {
    match direction {
        ShiftDirection::Left => "l",
        ShiftDirection::Right => "r",
    }
}

fn shift_count_str(count: ShiftCount) -> Cow<'static, str> {
    match count {
        ShiftCount::Constant(value) => format!("#{value}").into(),
        ShiftCount::Register(r) => DATA_REGISTERS[r.0 as usize].into(),
    }
}

#[derive(Debug, Clone, Copy)]
pub enum DisassembledIndex {
    Data(u8),
    Address(u8),
}

impl DisassembledIndex {
    #[must_use]
    pub fn resolve(self, cpu: &M68000, size: IndexSize) -> u32 {
        let register_value = match self {
            Self::Data(r) => cpu.data_registers()[r as usize],
            Self::Address(7) => cpu.stack_pointer(),
            Self::Address(r) => cpu.address_registers()[r as usize],
        };

        match size {
            IndexSize::SignExtendedWord => register_value as i16 as u32,
            IndexSize::LongWord => register_value,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum DisassembledMemoryAccess {
    Absolute {
        address: u32,
    },
    AddressIndirect {
        register: u8,
        displacement: i32,
        index: Option<(DisassembledIndex, IndexSize)>,
    },
    PcRelative {
        pc: u32,
        displacement: i32,
        index: Option<(DisassembledIndex, IndexSize)>,
    },
}

impl DisassembledMemoryAccess {
    #[must_use]
    pub fn resolve(self, cpu: &M68000) -> u32 {
        match self {
            Self::Absolute { address } => address,
            Self::AddressIndirect { register, displacement, index } => {
                let mut address = address_register(cpu, register).wrapping_add_signed(displacement);
                if let Some((index, size)) = index {
                    address = address.wrapping_add(index.resolve(cpu, size));
                }
                address
            }
            Self::PcRelative { pc, displacement, index } => {
                let mut address = pc.wrapping_add_signed(displacement);
                if let Some((index, size)) = index {
                    address = address.wrapping_add(index.resolve(cpu, size));
                }
                address
            }
        }
    }
}

fn address_register(cpu: &M68000, register: u8) -> u32 {
    match register {
        7 => cpu.stack_pointer(),
        _ => cpu.address_registers()[register as usize],
    }
}

#[derive(Debug, Clone)]
pub struct DisassembledInstruction {
    pub opcodes: Vec<u16>,
    pub text: String,
    pub next_opcode_addr: u32,
    pub memory_read: Option<DisassembledMemoryAccess>,
}

impl DisassembledInstruction {
    #[must_use]
    pub fn new() -> Self {
        Self { opcodes: Vec::new(), text: String::new(), next_opcode_addr: 0, memory_read: None }
    }

    fn clear(&mut self) {
        self.opcodes.clear();
        self.text.clear();
        self.memory_read = None;
    }
}

impl Default for DisassembledInstruction {
    fn default() -> Self {
        Self::new()
    }
}

struct WordReader<F> {
    pc: u32,
    read: F,
    opcodes: Vec<u16>,
}

impl<F: FnMut() -> u16> WordReader<F> {
    fn read_word(&mut self) -> u16 {
        self.pc = (self.pc + 2) & 0xFFFFFF;

        let word = (self.read)();
        self.opcodes.push(word);
        word
    }
}

struct Disassembler<'a, F> {
    reader: WordReader<F>,
    out: &'a mut DisassembledInstruction,
}

pub fn disassemble_into(
    out: &mut DisassembledInstruction,
    pc: u32,
    read_word: impl FnMut() -> u16,
) {
    out.clear();
    Disassembler::new(pc, read_word, out).disassemble();
}

impl<'a, F> Disassembler<'a, F>
where
    F: FnMut() -> u16,
{
    fn new(pc: u32, read_word: F, out: &'a mut DisassembledInstruction) -> Self {
        let reader = WordReader { pc, read: read_word, opcodes: mem::take(&mut out.opcodes) };
        Self { reader, out }
    }

    fn read_word(&mut self) -> u16 {
        self.reader.read_word()
    }

    fn disassemble(mut self) {
        let instruction = Instruction::decode_from(self.read_word());

        self.out.text = match instruction {
            Instruction::Add { size, source, dest, with_extend } => {
                self.add(size, source, dest, with_extend)
            }
            Instruction::AddDecimal { source, dest } => self.abcd(source, dest),
            Instruction::And { size, source, dest } => self.and(size, source, dest),
            Instruction::AndToCcr => self.andi_to_ccr(),
            Instruction::AndToSr => self.andi_to_sr(),
            Instruction::ArithmeticShiftMemory(direction, dest) => {
                self.asl_asr_memory(direction, dest)
            }
            Instruction::ArithmeticShiftRegister(size, direction, register, count) => {
                Self::asl_asr_register(size, direction, register, count)
            }
            Instruction::BitTest { source, dest } => self.btst(source, dest),
            Instruction::BitTestAndChange { source, dest } => self.bchg(source, dest),
            Instruction::BitTestAndClear { source, dest } => self.bclr(source, dest),
            Instruction::BitTestAndSet { source, dest } => self.bset(source, dest),
            Instruction::Branch(condition, displacement) => self.branch(condition, displacement),
            Instruction::BranchDecrement(condition, register) => self.dbcc(condition, register),
            Instruction::BranchToSubroutine(displacement) => self.bsr(displacement),
            Instruction::CheckRegister(register, source) => self.chk(register, source),
            Instruction::Clear(size, dest) => self.clr(size, dest),
            Instruction::Compare { size, source, dest } => self.cmp(size, source, dest),
            Instruction::DivideSigned(dest, source) => self.divs(dest, source),
            Instruction::DivideUnsigned(dest, source) => self.divu(dest, source),
            Instruction::ExchangeAddress(a0, a1) => Self::exg_address(a0, a1),
            Instruction::ExchangeData(d0, d1) => Self::exg_data(d0, d1),
            Instruction::ExchangeDataAddress(d, a) => Self::exg_data_address(d, a),
            Instruction::ExclusiveOr { size, source, dest } => self.eor(size, source, dest),
            Instruction::ExclusiveOrToCcr => self.eori_to_ccr(),
            Instruction::ExclusiveOrToSr => self.eori_to_sr(),
            Instruction::Extend(size, dest) => Self::ext(size, dest),
            Instruction::Illegal { opcode } => {
                format!("illegal ;0x{opcode:04X}")
            }
            Instruction::Jump(dest) => self.jmp(dest),
            Instruction::JumpToSubroutine(dest) => self.jsr(dest),
            Instruction::Link(register) => self.link(register),
            Instruction::LoadEffectiveAddress(source, dest) => self.lea(source, dest),
            Instruction::LogicalShiftMemory(direction, dest) => {
                self.lsl_lsr_memory(direction, dest)
            }
            Instruction::LogicalShiftRegister(size, direction, register, count) => {
                Self::lsl_lsr_register(size, direction, register, count)
            }
            Instruction::Move { size, source, dest } => self.move_(size, source, dest),
            Instruction::MoveFromSr(dest) => self.move_from_sr(dest),
            Instruction::MoveMultiple(size, am, direction) => self.movem(size, am, direction),
            Instruction::MovePeripheral(size, data, address, direction) => {
                self.movep(size, data, address, direction)
            }
            Instruction::MoveQuick(value, register) => Self::moveq(value, register),
            Instruction::MoveToCcr(source) => self.move_to_ccr(source),
            Instruction::MoveToSr(source) => self.move_to_sr(source),
            Instruction::MoveUsp(direction, register) => Self::move_usp(direction, register),
            Instruction::MultiplySigned(dest, source) => self.muls(dest, source),
            Instruction::MultiplyUnsigned(dest, source) => self.mulu(dest, source),
            Instruction::Negate { size, dest, with_extend } => self.neg(size, dest, with_extend),
            Instruction::NegateDecimal(dest) => self.nbcd(dest),
            Instruction::NoOp => "nop".into(),
            Instruction::Not(size, dest) => self.not(size, dest),
            Instruction::Or { size, source, dest } => self.or(size, source, dest),
            Instruction::OrToCcr => self.ori_to_ccr(),
            Instruction::OrToSr => self.ori_to_sr(),
            Instruction::PushEffectiveAddress(source) => self.pea(source),
            Instruction::Reset => "reset".into(),
            Instruction::Return { restore_ccr } => {
                if restore_ccr {
                    "rtr".into()
                } else {
                    "rts".into()
                }
            }
            Instruction::ReturnFromException => "rte".into(),
            Instruction::RotateMemory(direction, dest) => self.rol_ror_memory(direction, dest),
            Instruction::RotateRegister(size, direction, register, count) => {
                Self::rol_ror_register(size, direction, register, count)
            }
            Instruction::RotateThruExtendMemory(direction, dest) => {
                self.roxl_roxr_memory(direction, dest)
            }
            Instruction::RotateThruExtendRegister(size, direction, register, count) => {
                Self::roxl_roxr_register(size, direction, register, count)
            }
            Instruction::Set(condition, dest) => self.scc(condition, dest),
            Instruction::Subtract { size, source, dest, with_extend } => {
                self.sub(size, source, dest, with_extend)
            }
            Instruction::SubtractDecimal { source, dest } => self.sbcd(source, dest),
            Instruction::Swap(register) => Self::swap(register),
            Instruction::Stop => "stop".into(),
            Instruction::Test(size, source) => self.tst(size, source),
            Instruction::TestAndSet(dest) => self.tas(dest),
            Instruction::Trap(vector) => Self::trap(vector),
            Instruction::TrapOnOverflow => "trapv".into(),
            Instruction::Unlink(register) => Self::unlk(register),
        };

        self.out.opcodes = mem::take(&mut self.reader.opcodes);
        self.out.next_opcode_addr = self.reader.pc;
    }

    fn binary_op(
        &mut self,
        name: &str,
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
    ) -> String {
        let source_str = self.operand_string(source, size);
        let dest_str = self.operand_string(dest, size);
        format!("{name}.{} {source_str}, {dest_str}", size_str(size))
    }

    fn add(
        &mut self,
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
        with_extend: bool,
    ) -> String {
        let name = if with_extend {
            "addx"
        } else {
            match (source, dest) {
                (AddressingMode::Quick(_), _) => "addq",
                (_, AddressingMode::AddressDirect(_)) => "adda",
                (AddressingMode::Immediate, _) => "addi",
                _ => "add",
            }
        };

        self.binary_op(name, size, source, dest)
    }

    fn abcd(&mut self, source: AddressingMode, dest: AddressingMode) -> String {
        let source_str = self.operand_string(source, OpSize::Byte);
        let dest_str = self.operand_string(dest, OpSize::Byte);
        format!("abcd {source_str}, {dest_str}")
    }

    fn and(&mut self, size: OpSize, source: AddressingMode, dest: AddressingMode) -> String {
        let name = match source {
            AddressingMode::Immediate => "andi",
            _ => "and",
        };

        self.binary_op(name, size, source, dest)
    }

    fn andi_to_ccr(&mut self) -> String {
        let source_str = self.operand_string(AddressingMode::Immediate, OpSize::Byte);
        format!("andi {source_str}, ccr")
    }

    fn andi_to_sr(&mut self) -> String {
        let source_str = self.operand_string(AddressingMode::Immediate, OpSize::Word);
        format!("andi {source_str}, sr")
    }

    fn asl_asr_memory(&mut self, direction: ShiftDirection, dest: AddressingMode) -> String {
        let dest_str = self.operand_string(dest, OpSize::Word);
        format!("as{} {dest_str}", direction_str(direction))
    }

    fn asl_asr_register(
        size: OpSize,
        direction: ShiftDirection,
        register: DataRegister,
        count: ShiftCount,
    ) -> String {
        format!(
            "as{}.{} {}, {}",
            direction_str(direction),
            size_str(size),
            shift_count_str(count),
            DATA_REGISTERS[register.0 as usize]
        )
    }

    fn bit_op(&mut self, name: &str, source: AddressingMode, dest: AddressingMode) -> String {
        let source_str = match source {
            AddressingMode::Immediate => {
                let bit = self.read_word() & 0xFF;
                format!("#{bit}").into()
            }
            _ => self.operand_string(source, OpSize::Byte),
        };
        let dest_str = self.operand_string(dest, OpSize::Byte);
        format!("{name} {source_str}, {dest_str}")
    }

    fn btst(&mut self, source: AddressingMode, dest: AddressingMode) -> String {
        self.bit_op("btst", source, dest)
    }

    fn bchg(&mut self, source: AddressingMode, dest: AddressingMode) -> String {
        self.bit_op("bchg", source, dest)
    }

    fn bclr(&mut self, source: AddressingMode, dest: AddressingMode) -> String {
        self.bit_op("bclr", source, dest)
    }

    fn bset(&mut self, source: AddressingMode, dest: AddressingMode) -> String {
        self.bit_op("bset", source, dest)
    }

    fn branch(&mut self, condition: BranchCondition, displacement: i8) -> String {
        let pc = self.reader.pc;
        let effective_displacement = match displacement {
            0 => self.read_word() as i16,
            _ => displacement.into(),
        };

        let name = match condition {
            BranchCondition::True => "bra".into(),
            BranchCondition::False => "braf".into(),
            _ => format!("b{}", condition_suffix(condition)),
        };

        format!("{name} ${:06X}", pc.wrapping_add_signed(effective_displacement.into()))
    }

    fn dbcc(&mut self, condition: BranchCondition, register: DataRegister) -> String {
        let pc = self.reader.pc;
        let displacement = self.read_word() as i16;
        format!(
            "db{} {}, ${:06X}",
            condition_suffix(condition),
            DATA_REGISTERS[register.0 as usize],
            pc.wrapping_add_signed(displacement.into())
        )
    }

    fn bsr(&mut self, displacement: i8) -> String {
        let pc = self.reader.pc;
        let effective_displacement = match displacement {
            0 => self.read_word() as i16,
            _ => displacement.into(),
        };

        format!("bsr ${:06X}", pc.wrapping_add_signed(effective_displacement.into()))
    }

    fn chk(&mut self, register: DataRegister, source: AddressingMode) -> String {
        let source_str = self.operand_string(source, OpSize::Word);
        format!("chk {source_str}, {}", DATA_REGISTERS[register.0 as usize])
    }

    fn clr(&mut self, size: OpSize, dest: AddressingMode) -> String {
        let dest_str = self.operand_string(dest, size);
        format!("clr.{} {dest_str}", size_str(size))
    }

    fn cmp(&mut self, size: OpSize, source: AddressingMode, dest: AddressingMode) -> String {
        let name = match (source, dest) {
            (_, AddressingMode::AddressDirect(_)) => "cmpa",
            (AddressingMode::Immediate, _) => "cmpi",
            (
                AddressingMode::AddressIndirectPostincrement(_),
                AddressingMode::AddressIndirectPostincrement(_),
            ) => "cmpm",
            _ => "cmp",
        };

        self.binary_op(name, size, source, dest)
    }

    fn muldiv_op(&mut self, name: &str, dest: DataRegister, source: AddressingMode) -> String {
        let source = self.operand_string(source, OpSize::Word);
        format!("{name} {source}, {}", DATA_REGISTERS[dest.0 as usize])
    }

    fn divs(&mut self, dest: DataRegister, source: AddressingMode) -> String {
        self.muldiv_op("divs", dest, source)
    }

    fn divu(&mut self, dest: DataRegister, source: AddressingMode) -> String {
        self.muldiv_op("divu", dest, source)
    }

    fn exg_address(a0: AddressRegister, a1: AddressRegister) -> String {
        format!("exg {}, {}", ADDRESS_REGISTERS[a0.0 as usize], ADDRESS_REGISTERS[a1.0 as usize])
    }

    fn exg_data(d0: DataRegister, d1: DataRegister) -> String {
        format!("exg {}, {}", DATA_REGISTERS[d0.0 as usize], DATA_REGISTERS[d1.0 as usize])
    }

    fn exg_data_address(d: DataRegister, a: AddressRegister) -> String {
        format!("exg {}, {}", DATA_REGISTERS[d.0 as usize], ADDRESS_REGISTERS[a.0 as usize])
    }

    fn eor(&mut self, size: OpSize, source: AddressingMode, dest: AddressingMode) -> String {
        let name = match source {
            AddressingMode::Immediate => "eori",
            _ => "eor",
        };

        self.binary_op(name, size, source, dest)
    }

    fn eori_to_ccr(&mut self) -> String {
        let source_str = self.operand_string(AddressingMode::Immediate, OpSize::Byte);
        format!("eori {source_str}, ccr")
    }

    fn eori_to_sr(&mut self) -> String {
        let source_str = self.operand_string(AddressingMode::Immediate, OpSize::Word);
        format!("eori {source_str}, sr")
    }

    fn ext(size: OpSize, dest: DataRegister) -> String {
        format!("ext.{} {}", size_str(size), DATA_REGISTERS[dest.0 as usize])
    }

    fn jmp(&mut self, dest: AddressingMode) -> String {
        let dest_str = self.operand_string(dest, OpSize::LongWord);
        format!("jmp {dest_str}")
    }

    fn jsr(&mut self, dest: AddressingMode) -> String {
        let dest_str = self.operand_string(dest, OpSize::LongWord);
        format!("jsr {dest_str}")
    }

    fn link(&mut self, register: AddressRegister) -> String {
        let displacement = self.read_word() as i16;
        format!("link {}, {displacement}", ADDRESS_REGISTERS[register.0 as usize])
    }

    fn lea(&mut self, source: AddressingMode, dest: AddressRegister) -> String {
        let source_str = self.operand_string(source, OpSize::LongWord);
        format!("lea {source_str}, {}", ADDRESS_REGISTERS[dest.0 as usize])
    }

    fn lsl_lsr_memory(&mut self, direction: ShiftDirection, dest: AddressingMode) -> String {
        let dest_str = self.operand_string(dest, OpSize::Word);
        format!("ls{} {dest_str}", direction_str(direction))
    }

    fn lsl_lsr_register(
        size: OpSize,
        direction: ShiftDirection,
        register: DataRegister,
        count: ShiftCount,
    ) -> String {
        format!(
            "ls{}.{} {}, {}",
            direction_str(direction),
            size_str(size),
            shift_count_str(count),
            DATA_REGISTERS[register.0 as usize]
        )
    }

    fn move_(&mut self, size: OpSize, source: AddressingMode, dest: AddressingMode) -> String {
        let name = match (source, dest) {
            (AddressingMode::Quick(_), _) => "moveq",
            (_, AddressingMode::AddressDirect(_)) => "movea",
            _ => "move",
        };

        self.binary_op(name, size, source, dest)
    }

    fn move_from_sr(&mut self, dest: AddressingMode) -> String {
        let dest_str = self.operand_string(dest, OpSize::Word);
        format!("move sr, {dest_str}")
    }

    fn movem(&mut self, size: OpSize, am: AddressingMode, direction: Direction) -> String {
        let registers = self.read_word();
        let registers_str = movem_registers_str(registers, am);
        let memory_str = self.operand_string(am, size);

        match direction {
            Direction::RegisterToMemory => {
                format!("movem.{} {registers_str}, {memory_str}", size_str(size))
            }
            Direction::MemoryToRegister => {
                format!("movem.{} {memory_str}, {registers_str}", size_str(size))
            }
        }
    }

    fn movep(
        &mut self,
        size: OpSize,
        data: DataRegister,
        address: AddressRegister,
        direction: Direction,
    ) -> String {
        let displacement = self.read_word() as i16;
        match direction {
            Direction::RegisterToMemory => {
                format!(
                    "movep.{} {}, ({displacement},{})",
                    size_str(size),
                    DATA_REGISTERS[data.0 as usize],
                    ADDRESS_REGISTERS[address.0 as usize]
                )
            }
            Direction::MemoryToRegister => {
                format!(
                    "movep.{} ({displacement},{}), {}",
                    size_str(size),
                    ADDRESS_REGISTERS[address.0 as usize],
                    DATA_REGISTERS[data.0 as usize]
                )
            }
        }
    }

    fn moveq(value: i8, register: DataRegister) -> String {
        format!("moveq #{value}, {}", DATA_REGISTERS[register.0 as usize])
    }

    fn move_to_ccr(&mut self, source: AddressingMode) -> String {
        let source_str = self.operand_string(source, OpSize::Byte);
        format!("move {source_str}, ccr")
    }

    fn move_to_sr(&mut self, source: AddressingMode) -> String {
        let source_str = self.operand_string(source, OpSize::Word);
        format!("move {source_str}, sr")
    }

    fn move_usp(direction: UspDirection, register: AddressRegister) -> String {
        match direction {
            UspDirection::RegisterToUsp => {
                format!("move {}, usp", ADDRESS_REGISTERS[register.0 as usize])
            }
            UspDirection::UspToRegister => {
                format!("move usp, {}", ADDRESS_REGISTERS[register.0 as usize])
            }
        }
    }

    fn muls(&mut self, dest: DataRegister, source: AddressingMode) -> String {
        self.muldiv_op("muls", dest, source)
    }

    fn mulu(&mut self, dest: DataRegister, source: AddressingMode) -> String {
        self.muldiv_op("mulu", dest, source)
    }

    fn neg(&mut self, size: OpSize, dest: AddressingMode, with_extend: bool) -> String {
        let name = if with_extend { "negx" } else { "neg" };
        let dest_str = self.operand_string(dest, size);
        format!("{name}.{} {dest_str}", size_str(size))
    }

    fn nbcd(&mut self, dest: AddressingMode) -> String {
        let dest_str = self.operand_string(dest, OpSize::Byte);
        format!("nbcd {dest_str}")
    }

    fn not(&mut self, size: OpSize, dest: AddressingMode) -> String {
        let dest_str = self.operand_string(dest, size);
        format!("not.{} {dest_str}", size_str(size))
    }

    fn or(&mut self, size: OpSize, source: AddressingMode, dest: AddressingMode) -> String {
        let name = match source {
            AddressingMode::Immediate => "ori",
            _ => "or",
        };

        self.binary_op(name, size, source, dest)
    }

    fn ori_to_ccr(&mut self) -> String {
        let source_str = self.operand_string(AddressingMode::Immediate, OpSize::Byte);
        format!("ori {source_str}, ccr")
    }

    fn ori_to_sr(&mut self) -> String {
        let source_str = self.operand_string(AddressingMode::Immediate, OpSize::Word);
        format!("ori {source_str}, sr")
    }

    fn pea(&mut self, source: AddressingMode) -> String {
        let source_str = self.operand_string(source, OpSize::LongWord);
        format!("pea {source_str}")
    }

    fn rol_ror_memory(&mut self, direction: ShiftDirection, dest: AddressingMode) -> String {
        let dest_str = self.operand_string(dest, OpSize::Word);
        format!("ro{} {dest_str}", direction_str(direction))
    }

    fn rol_ror_register(
        size: OpSize,
        direction: ShiftDirection,
        register: DataRegister,
        count: ShiftCount,
    ) -> String {
        format!(
            "ro{}.{} {}, {}",
            direction_str(direction),
            size_str(size),
            shift_count_str(count),
            DATA_REGISTERS[register.0 as usize]
        )
    }

    fn roxl_roxr_memory(&mut self, direction: ShiftDirection, dest: AddressingMode) -> String {
        let dest_str = self.operand_string(dest, OpSize::Word);
        format!("rox{} {dest_str}", direction_str(direction))
    }

    fn roxl_roxr_register(
        size: OpSize,
        direction: ShiftDirection,
        register: DataRegister,
        count: ShiftCount,
    ) -> String {
        format!(
            "rox{}.{} {}, {}",
            direction_str(direction),
            size_str(size),
            shift_count_str(count),
            DATA_REGISTERS[register.0 as usize]
        )
    }

    fn scc(&mut self, condition: BranchCondition, dest: AddressingMode) -> String {
        let dest_str = self.operand_string(dest, OpSize::Byte);
        format!("s{} {dest_str}", condition_suffix(condition))
    }

    fn sub(
        &mut self,
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
        with_extend: bool,
    ) -> String {
        let name = if with_extend {
            "subx"
        } else {
            match (source, dest) {
                (AddressingMode::Quick(_), _) => "subq",
                (_, AddressingMode::AddressDirect(_)) => "suba",
                (AddressingMode::Immediate, _) => "subi",
                _ => "sub",
            }
        };

        self.binary_op(name, size, source, dest)
    }

    fn sbcd(&mut self, source: AddressingMode, dest: AddressingMode) -> String {
        let source_str = self.operand_string(source, OpSize::Byte);
        let dest_str = self.operand_string(dest, OpSize::Byte);
        format!("sbcd {source_str}, {dest_str}")
    }

    fn swap(register: DataRegister) -> String {
        format!("swap {}", DATA_REGISTERS[register.0 as usize])
    }

    fn tst(&mut self, size: OpSize, source: AddressingMode) -> String {
        let source_str = self.operand_string(source, size);
        format!("tst.{} {source_str}", size_str(size))
    }

    fn tas(&mut self, dest: AddressingMode) -> String {
        let dest_str = self.operand_string(dest, OpSize::Byte);
        format!("tas {dest_str}")
    }

    fn trap(vector: u32) -> String {
        format!("trap #{vector}")
    }

    fn unlk(register: AddressRegister) -> String {
        format!("unlk {}", ADDRESS_REGISTERS[register.0 as usize])
    }

    fn operand_string(&mut self, am: AddressingMode, size: OpSize) -> Cow<'static, str> {
        fn index_register_str(index: IndexRegister) -> &'static str {
            match index {
                IndexRegister::Data(r) => DATA_REGISTERS[r.0 as usize],
                IndexRegister::Address(r) => ADDRESS_REGISTERS[r.0 as usize],
            }
        }

        fn index_size_str(size: IndexSize) -> &'static str {
            match size {
                IndexSize::SignExtendedWord => "w",
                IndexSize::LongWord => "l",
            }
        }

        match am {
            AddressingMode::DataDirect(r) => DATA_REGISTERS[r.0 as usize].into(),
            AddressingMode::AddressDirect(r) => ADDRESS_REGISTERS[r.0 as usize].into(),
            AddressingMode::AddressIndirect(r) => ADDRESS_INDIRECT[r.0 as usize].into(),
            AddressingMode::AddressIndirectPostincrement(r) => ADDRESS_POSTINC[r.0 as usize].into(),
            AddressingMode::AddressIndirectPredecrement(r) => ADDRESS_PREDEC[r.0 as usize].into(),
            AddressingMode::AddressIndirectDisplacement(r) => {
                let displacement = self.read_word() as i16;
                format!("({displacement},a{})", r.0).into()
            }
            AddressingMode::AddressIndirectIndexed(r) => {
                let extension = self.read_word();
                let (index_register, index_size) = crate::core::parse_index(extension);
                let displacement = extension as i8;

                format!(
                    "({displacement},a{},{}.{})",
                    r.0,
                    index_register_str(index_register),
                    index_size_str(index_size)
                )
                .into()
            }
            AddressingMode::PcRelativeDisplacement => {
                let displacement = self.read_word() as i16;
                format!("({displacement},pc)").into()
            }
            AddressingMode::PcRelativeIndexed => {
                let extension = self.read_word();
                let (index_register, index_size) = crate::core::parse_index(extension);
                let displacement = extension as i8;

                format!(
                    "({displacement},pc,{}.{})",
                    index_register_str(index_register),
                    index_size_str(index_size)
                )
                .into()
            }
            AddressingMode::AbsoluteShort => {
                let address = (self.read_word() as i16 as u32) & 0xFFFFFF;
                format!("(0x{address:06X})").into()
            }
            AddressingMode::AbsoluteLong => {
                let high: u32 = self.read_word().into();
                let low: u32 = self.read_word().into();
                let address = (low | (high << 16)) & 0xFFFFFF;
                format!("(0x{address:06X})").into()
            }
            AddressingMode::Immediate => {
                let extension = self.read_word();
                match size {
                    OpSize::Byte => format!("#0x{:02X}", extension & 0xFF).into(),
                    OpSize::Word => format!("#0x{extension:04X}").into(),
                    OpSize::LongWord => {
                        let low = self.read_word();
                        let immediate = u32::from(low) | (u32::from(extension) << 16);
                        format!("#0x{immediate:08X}").into()
                    }
                }
            }
            AddressingMode::Quick(value) => format!("#{value}").into(),
        }
    }
}

fn movem_registers_str(extension: u16, am: AddressingMode) -> String {
    let reverse = matches!(am, AddressingMode::AddressIndirectPredecrement(_));

    let data_fn = |i: u8| if reverse { extension.bit(15 - i) } else { extension.bit(i) };
    let address_fn = |i: u8| if reverse { extension.bit(7 - i) } else { extension.bit(8 + i) };

    let mut out = String::new();
    movem_str_half(data_fn, DATA_REGISTERS, &mut out);
    movem_str_half(address_fn, ADDRESS_REGISTERS, &mut out);

    if out.is_empty() {
        // Is this even a valid MOVEM instruction?
        return "empty".into();
    }

    out
}

fn movem_str_half(register_fn: impl Fn(u8) -> bool, register_strs: &[&str; 8], out: &mut String) {
    let mut last_set: Option<u8> = None;
    let mut i = 0;
    loop {
        if i < 8 && register_fn(i) {
            if last_set.is_none() {
                last_set = Some(i);
            }
        } else if let Some(last_set) = last_set.take() {
            if !out.is_empty() {
                out.push('/');
            }

            out.push_str(register_strs[last_set as usize]);
            if last_set != i - 1 {
                out.push('-');
                out.push_str(register_strs[(i - 1) as usize]);
            }
        }

        if i == 8 {
            break;
        }

        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{AddressRegister, DataRegister};

    #[test]
    fn movem() {
        assert_eq!(
            movem_registers_str(0x0707, AddressingMode::DataDirect(DataRegister(0))),
            "d0-d2/a0-a2"
        );
        assert_eq!(
            movem_registers_str(
                0x0707,
                AddressingMode::AddressIndirectPredecrement(AddressRegister(0))
            ),
            "d5-d7/a5-a7"
        );
        assert_eq!(
            movem_registers_str(0xAA55, AddressingMode::DataDirect(DataRegister(0))),
            "d0/d2/d4/d6/a1/a3/a5/a7"
        );
        assert_eq!(
            movem_registers_str(0xFFFF, AddressingMode::DataDirect(DataRegister(0))),
            "d0-d7/a0-a7"
        );
    }
}
