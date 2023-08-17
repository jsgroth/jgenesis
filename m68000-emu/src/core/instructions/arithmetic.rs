use crate::core::instructions::{Direction, ExtendOpMode};
use crate::core::{
    AddressRegister, AddressingMode, ConditionCodes, DataRegister, Exception, ExecuteResult,
    Instruction, InstructionExecutor, OpSize, ResolvedAddress, SizedValue,
};
use crate::traits::{BusInterface, GetBit, SignBit};

macro_rules! impl_extend_op_method {
    ($name:ident, $bytes_fn:ident, $words_fn:ident, $long_words_fn:ident) => {
        fn $name(
            &mut self,
            size: OpSize,
            source: AddressingMode,
            dest: AddressingMode,
        ) -> ExecuteResult<u32> {
            let (_, operand_r) = self.read_extend_operand(size, source)?;
            let operand_r: u32 = operand_r.into();

            let (dest_resolved, operand_l) = self.read_extend_operand(size, dest)?;
            let operand_l: u32 = operand_l.into();

            let extend = self.registers.ccr.extend;
            let (sum, carry, overflow) = match size {
                OpSize::Byte => $bytes_fn(operand_l as u8, operand_r as u8, extend),
                OpSize::Word => $words_fn(operand_l as u16, operand_r as u16, extend),
                OpSize::LongWord => $long_words_fn(operand_l, operand_r, extend),
            };

            self.registers.ccr = ConditionCodes {
                carry,
                overflow,
                zero: self.registers.ccr.zero && sum.is_zero(),
                negative: sum.sign_bit(),
                extend: carry,
            };

            self.write_resolved(dest_resolved, sum)?;

            // ADDX and SUBX only support Dx,Dy and -(Ax),-(Ay)
            Ok(match (size, source) {
                (OpSize::Byte | OpSize::Word, AddressingMode::DataDirect(..)) => 4,
                (OpSize::LongWord, AddressingMode::DataDirect(..)) => 8,
                (OpSize::Byte | OpSize::Word, _) => 18,
                (OpSize::LongWord, _) => 30,
            })
        }
    };
}

