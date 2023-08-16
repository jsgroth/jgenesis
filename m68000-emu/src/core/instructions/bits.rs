use crate::core::instructions::{Direction, ShiftCount, ShiftDirection};
use crate::core::{
    AddressingMode, ConditionCodes, DataRegister, Exception, ExecuteResult, Instruction,
    InstructionExecutor, OpSize, SizedValue,
};
use crate::traits::{BusInterface, GetBit, SignBit};

macro_rules! impl_bit_op {
    ($name:ident, $operator:tt) => {
        pub(super) fn $name(&mut self, size: OpSize, source: AddressingMode, dest: AddressingMode) -> ExecuteResult<u32> {
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

            Ok(0)
        }
    }
}

macro_rules! impl_bit_op_to_ccr {
    ($name:ident, $operator:tt) => {
        pub(super) fn $name(&mut self) -> ExecuteResult<u32> {
            let byte = self.read_byte(AddressingMode::Immediate)?;
            let value = byte $operator (u8::from(self.registers.ccr));
            self.registers.ccr = value.into();

            Ok(0)
        }
    }
}

macro_rules! impl_bit_op_to_sr {
    ($name:ident, $operator:tt) => {
        pub(super) fn $name(&mut self) -> ExecuteResult<u32> {
            let word = self.read_word(AddressingMode::Immediate)?;
            let value = word $operator self.registers.status_register();
            self.registers.set_status_register(value);

            Ok(0)
        }
    }
}

macro_rules! impl_bit_test_op {
    ($name:ident $(, |$value:ident, $bit:ident| $body:block)?) => {
        pub(super) fn $name(&mut self, source: AddressingMode, dest: AddressingMode) -> ExecuteResult<u32> {
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

            Ok(0)
        }
    }
}

macro_rules! impl_shift_register_op {
    (
        $name:ident,
        $operator:tt,
        $signed_t:ty,
        $unsigned_t:ty,
        $register_write_method:ident,
        carry: $carry_bit:expr
        $(, set $overflow:ident)?
    ) => {
        fn $name(&mut self, register: DataRegister, count: ShiftCount) -> u32 {
            let shifts = count.get(self.registers) % 64;

            self.registers.ccr.carry = false;
            self.registers.ccr.overflow = false;

            let mut value = register.read_from(self.registers) as $signed_t;

            for _ in 0..shifts {
                let carry = value.bit($carry_bit);
                self.registers.ccr.carry = carry;
                self.registers.ccr.extend = carry;

                $(
                    if value.sign_bit() != (value $operator 1).sign_bit() {
                        self.registers.ccr.$overflow = true;
                    }
                )?

                value = value $operator 1;
            }

            register.$register_write_method(self.registers, value as $unsigned_t);

            self.registers.ccr.zero = value == 0;
            self.registers.ccr.negative = value.sign_bit();

            shifts.into()
        }
    }
}

macro_rules! impl_rotate_register_op {
    (@set_initial_carry) => {
        false
    };
    (@set_initial_carry $extend:expr) => {
        $extend
    };
    (@set_carry $carry:expr) => {
        $carry
    };
    (@set_carry $carry:expr, $extend:expr) => {
        $extend
    };
    (
        $name:ident,
        $operator:tt,
        $t:ty,
        $register_write_method:ident,
        carry: $carry_bit:expr
        $(, rotate_in: $opposite_op:tt $opposite_shift:expr)?
        $(, thru $extend:ident)?
    ) => {
        fn $name(&mut self, register: DataRegister, count: ShiftCount) -> u32 {
            let rotates = count.get(self.registers) % 64;

            self.registers.ccr.overflow = false;
            self.registers.ccr.carry = impl_rotate_register_op!(@set_initial_carry $( self.registers.ccr.$extend )?);

            let mut value = register.read_from(self.registers) as $t;
            for _ in 0..rotates {
                let carry = value.bit($carry_bit);
                let rotating_in = impl_rotate_register_op!(@set_carry carry $(, self.registers.ccr.$extend )?);

                value = (value $operator 1) | (<$t>::from(rotating_in) $( $opposite_op $opposite_shift )? );

                self.registers.ccr.carry = carry;
                $(
                    self.registers.ccr.$extend = carry;
                )?
            }

            register.$register_write_method(self.registers, value);

            self.registers.ccr.zero = value == 0;
            self.registers.ccr.negative = value.sign_bit();

            rotates.into()
        }
    };
}

