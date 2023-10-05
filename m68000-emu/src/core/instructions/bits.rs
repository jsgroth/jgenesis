use crate::core::instructions::{ShiftCount, ShiftDirection};
use crate::core::{
    AddressingMode, ConditionCodes, DataRegister, Exception, ExecuteResult, InstructionExecutor,
    OpSize,
};
use crate::traits::BusInterface;
use jgenesis_traits::num::{GetBit, SignBit};

macro_rules! impl_bit_op {
    ($name:ident, $operator:tt, $read_method:ident, $read_resolved_method:ident, $write_method:ident, $size:expr) => {
        pub(super) fn $name(&mut self, source: AddressingMode, dest: AddressingMode) -> ExecuteResult<u32> {
            let operand_l = self.$read_method(source)?;

            let dest_resolved = self.resolve_address_with_post(dest, $size)?;
            let operand_r = self.$read_resolved_method(dest_resolved)?;

            let value = operand_l $operator operand_r;

            self.registers.ccr = ConditionCodes {
                carry: false,
                overflow: false,
                zero: value == 0,
                negative: value.sign_bit(),
                ..self.registers.ccr
            };

            self.$write_method(dest_resolved, value)?;

            Ok(super::binary_op_cycles($size, source, dest))
        }
    }
}

macro_rules! impl_bit_op_all_sizes {
    ($byte_name:ident, $word_name:ident, $long_word_name:ident, $operator:tt) => {
        impl_bit_op!(
            $byte_name,
            $operator,
            read_byte,
            read_byte_resolved_as_result,
            write_byte_resolved_as_result,
            OpSize::Byte
        );
        impl_bit_op!(
            $word_name,
            $operator,
            read_word,
            read_word_resolved,
            write_word_resolved,
            OpSize::Word
        );
        impl_bit_op!(
            $long_word_name,
            $operator,
            read_long_word,
            read_long_word_resolved,
            write_long_word_resolved,
            OpSize::LongWord
        );
    };
}

macro_rules! impl_bit_op_to_ccr {
    ($name:ident, $operator:tt) => {
        pub(super) fn $name(&mut self) -> ExecuteResult<u32> {
            let byte = self.read_byte(AddressingMode::Immediate)?;
            let value = byte $operator (u8::from(self.registers.ccr));
            self.registers.ccr = value.into();

            Ok(20)
        }
    }
}

macro_rules! impl_bit_op_to_sr {
    ($name:ident, $operator:tt) => {
        pub(super) fn $name(&mut self) -> ExecuteResult<u32> {
            if !self.registers.supervisor_mode {
                return Err(Exception::PrivilegeViolation);
            }

            let word = self.read_word(AddressingMode::Immediate)?;
            let value = word $operator self.registers.status_register();
            self.registers.set_status_register(value);

            Ok(20)
        }
    }
}

macro_rules! impl_not {
    ($name:ident, $read_method:ident, $write_method:ident, $size:expr) => {
        pub(super) fn $name(&mut self, dest: AddressingMode) -> ExecuteResult<u32> {
            let dest_resolved = self.resolve_address_with_post(dest, $size)?;
            let value = self.$read_method(dest_resolved)?;
            let negated = !value;

            self.registers.ccr = ConditionCodes {
                carry: false,
                overflow: false,
                zero: negated == 0,
                negative: negated.sign_bit(),
                ..self.registers.ccr
            };

            self.$write_method(dest_resolved, negated)?;

            Ok(super::unary_op_cycles($size, dest))
        }
    };
}

macro_rules! impl_clr {
    ($name:ident, $read_method:ident, $write_method:ident, $size:expr) => {
        pub(super) fn $name(&mut self, dest: AddressingMode) -> ExecuteResult<u32> {
            let dest_resolved = self.resolve_address_with_post(dest, $size)?;
            // No-op read
            self.$read_method(dest_resolved)?;

            self.registers.ccr = ConditionCodes {
                carry: false,
                overflow: false,
                zero: true,
                negative: false,
                ..self.registers.ccr
            };

            self.$write_method(dest_resolved, 0)?;

            Ok(super::unary_op_cycles($size, dest))
        }
    };
}

