use crate::core::instructions::{
    GetBit, Instruction, ModifyTarget8, ReadTarget16, ReadTarget8, TransferMode, WriteTarget16,
    WriteTarget8,
};
use crate::core::{IndexRegister, InterruptMode, Register16, Register8, Registers};
use crate::traits::AddressSpace;

#[derive(Debug, Clone, Copy)]
pub struct ParseResult {
    instruction: Instruction,
    t_cycles: u32,
}

#[derive(Debug)]
pub struct InstructionParser<'registers, 'address_space, A> {
    registers: &'registers mut Registers,
    address_space: &'address_space mut A,
    t_cycles: u32,
    index: Option<IndexRegister>,
}

impl<'registers, 'address_space, A: AddressSpace> InstructionParser<'registers, 'address_space, A> {
    const OPCODE_T_CYCLES: u32 = 4;
    const OPERAND_T_CYCLES: u32 = 3;

    pub fn new(registers: &'registers mut Registers, address_space: &'address_space mut A) -> Self {
        Self {
            registers,
            address_space,
            t_cycles: 0,
            index: None,
        }
    }

    fn fetch_opcode_byte(&mut self) -> u8 {
        let byte = self.address_space.read(self.registers.pc);
        self.registers.pc = self.registers.pc.wrapping_add(1);
        self.t_cycles += Self::OPCODE_T_CYCLES;

        byte
    }

    fn fetch_operand_byte(&mut self) -> u8 {
        let byte = self.address_space.read(self.registers.pc);
        self.registers.pc = self.registers.pc.wrapping_add(1);
        self.t_cycles += Self::OPERAND_T_CYCLES;

        byte
    }

    fn fetch_u16_operand(&mut self) -> u16 {
        let operand_lsb = self.fetch_operand_byte();
        let operand_msb = self.fetch_operand_byte();
        u16::from_le_bytes([operand_lsb, operand_msb])
    }

    fn parse_index_prefix(&mut self) -> u8 {
        loop {
            let opcode = self.fetch_opcode_byte();
            match opcode {
                0xDD => {
                    self.index = Some(IndexRegister::IX);
                }
                0xFD => {
                    self.index = Some(IndexRegister::IY);
                }
                _ => {
                    return opcode;
                }
            }
        }
    }

    fn index_or_hl(&self) -> Register16 {
        self.index.map_or(Register16::HL, IndexRegister::into)
    }

    fn parse_read_target_8(&mut self, opcode: u8) -> ReadTarget8 {
        match opcode & 0x07 {
            0x00 => ReadTarget8::Register(Register8::B),
            0x01 => ReadTarget8::Register(Register8::C),
            0x02 => ReadTarget8::Register(Register8::D),
            0x03 => ReadTarget8::Register(Register8::E),
            0x04 => ReadTarget8::Register(Register8::H),
            0x05 => ReadTarget8::Register(Register8::L),
            0x06 => match self.index {
                Some(index) => {
                    let d = self.fetch_operand_byte() as i8;
                    ReadTarget8::Indexed(index, d)
                }
                None => ReadTarget8::Indirect(Register16::HL),
            },
            0x07 => ReadTarget8::Register(Register8::A),
            _ => unreachable!("value & 0x07 is <= 0x07"),
        }
    }

    fn parse_write_target_8(&mut self, opcode: u8) -> WriteTarget8 {
        match opcode & 0x07 {
            0x00 => WriteTarget8::Register(Register8::B),
            0x01 => WriteTarget8::Register(Register8::C),
            0x02 => WriteTarget8::Register(Register8::D),
            0x03 => WriteTarget8::Register(Register8::E),
            0x04 => WriteTarget8::Register(Register8::H),
            0x05 => WriteTarget8::Register(Register8::L),
            0x06 => match self.index {
                Some(index) => {
                    let d = self.fetch_operand_byte() as i8;
                    WriteTarget8::Indexed(index, d)
                }
                None => WriteTarget8::Indirect(Register16::HL),
            },
            0x07 => WriteTarget8::Register(Register8::A),
            _ => unreachable!("value & 0x07 is <= 0x07"),
        }
    }

    fn parse_modify_target_8(&mut self, opcode: u8) -> ModifyTarget8 {
        match opcode & 0x07 {
            0x00 => ModifyTarget8::Register(Register8::B),
            0x01 => ModifyTarget8::Register(Register8::C),
            0x02 => ModifyTarget8::Register(Register8::D),
            0x03 => ModifyTarget8::Register(Register8::E),
            0x04 => ModifyTarget8::Register(Register8::H),
            0x05 => ModifyTarget8::Register(Register8::L),
            0x06 => match self.index {
                Some(index) => {
                    let d = self.fetch_operand_byte() as i8;
                    ModifyTarget8::Indexed(index, d)
                }
                None => ModifyTarget8::Indirect(Register16::HL),
            },
            0x07 => ModifyTarget8::Register(Register8::A),
            _ => unreachable!("value & 0x07 is <= 0x07"),
        }
    }