macro_rules! impl_op_method {
    ($name:ident, $aname:ident, $xname:ident, $bytes_fn:ident, $words_fn:ident, $long_words_fn:ident) => {
        pub(super) fn $name(
            &mut self,
            size: OpSize,
            source: AddressingMode,
            dest: AddressingMode,
            with_extend: bool,
        ) -> ExecuteResult<u32> {
            if with_extend {
                return self.$xname(size, source, dest);
            }

            if let AddressingMode::AddressDirect(register) = dest {
                return self.$aname(size, source, register);
            }

            let operand_r: u32 = self.read(source, size)?.into();

            let dest_resolved = self.resolve_address_with_post(dest, size)?;
            let operand_l: u32 = self.read_resolved(dest_resolved, size)?.into();

            let (sum, carry, overflow) = match size {
                OpSize::Byte => $bytes_fn(operand_l as u8, operand_r as u8, false),
                OpSize::Word => $words_fn(operand_l as u16, operand_r as u16, false),
                OpSize::LongWord => $long_words_fn(operand_l, operand_r, false),
            };

            if !dest.is_address_direct() {
                self.registers.ccr = ConditionCodes {
                    carry,
                    overflow,
                    zero: sum.is_zero(),
                    negative: sum.sign_bit(),
                    extend: carry,
                };
            }

            self.write_resolved(dest_resolved, sum)?;

            Ok(super::binary_op_cycles(size, source, dest))
        }
    };
}

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    fn read_extend_operand(
        &mut self,
        size: OpSize,
        addressing_mode: AddressingMode,
    ) -> ExecuteResult<(ResolvedAddress, SizedValue)> {
        match (size, addressing_mode) {
            (OpSize::LongWord, AddressingMode::AddressIndirectPredecrement(register)) => {
                let address = register.read_from(self.registers).wrapping_sub(2);
                register.write_long_word_to(self.registers, address);
                let low_word = self.read_bus_word(address)?;

                let address = address.wrapping_sub(2);
                register.write_long_word_to(self.registers, address);
                let high_word = self.read_bus_word(address)?;

                let value = (u32::from(high_word) << 16) | u32::from(low_word);
                Ok((ResolvedAddress::Memory(address), value.into()))
            }
            _ => {
                let address = self.resolve_address_with_post(addressing_mode, size)?;
                let value = self.read_resolved(address, size)?;
                Ok((address, value))
            }
        }
    }

    fn read_address_operand(&mut self, size: OpSize, source: AddressingMode) -> ExecuteResult<u32> {
        let value = match size {
            OpSize::Word => self.read_word(source)? as i16 as u32,
            OpSize::LongWord => self.read_long_word(source)?,
            OpSize::Byte => panic!("ADDA does not support bytes"),
        };
        Ok(value)
    }

    fn adda(
        &mut self,
        size: OpSize,
        source: AddressingMode,
        dest: AddressRegister,
    ) -> ExecuteResult<u32> {
        let operand_r = self.read_address_operand(size, source)?;
        let operand_l = dest.read_from(self.registers);

        let sum = operand_l.wrapping_add(operand_r);
        dest.write_long_word_to(self.registers, sum);

        Ok(super::binary_op_cycles(
            size,
            source,
            AddressingMode::AddressDirect(dest),
        ))
    }

    impl_extend_op_method!(addx, add_bytes, add_words, add_long_words);
    impl_op_method!(add, adda, addx, add_bytes, add_words, add_long_words);

    fn suba(
        &mut self,
        size: OpSize,
        source: AddressingMode,
        dest: AddressRegister,
    ) -> ExecuteResult<u32> {
        let operand_r = self.read_address_operand(size, source)?;
        let operand_l = dest.read_from(self.registers);

        let difference = operand_l.wrapping_sub(operand_r);
        dest.write_long_word_to(self.registers, difference);

        Ok(super::binary_op_cycles(
            size,
            source,
            AddressingMode::AddressDirect(dest),
        ))
    }

    impl_extend_op_method!(subx, sub_bytes, sub_words, sub_long_words);
    impl_op_method!(sub, suba, subx, sub_bytes, sub_words, sub_long_words);

    pub(super) fn neg(
        &mut self,
        size: OpSize,
        dest: AddressingMode,
        with_extend: bool,
    ) -> ExecuteResult<u32> {
        if with_extend {
            return self.negx(size, dest);
        }

        let dest_resolved = self.resolve_address_with_post(dest, size)?;
        let operand_r: u32 = self.read_resolved(dest_resolved, size)?.into();
        let (difference, carry, overflow) = match size {
            OpSize::Byte => sub_bytes(0, operand_r as u8, false),
            OpSize::Word => sub_words(0, operand_r as u16, false),
            OpSize::LongWord => sub_long_words(0, operand_r, false),
        };

        self.registers.ccr = ConditionCodes {
            carry,
            overflow,
            zero: difference.is_zero(),
            negative: difference.sign_bit(),
            extend: carry,
        };

        self.write_resolved(dest_resolved, difference)?;

        Ok(super::unary_op_cycles(size, dest))
    }

    fn negx(&mut self, size: OpSize, dest: AddressingMode) -> ExecuteResult<u32> {
        let dest_resolved = self.resolve_address_with_post(dest, size)?;
        let operand_r: u32 = self.read_resolved(dest_resolved, size)?.into();

        let extend = self.registers.ccr.extend;
        let (difference, carry, overflow) = match size {
            OpSize::Byte => sub_bytes(0, operand_r as u8, extend),
            OpSize::Word => sub_words(0, operand_r as u16, extend),
            OpSize::LongWord => sub_long_words(0, operand_r, extend),
        };

        self.registers.ccr = ConditionCodes {
            carry,
            overflow,
            zero: self.registers.ccr.zero && difference.is_zero(),
            negative: difference.sign_bit(),
            extend: carry,
        };

        self.write_resolved(dest_resolved, difference)?;

        Ok(super::unary_op_cycles(size, dest))
    }

    pub(super) fn cmp(
        &mut self,
        size: OpSize,
        source: AddressingMode,
        dest: AddressingMode,
    ) -> ExecuteResult<u32> {
        if let AddressingMode::AddressDirect(dest) = dest {
            return self.cmpa(size, source, dest);
        }

        let source_operand: u32 = self.read(source, size)?.into();
        let dest_operand: u32 = self.read(dest, size)?.into();

        let ccr = &mut self.registers.ccr;
        match size {
            OpSize::Byte => {
                compare_bytes(source_operand as u8, dest_operand as u8, ccr);
            }
            OpSize::Word => {
                compare_words(source_operand as u16, dest_operand as u16, ccr);
            }
            OpSize::LongWord => {
                compare_long_words(source_operand, dest_operand, ccr);
            }
        }

        Ok(match (size, source, dest) {
            // CMPM.b / CMPM.w
            (
                OpSize::Byte | OpSize::Word,
                AddressingMode::AddressIndirectPostincrement(..),
                AddressingMode::AddressIndirectPostincrement(..),
            ) => 12,
            // CMPM.l
            (
                OpSize::LongWord,
                AddressingMode::AddressIndirectPostincrement(..),
                AddressingMode::AddressIndirectPostincrement(..),
            ) => 20,
            // CMPI.b / CMPI.w
            (
                OpSize::Byte | OpSize::Word,
                AddressingMode::Immediate,
                AddressingMode::DataDirect(..),
            ) => 8,
            (OpSize::Byte | OpSize::Word, AddressingMode::Immediate, _) => {
                super::binary_op_cycles(size, source, dest) - 4
            }
            // CMPI.l
            (OpSize::LongWord, AddressingMode::Immediate, AddressingMode::DataDirect(..)) => 14,
            (OpSize::LongWord, AddressingMode::Immediate, _) => {
                super::binary_op_cycles(size, source, dest) - 8
            }
            // CMP
            (
                OpSize::LongWord,
                AddressingMode::DataDirect(..) | AddressingMode::AddressDirect(..),
                AddressingMode::DataDirect(..),
            ) => 6,
            _ => super::binary_op_cycles(size, source, dest),
        })
    }

    fn cmpa(
        &mut self,
        size: OpSize,
        source: AddressingMode,
        dest: AddressRegister,
    ) -> ExecuteResult<u32> {
        let source_operand = self.read_address_operand(size, source)?;
        let dest_operand = dest.read_from(self.registers);

        compare_long_words(source_operand, dest_operand, &mut self.registers.ccr);

        Ok(6 + source.address_calculation_cycles(size))
    }

    pub(super) fn muls(
        &mut self,
        register: DataRegister,
        source: AddressingMode,
    ) -> ExecuteResult<u32> {
        let operand_l = self.read_word(source)? as i16;
        let operand_r = register.read_from(self.registers) as i16;

        let value = (i32::from(operand_l) * i32::from(operand_r)) as u32;
        register.write_long_word_to(self.registers, value);

        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero: value == 0,
            negative: value.sign_bit(),
            ..self.registers.ccr
        };

        Ok(0)
    }

    pub(super) fn mulu(
        &mut self,
        register: DataRegister,
        source: AddressingMode,
    ) -> ExecuteResult<u32> {
        let operand_l = self.read_word(source)?;
        let operand_r = register.read_from(self.registers) as u16;

        let value = u32::from(operand_l) * u32::from(operand_r);
        register.write_long_word_to(self.registers, value);

        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero: value == 0,
            negative: value.sign_bit(),
            ..self.registers.ccr
        };

        Ok(0)
    }

    pub(super) fn divs(
        &mut self,
        register: DataRegister,
        source: AddressingMode,
    ) -> ExecuteResult<u32> {
        let operand_l = register.read_from(self.registers) as i32;
        let operand_r: i32 = (self.read_word(source)? as i16).into();

        if operand_r == 0 {
            self.registers.ccr = ConditionCodes {
                carry: false,
                overflow: false,
                zero: false,
                negative: false,
                ..self.registers.ccr
            };
            return Err(Exception::DivisionByZero);
        }

        let quotient = operand_l / operand_r;
        let remainder = operand_l % operand_r;

        if quotient > i16::MAX.into() || quotient < i16::MIN.into() {
            self.registers.ccr = ConditionCodes {
                carry: false,
                overflow: true,
                ..self.registers.ccr
            };
            return Ok(0);
        }

        let value = ((quotient as u32) & 0x0000_FFFF) | ((remainder as u32) << 16);
        register.write_long_word_to(self.registers, value);

        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero: value == 0,
            negative: quotient < 0,
            ..self.registers.ccr
        };

        Ok(0)
    }

    pub(super) fn divu(
        &mut self,
        register: DataRegister,
        source: AddressingMode,
    ) -> ExecuteResult<u32> {
        let operand_l = register.read_from(self.registers);
        let operand_r: u32 = self.read_word(source)?.into();

        if operand_r == 0 {
            self.registers.ccr = ConditionCodes {
                carry: false,
                overflow: false,
                zero: false,
                negative: false,
                ..self.registers.ccr
            };
            return Err(Exception::DivisionByZero);
        }

        let quotient = operand_l / operand_r;
        let remainder = operand_l % operand_r;

        if quotient > u16::MAX.into() {
            self.registers.ccr = ConditionCodes {
                carry: false,
                overflow: true,
                ..self.registers.ccr
            };
            return Ok(0);
        }

        let value = (quotient & 0x0000_FFFF) | (remainder << 16);
        register.write_long_word_to(self.registers, value);

        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero: value == 0,
            negative: quotient.bit(15),
            ..self.registers.ccr
        };

        Ok(0)
    }

    pub(super) fn abcd(
        &mut self,
        source: AddressingMode,
        dest: AddressingMode,
    ) -> ExecuteResult<u32> {
        let operand_l = self.read_byte(source)?;

        let dest_resolved = self.resolve_address(dest, OpSize::Byte)?;
        let operand_r = self.read_byte_resolved(dest_resolved);

        let extend: u8 = self.registers.ccr.extend.into();

        let (sum, carry) = match operand_l.overflowing_add(operand_r) {
            (sum, true) => (sum + extend, true),
            (sum, false) => sum.overflowing_add(extend),
        };

        let mut diff = 0;
        if ((operand_l & 0x0F) + (operand_r & 0x0F) + extend >= 0x10) || (sum & 0x0F > 0x09) {
            diff += 0x06;
        }
        if sum > 0x99 || carry {
            diff += 0x60;
        }

        let (corrected_sum, corrected_carry) = sum.overflowing_add(diff);

        let bit_6_carry = (sum & 0x7F) + (diff & 0x7F) >= 0x80;
        let overflow = bit_6_carry != corrected_carry;

        log::trace!("sum={sum:02X}");

        let carry = carry || corrected_carry;
        self.registers.ccr = ConditionCodes {
            carry,
            overflow,
            zero: self.registers.ccr.zero && corrected_sum == 0,
            negative: corrected_sum.sign_bit(),
            extend: carry,
        };

        self.write_byte_resolved(dest_resolved, corrected_sum);

        // ABCD only supports Dx,Dy and -(Ax),-(Ay)
        Ok(match source {
            AddressingMode::DataDirect(..) => 6,
            _ => 18,
        })
    }

    pub(super) fn sbcd(
        &mut self,
        source: AddressingMode,
        dest: AddressingMode,
    ) -> ExecuteResult<u32> {
        let operand_r = self.read_byte(source)?;

        let dest_resolved = self.resolve_address(dest, OpSize::Byte)?;
        let operand_l = self.read_byte_resolved(dest_resolved);

        let difference = self.decimal_subtract(operand_l, operand_r);

        self.write_byte_resolved(dest_resolved, difference);

        // SBCD only supports Dx,Dy and -(Ax),-(Ay)
        Ok(match source {
            AddressingMode::DataDirect(..) => 6,
            _ => 18,
        })
    }

    pub(super) fn nbcd(&mut self, dest: AddressingMode) -> ExecuteResult<u32> {
        let dest_resolved = self.resolve_address_with_post(dest, OpSize::Byte)?;
        let operand_r = self.read_byte_resolved(dest_resolved);

        let difference = self.decimal_subtract(0, operand_r);

        self.write_byte_resolved(dest_resolved, difference);

        Ok(if dest.is_data_direct() {
            6
        } else {
            super::unary_op_cycles(OpSize::Byte, dest)
        })
    }

    fn decimal_subtract(&mut self, operand_l: u8, operand_r: u8) -> u8 {
        let extend: u8 = self.registers.ccr.extend.into();

        let (difference, borrow) = match operand_l.overflowing_sub(operand_r) {
            (difference, true) => (difference - extend, true),
            (difference, false) => difference.overflowing_sub(extend),
        };

        let mut diff = 0;
        if operand_l & 0x0F < (operand_r & 0x0F) + extend {
            diff += 0x06;
        }
        if borrow {
            diff += 0x60;
        }

        let (corrected_difference, corrected_borrow) = difference.overflowing_sub(diff);

        let bit_6_borrow = difference & 0x7F < diff & 0x7F;
        let overflow = bit_6_borrow != corrected_borrow;

        let borrow = borrow || corrected_borrow;
        self.registers.ccr = ConditionCodes {
            carry: borrow,
            overflow,
            zero: self.registers.ccr.zero && corrected_difference == 0,
            negative: corrected_difference.sign_bit(),
            extend: borrow,
        };

        corrected_difference
    }
}

