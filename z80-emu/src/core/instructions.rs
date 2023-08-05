use crate::core::{IndexRegister, Register16, Register8, Registers};
use crate::traits::AddressSpace;

trait GetBit: Copy {
    fn bit(self, i: u8) -> bool;
}

impl GetBit for u8 {
    fn bit(self, i: u8) -> bool {
        assert!(i <= 7);
        self & (1 << i) != 0
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ReadTarget8 {
    Immediate(u8),
    Register(Register8),
    Indirect(Register16),
    Direct(u16),
    Indexed(IndexRegister, i8),
}

impl ReadTarget8 {
    fn read<A: AddressSpace>(self, registers: &Registers, address_space: &mut A) -> u8 {
        match self {
            Self::Immediate(n) => n,
            Self::Register(register) => register.read(registers),
            Self::Indirect(register) => {
                let address = register.read(registers);
                address_space.read(address)
            }
            Self::Direct(address) => address_space.read(address),
            Self::Indexed(index, d) => {
                let address = index_address(index.read(registers), d);
                address_space.read(address)
            }
        }
    }

    fn t_cycles_required(self) -> u32 {
        match self {
            Self::Immediate(..) | Self::Register(..) => 0,
            Self::Indirect(..) | Self::Direct(..) => 3,
            Self::Indexed(..) => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteTarget8 {
    Register(Register8),
    Indirect(Register16),
    Direct(u16),
    Indexed(IndexRegister, i8),
}

impl WriteTarget8 {
    fn write<A: AddressSpace>(self, registers: &mut Registers, address_space: &mut A, value: u8) {
        match self {
            Self::Register(register) => {
                register.write(registers, value);
            }
            Self::Indirect(register) => {
                let address = register.read(registers);
                address_space.write(address, value);
            }
            Self::Direct(address) => {
                address_space.write(address, value);
            }
            Self::Indexed(index, d) => {
                let address = index_address(index.read(registers), d);
                address_space.write(address, value);
            }
        }
    }

    fn t_cycles_required(self) -> u32 {
        match self {
            Self::Register(..) => 0,
            Self::Indirect(..) | Self::Direct(..) => 3,
            Self::Indexed(..) => 8,
        }
    }
}

fn index_address(address: u16, d: i8) -> u16 {
    (i32::from(address) + i32::from(d)) as u16
}

#[derive(Debug, Clone, Copy)]
pub enum ReadTarget16 {
    Immediate(u16),
    Register(Register16),
    Direct(u16),
}

impl ReadTarget16 {
    fn read<A: AddressSpace>(self, registers: &Registers, address_space: &mut A) -> u16 {
        match self {
            Self::Immediate(nn) => nn,
            Self::Register(register) => register.read(registers),
            Self::Direct(nn) => {
                let lsb = address_space.read(nn);
                let msb = address_space.read(nn.wrapping_add(1));
                u16::from_le_bytes([lsb, msb])
            }
        }
    }

    fn t_cycles_required(self) -> u32 {
        match self {
            Self::Immediate(..) | Self::Register(..) => 0,
            Self::Direct(..) => 6,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum WriteTarget16 {
    Register(Register16),
    Direct(u16),
}

impl WriteTarget16 {
    fn write<A: AddressSpace>(self, registers: &mut Registers, address_space: &mut A, value: u16) {
        match self {
            Self::Register(register) => {
                register.write(registers, value);
            }
            Self::Direct(nn) => {
                let [lsb, msb] = value.to_le_bytes();
                address_space.write(nn, lsb);
                address_space.write(nn.wrapping_add(1), msb);
            }
        }
    }

    fn t_cycles_required(self) -> u32 {
        match self {
            Self::Register(..) => 0,
            Self::Direct(..) => 6,
        }
    }
}

pub struct ExecuteResult {
    t_cycles: u32,
}

#[derive(Debug, Clone, Copy)]
pub enum Instruction {
    Load8(WriteTarget8, ReadTarget8),
    Load16(WriteTarget16, ReadTarget16),
    Push(Register16),
    Pop(Register16),
    Halt,
}

impl Instruction {
    pub fn execute<A>(self, registers: &mut Registers, address_space: &mut A) -> ExecuteResult
    where
        A: AddressSpace,
    {
        match self {
            Self::Load8(write_target, read_target) => {
                load_8(registers, address_space, write_target, read_target)
            }
            Self::Load16(write_target, read_target) => {
                load_16(registers, address_space, write_target, read_target)
            }
            Self::Push(register) => push(registers, address_space, register),
            Self::Pop(register) => pop(registers, address_space, register),
            Self::Halt => todo!("halt not implemented"),
        }
    }
}

fn load_8<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
    write_target: WriteTarget8,
    read_target: ReadTarget8,
) -> ExecuteResult {
    let value = read_target.read(registers, address_space);
    write_target.write(registers, address_space, value);

    if write_target == WriteTarget8::Register(Register8::A)
        && matches!(
            read_target,
            ReadTarget8::Register(Register8::I | Register8::R)
        )
    {
        // TODO set overflow to false if an interrupt has triggered
        registers
            .f
            .set_sign(value.bit(7))
            .set_zero(value == 0)
            .set_half_carry(false)
            .set_overflow(registers.iff2)
            .set_subtract(false);
    }

    ExecuteResult {
        t_cycles: read_target.t_cycles_required() + write_target.t_cycles_required(),
    }
}

fn load_16<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
    write_target: WriteTarget16,
    read_target: ReadTarget16,
) -> ExecuteResult {
    let value = read_target.read(registers, address_space);
    write_target.write(registers, address_space, value);

    ExecuteResult {
        t_cycles: read_target.t_cycles_required() + write_target.t_cycles_required(),
    }
}

fn push<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
    register: Register16,
) -> ExecuteResult {
    let value = register.read(registers);
    let [lsb, msb] = value.to_le_bytes();

    registers.sp = registers.sp.wrapping_sub(1);
    address_space.write(registers.sp, msb);
    registers.sp = registers.sp.wrapping_sub(1);
    address_space.write(registers.sp, lsb);

    ExecuteResult {
        // Extra cycle for the opcode read + 2 memory writes
        t_cycles: 7,
    }
}

fn pop<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
    register: Register16,
) -> ExecuteResult {
    let lsb = address_space.read(registers.sp);
    registers.sp = registers.sp.wrapping_add(1);
    let msb = address_space.read(registers.sp);
    registers.sp = registers.sp.wrapping_add(1);

    let value = u16::from_le_bytes([lsb, msb]);
    register.write(registers, value);

    ExecuteResult {
        // 2 memory reads
        t_cycles: 6,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ParseResult {
    instruction: Instruction,
    t_cycles: u32,
}

#[derive(Debug, Clone)]
struct InstructionParser {
    t_cycles: u32,
}

impl InstructionParser {
    const OPCODE_T_CYCLES: u32 = 4;
    const OPERAND_T_CYCLES: u32 = 3;

    fn new() -> Self {
        Self { t_cycles: 0 }
    }

    fn fetch_opcode_byte<A: AddressSpace>(
        &mut self,
        registers: &mut Registers,
        address_space: &mut A,
    ) -> u8 {
        let byte = address_space.read(registers.pc);
        registers.pc = registers.pc.wrapping_add(1);
        self.t_cycles += Self::OPCODE_T_CYCLES;

        byte
    }

    fn fetch_operand_byte<A: AddressSpace>(
        &mut self,
        registers: &mut Registers,
        address_space: &mut A,
    ) -> u8 {
        let byte = address_space.read(registers.pc);
        registers.pc = registers.pc.wrapping_add(1);
        self.t_cycles += Self::OPERAND_T_CYCLES;

        byte
    }

    fn fetch_u16_operand<A: AddressSpace>(
        &mut self,
        registers: &mut Registers,
        address_space: &mut A,
    ) -> u16 {
        let operand_lsb = self.fetch_operand_byte(registers, address_space);
        let operand_msb = self.fetch_operand_byte(registers, address_space);
        u16::from_le_bytes([operand_lsb, operand_msb])
    }

    fn parse_indexed_instruction<A: AddressSpace>(
        &mut self,
        registers: &mut Registers,
        address_space: &mut A,
        index: IndexRegister,
    ) -> Instruction {
        use ReadTarget16 as RT16;
        use ReadTarget8 as RT8;
        use Register16 as R16;
        use WriteTarget16 as WT16;
        use WriteTarget8 as WT8;

        let opcode_2 = self.fetch_opcode_byte(registers, address_space);

        match opcode_2 {
            0x21 => {
                let nn = self.fetch_u16_operand(registers, address_space);
                Instruction::Load16(WT16::Register(index.into()), RT16::Immediate(nn))
            }
            0x22 => {
                let nn = self.fetch_u16_operand(registers, address_space);
                Instruction::Load16(WT16::Direct(nn), RT16::Register(index.into()))
            }
            0x2A => {
                let nn = self.fetch_u16_operand(registers, address_space);
                Instruction::Load16(WT16::Register(index.into()), RT16::Direct(nn))
            }
            0x36 => {
                let d = self.fetch_operand_byte(registers, address_space) as i8;
                let n = self.fetch_operand_byte(registers, address_space);

                // LD (IX+d), n overlaps the second operand fetch with the indexing
                self.t_cycles -= Self::OPERAND_T_CYCLES;

                Instruction::Load8(WT8::Indexed(index, d), RT8::Immediate(n))
            }
            0x46 | 0x4E | 0x56 | 0x5E | 0x66 | 0x6E | 0x76 | 0x7E => {
                let write_target = parse_write_target_8(opcode_2 >> 3);
                let d = self.fetch_operand_byte(registers, address_space) as i8;
                Instruction::Load8(write_target, RT8::Indexed(index, d))
            }
            0x70..=0x77 => {
                let read_target = parse_read_target_8(opcode_2);
                let d = self.fetch_operand_byte(registers, address_space) as i8;
                Instruction::Load8(WT8::Indexed(index, d), read_target)
            }
            0xE1 => Instruction::Pop(index.into()),
            0xE5 => Instruction::Push(index.into()),
            0xF9 => Instruction::Load16(WT16::Register(R16::SP), RT16::Register(index.into())),
            _ => panic!("indexed X/Y opcode not implemented yet: {opcode_2:02X}"),
        }
    }

    fn parse_ed_prefix_instruction<A: AddressSpace>(
        &mut self,
        registers: &mut Registers,
        address_space: &mut A,
    ) -> Instruction {
        use ReadTarget16 as RT16;
        use ReadTarget8 as RT8;
        use Register8 as R8;
        use WriteTarget16 as WT16;
        use WriteTarget8 as WT8;

        let opcode_2 = self.fetch_opcode_byte(registers, address_space);

        match opcode_2 {
            0x03 | 0x13 | 0x23 | 0x33 => {
                let register = parse_load_register_16(opcode_2);
                let nn = self.fetch_u16_operand(registers, address_space);
                Instruction::Load16(WT16::Direct(nn), RT16::Register(register))
            }
            0x47 => {
                self.t_cycles += 1;
                Instruction::Load8(WT8::Register(R8::I), RT8::Register(R8::A))
            }
            0x4B | 0x5B | 0x6B | 0x7B => {
                let register = parse_load_register_16(opcode_2);
                let nn = self.fetch_u16_operand(registers, address_space);
                Instruction::Load16(WT16::Register(register), RT16::Direct(nn))
            }
            0x4F => {
                // TODO do this t_cycle increment in ReadTarget/WriteTarget instead?
                self.t_cycles += 1;
                Instruction::Load8(WT8::Register(R8::R), RT8::Register(R8::A))
            }
            0x57 => {
                self.t_cycles += 1;
                Instruction::Load8(WT8::Register(R8::A), RT8::Register(R8::I))
            }
            0x5F => {
                self.t_cycles += 1;
                Instruction::Load8(WT8::Register(R8::A), RT8::Register(R8::R))
            }
            _ => panic!("ED-prefixed opcode not implemented: {opcode_2:02X}"),
        }
    }

    fn parse<A: AddressSpace>(
        mut self,
        registers: &mut Registers,
        address_space: &mut A,
    ) -> ParseResult {
        use ReadTarget16 as RT16;
        use ReadTarget8 as RT8;
        use Register16 as R16;
        use Register8 as R8;
        use WriteTarget16 as WT16;
        use WriteTarget8 as WT8;

        let opcode = self.fetch_opcode_byte(registers, address_space);

        let instruction = match opcode {
            0x01 | 0x11 | 0x21 | 0x31 => {
                let register = parse_load_register_16(opcode);
                let nn = self.fetch_u16_operand(registers, address_space);
                Instruction::Load16(WT16::Register(register), RT16::Immediate(nn))
            }
            0x02 => Instruction::Load8(WT8::Indirect(R16::BC), RT8::Register(R8::A)),
            0x0A => Instruction::Load8(WT8::Register(R8::A), RT8::Indirect(R16::BC)),
            0x12 => Instruction::Load8(WT8::Indirect(R16::DE), RT8::Register(R8::A)),
            0x1A => Instruction::Load8(WT8::Register(R8::A), RT8::Indirect(R16::DE)),
            0x22 => {
                let nn = self.fetch_u16_operand(registers, address_space);
                Instruction::Load16(WT16::Direct(nn), RT16::Register(R16::HL))
            }
            0x2A => {
                let nn = self.fetch_u16_operand(registers, address_space);
                Instruction::Load16(WT16::Register(R16::HL), RT16::Direct(nn))
            }
            0x32 => {
                let nn = self.fetch_u16_operand(registers, address_space);
                Instruction::Load8(WT8::Direct(nn), RT8::Register(R8::A))
            }
            0x3A => {
                let nn = self.fetch_u16_operand(registers, address_space);
                Instruction::Load8(WT8::Register(R8::A), RT8::Direct(nn))
            }
            0x40..=0x7F => {
                if opcode == 0x76 {
                    Instruction::Halt
                } else {
                    let write_target = parse_write_target_8(opcode >> 3);
                    let read_target = parse_read_target_8(opcode);
                    Instruction::Load8(write_target, read_target)
                }
            }
            0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x36 | 0x3E => {
                let write_target = parse_write_target_8(opcode >> 3);
                let n = self.fetch_operand_byte(registers, address_space);
                Instruction::Load8(write_target, RT8::Immediate(n))
            }
            0xC1 | 0xD1 | 0xE1 | 0xF1 => {
                let register = parse_stack_register(opcode);
                Instruction::Pop(register)
            }
            0xC5 | 0xD5 | 0xE5 | 0xF5 => {
                let register = parse_stack_register(opcode);
                Instruction::Push(register)
            }
            0xDD => self.parse_indexed_instruction(registers, address_space, IndexRegister::IX),
            0xED => self.parse_ed_prefix_instruction(registers, address_space),
            0xF9 => Instruction::Load16(WT16::Register(R16::SP), RT16::Register(R16::HL)),
            0xFD => self.parse_indexed_instruction(registers, address_space, IndexRegister::IY),
            _ => panic!("opcode not implemented yet: {opcode:02X}"),
        };

        ParseResult {
            instruction,
            t_cycles: self.t_cycles,
        }
    }
}

pub fn parse_next_instruction<A: AddressSpace>(
    registers: &mut Registers,
    address_space: &mut A,
) -> ParseResult {
    InstructionParser::new().parse(registers, address_space)
}

fn parse_read_target_8(opcode: u8) -> ReadTarget8 {
    match opcode & 0x07 {
        0x00 => ReadTarget8::Register(Register8::B),
        0x01 => ReadTarget8::Register(Register8::C),
        0x02 => ReadTarget8::Register(Register8::D),
        0x03 => ReadTarget8::Register(Register8::E),
        0x04 => ReadTarget8::Register(Register8::H),
        0x05 => ReadTarget8::Register(Register8::L),
        0x06 => ReadTarget8::Indirect(Register16::HL),
        0x07 => ReadTarget8::Register(Register8::A),
        _ => unreachable!("value & 0x07 is <= 0x07"),
    }
}

fn parse_write_target_8(opcode: u8) -> WriteTarget8 {
    match opcode & 0x07 {
        0x00 => WriteTarget8::Register(Register8::B),
        0x01 => WriteTarget8::Register(Register8::C),
        0x02 => WriteTarget8::Register(Register8::D),
        0x03 => WriteTarget8::Register(Register8::E),
        0x04 => WriteTarget8::Register(Register8::H),
        0x05 => WriteTarget8::Register(Register8::L),
        0x06 => WriteTarget8::Indirect(Register16::HL),
        0x07 => WriteTarget8::Register(Register8::A),
        _ => unreachable!("value & 0x07 is <= 0x07"),
    }
}

fn parse_load_register_16(opcode: u8) -> Register16 {
    match opcode & 0x30 {
        0x00 => Register16::BC,
        0x10 => Register16::DE,
        0x20 => Register16::HL,
        0x30 => Register16::SP,
        _ => unreachable!("value & 0x30 is always 0x00/0x10/0x20/0x30"),
    }
}

fn parse_stack_register(opcode: u8) -> Register16 {
    match opcode & 0x30 {
        0x00 => Register16::BC,
        0x10 => Register16::DE,
        0x20 => Register16::HL,
        0x30 => Register16::AF,
        _ => unreachable!("value & 0x30 is always 0x00/0x10/0x20/0x30"),
    }
}
