use crate::core::{
    AddressRegister, AddressingMode, ConditionCodes, DataRegister, Exception, ExecuteResult,
    InstructionExecutor, OpSize, ResolvedAddress,
};
use crate::traits::BusInterface;
use jgenesis_traits::num::{GetBit, SignBit};

macro_rules! impl_extend_op_method {
    ($name:ident, $read_method:ident, $write_method:ident, $op_fn:ident, $size:expr) => {
        fn $name(&mut self, source: AddressingMode, dest: AddressingMode) -> ExecuteResult<u32> {
            let (_, operand_r) = self.$read_method(source)?;
            let (dest_resolved, operand_l) = self.$read_method(dest)?;

            let (value, carry, overflow) = $op_fn(operand_l, operand_r, self.registers.ccr.extend);

            self.registers.ccr = ConditionCodes {
                carry,
                overflow,
                zero: self.registers.ccr.zero && value == 0,
                negative: value.sign_bit(),
                extend: carry,
            };

            self.$write_method(dest_resolved, value)?;

            // ADDX and SUBX only support Dx,Dy and -(Ax),-(Ay)
            Ok(match ($size, source) {
                (OpSize::Byte | OpSize::Word, AddressingMode::DataDirect(..)) => 4,
                (OpSize::LongWord, AddressingMode::DataDirect(..)) => 8,
                (OpSize::Byte | OpSize::Word, _) => 18,
                (OpSize::LongWord, _) => 30,
            })
        }
    };
}

macro_rules! impl_op_method {
    ($name:ident, $aname:ident, $xname:ident, $read_method:ident, $read_resolved_method:ident, $write_method:ident, $op_fn:ident, $size:expr) => {
        pub(super) fn $name(
            &mut self,
            source: AddressingMode,
            dest: AddressingMode,
            with_extend: bool,
        ) -> ExecuteResult<u32> {
            if with_extend {
                return self.$xname(source, dest);
            }

            if let AddressingMode::AddressDirect(register) = dest {
                return self.$aname($size, source, register);
            }

            let operand_r = self.$read_method(source)?;

            let dest_resolved = self.resolve_address_with_post(dest, $size)?;
            let operand_l = self.$read_resolved_method(dest_resolved)?;

            let (value, carry, overflow) = $op_fn(operand_l, operand_r, false);

            if !dest.is_address_direct() {
                self.registers.ccr = ConditionCodes {
                    carry,
                    overflow,
                    zero: value == 0,
                    negative: value.sign_bit(),
                    extend: carry,
                };
            }

            self.$write_method(dest_resolved, value)?;

            Ok(super::binary_op_cycles($size, source, dest))
        }
    };
}

macro_rules! impl_neg {
    ($name:ident, $read_method:ident, $write_method:ident, $sub_fn:ident, $size:expr) => {
        pub(super) fn $name(
            &mut self,
            dest: AddressingMode,
            with_extend: bool,
        ) -> ExecuteResult<u32> {
            let dest_resolved = self.resolve_address_with_post(dest, $size)?;
            let operand_r = self.$read_method(dest_resolved)?;
            let extend = with_extend && self.registers.ccr.extend;
            let (difference, carry, overflow) = $sub_fn(0, operand_r, extend);

            self.registers.ccr = ConditionCodes {
                carry,
                overflow,
                zero: (!with_extend || self.registers.ccr.zero) && difference == 0,
                negative: difference.sign_bit(),
                extend: carry,
            };

            self.$write_method(dest_resolved, difference)?;

            Ok(super::unary_op_cycles($size, dest))
        }
    };
}

macro_rules! impl_cmp {
    ($name:ident, $read_method:ident, $cmp_fn:ident, $cycles_fn:ident, $size:expr) => {
        pub(super) fn $name(
            &mut self,
            source: AddressingMode,
            dest: AddressingMode,
        ) -> ExecuteResult<u32> {
            if let AddressingMode::AddressDirect(dest) = dest {
                return self.cmpa($size, source, dest);
            }

            let source_operand = self.$read_method(source)?;
            let dest_operand = self.$read_method(dest)?;

            $cmp_fn(source_operand, dest_operand, &mut self.registers.ccr);

            let cycles = $cycles_fn(source, dest);
            Ok(cycles)
        }
    };
}

macro_rules! impl_cmp_cycles_byte_word {
    ($name:ident, $size:expr) => {
        #[inline]
        fn $name(source: AddressingMode, dest: AddressingMode) -> u32 {
            match (source, dest) {
                // CMPM.b / CMPM.w
                (
                    AddressingMode::AddressIndirectPostincrement(..),
                    AddressingMode::AddressIndirectPostincrement(..),
                ) => 12,
                // CMPI.b / CMPI.w
                (AddressingMode::Immediate, AddressingMode::DataDirect(..)) => 8,
                (AddressingMode::Immediate, _) => super::binary_op_cycles($size, source, dest) - 4,
                // CMP
                _ => super::binary_op_cycles($size, source, dest),
            }
        }
    };
}

impl_cmp_cycles_byte_word!(cmp_cycles_byte, OpSize::Byte);
impl_cmp_cycles_byte_word!(cmp_cycles_word, OpSize::Word);