macro_rules! impl_add_fn {
    ($name:ident, $t:ty, $overflow_mask:expr) => {
        fn $name(operand_l: $t, operand_r: $t, extend: bool) -> (SizedValue, bool, bool) {
            let extend_operand = <$t>::from(extend);
            let (sum, carry) = match operand_l.overflowing_add(operand_r) {
                (sum, true) => (sum + extend_operand, true),
                (sum, false) => sum.overflowing_add(extend_operand),
            };

            let bit_m1_carry =
                (operand_l & $overflow_mask) + (operand_r & $overflow_mask) + extend_operand
                    > $overflow_mask;
            let overflow = bit_m1_carry != carry;

            (sum.into(), carry, overflow)
        }
    };
}

impl_add_fn!(add_bytes, u8, 0x7F);
impl_add_fn!(add_words, u16, 0x7FFF);
impl_add_fn!(add_long_words, u32, 0x7FFF_FFFF);

macro_rules! impl_sub_fn {
    ($name:ident, $t:ty, $overflow_mask:expr) => {
        fn $name(operand_l: $t, operand_r: $t, extend: bool) -> (SizedValue, bool, bool) {
            let extend_operand = <$t>::from(extend);
            let (difference, borrow) = match operand_l.overflowing_sub(operand_r) {
                (difference, true) => (difference - extend_operand, true),
                (difference, false) => difference.overflowing_sub(extend_operand),
            };

            let bit_m1_borrow =
                operand_l & $overflow_mask < (operand_r & $overflow_mask) + extend_operand;
            let overflow = bit_m1_borrow != borrow;

            log::trace!("{operand_l} - {operand_r} = {difference}");

            (difference.into(), borrow, overflow)
        }
    };
}