    fn parse_register_16(&self, opcode: u8, x30_register: Register16) -> Register16 {
        match opcode & 0x30 {
            0x00 => Register16::BC,
            0x10 => Register16::DE,
            0x20 => match self.index {
                Some(index) => index.into(),
                None => Register16::HL,
            },
            0x30 => x30_register,
            _ => unreachable!("value & 0x30 is always 0x00/0x10/0x20/0x30"),
        }
    }

    fn parse_load_register_16(&self, opcode: u8) -> Register16 {
        self.parse_register_16(opcode, Register16::SP)
    }

    fn parse_stack_register(&self, opcode: u8) -> Register16 {
        self.parse_register_16(opcode, Register16::AF)
    }

    fn parse_cb_prefix_instruction(&mut self) -> Instruction {
        // If this opcode was prefixed by DD or FD, the indexing offset comes before the second
        // opcode byte
        let x06_modify_target = match self.index {
            Some(index) => {
                let d = self.fetch_operand_byte() as i8;
                ModifyTarget8::Indexed(index, d)
            }
            None => ModifyTarget8::Indirect(Register16::HL),
        };

        let opcode_2 = self.fetch_opcode_byte();

        let target_register = match opcode_2 & 0x07 {
            0x00 => Some(Register8::B),
            0x01 => Some(Register8::C),
            0x02 => Some(Register8::D),
            0x03 => Some(Register8::E),
            0x04 => Some(Register8::H),
            0x05 => Some(Register8::L),
            0x06 => None,
            0x07 => Some(Register8::A),
            _ => unreachable!("value & 0x07 is always <= 0x07"),
        };

        let (modify_target, side_effect) = match (x06_modify_target, target_register) {
            (ModifyTarget8::Indirect(..), Some(register)) => {
                (ModifyTarget8::Register(register), None)
            }
            (ModifyTarget8::Indexed(..), Some(register)) => (x06_modify_target, Some(register)),
            (_, None) => (x06_modify_target, None),
            (ModifyTarget8::Register(..), Some(..)) => {
                unreachable!("x06_modify_target is indexed or indirect")
            }
        };

        match opcode_2 {
            0x00..=0x07 => Instruction::RotateLeft {
                modify_target,
                thru_carry: false,
                side_effect,
            },
            0x08..=0x0F => Instruction::RotateRight {
                modify_target,
                thru_carry: false,
                side_effect,
            },
            0x10..=0x17 => Instruction::RotateLeft {
                modify_target,
                thru_carry: true,
                side_effect,
            },
            0x18..=0x1F => Instruction::RotateRight {
                modify_target,
                thru_carry: true,
                side_effect,
            },
            0x20..=0x27 => Instruction::ShiftLeft {
                modify_target,
                side_effect,
            },
            0x28..=0x2F => Instruction::ShiftRightArithmetic {
                modify_target,
                side_effect,
            },
            0x38..=0x3F => Instruction::ShiftRightLogical {
                modify_target,
                side_effect,
            },
            0x40..=0x7F => {
                let bit = (opcode_2 >> 3) & 0x07;
                Instruction::TestBit(modify_target, bit)
            }
            0x80..=0xFF => {
                let bit = (opcode_2 >> 3) & 0x07;
                let value = opcode_2.bit(6);
                Instruction::SetBit {
                    modify_target,
                    bit,
                    value,
                    side_effect,
                }
            }
            _ => panic!("CB-prefixed opcode not implemented yet: {opcode_2:02X}"),
        }
    }

