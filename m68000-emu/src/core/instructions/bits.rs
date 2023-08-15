use crate::core::instructions::Direction;
use crate::core::{
    AddressingMode, ConditionCodes, DataRegister, Exception, ExecuteResult, Instruction,
    InstructionExecutor, OpSize, SizedValue,
};
use crate::traits::{BusInterface, GetBit, SignBit};

macro_rules! impl_bit_op {
    ($name:ident, $operator:tt) => {
        pub(super) fn $name(&mut self, size: OpSize, source: AddressingMode, dest: AddressingMode) -> ExecuteResult<()> {
            let operand_l = self.read(source, size)?;

            let dest_resolved = self.resolve_address_with_post(dest, size)?;
            let operand_r = self.read_resolved(dest_resolved, size)?;

            let value = u32::from(operand_l) $operator u32::from(operand_r);
            let value = SizedValue::from_size(value, size);

            self.registers.ccr = ConditionCodes {
                carry: false,
                overflow: false,
                zero: value.is_zero(),
                negative: value.sign_bit(),
                ..self.registers.ccr
            };

            self.write_resolved(dest_resolved, value)?;

            Ok(())
        }
    }
}

macro_rules! impl_bit_op_to_ccr {
    ($name:ident, $operator:tt) => {
        pub(super) fn $name(&mut self) -> ExecuteResult<()> {
            let byte = self.read_byte(AddressingMode::Immediate)?;
            let value = byte $operator (u8::from(self.registers.ccr));
            self.registers.ccr = value.into();

            Ok(())
        }
    }
}

macro_rules! impl_bit_op_to_sr {
    ($name:ident, $operator:tt) => {
        pub(super) fn $name(&mut self) -> ExecuteResult<()> {
            let word = self.read_word(AddressingMode::Immediate)?;
            let value = word $operator self.registers.status_register();
            self.registers.set_status_register(value);

            Ok(())
        }
    }
}

macro_rules! impl_bit_test_op {
    ($name:ident $(, |$value:ident, $bit:ident| $body:block)?) => {
        pub(super) fn $name(&mut self, source: AddressingMode, dest: AddressingMode) -> ExecuteResult<()> {
            let bit_index = self.read_byte(source)?;

            match dest {
                AddressingMode::DataDirect(register) => {
                    let value = register.read_from(self.registers);
                    let bit = bit_index % 32;
                    self.registers.ccr.zero = !value.bit(bit);
                    $(
                        let $value = value;
                        let $bit = bit;
                        let value = $body;
                        register.write_long_word_to(self.registers, value);
                    )?
                }
                _ => {
                    let dest_resolved = self.resolve_address_with_post(dest, OpSize::Byte)?;
                    let value = self.read_byte_resolved(dest_resolved);
                    let bit = bit_index % 8;
                    self.registers.ccr.zero = !value.bit(bit);
                    $(
                        let $value = value;
                        let $bit = bit;
                        let value = $body;
                        self.write_byte_resolved(dest_resolved, value);
                    )?
                }
            }

            Ok(())
        }
    }
}

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    impl_bit_op!(and, &);
    impl_bit_op!(or, |);
    impl_bit_op!(eor, ^);

    impl_bit_op_to_ccr!(andi_to_ccr, &);
    impl_bit_op_to_ccr!(ori_to_ccr, |);
    impl_bit_op_to_ccr!(eori_to_ccr, ^);

    impl_bit_op_to_sr!(andi_to_sr, &);
    impl_bit_op_to_sr!(ori_to_sr, |);
    impl_bit_op_to_sr!(eori_to_sr, ^);

    impl_bit_test_op!(btst);
    impl_bit_test_op!(bclr, |value, bit| { value & !(1 << bit) });
    impl_bit_test_op!(bset, |value, bit| { value | (1 << bit) });
    impl_bit_test_op!(bchg, |value, bit| {
        if value.bit(bit) {
            value & !(1 << bit)
        } else {
            value | (1 << bit)
        }
    });

    pub(super) fn not(&mut self, size: OpSize, dest: AddressingMode) -> ExecuteResult<()> {
        let dest_resolved = self.resolve_address_with_post(dest, size)?;
        let value: u32 = self.read_resolved(dest_resolved, size)?.into();
        let negated = SizedValue::from_size(!value, size);

        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero: negated.is_zero(),
            negative: negated.sign_bit(),
            ..self.registers.ccr
        };

        self.write_resolved(dest_resolved, negated)?;

        Ok(())
    }

    pub(super) fn clr(&mut self, size: OpSize, dest: AddressingMode) -> ExecuteResult<()> {
        let dest_resolved = self.resolve_address_with_post(dest, size)?;
        self.read_resolved(dest_resolved, size)?;

        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero: true,
            negative: false,
            ..self.registers.ccr
        };

        self.write_resolved(dest_resolved, SizedValue::from_size(0, size))?;

        Ok(())
    }

    pub(super) fn ext(&mut self, size: OpSize, register: DataRegister) {
        let (zero, sign) = match size {
            OpSize::Word => {
                let byte = register.read_from(self.registers) as u8;
                let sign_extended = byte as i8 as u16;
                register.write_word_to(self.registers, sign_extended);
                (sign_extended == 0, sign_extended.sign_bit())
            }
            OpSize::LongWord => {
                let word = register.read_from(self.registers) as u16;
                let sign_extended = word as i16 as u32;
                register.write_long_word_to(self.registers, sign_extended);
                (sign_extended == 0, sign_extended.sign_bit())
            }
            OpSize::Byte => panic!("EXT does not support size byte"),
        };

        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero,
            negative: sign,
            ..self.registers.ccr
        };
    }

    pub(super) fn swap(&mut self, register: DataRegister) {
        let [b3, b2, b1, b0] = register.read_from(self.registers).to_be_bytes();
        let value = u32::from_be_bytes([b1, b0, b3, b2]);
        register.write_long_word_to(self.registers, value);

        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero: value == 0,
            negative: value.sign_bit(),
            ..self.registers.ccr
        };
    }
}