impl_sub_fn!(sub_bytes, u8, 0x7F);
impl_sub_fn!(sub_words, u16, 0x7FFF);
impl_sub_fn!(sub_long_words, u32, 0x7FFF_FFFF);

macro_rules! impl_compare_fn {
    ($name:ident, $t:ty, $overflow_mask:expr) => {
        fn $name(source: $t, dest: $t, ccr: &mut ConditionCodes) {
            let (difference, borrow) = dest.overflowing_sub(source);
            let bit_m1_borrow = dest & $overflow_mask < source & $overflow_mask;
            let overflow = bit_m1_borrow != borrow;

            *ccr = ConditionCodes {
                carry: borrow,
                overflow,
                zero: difference == 0,
                negative: difference.sign_bit(),
                ..*ccr
            };
        }
    };
}

impl_compare_fn!(compare_bytes, u8, 0x7F);
impl_compare_fn!(compare_words, u16, 0x7FFF);
impl_compare_fn!(compare_long_words, u32, 0x7FFF_FFFF);

macro_rules! impl_decode_fn {
    ($name:ident, $instruction:ident) => {
        pub(super) fn $name(opcode: u16) -> ExecuteResult<Instruction> {
            let register = ((opcode >> 9) & 0x07) as u8;
            let size = OpSize::parse_from_opcode(opcode);
            let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;

            match size {
                Ok(size) => {
                    let direction = Direction::parse_from_opcode(opcode);

                    if direction == Direction::RegisterToMemory
                        && matches!(
                            addressing_mode,
                            AddressingMode::DataDirect(..) | AddressingMode::AddressDirect(..)
                        )
                    {
                        // ADDX / SUBX
                        let mode = ExtendOpMode::parse_from_opcode(opcode);
                        let source = (opcode & 0x07) as u8;

                        return match mode {
                            ExtendOpMode::DataDirect => Ok(Instruction::$instruction {
                                size,
                                source: AddressingMode::DataDirect(source.into()),
                                dest: AddressingMode::DataDirect(register.into()),
                                with_extend: true,
                            }),
                            ExtendOpMode::AddressIndirectPredecrement => {
                                Ok(Instruction::$instruction {
                                    size,
                                    source: AddressingMode::AddressIndirectPredecrement(
                                        source.into(),
                                    ),
                                    dest: AddressingMode::AddressIndirectPredecrement(
                                        register.into(),
                                    ),
                                    with_extend: true,
                                })
                            }
                        };
                    }

                    if direction == Direction::RegisterToMemory && !addressing_mode.is_writable() {
                        return Err(Exception::IllegalInstruction(opcode));
                    }

                    let register_am = AddressingMode::DataDirect(register.into());
                    let (source, dest) = match direction {
                        Direction::RegisterToMemory => (register_am, addressing_mode),
                        Direction::MemoryToRegister => (addressing_mode, register_am),
                    };

                    Ok(Instruction::$instruction {
                        size,
                        source,
                        dest,
                        with_extend: false,
                    })
                }
                Err(_) => {
                    // ADDA / SUBA
                    let size = if opcode.bit(8) {
                        OpSize::LongWord
                    } else {
                        OpSize::Word
                    };

                    Ok(Instruction::$instruction {
                        size,
                        source: addressing_mode,
                        dest: AddressingMode::AddressDirect(register.into()),
                        with_extend: false,
                    })
                }
            }
        }
    };
}