#[allow(clippy::assign_op_pattern)]
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

    impl_shift_register_op!(asl_register_u8, <<, i8, u8, write_byte_to, carry: 7, set overflow);
    impl_shift_register_op!(asl_register_u16, <<, i16, u16, write_word_to, carry: 15, set overflow);
    impl_shift_register_op!(asl_register_u32, <<, i32, u32, write_long_word_to, carry: 31, set overflow);

    impl_shift_register_op!(asr_register_u8, >>, i8, u8, write_byte_to, carry: 0, set overflow);
    impl_shift_register_op!(asr_register_u16, >>, i16, u16, write_word_to, carry: 0, set overflow);
    impl_shift_register_op!(asr_register_u32, >>, i32, u32, write_long_word_to, carry: 0, set overflow);

    impl_shift_register_op!(lsl_register_u8, <<, u8, u8, write_byte_to, carry: 7);
    impl_shift_register_op!(lsl_register_u16, <<, u16, u16, write_word_to, carry: 15);
    impl_shift_register_op!(lsl_register_u32, <<, u32, u32, write_long_word_to, carry: 31);

    impl_shift_register_op!(lsr_register_u8, >>, u8, u8, write_byte_to, carry: 0);
    impl_shift_register_op!(lsr_register_u16, >>, u16, u16, write_word_to, carry: 0);
    impl_shift_register_op!(lsr_register_u32, >>, u32, u32, write_long_word_to, carry: 0);

    impl_rotate_register_op!(rol_register_u8, <<, u8, write_byte_to, carry: 7);
    impl_rotate_register_op!(rol_register_u16, <<, u16, write_word_to, carry: 15);
    impl_rotate_register_op!(rol_register_u32, <<, u32, write_long_word_to, carry: 31);

    impl_rotate_register_op!(ror_register_u8, >>, u8, write_byte_to, carry: 0, rotate_in: << 7);
    impl_rotate_register_op!(ror_register_u16, >>, u16, write_word_to, carry: 0, rotate_in: << 15);
    impl_rotate_register_op!(ror_register_u32, >>, u32, write_long_word_to, carry: 0, rotate_in: << 31);

    impl_rotate_register_op!(roxl_register_u8, <<, u8, write_byte_to, carry: 7, thru extend);
    impl_rotate_register_op!(roxl_register_u16, <<, u16, write_word_to, carry: 15, thru extend);
    impl_rotate_register_op!(roxl_register_u32, <<, u32, write_long_word_to, carry: 31, thru extend);

    impl_rotate_register_op!(roxr_register_u8, >>, u8, write_byte_to, carry: 0, rotate_in: << 7, thru extend);
    impl_rotate_register_op!(roxr_register_u16, >>, u16, write_word_to, carry: 0, rotate_in: << 15, thru extend);
    impl_rotate_register_op!(roxr_register_u32, >>, u32, write_long_word_to, carry: 0, rotate_in: << 31, thru extend);

    pub(super) fn not(&mut self, size: OpSize, dest: AddressingMode) -> ExecuteResult<u32> {
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

        Ok(0)
    }

    pub(super) fn clr(&mut self, size: OpSize, dest: AddressingMode) -> ExecuteResult<u32> {
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

        Ok(0)
    }

    pub(super) fn ext(&mut self, size: OpSize, register: DataRegister) -> u32 {
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

        4
    }

    pub(super) fn swap(&mut self, register: DataRegister) -> u32 {
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

        4
    }

    pub(super) fn asd_register(
        &mut self,
        size: OpSize,
        direction: ShiftDirection,
        register: DataRegister,
        count: ShiftCount,
    ) -> u32 {
        let shifts = match (size, direction) {
            (OpSize::Byte, ShiftDirection::Left) => self.asl_register_u8(register, count),
            (OpSize::Byte, ShiftDirection::Right) => self.asr_register_u8(register, count),
            (OpSize::Word, ShiftDirection::Left) => self.asl_register_u16(register, count),
            (OpSize::Word, ShiftDirection::Right) => self.asr_register_u16(register, count),
            (OpSize::LongWord, ShiftDirection::Left) => self.asl_register_u32(register, count),
            (OpSize::LongWord, ShiftDirection::Right) => self.asr_register_u32(register, count),
        };

        shift_register_cycles(size, shifts)
    }

    pub(super) fn asd_memory(
        &mut self,
        direction: ShiftDirection,
        dest: AddressingMode,
    ) -> ExecuteResult<u32> {
        let dest_resolved = self.resolve_address_with_post(dest, OpSize::Word)?;
        let original = self.read_word_resolved(dest_resolved)?;

        let (value, carry) = match direction {
            ShiftDirection::Left => (original << 1, original.bit(15)),
            ShiftDirection::Right => ((original >> 1) | (original & 0x8000), original.bit(0)),
        };

        let overflow = original.sign_bit() != value.sign_bit();

        self.registers.ccr = ConditionCodes {
            carry,
            overflow,
            zero: value == 0,
            negative: value.sign_bit(),
            extend: carry,
        };

        self.write_word_resolved(dest_resolved, value)?;

        Ok(0)
    }

    pub(super) fn lsd_register(
        &mut self,
        size: OpSize,
        direction: ShiftDirection,
        register: DataRegister,
        count: ShiftCount,
    ) -> u32 {
        let shifts = match (size, direction) {
            (OpSize::Byte, ShiftDirection::Left) => self.lsl_register_u8(register, count),
            (OpSize::Byte, ShiftDirection::Right) => self.lsr_register_u8(register, count),
            (OpSize::Word, ShiftDirection::Left) => self.lsl_register_u16(register, count),
            (OpSize::Word, ShiftDirection::Right) => self.lsr_register_u16(register, count),
            (OpSize::LongWord, ShiftDirection::Left) => self.lsl_register_u32(register, count),
            (OpSize::LongWord, ShiftDirection::Right) => self.lsr_register_u32(register, count),
        };

        shift_register_cycles(size, shifts)
    }

    pub(super) fn lsd_memory(
        &mut self,
        direction: ShiftDirection,
        dest: AddressingMode,
    ) -> ExecuteResult<u32> {
        let dest_resolved = self.resolve_address_with_post(dest, OpSize::Word)?;
        let original = self.read_word_resolved(dest_resolved)?;

        let (value, carry) = match direction {
            ShiftDirection::Left => (original << 1, original.bit(15)),
            ShiftDirection::Right => (original >> 1, original.bit(0)),
        };

        self.registers.ccr = ConditionCodes {
            carry,
            overflow: false,
            zero: value == 0,
            negative: value.sign_bit(),
            extend: carry,
        };

        self.write_word_resolved(dest_resolved, value)?;

        Ok(0)
    }

    pub(super) fn rod_register(
        &mut self,
        size: OpSize,
        direction: ShiftDirection,
        register: DataRegister,
        count: ShiftCount,
    ) -> u32 {
        let rotates = match (size, direction) {
            (OpSize::Byte, ShiftDirection::Left) => self.rol_register_u8(register, count),
            (OpSize::Byte, ShiftDirection::Right) => self.ror_register_u8(register, count),
            (OpSize::Word, ShiftDirection::Left) => self.rol_register_u16(register, count),
            (OpSize::Word, ShiftDirection::Right) => self.ror_register_u16(register, count),
            (OpSize::LongWord, ShiftDirection::Left) => self.rol_register_u32(register, count),
            (OpSize::LongWord, ShiftDirection::Right) => self.ror_register_u32(register, count),
        };

        shift_register_cycles(size, rotates)
    }

    pub(super) fn rod_memory(
        &mut self,
        direction: ShiftDirection,
        dest: AddressingMode,
    ) -> ExecuteResult<u32> {
        let dest_resolved = self.resolve_address_with_post(dest, OpSize::Word)?;
        let original = self.read_word_resolved(dest_resolved)?;

        let (value, carry) = match direction {
            ShiftDirection::Left => (
                (original << 1) | u16::from(original.sign_bit()),
                original.sign_bit(),
            ),
            ShiftDirection::Right => (
                (original >> 1) | (u16::from(original.bit(0)) << 15),
                original.bit(0),
            ),
        };

        self.registers.ccr = ConditionCodes {
            carry,
            overflow: false,
            zero: value == 0,
            negative: value.sign_bit(),
            ..self.registers.ccr
        };

        self.write_word_resolved(dest_resolved, value)?;

        Ok(0)
    }

    pub(super) fn roxd_register(
        &mut self,
        size: OpSize,
        direction: ShiftDirection,
        register: DataRegister,
        count: ShiftCount,
    ) -> u32 {
        let rotates = match (size, direction) {
            (OpSize::Byte, ShiftDirection::Left) => self.roxl_register_u8(register, count),
            (OpSize::Byte, ShiftDirection::Right) => self.roxr_register_u8(register, count),
            (OpSize::Word, ShiftDirection::Left) => self.roxl_register_u16(register, count),
            (OpSize::Word, ShiftDirection::Right) => self.roxr_register_u16(register, count),
            (OpSize::LongWord, ShiftDirection::Left) => self.roxl_register_u32(register, count),
            (OpSize::LongWord, ShiftDirection::Right) => self.roxr_register_u32(register, count),
        };

        shift_register_cycles(size, rotates)
    }

    pub(super) fn roxd_memory(
        &mut self,
        direction: ShiftDirection,
        dest: AddressingMode,
    ) -> ExecuteResult<u32> {
        let dest_resolved = self.resolve_address_with_post(dest, OpSize::Word)?;
        let original = self.read_word_resolved(dest_resolved)?;

        let extend = self.registers.ccr.extend;
        let (value, carry) = match direction {
            ShiftDirection::Left => ((original << 1) | u16::from(extend), original.bit(15)),
            ShiftDirection::Right => ((original >> 1) | (u16::from(extend) << 15), original.bit(0)),
        };

        self.registers.ccr = ConditionCodes {
            carry,
            overflow: false,
            zero: value == 0,
            negative: value.sign_bit(),
            extend: carry,
        };

        self.write_word_resolved(dest_resolved, value)?;

        Ok(0)
    }

    pub(super) fn tst(&mut self, size: OpSize, source: AddressingMode) -> ExecuteResult<u32> {
        let value = self.read(source, size)?;

        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero: value.is_zero(),
            negative: value.sign_bit(),
            ..self.registers.ccr
        };

        Ok(0)
    }

    pub(super) fn tas(&mut self, dest: AddressingMode) -> ExecuteResult<u32> {
        let dest_resolved = self.resolve_address_with_post(dest, OpSize::Byte)?;
        let value = self.read_byte_resolved(dest_resolved);

        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero: value == 0,
            negative: value.sign_bit(),
            ..self.registers.ccr
        };

        self.write_byte_resolved(dest_resolved, value | 0x80);

        Ok(0)
    }
}