macro_rules! impl_bit_test_op {
    (@cycle_count $source:expr, $dest:expr, $bit:expr $(, $extra_d_write_cycles:expr)?) => {
        {
            let dest_cycles = if $dest.is_data_direct() {
                2 $(+ $extra_d_write_cycles)? + if $bit % 32 < 16 { 0 } else { 2 }
            } else {
                4 + $dest.address_calculation_cycles(OpSize::Byte)
            };

            let source_cycles = match $source {
                AddressingMode::Immediate => 4,
                _ => 0
            };

            4 + source_cycles + dest_cycles
        }
    };
    ($name:ident, |$value:ident, $bit:ident| $body:block $(, extra_d_write_cycles: $extra_d_write_cycles:expr)?) => {
        pub(super) fn $name(&mut self, source: AddressingMode, dest: AddressingMode) -> ExecuteResult<u32> {
            let bit_index = self.read_byte(source)?;

            match dest {
                AddressingMode::DataDirect(register) => {
                    let $value = register.read_from(self.registers);
                    let $bit = bit_index % 32;
                    self.registers.ccr.zero = !$value.bit($bit);

                    let value = $body;
                    register.write_long_word_to(self.registers, value);
                }
                _ => {
                    let dest_resolved = self.resolve_address_with_post(dest, OpSize::Byte)?;
                    let $value = self.read_byte_resolved(dest_resolved);
                    let $bit = bit_index % 8;
                    self.registers.ccr.zero = !$value.bit($bit);

                    let value = $body;
                    self.write_byte_resolved(dest_resolved, value);
                }
            }

            let cycles = impl_bit_test_op!(@cycle_count source, dest, bit_index $(, $extra_d_write_cycles)?);
            Ok(cycles)
        }
    };
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

macro_rules! impl_tst {
    ($name:ident, $read_method:ident, $size:expr) => {
        pub(super) fn $name(&mut self, source: AddressingMode) -> ExecuteResult<u32> {
            let value = self.$read_method(source)?;

            self.registers.ccr = ConditionCodes {
                carry: false,
                overflow: false,
                zero: value == 0,
                negative: value.sign_bit(),
                ..self.registers.ccr
            };

            Ok(4 + source.address_calculation_cycles($size))
        }
    };
}

#[allow(clippy::assign_op_pattern)]
impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    impl_bit_op_all_sizes!(and_byte, and_word, and_long_word, &);
    impl_bit_op_all_sizes!(or_byte, or_word, or_long_word, |);
    impl_bit_op_all_sizes!(eor_byte, eor_word, eor_long_word, ^);

    impl_bit_op_to_ccr!(andi_to_ccr, &);
    impl_bit_op_to_ccr!(ori_to_ccr, |);
    impl_bit_op_to_ccr!(eori_to_ccr, ^);

    impl_bit_op_to_sr!(andi_to_sr, &);
    impl_bit_op_to_sr!(ori_to_sr, |);
    impl_bit_op_to_sr!(eori_to_sr, ^);

    pub(super) fn btst(
        &mut self,
        source: AddressingMode,
        dest: AddressingMode,
    ) -> ExecuteResult<u32> {
        let bit_index = self.read_byte(source)?;

        match dest {
            AddressingMode::DataDirect(register) => {
                let value = register.read_from(self.registers);
                let bit = bit_index % 32;
                self.registers.ccr.zero = !value.bit(bit);
            }
            _ => {
                let dest_resolved = self.resolve_address_with_post(dest, OpSize::Byte)?;
                let value = self.read_byte_resolved(dest_resolved);
                let bit = bit_index % 8;
                self.registers.ccr.zero = !value.bit(bit);
            }
        }

        let dest_cycles = match dest {
            AddressingMode::DataDirect(..) => 2,
            AddressingMode::Immediate => 6,
            _ => dest.address_calculation_cycles(OpSize::Byte),
        };

        let source_cycles = match source {
            AddressingMode::Immediate => 4,
            _ => 0,
        };

        Ok(4 + source_cycles + dest_cycles)
    }

    impl_bit_test_op!(bclr, |value, bit| { value & !(1 << bit) }, extra_d_write_cycles: 2);
    impl_bit_test_op!(bset, |value, bit| { value | (1 << bit) });
    impl_bit_test_op!(bchg, |value, bit| {
        if value.bit(bit) { value & !(1 << bit) } else { value | (1 << bit) }
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

    impl_not!(not_byte, read_byte_resolved_as_result, write_byte_resolved_as_result, OpSize::Byte);
    impl_not!(not_word, read_word_resolved, write_word_resolved, OpSize::Word);
    impl_not!(not_long_word, read_long_word_resolved, write_long_word_resolved, OpSize::LongWord);

    impl_clr!(clr_byte, read_byte_resolved_as_result, write_byte_resolved_as_result, OpSize::Byte);
    impl_clr!(clr_word, read_word_resolved, write_word_resolved, OpSize::Word);
    impl_clr!(clr_long_word, read_long_word_resolved, write_long_word_resolved, OpSize::LongWord);

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

        Ok(shift_memory_cycles(dest))
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

        Ok(shift_memory_cycles(dest))
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
            ShiftDirection::Left => {
                ((original << 1) | u16::from(original.sign_bit()), original.sign_bit())
            }
            ShiftDirection::Right => {
                ((original >> 1) | (u16::from(original.bit(0)) << 15), original.bit(0))
            }
        };

        self.registers.ccr = ConditionCodes {
            carry,
            overflow: false,
            zero: value == 0,
            negative: value.sign_bit(),
            ..self.registers.ccr
        };

        self.write_word_resolved(dest_resolved, value)?;

        Ok(shift_memory_cycles(dest))
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

        Ok(shift_memory_cycles(dest))
    }

    impl_tst!(tst_byte, read_byte, OpSize::Byte);
    impl_tst!(tst_word, read_word, OpSize::Word);
    impl_tst!(tst_long_word, read_long_word, OpSize::LongWord);

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

        if dest.is_data_direct() || self.allow_tas_writes {
            self.write_byte_resolved(dest_resolved, value | 0x80);
        }

        Ok(if dest.is_data_direct() {
            4
        } else {
            10 + dest.address_calculation_cycles(OpSize::Byte)
        })
    }
}

fn shift_register_cycles(size: OpSize, shifts: u32) -> u32 {
    let base_cycles = match size {
        OpSize::Byte | OpSize::Word => 6,
        OpSize::LongWord => 8,
    };
    base_cycles + 2 * shifts
}

fn shift_memory_cycles(dest: AddressingMode) -> u32 {
    8 + dest.address_calculation_cycles(OpSize::Word)
}