impl_decode_fn!(decode_add, Add);
impl_decode_fn!(decode_sub, Subtract);

pub(super) fn decode_addq_subq(opcode: u16, size: OpSize) -> ExecuteResult<Instruction> {
    let dest = AddressingMode::parse_from_opcode(opcode)?;
    let operand = ((opcode >> 9) & 0x07) as u8;
    let operand = if operand == 0 { 8 } else { operand };

    if !dest.is_writable() || (size == OpSize::Byte && dest.is_address_direct()) {
        return Err(Exception::IllegalInstruction(opcode));
    }

    if !opcode.bit(8) {
        Ok(Instruction::Add {
            size,
            source: AddressingMode::Quick(operand),
            dest,
            with_extend: false,
        })
    } else {
        Ok(Instruction::Subtract {
            size,
            source: AddressingMode::Quick(operand),
            dest,
            with_extend: false,
        })
    }
}

macro_rules! impl_decode_immediate_fn {
    ($name:ident, $instruction:ident) => {
        pub(super) fn $name(opcode: u16) -> ExecuteResult<Instruction> {
            let size = OpSize::parse_from_opcode(opcode)?;
            let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;

            if addressing_mode.is_address_direct() || !addressing_mode.is_writable() {
                return Err(Exception::IllegalInstruction(opcode));
            }

            Ok(Instruction::$instruction {
                size,
                source: AddressingMode::Immediate,
                dest: addressing_mode,
                with_extend: false,
            })
        }
    };
}

