use crate::core::instructions::{
    BranchCondition, Direction, Instruction, ShiftCount, ShiftDirection, UspDirection,
};
use crate::core::{AddressRegister, AddressingMode, DataRegister, OpSize, instructions};
use jgenesis_common::num::GetBit;
use std::borrow::Cow;

#[derive(Debug, Clone)]
pub struct Disassembly {
    pub string: String,
    pub byte_length: usize,
    pub new_pc: u32,
}

pub fn disassemble(bytes: &[u8], pc: u32) -> Disassembly {
    Disassembler { pc, bytes }.disassemble()
}

struct Disassembler<'a> {
    pc: u32,
    bytes: &'a [u8],
}

impl Disassembler<'_> {
    fn disassemble(mut self) -> Disassembly {
        let pc = self.pc;

        let opcode = self.next_word();

        let string = match instructions::decode_first_opcode(opcode) {
            Instruction::Add { size, source, dest, with_extend } => {
                self.add(size, source, dest, with_extend)
            }
            Instruction::AddDecimal { source, dest } => self.abcd(source, dest),
            Instruction::And { size, source, dest } => self.and(size, source, dest),
            Instruction::AndToCcr => self.andi_to_ccr(),
            Instruction::AndToSr => self.andi_to_sr(),
            Instruction::ArithmeticShiftMemory(direction, dest) => {
                self.shift_memory("asl", "asr", direction, dest)
            }
            Instruction::ArithmeticShiftRegister(size, direction, register, count) => {
                self.shift_register("asl", "asr", size, direction, register, count)
            }
            Instruction::BitTest { source, dest } => self.bit_instruction("btst", source, dest),
            Instruction::BitTestAndChange { source, dest } => {
                self.bit_instruction("bchg", source, dest)
            }
            Instruction::BitTestAndClear { source, dest } => {
                self.bit_instruction("bclr", source, dest)
            }
            Instruction::BitTestAndSet { source, dest } => {
                self.bit_instruction("bset", source, dest)
            }
            Instruction::Branch(condition, displacement) => self.branch(condition, displacement),
            Instruction::BranchDecrement(condition, register) => {
                self.decrement_branch(condition, register)
            }
            Instruction::BranchToSubroutine(displacement) => self.bsr(displacement),
            Instruction::CheckRegister(register, am) => self.chk(am, register),
            Instruction::Clear(size, dest) => self.clr(size, dest),
            Instruction::Compare { size, source, dest } => self.cmp(size, source, dest),
            Instruction::DivideSigned(dest, source) => self.muldiv("divs", source, dest),
            Instruction::DivideUnsigned(dest, source) => self.muldiv("divu", source, dest),
            Instruction::ExchangeAddress(x, y) => {
                self.exg(AddressingMode::AddressDirect(x), AddressingMode::AddressDirect(y))
            }
            Instruction::ExchangeData(x, y) => {
                self.exg(AddressingMode::DataDirect(x), AddressingMode::DataDirect(y))
            }
            Instruction::ExchangeDataAddress(x, y) => {
                self.exg(AddressingMode::DataDirect(x), AddressingMode::AddressDirect(y))
            }
            Instruction::ExclusiveOr { size, source, dest } => self.eor(size, source, dest),
            Instruction::ExclusiveOrToCcr => self.eori_to_ccr(),
            Instruction::ExclusiveOrToSr => self.eori_to_sr(),
            Instruction::Extend(size, register) => Self::ext(size, register),
            Instruction::Illegal { .. } => "illegal".into(),
            Instruction::Jump(am) => self.jmp(am),
            Instruction::JumpToSubroutine(am) => self.jsr(am),
            Instruction::Link(register) => self.link(register),
            Instruction::LoadEffectiveAddress(am, register) => self.lea(am, register),
            Instruction::LogicalShiftMemory(direction, dest) => {
                self.shift_memory("lsl", "lsr", direction, dest)
            }
            Instruction::LogicalShiftRegister(size, direction, register, count) => {
                self.shift_register("lsl", "lsr", size, direction, register, count)
            }
            Instruction::Move { size, source, dest } => self.move_(size, source, dest),
            Instruction::MoveFromSr(am) => self.move_from_sr(am),
            Instruction::MoveMultiple(size, am, direction) => self.movem(size, am, direction),
            Instruction::MovePeripheral(size, dr, ar, direction) => {
                self.movep(size, dr, ar, direction)
            }
            Instruction::MoveQuick(immediate, register) => Self::moveq(immediate, register),
            Instruction::MoveToCcr(am) => self.move_to_ccr(am),
            Instruction::MoveToSr(am) => self.move_to_sr(am),
            Instruction::MoveUsp(direction, register) => Self::move_usp(direction, register),
            Instruction::MultiplySigned(dest, source) => self.muldiv("muls", source, dest),
            Instruction::MultiplyUnsigned(dest, source) => self.muldiv("mulu", source, dest),
            Instruction::Negate { size, dest, with_extend } => self.neg(size, dest, with_extend),
            Instruction::NegateDecimal(dest) => self.nbcd(dest),
            Instruction::NoOp => "nop".into(),
            Instruction::Not(size, dest) => self.not(size, dest),
            Instruction::Or { size, source, dest } => self.or(size, source, dest),
            Instruction::OrToCcr => self.ori_to_ccr(),
            Instruction::OrToSr => self.ori_to_sr(),
            Instruction::PushEffectiveAddress(am) => self.pea(am),
            Instruction::Reset => "reset".into(),
            Instruction::Return { restore_ccr } => {
                if restore_ccr {
                    "rtr".into()
                } else {
                    "rts".into()
                }
            }
            Instruction::ReturnFromException => "rte".into(),
            Instruction::RotateMemory(direction, dest) => {
                self.shift_memory("rol", "ror", direction, dest)
            }
            Instruction::RotateRegister(size, direction, register, count) => {
                self.shift_register("rol", "ror", size, direction, register, count)
            }
            Instruction::RotateThruExtendMemory(direction, dest) => {
                self.shift_memory("roxl", "roxr", direction, dest)
            }
            Instruction::RotateThruExtendRegister(size, direction, register, count) => {
                self.shift_register("roxl", "roxr", size, direction, register, count)
            }
            Instruction::Set(condition, dest) => self.set(condition, dest),
            Instruction::Subtract { size, source, dest, with_extend } => {
                self.sub(size, source, dest, with_extend)
            }
            Instruction::SubtractDecimal { source, dest } => self.sbcd(source, dest),
            Instruction::Swap(register) => {
                format!("swap D{}", register.0)
            }
            Instruction::Stop => "stop".into(),
            Instruction::Test(size, dest) => self.tst(size, dest),
            Instruction::TestAndSet(dest) => self.tas(dest),
            Instruction::Trap(trap) => {
                format!("trap #{trap}")
            }
            Instruction::TrapOnOverflow => "trapv".into(),
            Instruction::Unlink(register) => {
                format!("unlk A{}", register.0)
            }
        };

        let instruction_len = self.pc.wrapping_sub(pc);

        Disassembly { string, byte_length: instruction_len as usize, new_pc: self.pc }
    }

    fn next_word(&mut self) -> u16 {
        let word = u16::from_be_bytes([self.bytes[0], self.bytes[1]]);
        self.bytes = &self.bytes[2..];
        self.pc = self.pc.wrapping_add(2);
        word
    }

    fn resolve_addressing_mode(&mut self, am: AddressingMode, size: OpSize) -> Cow<'static, str> {
        match am {
            AddressingMode::DataDirect(register) => {
                ["D0", "D1", "D2", "D3", "D4", "D5", "D6", "D7"][register.0 as usize].into()
            }
            AddressingMode::AddressDirect(register) => {
                ["A0", "A1", "A2", "A3", "A4", "A5", "A6", "A7"][register.0 as usize].into()
            }
            AddressingMode::AddressIndirect(register) => {
                ["(A0)", "(A1)", "(A2)", "(A3)", "(A4)", "(A5)", "(A6)", "(A7)"]
                    [register.0 as usize]
                    .into()
            }
            AddressingMode::AddressIndirectPredecrement(register) => {
                ["-(A0)", "-(A1)", "-(A2)", "-(A3)", "-(A4)", "-(A5)", "-(A6)", "-(A7)"]
                    [register.0 as usize]
                    .into()
            }
            AddressingMode::AddressIndirectPostincrement(register) => {
                ["(A0)+", "(A1)+", "(A2)+", "(A3)+", "(A4)+", "(A5)+", "(A6)+", "(A7)+"]
                    [register.0 as usize]
                    .into()
            }
            AddressingMode::AddressIndirectDisplacement(register) => {
                let displacement = self.next_word() as i16;

                format!("({displacement},A{})", register.0).into()
            }
            AddressingMode::AddressIndirectIndexed(register) => {
                let extension = self.next_word();

                let register =
                    ["A0", "A1", "A2", "A3", "A4", "A5", "A6", "A7"][register.0 as usize];
                resolve_indexed_addressing_mode(register, extension)
            }
            AddressingMode::PcRelativeDisplacement => {
                let displacement = self.next_word() as i16;

                format!("({displacement},PC)").into()
            }
            AddressingMode::PcRelativeIndexed => {
                let extension = self.next_word();

                resolve_indexed_addressing_mode("PC", extension)
            }
            AddressingMode::AbsoluteShort => {
                let address = self.next_word();

                format!("(${address:04X}).w").into()
            }
            AddressingMode::AbsoluteLong => {
                let address = self.next_word();

                format!("(${address:06X}).l").into()
            }
            AddressingMode::Immediate => {
                let first_word = self.next_word();
                match size {
                    OpSize::Byte => format!("#0x{:02X}", first_word & 0xFF).into(),
                    OpSize::Word => format!("#0x{first_word:04X}").into(),
                    OpSize::LongWord => {
                        let second_word = self.next_word();
                        format!("#0x{first_word:04X}{second_word:04X}").into()
                    }
                }
            }
            AddressingMode::Quick(immediate) => format!("#0x{immediate:X}").into(),
        }
    }

    fn add(
        &mut self,
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
        with_extend: bool,
    ) -> String {
        let instruction = if with_extend {
            "addx"
        } else if matches!(source, AddressingMode::Quick(_)) {
            "addq"
        } else if matches!(dest, AddressingMode::AddressDirect(_)) {
            "adda"
        } else if matches!(source, AddressingMode::Immediate) {
            "addi"
        } else {
            "add"
        };

        let source = self.resolve_addressing_mode(source, size);
        let dest = self.resolve_addressing_mode(dest, size);
        let size = resolve_size(size);

        format!("{instruction}.{size} {source}, {dest}")
    }

    fn abcd(&mut self, source: AddressingMode, dest: AddressingMode) -> String {
        let source = self.resolve_addressing_mode(source, OpSize::Byte);
        let dest = self.resolve_addressing_mode(dest, OpSize::Byte);
        format!("abcd {source}, {dest}")
    }

    fn sub(
        &mut self,
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
        with_extend: bool,
    ) -> String {
        let instruction = if with_extend {
            "subx"
        } else if matches!(source, AddressingMode::Quick(_)) {
            "subq"
        } else if matches!(dest, AddressingMode::AddressDirect(_)) {
            "suba"
        } else if matches!(source, AddressingMode::Immediate) {
            "subi"
        } else {
            "sub"
        };

        let source = self.resolve_addressing_mode(source, size);
        let dest = self.resolve_addressing_mode(dest, size);
        let size = resolve_size(size);

        format!("{instruction}.{size} {source}, {dest}")
    }

    fn sbcd(&mut self, source: AddressingMode, dest: AddressingMode) -> String {
        let source = self.resolve_addressing_mode(source, OpSize::Byte);
        let dest = self.resolve_addressing_mode(dest, OpSize::Byte);
        format!("sbcd {source}, {dest}")
    }

    fn and(&mut self, size: OpSize, source: AddressingMode, dest: AddressingMode) -> String {
        let instruction = match source {
            AddressingMode::Immediate => "andi",
            _ => "and",
        };
        let source = self.resolve_addressing_mode(source, size);
        let dest = self.resolve_addressing_mode(dest, size);
        format!("{instruction}.{size} {source}, {dest}")
    }

    fn andi_to_ccr(&mut self) -> String {
        let source = self.resolve_addressing_mode(AddressingMode::Immediate, OpSize::Byte);
        format!("andi {source}, CCR")
    }

    fn andi_to_sr(&mut self) -> String {
        let source = self.resolve_addressing_mode(AddressingMode::Immediate, OpSize::Word);
        format!("andi {source}, SR")
    }

    fn eor(&mut self, size: OpSize, source: AddressingMode, dest: AddressingMode) -> String {
        let instruction = match source {
            AddressingMode::Immediate => "eori",
            _ => "eor",
        };
        let source = self.resolve_addressing_mode(source, size);
        let dest = self.resolve_addressing_mode(dest, size);
        format!("{instruction}.{size} {source}, {dest}")
    }

    fn eori_to_ccr(&mut self) -> String {
        let source = self.resolve_addressing_mode(AddressingMode::Immediate, OpSize::Byte);
        format!("eori {source}, CCR")
    }

    fn eori_to_sr(&mut self) -> String {
        let source = self.resolve_addressing_mode(AddressingMode::Immediate, OpSize::Word);
        format!("eori {source}, SR")
    }

    fn or(&mut self, size: OpSize, source: AddressingMode, dest: AddressingMode) -> String {
        let instruction = match source {
            AddressingMode::Immediate => "ori",
            _ => "or",
        };
        let source = self.resolve_addressing_mode(source, size);
        let dest = self.resolve_addressing_mode(dest, size);
        format!("{instruction}.{size} {source}, {dest}")
    }

    fn ori_to_ccr(&mut self) -> String {
        let source = self.resolve_addressing_mode(AddressingMode::Immediate, OpSize::Byte);
        format!("ori {source}, CCR")
    }

    fn ori_to_sr(&mut self) -> String {
        let source = self.resolve_addressing_mode(AddressingMode::Immediate, OpSize::Word);
        format!("ori {source}, SR")
    }

    fn shift_memory(
        &mut self,
        l: &str,
        r: &str,
        direction: ShiftDirection,
        dest: AddressingMode,
    ) -> String {
        let instruction = match direction {
            ShiftDirection::Left => format!("{l}.w"),
            ShiftDirection::Right => format!("{r}.w"),
        };

        let dest = self.resolve_addressing_mode(dest, OpSize::Word);
        format!("{instruction} {dest}")
    }

    fn shift_register(
        &mut self,
        l: &str,
        r: &str,
        size: OpSize,
        direction: ShiftDirection,
        register: DataRegister,
        count: ShiftCount,
    ) -> String {
        let instruction = match direction {
            ShiftDirection::Left => l,
            ShiftDirection::Right => r,
        };

        let source = match count {
            ShiftCount::Constant(c) => format!("#{c}"),
            ShiftCount::Register(r) => format!("D{}", r.0),
        };

        let dest = self.resolve_addressing_mode(AddressingMode::DataDirect(register), size);
        let size = resolve_size(size);

        format!("{instruction}.{size} {source}, {dest}")
    }

    fn bit_instruction(
        &mut self,
        instruction: &str,
        source: AddressingMode,
        dest: AddressingMode,
    ) -> String {
        let source = self.resolve_addressing_mode(source, OpSize::Byte);
        let dest = self.resolve_addressing_mode(dest, OpSize::Byte);

        format!("{instruction} {source}, {dest}")
    }

    fn branch(&mut self, condition: BranchCondition, displacement: i8) -> String {
        let instruction: Cow<'static, str> = match condition {
            BranchCondition::True => "bra".into(),
            _ => format!("b{}", resolve_branch_condition(condition)).into(),
        };

        let pc = self.pc;
        let displacement = self.read_branch_displacement(displacement);

        let new_pc = pc.wrapping_add_signed(displacement.into());

        format!("{instruction} ${new_pc:06X}")
    }

    fn read_branch_displacement(&mut self, displacement: i8) -> i16 {
        match displacement {
            0 => self.next_word() as i16,
            _ => displacement.into(),
        }
    }

    fn decrement_branch(&mut self, condition: BranchCondition, register: DataRegister) -> String {
        let suffix = resolve_branch_condition(condition);

        let pc = self.pc;
        let displacement = self.next_word() as i16;
        let new_pc = pc.wrapping_add_signed(displacement.into());

        format!("db{suffix} D{}, ${new_pc:06X}", register.0)
    }

    fn bsr(&mut self, displacement: i8) -> String {
        let pc = self.pc;
        let displacement = self.read_branch_displacement(displacement);
        let new_pc = pc.wrapping_add_signed(displacement.into());

        format!("bsr ${new_pc:06X}")
    }

    fn chk(&mut self, am: AddressingMode, register: DataRegister) -> String {
        let am = self.resolve_addressing_mode(am, OpSize::Word);

        format!("chk {am}, D{}", register.0)
    }

    fn clr(&mut self, size: OpSize, dest: AddressingMode) -> String {
        let dest = self.resolve_addressing_mode(dest, size);
        let size = resolve_size(size);

        format!("clr.{size} {dest}")
    }

    fn cmp(&mut self, size: OpSize, source: AddressingMode, dest: AddressingMode) -> String {
        let instruction = match (source, dest) {
            (
                AddressingMode::AddressIndirectPostincrement(_),
                AddressingMode::AddressIndirectPostincrement(_),
            ) => "cmpm",
            (_, AddressingMode::AddressDirect(_)) => "cmpa",
            (AddressingMode::Immediate, _) => "cmpi",
            _ => "cmp",
        };

        let source = self.resolve_addressing_mode(source, size);
        let dest = self.resolve_addressing_mode(dest, size);
        let size = resolve_size(size);

        format!("{instruction}.{size} {source}, {dest}")
    }

    fn muldiv(&mut self, instruction: &str, source: AddressingMode, dest: DataRegister) -> String {
        let source = self.resolve_addressing_mode(source, OpSize::Word);

        format!("{instruction} {source}, D{}", dest.0)
    }

    fn exg(&mut self, x: AddressingMode, y: AddressingMode) -> String {
        let x = self.resolve_addressing_mode(x, OpSize::LongWord);
        let y = self.resolve_addressing_mode(y, OpSize::LongWord);

        format!("exg {x}, {y}")
    }

    fn ext(size: OpSize, register: DataRegister) -> String {
        let size = resolve_size(size);
        format!("ext.{size} D{}", register.0)
    }

    fn jmp(&mut self, am: AddressingMode) -> String {
        let dest = self.resolve_addressing_mode(am, OpSize::LongWord);
        format!("jmp {dest}")
    }

    fn jsr(&mut self, am: AddressingMode) -> String {
        let dest = self.resolve_addressing_mode(am, OpSize::LongWord);
        format!("jsr {dest}")
    }

    fn link(&mut self, register: AddressRegister) -> String {
        let displacement = self.next_word() as i16;
        format!("link A{}, #{displacement}", register.0)
    }

    fn lea(&mut self, am: AddressingMode, register: AddressRegister) -> String {
        let am = self.resolve_addressing_mode(am, OpSize::LongWord);
        format!("lea {am}, A{}", register.0)
    }

    fn move_(&mut self, size: OpSize, source: AddressingMode, dest: AddressingMode) -> String {
        let instruction = match dest {
            AddressingMode::AddressDirect(_) => "movea",
            _ => "move",
        };

        let source = self.resolve_addressing_mode(source, size);
        let dest = self.resolve_addressing_mode(dest, size);
        let size = resolve_size(size);

        format!("{instruction}.{size} {source}, {dest}")
    }

    fn move_from_sr(&mut self, am: AddressingMode) -> String {
        let am = self.resolve_addressing_mode(am, OpSize::Word);
        format!("move SR, {am}")
    }

    fn move_to_sr(&mut self, am: AddressingMode) -> String {
        let am = self.resolve_addressing_mode(am, OpSize::Word);
        format!("move {am}, SR")
    }

    fn move_to_ccr(&mut self, am: AddressingMode) -> String {
        let am = self.resolve_addressing_mode(am, OpSize::Word);
        format!("move {am}, CCR")
    }

    fn move_usp(direction: UspDirection, register: AddressRegister) -> String {
        match direction {
            UspDirection::RegisterToUsp => format!("move A{}, USP", register.0),
            UspDirection::UspToRegister => format!("move USP, A{}", register.0),
        }
    }

    fn moveq(immediate: i8, register: DataRegister) -> String {
        format!("moveq #0x{:02X}, D{}", immediate as u8, register.0)
    }

    fn movem(&mut self, size: OpSize, am: AddressingMode, direction: Direction) -> String {
        let mask = self.next_word();

        let mut runs = Vec::new();
        for (start, ch) in [(0, 'D'), (8, 'A')] {
            let mut run_start: Option<u8> = None;

            for i in 0..8 {
                let bit = match am {
                    AddressingMode::AddressIndirectPredecrement(_) => 15 - (start + i),
                    _ => start + i,
                };
                if mask.bit(bit) {
                    if run_start.is_none() {
                        run_start = Some(i);
                    }
                } else if let Some(b) = run_start {
                    if b == i - 1 {
                        runs.push(format!("{ch}{b}"));
                    } else {
                        runs.push(format!("{ch}{b}-{}", i - 1));
                    }
                    run_start = None;
                }
            }

            if let Some(b) = run_start {
                if b == 7 {
                    runs.push(format!("{ch}7"));
                } else {
                    runs.push(format!("{ch}{b}-7"));
                }
            }
        }

        let runs_joined = runs.join("/");

        let am = self.resolve_addressing_mode(am, size);
        let size = resolve_size(size);

        match direction {
            Direction::RegisterToMemory => format!("movem.{size} {runs_joined}, {am}"),
            Direction::MemoryToRegister => format!("movem.{size} {am}, {runs_joined}"),
        }
    }

    fn movep(
        &mut self,
        size: OpSize,
        dr: DataRegister,
        ar: AddressRegister,
        direction: Direction,
    ) -> String {
        let displacement = self.next_word() as i16;
        let size = resolve_size(size);

        match direction {
            Direction::RegisterToMemory => {
                format!("movep.{size} D{}, ({displacement},A{})", dr.0, ar.0)
            }
            Direction::MemoryToRegister => {
                format!("movep.{size} ({displacement},A{}), D{}", ar.0, dr.0)
            }
        }
    }

    fn neg(&mut self, size: OpSize, dest: AddressingMode, with_extend: bool) -> String {
        let instruction = if with_extend { "negx" } else { "neg" };
        let dest = self.resolve_addressing_mode(dest, size);
        let size = resolve_size(size);

        format!("{instruction}.{size} {dest}")
    }

    fn nbcd(&mut self, dest: AddressingMode) -> String {
        let dest = self.resolve_addressing_mode(dest, OpSize::Byte);
        format!("nbcd {dest}")
    }

    fn not(&mut self, size: OpSize, dest: AddressingMode) -> String {
        let dest = self.resolve_addressing_mode(dest, size);
        let size = resolve_size(size);
        format!("not.{size} {dest}")
    }

    fn pea(&mut self, dest: AddressingMode) -> String {
        let dest = self.resolve_addressing_mode(dest, OpSize::LongWord);
        format!("pea {dest}")
    }

    fn set(&mut self, condition: BranchCondition, dest: AddressingMode) -> String {
        let condition = resolve_branch_condition(condition);
        let dest = self.resolve_addressing_mode(dest, OpSize::Byte);
        format!("s{condition} {dest}")
    }

    fn tst(&mut self, size: OpSize, dest: AddressingMode) -> String {
        let dest = self.resolve_addressing_mode(dest, size);
        let size = resolve_size(size);
        format!("tst.{size} {dest}")
    }

    fn tas(&mut self, dest: AddressingMode) -> String {
        let dest = self.resolve_addressing_mode(dest, OpSize::Byte);
        format!("tas {dest}")
    }
}

fn resolve_size(size: OpSize) -> char {
    match size {
        OpSize::Byte => 'b',
        OpSize::Word => 'w',
        OpSize::LongWord => 'l',
    }
}

fn resolve_indexed_addressing_mode(register: &str, extension: u16) -> Cow<'static, str> {
    let index_register = (extension >> 12) & 7;
    let index_type = if extension.bit(15) { "A" } else { "D" };
    let index_size = if extension.bit(11) { "w" } else { "l" };
    let displacement = extension as i8;
    format!("({displacement},{register},{index_type}{index_register}.{index_size})").into()
}

fn resolve_branch_condition(condition: BranchCondition) -> &'static str {
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