#[inline]
fn cmp_cycles_long_word(source: AddressingMode, dest: AddressingMode) -> u32 {
    match (source, dest) {
        // CMPM.l
        (
            AddressingMode::AddressIndirectPostincrement(..),
            AddressingMode::AddressIndirectPostincrement(..),
        ) => 20,
        // CMPI.l
        (AddressingMode::Immediate, AddressingMode::DataDirect(..)) => 14,
        (AddressingMode::Immediate, _) => {
            super::binary_op_cycles(OpSize::LongWord, source, dest) - 8
        }
        // CMP
        (
            AddressingMode::DataDirect(..) | AddressingMode::AddressDirect(..),
            AddressingMode::DataDirect(..),
        ) => 6,
        _ => super::binary_op_cycles(OpSize::LongWord, source, dest),
    }
}

impl<'registers, 'bus, B: BusInterface> InstructionExecutor<'registers, 'bus, B> {
    fn read_byte_for_extend(
        &mut self,
        source: AddressingMode,
    ) -> ExecuteResult<(ResolvedAddress, u8)> {
        let address = self.resolve_address_with_post(source, OpSize::Byte)?;
        let byte = self.read_byte_resolved(address);
        Ok((address, byte))
    }

    fn read_word_for_extend(
        &mut self,
        source: AddressingMode,
    ) -> ExecuteResult<(ResolvedAddress, u16)> {
        let address = self.resolve_address_with_post(source, OpSize::Word)?;
        let word = self.read_word_resolved(address)?;
        Ok((address, word))
    }

    fn read_long_word_for_extend(
        &mut self,
        source: AddressingMode,
    ) -> ExecuteResult<(ResolvedAddress, u32)> {
        match source {
            AddressingMode::AddressIndirectPredecrement(register) => {
                let address = register.read_from(self.registers).wrapping_sub(2);
                register.write_long_word_to(self.registers, address);
                let low_word = self.read_bus_word(address)?;

                let address = address.wrapping_sub(2);
                register.write_long_word_to(self.registers, address);
                let high_word = self.read_bus_word(address)?;

                let value = (u32::from(high_word) << 16) | u32::from(low_word);
                Ok((ResolvedAddress::Memory(address), value))
            }
            _ => {
                let address = self.resolve_address_with_post(source, OpSize::LongWord)?;
                let value = self.read_long_word_resolved(address)?;
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

        Ok(super::binary_op_cycles(size, source, AddressingMode::AddressDirect(dest)))
    }

    impl_extend_op_method!(
        addx_byte,
        read_byte_for_extend,
        write_byte_resolved_as_result,
        add_bytes,
        OpSize::Byte
    );
    impl_extend_op_method!(
        addx_word,
        read_word_for_extend,
        write_word_resolved,
        add_words,
        OpSize::Word
    );
    impl_extend_op_method!(
        addx_long_word,
        read_long_word_for_extend,
        write_long_word_resolved,
        add_long_words,
        OpSize::LongWord
    );

    impl_op_method!(
        add_byte,
        adda,
        addx_byte,
        read_byte,
        read_byte_resolved_as_result,
        write_byte_resolved_as_result,
        add_bytes,
        OpSize::Byte
    );
    impl_op_method!(
        add_word,
        adda,
        addx_word,
        read_word,
        read_word_resolved,
        write_word_resolved,
        add_words,
        OpSize::Word
    );
    impl_op_method!(
        add_long_word,
        adda,
        addx_long_word,
        read_long_word,
        read_long_word_resolved,
        write_long_word_resolved,
        add_long_words,
        OpSize::LongWord
    );

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

        Ok(super::binary_op_cycles(size, source, AddressingMode::AddressDirect(dest)))
    }

    impl_extend_op_method!(
        subx_byte,
        read_byte_for_extend,
        write_byte_resolved_as_result,
        sub_bytes,
        OpSize::Byte
    );
    impl_extend_op_method!(
        subx_word,
        read_word_for_extend,
        write_word_resolved,
        sub_words,
        OpSize::Word
    );
    impl_extend_op_method!(
        subx_long_word,
        read_long_word_for_extend,
        write_long_word_resolved,
        sub_long_words,
        OpSize::LongWord
    );

    impl_op_method!(
        sub_byte,
        suba,
        subx_byte,
        read_byte,
        read_byte_resolved_as_result,
        write_byte_resolved_as_result,
        sub_bytes,
        OpSize::Byte
    );
    impl_op_method!(
        sub_word,
        suba,
        subx_word,
        read_word,
        read_word_resolved,
        write_word_resolved,
        sub_words,
        OpSize::Word
    );
    impl_op_method!(
        sub_long_word,
        suba,
        subx_long_word,
        read_long_word,
        read_long_word_resolved,
        write_long_word_resolved,
        sub_long_words,
        OpSize::LongWord
    );

    impl_neg!(
        neg_byte,
        read_byte_resolved_as_result,
        write_byte_resolved_as_result,
        sub_bytes,
        OpSize::Byte
    );
    impl_neg!(neg_word, read_word_resolved, write_word_resolved, sub_words, OpSize::Word);
    impl_neg!(
        neg_long_word,
        read_long_word_resolved,
        write_long_word_resolved,
        sub_long_words,
        OpSize::LongWord
    );

    impl_cmp!(cmp_byte, read_byte, compare_bytes, cmp_cycles_byte, OpSize::Byte);
    impl_cmp!(cmp_word, read_word, compare_words, cmp_cycles_word, OpSize::Word);
    impl_cmp!(
        cmp_long_word,
        read_long_word,
        compare_long_words,
        cmp_cycles_long_word,
        OpSize::LongWord
    );

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

        let mut last_bit = false;
        let mut alternating_bits = 0;
        for i in 0..16 {
            let bit = operand_l.bit(i);
            if bit != last_bit {
                alternating_bits += 1;
            }
            last_bit = bit;
        }

        Ok(38 + 2 * alternating_bits + source.address_calculation_cycles(OpSize::Word))
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

        Ok(38 + 2 * operand_l.count_ones() + source.address_calculation_cycles(OpSize::Word))
    }