impl_decode_immediate_fn!(decode_addi, Add);
impl_decode_immediate_fn!(decode_subi, Subtract);

fn decode_negate(opcode: u16, with_extend: bool) -> ExecuteResult<Instruction> {
    let size = OpSize::parse_from_opcode(opcode)?;
    let dest = AddressingMode::parse_from_opcode(opcode)?;

    if dest.is_address_direct() || !dest.is_writable() {
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::Negate {
        size,
        dest,
        with_extend,
    })
}

pub(super) fn decode_neg(opcode: u16) -> ExecuteResult<Instruction> {
    decode_negate(opcode, false)
}

pub(super) fn decode_negx(opcode: u16) -> ExecuteResult<Instruction> {
    decode_negate(opcode, true)
}

pub(super) fn decode_cmp(opcode: u16) -> ExecuteResult<Instruction> {
    let register = ((opcode >> 9) & 0x07) as u8;
    let size = OpSize::parse_from_opcode(opcode)?;
    let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;

    Ok(Instruction::Compare {
        size,
        source: addressing_mode,
        dest: AddressingMode::DataDirect(register.into()),
    })
}

pub(super) fn decode_cmpa(opcode: u16) -> ExecuteResult<Instruction> {
    let register = ((opcode >> 9) & 0x07) as u8;
    let size = if opcode.bit(8) {
        OpSize::LongWord
    } else {
        OpSize::Word
    };
    let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;

    Ok(Instruction::Compare {
        size,
        source: addressing_mode,
        dest: AddressingMode::AddressDirect(register.into()),
    })
}

