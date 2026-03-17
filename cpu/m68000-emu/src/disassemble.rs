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

#[derive(Debug, Clone)]
pub struct DisassembledInstruction {
    pub opcodes: Vec<u16>,
    pub text: String,
    pub next_opcode_addr: u32,
}

impl DisassembledInstruction {
    #[must_use]
    pub fn new() -> Self {
        Self { opcodes: Vec::new(), text: String::new(), next_opcode_addr: 0 }
    }

    fn clear(&mut self) {
        self.opcodes.clear();
        self.text.clear();
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

pub fn disassemble_into(
    out: &mut DisassembledInstruction,
    pc: u32,
    read_word: impl FnMut() -> u16,
) {
    out.clear();

    let mut reader = WordReader { pc, read: read_word, opcodes: mem::take(&mut out.opcodes) };
    let instruction = Instruction::decode_from(reader.read_word());

    out.text = match instruction {
        Instruction::Add { size, source, dest, with_extend } => {
            add(size, source, dest, with_extend, &mut reader)
        }
        Instruction::AddDecimal { source, dest } => abcd(source, dest, &mut reader),
        Instruction::And { size, source, dest } => and(size, source, dest, &mut reader),
        Instruction::AndToCcr => andi_to_ccr(&mut reader),
        Instruction::AndToSr => andi_to_sr(&mut reader),
        Instruction::ArithmeticShiftMemory(direction, dest) => {
            asl_asr_memory(&mut reader, direction, dest)
        }
        Instruction::ArithmeticShiftRegister(size, direction, register, count) => {
            asl_asr_register(size, direction, register, count)
        }
        Instruction::BitTest { source, dest } => btst(source, dest, &mut reader),
        Instruction::BitTestAndChange { source, dest } => bchg(source, dest, &mut reader),
        Instruction::BitTestAndClear { source, dest } => bclr(source, dest, &mut reader),
        Instruction::BitTestAndSet { source, dest } => bset(source, dest, &mut reader),
        Instruction::Branch(condition, displacement) => {
            branch(condition, displacement, &mut reader)
        }
        Instruction::BranchDecrement(condition, register) => dbcc(condition, register, &mut reader),
        Instruction::BranchToSubroutine(displacement) => bsr(displacement, &mut reader),
        Instruction::CheckRegister(register, source) => chk(register, source, &mut reader),
        Instruction::Clear(size, dest) => clr(size, dest, &mut reader),
        Instruction::Compare { size, source, dest } => cmp(size, source, dest, &mut reader),
        Instruction::DivideSigned(dest, source) => divs(dest, source, &mut reader),
        Instruction::DivideUnsigned(dest, source) => divu(dest, source, &mut reader),
        Instruction::ExchangeAddress(a0, a1) => exg_address(a0, a1),
        Instruction::ExchangeData(d0, d1) => exg_data(d0, d1),
        Instruction::ExchangeDataAddress(d, a) => exg_data_address(d, a),
        Instruction::ExclusiveOr { size, source, dest } => eor(size, source, dest, &mut reader),
        Instruction::ExclusiveOrToCcr => eori_to_ccr(&mut reader),
        Instruction::ExclusiveOrToSr => eori_to_sr(&mut reader),
        Instruction::Extend(size, dest) => ext(size, dest),
        Instruction::Illegal { opcode } => {
            format!("illegal ;0x{opcode:04X}")
        }
        Instruction::Jump(dest) => jmp(dest, &mut reader),
        Instruction::JumpToSubroutine(dest) => jsr(dest, &mut reader),
        Instruction::Link(register) => link(register, &mut reader),
        Instruction::LoadEffectiveAddress(source, dest) => lea(source, dest, &mut reader),
        Instruction::LogicalShiftMemory(direction, dest) => {
            lsl_lsr_memory(direction, dest, &mut reader)
        }
        Instruction::LogicalShiftRegister(size, direction, register, count) => {
            lsl_lsr_register(size, direction, register, count)
        }
        Instruction::Move { size, source, dest } => move_(size, source, dest, &mut reader),
        Instruction::MoveFromSr(dest) => move_from_sr(dest, &mut reader),
        Instruction::MoveMultiple(size, am, direction) => movem(size, am, direction, &mut reader),
        Instruction::MovePeripheral(size, data, address, direction) => {
            movep(size, data, address, direction, &mut reader)
        }
        Instruction::MoveQuick(value, register) => moveq(value, register),
        Instruction::MoveToCcr(source) => move_to_ccr(source, &mut reader),
        Instruction::MoveToSr(source) => move_to_sr(source, &mut reader),
        Instruction::MoveUsp(direction, register) => move_usp(direction, register),
        Instruction::MultiplySigned(dest, source) => muls(dest, source, &mut reader),
        Instruction::MultiplyUnsigned(dest, source) => mulu(dest, source, &mut reader),
        Instruction::Negate { size, dest, with_extend } => {
            neg(size, dest, with_extend, &mut reader)
        }
        Instruction::NegateDecimal(dest) => nbcd(dest, &mut reader),
        Instruction::NoOp => "nop".into(),
        Instruction::Not(size, dest) => not(size, dest, &mut reader),
        Instruction::Or { size, source, dest } => or(size, source, dest, &mut reader),
        Instruction::OrToCcr => ori_to_ccr(&mut reader),
        Instruction::OrToSr => ori_to_sr(&mut reader),
        Instruction::PushEffectiveAddress(source) => pea(source, &mut reader),
        Instruction::Reset => "reset".into(),
        Instruction::Return { restore_ccr } => {
            if restore_ccr {
                "rtr".into()
            } else {
                "rts".into()
            }
        }
        Instruction::ReturnFromException => "rte".into(),
        Instruction::RotateMemory(direction, dest) => rol_ror_memory(direction, dest, &mut reader),
        Instruction::RotateRegister(size, direction, register, count) => {
            rol_ror_register(size, direction, register, count)
        }
        Instruction::RotateThruExtendMemory(direction, dest) => {
            roxl_roxr_memory(direction, dest, &mut reader)
        }
        Instruction::RotateThruExtendRegister(size, direction, register, count) => {
            roxl_roxr_register(size, direction, register, count)
        }
        Instruction::Set(condition, dest) => scc(condition, dest, &mut reader),
        Instruction::Subtract { size, source, dest, with_extend } => {
            sub(size, source, dest, with_extend, &mut reader)
        }
        Instruction::SubtractDecimal { source, dest } => sbcd(source, dest, &mut reader),
        Instruction::Swap(register) => swap(register),
        Instruction::Stop => "stop".into(),
        Instruction::Test(size, source) => tst(size, source, &mut reader),
        Instruction::TestAndSet(dest) => tas(dest, &mut reader),
        Instruction::Trap(vector) => trap(vector),
        Instruction::TrapOnOverflow => "trapv".into(),
        Instruction::Unlink(register) => unlk(register),
    };

    out.opcodes = mem::take(&mut reader.opcodes);
    out.next_opcode_addr = reader.pc;
}

fn binary_op(
    name: &str,
    size: OpSize,
    source: AddressingMode,
    dest: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let source_str = operand_string(source, size, reader);
    let dest_str = operand_string(dest, size, reader);
    format!("{name}.{} {source_str}, {dest_str}", size_str(size))
}

fn add(
    size: OpSize,
    source: AddressingMode,
    dest: AddressingMode,
    with_extend: bool,
    reader: &mut WordReader<impl FnMut() -> u16>,
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

    binary_op(name, size, source, dest, reader)
}

fn abcd(
    source: AddressingMode,
    dest: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let source_str = operand_string(source, OpSize::Byte, reader);
    let dest_str = operand_string(dest, OpSize::Byte, reader);
    format!("abcd {source_str}, {dest_str}")
}

fn and(
    size: OpSize,
    source: AddressingMode,
    dest: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let name = match source {
        AddressingMode::Immediate => "andi",
        _ => "and",
    };

    binary_op(name, size, source, dest, reader)
}

fn andi_to_ccr(reader: &mut WordReader<impl FnMut() -> u16>) -> String {
    let source_str = operand_string(AddressingMode::Immediate, OpSize::Byte, reader);
    format!("andi {source_str}, ccr")
}

fn andi_to_sr(reader: &mut WordReader<impl FnMut() -> u16>) -> String {
    let source_str = operand_string(AddressingMode::Immediate, OpSize::Word, reader);
    format!("andi {source_str}, sr")
}

fn asl_asr_memory(
    reader: &mut WordReader<impl FnMut() -> u16>,
    direction: ShiftDirection,
    dest: AddressingMode,
) -> String {
    let dest_str = operand_string(dest, OpSize::Word, reader);
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

fn bit_op(
    name: &str,
    source: AddressingMode,
    dest: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let source_str = match source {
        AddressingMode::Immediate => {
            let bit = reader.read_word() & 0xFF;
            format!("#{bit}").into()
        }
        _ => operand_string(source, OpSize::Byte, reader),
    };
    let dest_str = operand_string(dest, OpSize::Byte, reader);
    format!("{name} {source_str}, {dest_str}")
}

fn btst(
    source: AddressingMode,
    dest: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    bit_op("btst", source, dest, reader)
}

fn bchg(
    source: AddressingMode,
    dest: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    bit_op("bchg", source, dest, reader)
}

fn bclr(
    source: AddressingMode,
    dest: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    bit_op("bclr", source, dest, reader)
}

fn bset(
    source: AddressingMode,
    dest: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    bit_op("bset", source, dest, reader)
}

fn branch(
    condition: BranchCondition,
    displacement: i8,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let pc = reader.pc;
    let effective_displacement = match displacement {
        0 => reader.read_word() as i16,
        _ => displacement.into(),
    };

    let name = match condition {
        BranchCondition::True => "bra".into(),
        BranchCondition::False => "braf".into(),
        _ => format!("b{}", condition_suffix(condition)),
    };

    format!("{name} ${:06X}", pc.wrapping_add_signed(effective_displacement.into()))
}

fn dbcc(
    condition: BranchCondition,
    register: DataRegister,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let pc = reader.pc;
    let displacement = reader.read_word() as i16;
    format!(
        "db{} {}, ${:06X}",
        condition_suffix(condition),
        DATA_REGISTERS[register.0 as usize],
        pc.wrapping_add_signed(displacement.into())
    )
}

fn bsr(displacement: i8, reader: &mut WordReader<impl FnMut() -> u16>) -> String {
    let pc = reader.pc;
    let effective_displacement = match displacement {
        0 => reader.read_word() as i16,
        _ => displacement.into(),
    };

    format!("bsr ${:06X}", pc.wrapping_add_signed(effective_displacement.into()))
}

fn chk(
    register: DataRegister,
    source: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let source_str = operand_string(source, OpSize::Word, reader);
    format!("chk {source_str}, {}", DATA_REGISTERS[register.0 as usize])
}

fn clr(size: OpSize, dest: AddressingMode, reader: &mut WordReader<impl FnMut() -> u16>) -> String {
    let dest_str = operand_string(dest, size, reader);
    format!("clr.{} {dest_str}", size_str(size))
}

fn cmp(
    size: OpSize,
    source: AddressingMode,
    dest: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let name = match (source, dest) {
        (_, AddressingMode::AddressDirect(_)) => "cmpa",
        (AddressingMode::Immediate, _) => "cmpi",
        (
            AddressingMode::AddressIndirectPostincrement(_),
            AddressingMode::AddressIndirectPostincrement(_),
        ) => "cmpm",
        _ => "cmp",
    };

    binary_op(name, size, source, dest, reader)
}

fn muldiv_op(
    name: &str,
    dest: DataRegister,
    source: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let source = operand_string(source, OpSize::Word, reader);
    format!("{name} {source}, {}", DATA_REGISTERS[dest.0 as usize])
}

fn divs(
    dest: DataRegister,
    source: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    muldiv_op("divs", dest, source, reader)
}

fn divu(
    dest: DataRegister,
    source: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    muldiv_op("divu", dest, source, reader)
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

fn eor(
    size: OpSize,
    source: AddressingMode,
    dest: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let name = match source {
        AddressingMode::Immediate => "eori",
        _ => "eor",
    };

    binary_op(name, size, source, dest, reader)
}

fn eori_to_ccr(reader: &mut WordReader<impl FnMut() -> u16>) -> String {
    let source_str = operand_string(AddressingMode::Immediate, OpSize::Byte, reader);
    format!("eori {source_str}, ccr")
}

fn eori_to_sr(reader: &mut WordReader<impl FnMut() -> u16>) -> String {
    let source_str = operand_string(AddressingMode::Immediate, OpSize::Word, reader);
    format!("eori {source_str}, sr")
}

fn ext(size: OpSize, dest: DataRegister) -> String {
    format!("ext.{} {}", size_str(size), DATA_REGISTERS[dest.0 as usize])
}

fn jmp(dest: AddressingMode, reader: &mut WordReader<impl FnMut() -> u16>) -> String {
    let dest_str = operand_string(dest, OpSize::LongWord, reader);
    format!("jmp {dest_str}")
}

fn jsr(dest: AddressingMode, reader: &mut WordReader<impl FnMut() -> u16>) -> String {
    let dest_str = operand_string(dest, OpSize::LongWord, reader);
    format!("jsr {dest_str}")
}

fn link(register: AddressRegister, reader: &mut WordReader<impl FnMut() -> u16>) -> String {
    let displacement = reader.read_word() as i16;
    format!("link {}, {displacement}", ADDRESS_REGISTERS[register.0 as usize])
}

fn lea(
    source: AddressingMode,
    dest: AddressRegister,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let source_str = operand_string(source, OpSize::LongWord, reader);
    format!("lea {source_str}, {}", ADDRESS_REGISTERS[dest.0 as usize])
}

fn lsl_lsr_memory(
    direction: ShiftDirection,
    dest: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let dest_str = operand_string(dest, OpSize::Word, reader);
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

fn move_(
    size: OpSize,
    source: AddressingMode,
    dest: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let name = match (source, dest) {
        (AddressingMode::Quick(_), _) => "moveq",
        (_, AddressingMode::AddressDirect(_)) => "movea",
        _ => "move",
    };

    binary_op(name, size, source, dest, reader)
}

fn move_from_sr(dest: AddressingMode, reader: &mut WordReader<impl FnMut() -> u16>) -> String {
    let dest_str = operand_string(dest, OpSize::Word, reader);
    format!("move sr, {dest_str}")
}

fn movem(
    size: OpSize,
    am: AddressingMode,
    direction: Direction,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let registers = reader.read_word();
    let registers_str = movem_registers_str(registers, am);
    let memory_str = operand_string(am, size, reader);

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
    size: OpSize,
    data: DataRegister,
    address: AddressRegister,
    direction: Direction,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let displacement = reader.read_word() as i16;
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

fn move_to_ccr(source: AddressingMode, reader: &mut WordReader<impl FnMut() -> u16>) -> String {
    let source_str = operand_string(source, OpSize::Byte, reader);
    format!("move {source_str}, ccr")
}

fn move_to_sr(source: AddressingMode, reader: &mut WordReader<impl FnMut() -> u16>) -> String {
    let source_str = operand_string(source, OpSize::Word, reader);
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

fn muls(
    dest: DataRegister,
    source: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    muldiv_op("muls", dest, source, reader)
}

fn mulu(
    dest: DataRegister,
    source: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    muldiv_op("mulu", dest, source, reader)
}

fn neg(
    size: OpSize,
    dest: AddressingMode,
    with_extend: bool,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let name = if with_extend { "negx" } else { "neg" };
    let dest_str = operand_string(dest, size, reader);
    format!("{name}.{} {dest_str}", size_str(size))
}

fn nbcd(dest: AddressingMode, reader: &mut WordReader<impl FnMut() -> u16>) -> String {
    let dest_str = operand_string(dest, OpSize::Byte, reader);
    format!("nbcd {dest_str}")
}

fn not(size: OpSize, dest: AddressingMode, reader: &mut WordReader<impl FnMut() -> u16>) -> String {
    let dest_str = operand_string(dest, size, reader);
    format!("not.{} {dest_str}", size_str(size))
}

fn or(
    size: OpSize,
    source: AddressingMode,
    dest: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let name = match source {
        AddressingMode::Immediate => "ori",
        _ => "or",
    };

    binary_op(name, size, source, dest, reader)
}

fn ori_to_ccr(reader: &mut WordReader<impl FnMut() -> u16>) -> String {
    let source_str = operand_string(AddressingMode::Immediate, OpSize::Byte, reader);
    format!("ori {source_str}, ccr")
}

fn ori_to_sr(reader: &mut WordReader<impl FnMut() -> u16>) -> String {
    let source_str = operand_string(AddressingMode::Immediate, OpSize::Word, reader);
    format!("ori {source_str}, sr")
}

fn pea(source: AddressingMode, reader: &mut WordReader<impl FnMut() -> u16>) -> String {
    let source_str = operand_string(source, OpSize::LongWord, reader);
    format!("pea {source_str}")
}

fn rol_ror_memory(
    direction: ShiftDirection,
    dest: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let dest_str = operand_string(dest, OpSize::Word, reader);
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

fn roxl_roxr_memory(
    direction: ShiftDirection,
    dest: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let dest_str = operand_string(dest, OpSize::Word, reader);
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

fn scc(
    condition: BranchCondition,
    dest: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let dest_str = operand_string(dest, OpSize::Byte, reader);
    format!("s{} {dest_str}", condition_suffix(condition))
}

fn sub(
    size: OpSize,
    source: AddressingMode,
    dest: AddressingMode,
    with_extend: bool,
    reader: &mut WordReader<impl FnMut() -> u16>,
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

    binary_op(name, size, source, dest, reader)
}

fn sbcd(
    source: AddressingMode,
    dest: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let source_str = operand_string(source, OpSize::Byte, reader);
    let dest_str = operand_string(dest, OpSize::Byte, reader);
    format!("sbcd {source_str}, {dest_str}")
}

fn swap(register: DataRegister) -> String {
    format!("swap {}", DATA_REGISTERS[register.0 as usize])
}

fn tst(
    size: OpSize,
    source: AddressingMode,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> String {
    let source_str = operand_string(source, size, reader);
    format!("tst.{} {source_str}", size_str(size))
}

fn tas(dest: AddressingMode, reader: &mut WordReader<impl FnMut() -> u16>) -> String {
    let dest_str = operand_string(dest, OpSize::Byte, reader);
    format!("tas {dest_str}")
}

fn trap(vector: u32) -> String {
    format!("trap #{vector}")
}

fn unlk(register: AddressRegister) -> String {
    format!("unlk {}", ADDRESS_REGISTERS[register.0 as usize])
}

fn operand_string(
    am: AddressingMode,
    size: OpSize,
    reader: &mut WordReader<impl FnMut() -> u16>,
) -> Cow<'static, str> {
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
            let displacement = reader.read_word() as i16;
            format!("({displacement},a{})", r.0).into()
        }
        AddressingMode::AddressIndirectIndexed(r) => {
            let extension = reader.read_word();
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
            let displacement = reader.read_word() as i16;
            format!("({displacement},pc)").into()
        }
        AddressingMode::PcRelativeIndexed => {
            let extension = reader.read_word();
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
            let address = (reader.read_word() as i16 as u32) & 0xFFFFFF;
            format!("(0x{address:06X})").into()
        }
        AddressingMode::AbsoluteLong => {
            let high: u32 = reader.read_word().into();
            let low: u32 = reader.read_word().into();
            let address = (low | (high << 16)) & 0xFFFFFF;
            format!("(0x{address:06X})").into()
        }
        AddressingMode::Immediate => {
            let extension = reader.read_word();
            match size {
                OpSize::Byte => format!("#0x{:02X}", extension & 0xFF).into(),
                OpSize::Word => format!("#0x{extension:04X}").into(),
                OpSize::LongWord => {
                    let low = reader.read_word();
                    let immediate = u32::from(low) | (u32::from(extension) << 16);
                    format!("#0x{immediate:08X}").into()
                }
            }
        }
        AddressingMode::Quick(value) => format!("#{value}").into(),
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