    fn divide_by_zero_error(&mut self, source: AddressingMode) -> Exception {
        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero: false,
            negative: false,
            ..self.registers.ccr
        };

        Exception::DivisionByZero { cycles: source.address_calculation_cycles(OpSize::Word) }
    }

    pub(super) fn divs(
        &mut self,
        register: DataRegister,
        source: AddressingMode,
    ) -> ExecuteResult<u32> {
        let operand_l = register.read_from(self.registers) as i32;
        let operand_r: i32 = (self.read_word(source)? as i16).into();

        if operand_r == 0 {
            return Err(self.divide_by_zero_error(source));
        }

        let quotient = operand_l / operand_r;
        let remainder = operand_l % operand_r;

        if quotient > i16::MAX.into() || quotient < i16::MIN.into() {
            self.registers.ccr =
                ConditionCodes { carry: false, overflow: true, ..self.registers.ccr };
            // TODO this is the best case cycle count, not accurate
            return Ok(120);
        }

        let value = ((quotient as u32) & 0x0000_FFFF) | ((remainder as u32) << 16);
        register.write_long_word_to(self.registers, value);

        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero: quotient == 0,
            negative: quotient < 0,
            ..self.registers.ccr
        };

        // TODO this is the best case cycle count, not accurate
        Ok(120)
    }

    pub(super) fn divu(
        &mut self,
        register: DataRegister,
        source: AddressingMode,
    ) -> ExecuteResult<u32> {
        let operand_l = register.read_from(self.registers);
        let operand_r: u32 = self.read_word(source)?.into();

        if operand_r == 0 {
            return Err(self.divide_by_zero_error(source));
        }

        let quotient = operand_l / operand_r;
        let remainder = operand_l % operand_r;

        if quotient > u16::MAX.into() {
            self.registers.ccr =
                ConditionCodes { carry: false, overflow: true, ..self.registers.ccr };
            // TODO this is the best case cycle count, not accurate
            return Ok(76);
        }

        let value = (quotient & 0x0000_FFFF) | (remainder << 16);
        register.write_long_word_to(self.registers, value);

        self.registers.ccr = ConditionCodes {
            carry: false,
            overflow: false,
            zero: quotient == 0,
            negative: quotient.bit(15),
            ..self.registers.ccr
        };

        // TODO this is the best case cycle count, not accurate
        Ok(76)
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

        Ok(if dest.is_data_direct() { 6 } else { super::unary_op_cycles(OpSize::Byte, dest) })
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
        fn $name(operand_l: $t, operand_r: $t, extend: bool) -> ($t, bool, bool) {
            let extend_operand = <$t>::from(extend);
            let (sum, carry) = match operand_l.overflowing_add(operand_r) {
                (sum, true) => (sum + extend_operand, true),
                (sum, false) => sum.overflowing_add(extend_operand),
            };

            let bit_m1_carry =
                (operand_l & $overflow_mask) + (operand_r & $overflow_mask) + extend_operand
                    > $overflow_mask;
            let overflow = bit_m1_carry != carry;

            (sum, carry, overflow)
        }
    };
}

impl_add_fn!(add_bytes, u8, 0x7F);
impl_add_fn!(add_words, u16, 0x7FFF);
impl_add_fn!(add_long_words, u32, 0x7FFF_FFFF);

macro_rules! impl_sub_fn {
    ($name:ident, $t:ty, $overflow_mask:expr) => {
        fn $name(operand_l: $t, operand_r: $t, extend: bool) -> ($t, bool, bool) {
            let extend_operand = <$t>::from(extend);
            let (difference, borrow) = match operand_l.overflowing_sub(operand_r) {
                (difference, true) => (difference - extend_operand, true),
                (difference, false) => difference.overflowing_sub(extend_operand),
            };

            let bit_m1_borrow =
                operand_l & $overflow_mask < (operand_r & $overflow_mask) + extend_operand;
            let overflow = bit_m1_borrow != borrow;

            (difference, borrow, overflow)
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