pub(super) fn decode_cmpi(opcode: u16) -> ExecuteResult<Instruction> {
    let size = OpSize::parse_from_opcode(opcode)?;
    let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;

    if matches!(
        addressing_mode,
        AddressingMode::AddressDirect(..) | AddressingMode::Immediate
    ) {
        return Err(Exception::IllegalInstruction(opcode));
    }

    Ok(Instruction::Compare {
        size,
        source: AddressingMode::Immediate,
        dest: addressing_mode,
    })
}

pub(super) fn decode_cmpm(opcode: u16) -> ExecuteResult<Instruction> {
    let size = OpSize::parse_from_opcode(opcode)?;
    let source = (opcode & 0x07) as u8;
    let dest = ((opcode >> 9) & 0x07) as u8;

    Ok(Instruction::Compare {
        size,
        source: AddressingMode::AddressIndirectPostincrement(source.into()),
        dest: AddressingMode::AddressIndirectPostincrement(dest.into()),
    })
}

pub(super) fn decode_muls(opcode: u16) -> ExecuteResult<Instruction> {
    let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;
    let register = ((opcode >> 9) & 0x07) as u8;

    Ok(Instruction::MultiplySigned(
        register.into(),
        addressing_mode,
    ))
}

pub(super) fn decode_mulu(opcode: u16) -> ExecuteResult<Instruction> {
    let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;
    let register = ((opcode >> 9) & 0x07) as u8;

    Ok(Instruction::MultiplyUnsigned(
        register.into(),
        addressing_mode,
    ))
}

pub(super) fn decode_divs(opcode: u16) -> ExecuteResult<Instruction> {
    let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;
    let register = ((opcode >> 9) & 0x07) as u8;

    Ok(Instruction::DivideSigned(register.into(), addressing_mode))
}

pub(super) fn decode_divu(opcode: u16) -> ExecuteResult<Instruction> {
    let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;
    let register = ((opcode >> 9) & 0x07) as u8;

    Ok(Instruction::DivideUnsigned(
        register.into(),
        addressing_mode,
    ))
}

macro_rules! impl_decode_decimal {
    ($name:ident, $instruction:ident) => {
        pub(super) fn $name(opcode: u16) -> Instruction {
            let source_register = (opcode & 0x07) as u8;
            let dest_register = ((opcode >> 9) & 0x07) as u8;

            if opcode.bit(3) {
                Instruction::$instruction {
                    source: AddressingMode::AddressIndirectPredecrement(source_register.into()),
                    dest: AddressingMode::AddressIndirectPredecrement(dest_register.into()),
                }
            } else {
                Instruction::$instruction {
                    source: AddressingMode::DataDirect(source_register.into()),
                    dest: AddressingMode::DataDirect(dest_register.into()),
                }
            }
        }
    };
}

impl_decode_decimal!(decode_abcd, AddDecimal);
impl_decode_decimal!(decode_sbcd, SubtractDecimal);

pub(super) fn decode_nbcd(opcode: u16) -> ExecuteResult<Instruction> {
    let addressing_mode = AddressingMode::parse_from_opcode(opcode)?;

    Ok(Instruction::NegateDecimal(addressing_mode))
}