    fn parse_ed_prefix_instruction(&mut self) -> Instruction {
        use ReadTarget16 as RT16;
        use ReadTarget8 as RT8;
        use Register16 as R16;
        use Register8 as R8;
        use WriteTarget16 as WT16;
        use WriteTarget8 as WT8;

        // ED-prefixed instructions ignore any DD or FD prefixes
        self.index = None;

        let opcode_2 = self.fetch_opcode_byte();

        match opcode_2 {
            0x03 | 0x13 | 0x23 | 0x33 => {
                let register = self.parse_load_register_16(opcode_2);
                let nn = self.fetch_u16_operand();
                Instruction::Load16(WT16::Direct(nn), RT16::Register(register))
            }
            0x44 => Instruction::NegateAccumulator,
            0x47 => {
                self.t_cycles += 1;
                Instruction::Load8(WT8::Register(R8::I), RT8::Register(R8::A))
            }
            0x42 | 0x52 | 0x62 | 0x72 => {
                let read = self.parse_load_register_16(opcode_2);
                Instruction::Subtract16 {
                    write: R16::HL,
                    read,
                }
            }
            0x4A | 0x5A | 0x6A | 0x7A => {
                let read = self.parse_load_register_16(opcode_2);
                Instruction::Add16 {
                    write: R16::HL,
                    read,
                    with_carry: true,
                }
            }
            0x4B | 0x5B | 0x6B | 0x7B => {
                let register = self.parse_load_register_16(opcode_2);
                let nn = self.fetch_u16_operand();
                Instruction::Load16(WT16::Register(register), RT16::Direct(nn))
            }
            0x4F => {
                // TODO do this t_cycle increment in ReadTarget/WriteTarget instead?
                self.t_cycles += 1;
                Instruction::Load8(WT8::Register(R8::R), RT8::Register(R8::A))
            }
            0x46 => Instruction::SetInterruptMode(InterruptMode::Mode0),
            0x56 => Instruction::SetInterruptMode(InterruptMode::Mode1),
            0x57 => {
                // TODO do this t_cycle increment in ReadTarget/WriteTarget instead?
                self.t_cycles += 1;
                Instruction::Load8(WT8::Register(R8::A), RT8::Register(R8::I))
            }
            0x5E => Instruction::SetInterruptMode(InterruptMode::Mode2),
            0x5F => {
                // TODO do this t_cycle increment in ReadTarget/WriteTarget instead?
                self.t_cycles += 1;
                Instruction::Load8(WT8::Register(R8::A), RT8::Register(R8::R))
            }
            0x67 => Instruction::RotateRight12(ModifyTarget8::Indirect(R16::HL)),
            0x6F => Instruction::RotateLeft12(ModifyTarget8::Indirect(R16::HL)),
            0xA0 => Instruction::Transfer(TransferMode::Increment { repeat: false }),
            0xA1 => Instruction::CompareBlock(TransferMode::Increment { repeat: false }),
            0xA8 => Instruction::Transfer(TransferMode::Decrement { repeat: false }),
            0xA9 => Instruction::CompareBlock(TransferMode::Decrement { repeat: false }),
            0xB0 => Instruction::Transfer(TransferMode::Increment { repeat: true }),
            0xB1 => Instruction::CompareBlock(TransferMode::Increment { repeat: true }),
            0xB8 => Instruction::Transfer(TransferMode::Decrement { repeat: true }),
            0xB9 => Instruction::CompareBlock(TransferMode::Decrement { repeat: true }),
            _ => panic!("ED-prefixed opcode not implemented: {opcode_2:02X}"),
        }
    }