fn shift_register_cycles(size: OpSize, shifts: u32) -> u32 {
    let base_cycles = match size {
        OpSize::Byte | OpSize::Word => 6,
        OpSize::LongWord => 8,
    };
    base_cycles + 2 * shifts
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

macro_rules! impl_decode_shift_memory {
    ($name:ident, $instruction:ident) => {
        pub(super) fn $name(opcode: u16) -> ExecuteResult<Instruction> {
            let direction = ShiftDirection::parse_from_opcode(opcode);
            let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;

            Ok(Instruction::$instruction(direction, addressing_mode))
        }
    };
}

impl_decode_shift_memory!(decode_asd_memory, ArithmeticShiftMemory);
impl_decode_shift_memory!(decode_lsd_memory, LogicalShiftMemory);
impl_decode_shift_memory!(decode_rod_memory, RotateMemory);
impl_decode_shift_memory!(decode_roxd_memory, RotateThruExtendMemory);

macro_rules! impl_decode_shift_register {
    ($name:ident, $instruction:ident) => {
        pub(super) fn $name(opcode: u16) -> ExecuteResult<Instruction> {
            let size = OpSize::parse_from_opcode(opcode)?;
            let direction = ShiftDirection::parse_from_opcode(opcode);
            let register = (opcode & 0x07) as u8;
            let count = ShiftCount::parse_from_opcode(opcode);

            Ok(Instruction::$instruction(
                size,
                direction,
                register.into(),
                count,
            ))
        }
    };
}

impl_decode_shift_register!(decode_asd_register, ArithmeticShiftRegister);
impl_decode_shift_register!(decode_lsd_register, LogicalShiftRegister);
impl_decode_shift_register!(decode_rod_register, RotateRegister);
impl_decode_shift_register!(decode_roxd_register, RotateThruExtendRegister);

pub(super) fn decode_tst(opcode: u16) -> ExecuteResult<Instruction> {
    let size = OpSize::parse_from_opcode(opcode)?;
    let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;

    Ok(Instruction::Test(size, addressing_mode))
}

pub(super) fn decode_tas(opcode: u16) -> ExecuteResult<Instruction> {
    let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;

    if addressing_mode.is_address_direct() || !addressing_mode.is_writable() {
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::TestAndSet(addressing_mode))
}
