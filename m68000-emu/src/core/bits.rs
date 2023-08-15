use crate::core::{
    AddressingMode, ConditionCodes, Direction, Exception, ExecuteResult, Instruction,
    InstructionExecutor, OpSize, SizedValue,
};
use crate::traits::{BusInterface, SignBit};

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