    pub fn parse(mut self) -> ParseResult {
        use ReadTarget16 as RT16;
        use ReadTarget8 as RT8;
        use Register16 as R16;
        use Register8 as R8;
        use WriteTarget16 as WT16;
        use WriteTarget8 as WT8;

        let opcode = self.parse_index_prefix();

        let instruction = match opcode {
            0x00 => Instruction::NoOp,
            0x01 | 0x11 | 0x21 | 0x31 => {
                let register = self.parse_load_register_16(opcode);
                let nn = self.fetch_u16_operand();
                Instruction::Load16(WT16::Register(register), RT16::Immediate(nn))
            }
            0x02 => Instruction::Load8(WT8::Indirect(R16::BC), RT8::Register(R8::A)),
            0x03 | 0x13 | 0x23 | 0x33 => {
                let register = self.parse_load_register_16(opcode);
                Instruction::IncrementRegister16(register)
            }
            0x04 | 0x0C | 0x14 | 0x1C | 0x24 | 0x2C | 0x34 | 0x3C => {
                let modify_target = self.parse_modify_target_8(opcode);
                Instruction::Increment(modify_target)
            }
            0x05 | 0x0D | 0x15 | 0x1D | 0x25 | 0x2D | 0x35 | 0x3D => {
                let modify_target = self.parse_modify_target_8(opcode);
                Instruction::Decrement(modify_target)
            }
            0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x36 | 0x3E => {
                let write_target = self.parse_write_target_8(opcode >> 3);
                let n = self.fetch_operand_byte();
                Instruction::Load8(write_target, RT8::Immediate(n))
            }
            0x07 => Instruction::RotateLeft {
                modify_target: ModifyTarget8::Register(R8::A),
                thru_carry: false,
                side_effect: None,
            },
            0x08 => Instruction::ExchangeAF,
            0x09 | 0x19 | 0x29 | 0x39 => {
                let write = self.index_or_hl();
                let read = self.parse_load_register_16(opcode);
                Instruction::Add16 {
                    write,
                    read,
                    with_carry: false,
                }
            }
            0x0A => Instruction::Load8(WT8::Register(R8::A), RT8::Indirect(R16::BC)),
            0x0B | 0x1B | 0x2B | 0x3B => {
                let register = self.parse_load_register_16(opcode);
                Instruction::DecrementRegister16(register)
            }
            0x0F => Instruction::RotateRight {
                modify_target: ModifyTarget8::Register(R8::A),
                thru_carry: false,
                side_effect: None,
            },
            0x12 => Instruction::Load8(WT8::Indirect(R16::DE), RT8::Register(R8::A)),
            0x17 => Instruction::RotateLeft {
                modify_target: ModifyTarget8::Register(R8::A),
                thru_carry: true,
                side_effect: None,
            },
            0x1A => Instruction::Load8(WT8::Register(R8::A), RT8::Indirect(R16::DE)),
            0x1F => Instruction::RotateRight {
                modify_target: ModifyTarget8::Register(R8::A),
                thru_carry: true,
                side_effect: None,
            },
            0x22 => {
                let nn = self.fetch_u16_operand();
                Instruction::Load16(WT16::Direct(nn), RT16::Register(R16::HL))
            }
            0x27 => Instruction::DecimalAdjustAccumulator,
            0x2A => {
                let nn = self.fetch_u16_operand();
                Instruction::Load16(WT16::Register(R16::HL), RT16::Direct(nn))
            }
            0x2F => Instruction::ComplementAccumulator,
            0x32 => {
                let nn = self.fetch_u16_operand();
                Instruction::Load8(WT8::Direct(nn), RT8::Register(R8::A))
            }
            0x37 => Instruction::SetCarry,
            0x3A => {
                let nn = self.fetch_u16_operand();
                Instruction::Load8(WT8::Register(R8::A), RT8::Direct(nn))
            }
            0x3F => Instruction::ComplementCarry,
            0x40..=0x7F => {
                if opcode == 0x76 {
                    Instruction::Halt
                } else {
                    let write_target = self.parse_write_target_8(opcode >> 3);
                    let read_target = self.parse_read_target_8(opcode);

                    Instruction::Load8(write_target, read_target)
                }
            }
            0x80..=0x8F => {
                let read_target = self.parse_read_target_8(opcode);
                let with_carry = opcode.bit(3);
                Instruction::Add {
                    read_target,
                    with_carry,
                }
            }
            0x90..=0x9F => {
                let read_target = self.parse_read_target_8(opcode);
                let with_carry = opcode.bit(3);
                Instruction::Subtract {
                    read_target,
                    with_carry,
                }
            }
            0xA0..=0xA7 => {
                let read_target = self.parse_read_target_8(opcode);
                Instruction::And(read_target)
            }
            0xA8..=0xAF => {
                let read_target = self.parse_read_target_8(opcode);
                Instruction::Xor(read_target)
            }
            0xB0..=0xB7 => {
                let read_target = self.parse_read_target_8(opcode);
                Instruction::Or(read_target)
            }
            0xB8..=0xBF => {
                let read_target = self.parse_read_target_8(opcode);
                Instruction::Compare(read_target)
            }
            0xC1 | 0xD1 | 0xE1 | 0xF1 => {
                let register = self.parse_stack_register(opcode);
                Instruction::Pop(register)
            }
            0xC5 | 0xD5 | 0xE5 | 0xF5 => {
                let register = self.parse_stack_register(opcode);
                Instruction::Push(register)
            }
            0xC6 | 0xCE => {
                let n = self.fetch_operand_byte();
                let with_carry = opcode.bit(3);
                Instruction::Add {
                    read_target: RT8::Immediate(n),
                    with_carry,
                }
            }
            0xD6 | 0xDE => {
                let n = self.fetch_operand_byte();
                let with_carry = opcode.bit(3);
                Instruction::Subtract {
                    read_target: RT8::Immediate(n),
                    with_carry,
                }
            }
            0xD9 => Instruction::ExchangeGeneralPurpose,
            0xE3 => Instruction::ExchangeStack(self.index_or_hl()),
            0xE6 => {
                let n = self.fetch_operand_byte();
                Instruction::And(RT8::Immediate(n))
            }
            0xEB => Instruction::ExchangeDEHL,
            0xED => self.parse_ed_prefix_instruction(),
            0xEE => {
                let n = self.fetch_operand_byte();
                Instruction::Xor(RT8::Immediate(n))
            }
            0xF3 => Instruction::DisableInterrupts,
            0xF6 => {
                let n = self.fetch_operand_byte();
                Instruction::Or(RT8::Immediate(n))
            }
            0xF9 => {
                Instruction::Load16(WT16::Register(R16::SP), RT16::Register(self.index_or_hl()))
            }
            0xFB => Instruction::EnableInterrupts,
            0xFE => {
                let n = self.fetch_operand_byte();
                Instruction::Compare(RT8::Immediate(n))
            }
            _ => panic!("opcode not implemented yet: {opcode:02X}"),
        };

        ParseResult {
            instruction,
            t_cycles: self.t_cycles,
        }
    }
}