macro_rules! impl_decode_bit_op {
    ($name:ident, $instruction:ident) => {
        pub(super) fn $name(opcode: u16) -> ExecuteResult<Instruction> {
            let register = ((opcode >> 9) & 0x07) as u8;
            let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;
            let direction = Direction::parse_from_opcode(opcode);
            let size = OpSize::parse_from_opcode(opcode)?;

            if addressing_mode.is_address_direct()
                || (direction == Direction::RegisterToMemory && !addressing_mode.is_writable())
            {
                return Err(Exception::IllegalInstruction(opcode));
            }

            let register_am = AddressingMode::DataDirect(register.into());
            let (source, dest) = match direction {
                Direction::RegisterToMemory => (register_am, addressing_mode),
                Direction::MemoryToRegister => (addressing_mode, register_am),
            };

            Ok(Instruction::$instruction { size, source, dest })
        }
    };
}

impl_decode_bit_op!(decode_and, And);
impl_decode_bit_op!(decode_or, Or);
impl_decode_bit_op!(decode_eor, ExclusiveOr);

macro_rules! impl_decode_bit_op_immediate {
    ($name:ident, $instruction:ident, $ccr_instruction:ident, $sr_instruction:ident) => {
        pub(super) fn $name(opcode: u16, supervisor_mode: bool) -> ExecuteResult<Instruction> {
            let size = OpSize::parse_from_opcode(opcode)?;
            let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;
            if addressing_mode == AddressingMode::Immediate {
                return match size {
                    OpSize::Byte => Ok(Instruction::$ccr_instruction),
                    OpSize::Word => {
                        if supervisor_mode {
                            Ok(Instruction::$sr_instruction)
                        } else {
                            Err(Exception::PrivilegeViolation)
                        }
                    }
                    _ => Err(Exception::IllegalInstruction(opcode)),
                };
            }

            if addressing_mode.is_address_direct() || !addressing_mode.is_writable() {
                return Err(Exception::IllegalInstruction(opcode));
            }

            Ok(Instruction::$instruction {
                size,
                source: AddressingMode::Immediate,
                dest: addressing_mode,
            })
        }
    };
}
impl_decode_bit_op_immediate!(decode_andi, And, AndToCcr, AndToSr);
impl_decode_bit_op_immediate!(decode_ori, Or, OrToCcr, OrToSr);
impl_decode_bit_op_immediate!(decode_eori, ExclusiveOr, ExclusiveOrToCcr, ExclusiveOrToSr);

pub(super) fn decode_not(opcode: u16) -> ExecuteResult<Instruction> {
    let size = OpSize::parse_from_opcode(opcode)?;
    let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;

    if addressing_mode.is_address_direct() || !addressing_mode.is_writable() {
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::Not(size, addressing_mode))
}

pub(super) fn decode_clr(opcode: u16) -> ExecuteResult<Instruction> {
    let size = OpSize::parse_from_opcode(opcode)?;
    let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;

    if addressing_mode.is_address_direct() || !addressing_mode.is_writable() {
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::Clear(size, addressing_mode))
}

pub(super) fn decode_ext(opcode: u16) -> Instruction {
    let register = (opcode & 0x07) as u8;
    let size = if opcode.bit(6) {
        OpSize::LongWord
    } else {
        OpSize::Word
    };

    Instruction::Extend(size, register.into())
}

pub(super) fn decode_swap(opcode: u16) -> Instruction {
    let register = (opcode & 0x07) as u8;

    Instruction::Swap(register.into())
}

macro_rules! impl_decode_single_bit_static {
    ($name:ident, $instruction:ident) => {
        pub(super) fn $name(opcode: u16) -> ExecuteResult<Instruction> {
            let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;

            Ok(Instruction::$instruction {
                source: AddressingMode::Immediate,
                dest: addressing_mode,
            })
        }
    };
}

macro_rules! impl_decode_single_bit_dynamic {
    ($name:ident, $instruction:ident) => {
        pub(super) fn $name(opcode: u16) -> ExecuteResult<Instruction> {
            let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;
            let register = ((opcode >> 9) & 0x07) as u8;

            Ok(Instruction::$instruction {
                source: AddressingMode::DataDirect(register.into()),
                dest: addressing_mode,
            })
        }
    };
}

impl_decode_single_bit_static!(decode_btst_static, BitTest);
impl_decode_single_bit_dynamic!(decode_btst_dynamic, BitTest);

impl_decode_single_bit_static!(decode_bchg_static, BitTestAndChange);
impl_decode_single_bit_dynamic!(decode_bchg_dynamic, BitTestAndChange);

impl_decode_single_bit_static!(decode_bclr_static, BitTestAndClear);
impl_decode_single_bit_dynamic!(decode_bclr_dynamic, BitTestAndClear);

impl_decode_single_bit_static!(decode_bset_static, BitTestAndSet);
impl_decode_single_bit_dynamic!(decode_bset_dynamic, BitTestAndSet);
