#[allow(clippy::wildcard_imports)]
use super::*;
use crate::core::Registers;

fn adc_bcd(registers: &mut Registers, value: u8) {
    // Formulas from http://www.6502.org/tutorials/decimal_mode.html#A which correctly handle
    // invalid values and undocumented behaviors

    let original_a = registers.a as u8;
    let existing_carry: u8 = registers.p.carry.into();

    let mut al = (original_a & 0x0F) + (value & 0x0F) + existing_carry;
    if al >= 0x0A {
        al = 0x10 | ((al + 0x06) & 0x0F);
    }

    let mut a = u16::from(original_a & 0xF0) + u16::from(value & 0xF0) + u16::from(al);
    if a >= 0xA0 {
        a += 0x60;
    }

    let s = i16::from((original_a & 0xF0) as i8) + i16::from((value & 0xF0) as i8) + i16::from(al);
    let overflow = !(-128..128).contains(&s);

    let result = a as u8;
    registers.set_a_u8(result);

    registers.p.zero = result == 0;
    registers.p.carry = a >= 0x0100;
    registers.p.negative = result.sign_bit();
    registers.p.overflow = overflow;
}

fn adc_bcd_u16(registers: &mut Registers, value: u16) {
    let [value_lsb, value_msb] = value.to_le_bytes();

    adc_bcd(registers, value_lsb);
    registers.a = registers.a.swap_bytes();
    adc_bcd(registers, value_msb);
    registers.a = registers.a.swap_bytes();

    registers.p.zero = registers.a == 0;
}

fn sbc_bcd(registers: &mut Registers, value: u8) {
    // Formulas from http://www.6502.org/tutorials/decimal_mode.html#A which correctly handle
    // invalid values and undocumented behaviors

    let original_a = registers.a as u8;
    let existing_borrow: u8 = (!registers.p.carry).into();

    let mut al = u16::from(original_a & 0x0F)
        .wrapping_sub(u16::from(value & 0x0F))
        .wrapping_sub(u16::from(existing_borrow));
    if al.sign_bit() {
        al = (al.wrapping_sub(0x06) & 0x0F).wrapping_sub(0x10);
    }

    let mut a = u16::from(original_a & 0xF0).wrapping_sub(u16::from(value & 0xF0)).wrapping_add(al);
    if a.sign_bit() {
        a = a.wrapping_sub(0x60);
    }

    // Carry and overflow flags are set based on binary arithmetic
    let borrow = u16::from(original_a) < u16::from(value) + u16::from(existing_borrow);
    let bit_6_borrow = original_a & 0x7F < (value & 0x7F) + existing_borrow;
    let overflow = bit_6_borrow != borrow;

    let result = a as u8;
    registers.set_a_u8(result);

    registers.p.zero = result == 0;
    registers.p.negative = result.sign_bit();
    registers.p.carry = !borrow;
    registers.p.overflow = overflow;
}

fn sbc_bcd_u16(registers: &mut Registers, value: u16) {
    let [value_lsb, value_msb] = value.to_le_bytes();

    sbc_bcd(registers, value_lsb);
    registers.a = registers.a.swap_bytes();
    sbc_bcd(registers, value_msb);
    registers.a = registers.a.swap_bytes();

    registers.p.zero = registers.a == 0;
}

// ADC: Add with carry (8-bit)
impl_read_op_u8!(
    [
        immediate -> adc_immediate_u8,
        direct_page -> adc_direct_page_u8,
        direct_page_x -> adc_direct_page_x_u8,
        direct_page_indirect -> adc_direct_page_indirect_u8,
        direct_page_indexed_indirect -> adc_direct_page_indexed_indirect_u8,
        direct_page_indirect_indexed -> adc_direct_page_indirect_indexed_u8,
        direct_page_indirect_long -> adc_direct_page_indirect_long_u8,
        direct_page_indirect_long_indexed -> adc_direct_page_indirect_long_indexed_u8,
        absolute -> adc_absolute_u8,
        absolute_x -> adc_absolute_x_u8,
        absolute_y -> adc_absolute_y_u8,
        absolute_long -> adc_absolute_long_u8,
        absolute_long_x -> adc_absolute_long_x_u8,
        stack_relative -> adc_stack_relative_u8,
        stack_relative_indirect_indexed -> adc_stack_relative_indirect_indexed_u8,
    ]
    (|registers, value| {
        if registers.p.decimal_mode {
            adc_bcd(registers, value);
            return;
        }

        let a = registers.a as u8;
        let existing_carry: u8 = registers.p.carry.into();

        let (partial_sum, carry1) = a.overflowing_add(value);
        let (sum, carry2) = partial_sum.overflowing_add(existing_carry);
        let carry = carry1 || carry2;

        let bit_6_carry = (a & 0x7F) + (value & 0x7F) + existing_carry >= 0x80;
        let overflow = bit_6_carry != carry;

        registers.set_a_u8(sum);

        registers.p.zero = sum == 0;
        registers.p.negative = sum.sign_bit();
        registers.p.carry = carry;
        registers.p.overflow = overflow;
    })
);

// ADC: Add with carry (16-bit)
impl_read_op_u16!(
    [
        immediate -> adc_immediate_u16,
        direct_page -> adc_direct_page_u16,
        direct_page_x -> adc_direct_page_x_u16,
        direct_page_indirect -> adc_direct_page_indirect_u16,
        direct_page_indexed_indirect -> adc_direct_page_indexed_indirect_u16,
        direct_page_indirect_indexed -> adc_direct_page_indirect_indexed_u16,
        direct_page_indirect_long -> adc_direct_page_indirect_long_u16,
        direct_page_indirect_long_indexed -> adc_direct_page_indirect_long_indexed_u16,
        absolute -> adc_absolute_u16,
        absolute_x -> adc_absolute_x_u16,
        absolute_y -> adc_absolute_y_u16,
        absolute_long -> adc_absolute_long_u16,
        absolute_long_x -> adc_absolute_long_x_u16,
        stack_relative -> adc_stack_relative_u16,
        stack_relative_indirect_indexed -> adc_stack_relative_indirect_indexed_u16,
    ]
    (|registers, value| {
        if registers.p.decimal_mode {
            adc_bcd_u16(registers, value);
            return;
        }

        let a = registers.a;
        let existing_carry: u16 = registers.p.carry.into();

        let (partial_sum, carry1) = a.overflowing_add(value);
        let (sum, carry2) = partial_sum.overflowing_add(existing_carry);
        let carry = carry1 || carry2;

        let bit_14_carry = (a & 0x7FFF) + (value & 0x7FFF) + existing_carry >= 0x8000;
        let overflow = bit_14_carry != carry;

        registers.a = sum;

        registers.p.zero = sum == 0;
        registers.p.negative = sum.sign_bit();
        registers.p.carry = carry;
        registers.p.overflow = overflow;
    })
);

// SBC: Subtract with carry (8-bit)
impl_read_op_u8!(
    [
        immediate -> sbc_immediate_u8,
        direct_page -> sbc_direct_page_u8,
        direct_page_x -> sbc_direct_page_x_u8,
        direct_page_indirect -> sbc_direct_page_indirect_u8,
        direct_page_indexed_indirect -> sbc_direct_page_indexed_indirect_u8,
        direct_page_indirect_indexed -> sbc_direct_page_indirect_indexed_u8,
        direct_page_indirect_long -> sbc_direct_page_indirect_long_u8,
        direct_page_indirect_long_indexed -> sbc_direct_page_indirect_long_indexed_u8,
        absolute -> sbc_absolute_u8,
        absolute_x -> sbc_absolute_x_u8,
        absolute_y -> sbc_absolute_y_u8,
        absolute_long -> sbc_absolute_long_u8,
        absolute_long_x -> sbc_absolute_long_x_u8,
        stack_relative -> sbc_stack_relative_u8,
        stack_relative_indirect_indexed -> sbc_stack_relative_indirect_indexed_u8,
    ]
    (|registers, value| {
        if registers.p.decimal_mode {
            sbc_bcd(registers, value);
            return;
        }

        let a = registers.a as u8;
        let existing_borrow: u8 = (!registers.p.carry).into();

        let (partial_diff, borrow1) = a.overflowing_sub(value);
        let (difference, borrow2) = partial_diff.overflowing_sub(existing_borrow);
        let borrow = borrow1 || borrow2;

        let bit_6_borrow = a & 0x7F < (value & 0x7F) + existing_borrow;
        let overflow = bit_6_borrow != borrow;

        registers.set_a_u8(difference);

        registers.p.zero = difference == 0;
        registers.p.negative = difference.sign_bit();
        registers.p.carry = !borrow;
        registers.p.overflow = overflow;
    })
);

// SBC: Subtract with carry (16-bit)
impl_read_op_u16!(
    [
        immediate -> sbc_immediate_u16,
        direct_page -> sbc_direct_page_u16,
        direct_page_x -> sbc_direct_page_x_u16,
        direct_page_indirect -> sbc_direct_page_indirect_u16,
        direct_page_indexed_indirect -> sbc_direct_page_indexed_indirect_u16,
        direct_page_indirect_indexed -> sbc_direct_page_indirect_indexed_u16,
        direct_page_indirect_long -> sbc_direct_page_indirect_long_u16,
        direct_page_indirect_long_indexed -> sbc_direct_page_indirect_long_indexed_u16,
        absolute -> sbc_absolute_u16,
        absolute_x -> sbc_absolute_x_u16,
        absolute_y -> sbc_absolute_y_u16,
        absolute_long -> sbc_absolute_long_u16,
        absolute_long_x -> sbc_absolute_long_x_u16,
        stack_relative -> sbc_stack_relative_u16,
        stack_relative_indirect_indexed -> sbc_stack_relative_indirect_indexed_u16,
    ]
    (|registers, value| {
        if registers.p.decimal_mode {
            sbc_bcd_u16(registers, value);
            return;
        }

        let a = registers.a;
        let existing_borrow: u16 = (!registers.p.carry).into();

        let (partial_diff, borrow1) = a.overflowing_sub(value);
        let (difference, borrow2) = partial_diff.overflowing_sub(existing_borrow);
        let borrow = borrow1 || borrow2;

        let bit_14_borrow = a & 0x7FFF < (value & 0x7FFF) + existing_borrow;
        let overflow = bit_14_borrow != borrow;

        registers.a = difference;

        registers.p.zero = difference == 0;
        registers.p.negative = difference.sign_bit();
        registers.p.carry = !borrow;
        registers.p.overflow = overflow;
    })
);

// CMP: Compare to accumulator (8-bit)
impl_read_op_u8!(
    [
        immediate -> cmp_immediate_u8,
        direct_page -> cmp_direct_page_u8,
        direct_page_x -> cmp_direct_page_x_u8,
        direct_page_indirect -> cmp_direct_page_indirect_u8,
        direct_page_indexed_indirect -> cmp_direct_page_indexed_indirect_u8,
        direct_page_indirect_indexed -> cmp_direct_page_indirect_indexed_u8,
        direct_page_indirect_long -> cmp_direct_page_indirect_long_u8,
        direct_page_indirect_long_indexed -> cmp_direct_page_indirect_long_indexed_u8,
        absolute -> cmp_absolute_u8,
        absolute_x -> cmp_absolute_x_u8,
        absolute_y -> cmp_absolute_y_u8,
        absolute_long -> cmp_absolute_long_u8,
        absolute_long_x -> cmp_absolute_long_x_u8,
        stack_relative -> cmp_stack_relative_u8,
        stack_relative_indirect_indexed -> cmp_stack_relative_indirect_indexed_u8,
    ]
    (|registers, value| {
        let (difference, borrow) = (registers.a as u8).overflowing_sub(value);

        registers.p.carry = !borrow;
        registers.p.zero = difference == 0;
        registers.p.negative = difference.sign_bit();
    })
);

// CMP: Compare to accumulator (16-bit)
impl_read_op_u16!(
    [
        immediate -> cmp_immediate_u16,
        direct_page -> cmp_direct_page_u16,
        direct_page_x -> cmp_direct_page_x_u16,
        direct_page_indirect -> cmp_direct_page_indirect_u16,
        direct_page_indexed_indirect -> cmp_direct_page_indexed_indirect_u16,
        direct_page_indirect_indexed -> cmp_direct_page_indirect_indexed_u16,
        direct_page_indirect_long -> cmp_direct_page_indirect_long_u16,
        direct_page_indirect_long_indexed -> cmp_direct_page_indirect_long_indexed_u16,
        absolute -> cmp_absolute_u16,
        absolute_x -> cmp_absolute_x_u16,
        absolute_y -> cmp_absolute_y_u16,
        absolute_long -> cmp_absolute_long_u16,
        absolute_long_x -> cmp_absolute_long_x_u16,
        stack_relative -> cmp_stack_relative_u16,
        stack_relative_indirect_indexed -> cmp_stack_relative_indirect_indexed_u16,
    ]
    (|registers, value| {
        let (difference, borrow) = registers.a.overflowing_sub(value);

        registers.p.carry = !borrow;
        registers.p.zero = difference == 0;
        registers.p.negative = difference.sign_bit();
    })
);

// CPX: Compare to X register (8-bit)
impl_read_op_u8!(
    [
        immediate -> cpx_immediate_u8,
        direct_page -> cpx_direct_page_u8,
        absolute -> cpx_absolute_u8,
    ]
    (|registers, value| {
        let (difference, borrow) = (registers.x as u8).overflowing_sub(value);

        registers.p.carry = !borrow;
        registers.p.zero = difference == 0;
        registers.p.negative = difference.sign_bit();
    })
);

// CPX: Compare to X register (16-bit)
impl_read_op_u16!(
    [
        immediate -> cpx_immediate_u16,
        direct_page -> cpx_direct_page_u16,
        absolute -> cpx_absolute_u16,
    ]
    (|registers, value| {
        let (difference, borrow) = registers.x.overflowing_sub(value);

        registers.p.carry = !borrow;
        registers.p.zero = difference == 0;
        registers.p.negative = difference.sign_bit();
    })
);

// CPY: Compare to Y register (8-bit)
impl_read_op_u8!(
    [
        immediate -> cpy_immediate_u8,
        direct_page -> cpy_direct_page_u8,
        absolute -> cpy_absolute_u8,
    ]
    (|registers, value| {
        let (difference, borrow) = (registers.y as u8).overflowing_sub(value);

        registers.p.carry = !borrow;
        registers.p.zero = difference == 0;
        registers.p.negative = difference.sign_bit();
    })
);

// CPY: Compare to Y register (16-bit)
impl_read_op_u16!(
    [
        immediate -> cpy_immediate_u16,
        direct_page -> cpy_direct_page_u16,
        absolute -> cpy_absolute_u16,
    ]
    (|registers, value| {
        let (difference, borrow) = registers.y.overflowing_sub(value);

        registers.p.carry = !borrow;
        registers.p.zero = difference == 0;
        registers.p.negative = difference.sign_bit();
    })
);

// AND: Bitwise and (8-bit)
impl_read_op_u8!(
    [
        immediate -> and_immediate_u8,
        direct_page -> and_direct_page_u8,
        direct_page_x -> and_direct_page_x_u8,
        direct_page_indirect -> and_direct_page_indirect_u8,
        direct_page_indexed_indirect -> and_direct_page_indexed_indirect_u8,
        direct_page_indirect_indexed -> and_direct_page_indirect_indexed_u8,
        direct_page_indirect_long -> and_direct_page_indirect_long_u8,
        direct_page_indirect_long_indexed -> and_direct_page_indirect_long_indexed_u8,
        absolute -> and_absolute_u8,
        absolute_x -> and_absolute_x_u8,
        absolute_y -> and_absolute_y_u8,
        absolute_long -> and_absolute_long_u8,
        absolute_long_x -> and_absolute_long_x_u8,
        stack_relative -> and_stack_relative_u8,
        stack_relative_indirect_indexed -> and_stack_relative_indirect_indexed_u8,
    ]
    (|registers, value| {
        let result = (registers.a as u8) & value;
        registers.set_a_u8(result);

        registers.p.zero = result == 0;
        registers.p.negative = result.sign_bit();
    })
);

// AND: Bitwise and (16-bit)
impl_read_op_u16!(
    [
        immediate -> and_immediate_u16,
        direct_page -> and_direct_page_u16,
        direct_page_x -> and_direct_page_x_u16,
        direct_page_indirect -> and_direct_page_indirect_u16,
        direct_page_indexed_indirect -> and_direct_page_indexed_indirect_u16,
        direct_page_indirect_indexed -> and_direct_page_indirect_indexed_u16,
        direct_page_indirect_long -> and_direct_page_indirect_long_u16,
        direct_page_indirect_long_indexed -> and_direct_page_indirect_long_indexed_u16,
        absolute -> and_absolute_u16,
        absolute_x -> and_absolute_x_u16,
        absolute_y -> and_absolute_y_u16,
        absolute_long -> and_absolute_long_u16,
        absolute_long_x -> and_absolute_long_x_u16,
        stack_relative -> and_stack_relative_u16,
        stack_relative_indirect_indexed -> and_stack_relative_indirect_indexed_u16,
    ]
    (|registers, value| {
        let result = registers.a & value;
        registers.a = result;

        registers.p.zero = result == 0;
        registers.p.negative = result.sign_bit();
    })
);

// EOR: Exclusive or (8-bit)
impl_read_op_u8!(
    [
        immediate -> eor_immediate_u8,
        direct_page -> eor_direct_page_u8,
        direct_page_x -> eor_direct_page_x_u8,
        direct_page_indirect -> eor_direct_page_indirect_u8,
        direct_page_indexed_indirect -> eor_direct_page_indexed_indirect_u8,
        direct_page_indirect_indexed -> eor_direct_page_indirect_indexed_u8,
        direct_page_indirect_long -> eor_direct_page_indirect_long_u8,
        direct_page_indirect_long_indexed -> eor_direct_page_indirect_long_indexed_u8,
        absolute -> eor_absolute_u8,
        absolute_x -> eor_absolute_x_u8,
        absolute_y -> eor_absolute_y_u8,
        absolute_long -> eor_absolute_long_u8,
        absolute_long_x -> eor_absolute_long_x_u8,
        stack_relative -> eor_stack_relative_u8,
        stack_relative_indirect_indexed -> eor_stack_relative_indirect_indexed_u8,
    ]
    (|registers, value| {
        let result = (registers.a as u8) ^ value;
        registers.set_a_u8(result);

        registers.p.zero = result == 0;
        registers.p.negative = result.sign_bit();
    })
);

// EOR: Exclusive or (16-bit)
impl_read_op_u16!(
    [
        immediate -> eor_immediate_u16,
        direct_page -> eor_direct_page_u16,
        direct_page_x -> eor_direct_page_x_u16,
        direct_page_indirect -> eor_direct_page_indirect_u16,
        direct_page_indexed_indirect -> eor_direct_page_indexed_indirect_u16,
        direct_page_indirect_indexed -> eor_direct_page_indirect_indexed_u16,
        direct_page_indirect_long -> eor_direct_page_indirect_long_u16,
        direct_page_indirect_long_indexed -> eor_direct_page_indirect_long_indexed_u16,
        absolute -> eor_absolute_u16,
        absolute_x -> eor_absolute_x_u16,
        absolute_y -> eor_absolute_y_u16,
        absolute_long -> eor_absolute_long_u16,
        absolute_long_x -> eor_absolute_long_x_u16,
        stack_relative -> eor_stack_relative_u16,
        stack_relative_indirect_indexed -> eor_stack_relative_indirect_indexed_u16,
    ]
    (|registers, value| {
        let result = registers.a ^ value;
        registers.a = result;

        registers.p.zero = result == 0;
        registers.p.negative = result.sign_bit();
    })
);

// ORA: Bitwise or with accumulator (8-bit)
impl_read_op_u8!(
    [
        immediate -> ora_immediate_u8,
        direct_page -> ora_direct_page_u8,
        direct_page_x -> ora_direct_page_x_u8,
        direct_page_indirect -> ora_direct_page_indirect_u8,
        direct_page_indexed_indirect -> ora_direct_page_indexed_indirect_u8,
        direct_page_indirect_indexed -> ora_direct_page_indirect_indexed_u8,
        direct_page_indirect_long -> ora_direct_page_indirect_long_u8,
        direct_page_indirect_long_indexed -> ora_direct_page_indirect_long_indexed_u8,
        absolute -> ora_absolute_u8,
        absolute_x -> ora_absolute_x_u8,
        absolute_y -> ora_absolute_y_u8,
        absolute_long -> ora_absolute_long_u8,
        absolute_long_x -> ora_absolute_long_x_u8,
        stack_relative -> ora_stack_relative_u8,
        stack_relative_indirect_indexed -> ora_stack_relative_indirect_indexed_u8,
    ]
    (|registers, value| {
        let result = (registers.a as u8) | value;
        registers.set_a_u8(result);

        registers.p.zero = result == 0;
        registers.p.negative = result.sign_bit();
    })
);

// ORA: Bitwise or with accumulator (16-bit)
impl_read_op_u16!(
    [
        immediate -> ora_immediate_u16,
        direct_page -> ora_direct_page_u16,
        direct_page_x -> ora_direct_page_x_u16,
        direct_page_indirect -> ora_direct_page_indirect_u16,
        direct_page_indexed_indirect -> ora_direct_page_indexed_indirect_u16,
        direct_page_indirect_indexed -> ora_direct_page_indirect_indexed_u16,
        direct_page_indirect_long -> ora_direct_page_indirect_long_u16,
        direct_page_indirect_long_indexed -> ora_direct_page_indirect_long_indexed_u16,
        absolute -> ora_absolute_u16,
        absolute_x -> ora_absolute_x_u16,
        absolute_y -> ora_absolute_y_u16,
        absolute_long -> ora_absolute_long_u16,
        absolute_long_x -> ora_absolute_long_x_u16,
        stack_relative -> ora_stack_relative_u16,
        stack_relative_indirect_indexed -> ora_stack_relative_indirect_indexed_u16,
    ]
    (|registers, value| {
        let result = registers.a | value;
        registers.a = result;

        registers.p.zero = result == 0;
        registers.p.negative = result.sign_bit();
    })
);

macro_rules! impl_increment {
    ($registers:expr, $value:expr, $inc_method:ident) => {{
        let result = $value.$inc_method(1);
        $registers.p.zero = result == 0;
        $registers.p.negative = result.sign_bit();

        result
    }};
}

// INC: Increment (8-bit)
impl_modify_op_u8!(
    [
        accumulator -> inc_accumulator_u8,
        direct_page -> inc_direct_page_u8,
        direct_page_x -> inc_direct_page_x_u8,
        absolute -> inc_absolute_u8,
        absolute_x -> inc_absolute_x_u8,
    ]
    (|registers, value| {
        impl_increment!(registers, value, wrapping_add)
    })
);

// INC: Increment (16-bit)
impl_modify_op_u16!(
    [
        accumulator -> inc_accumulator_u16,
        direct_page -> inc_direct_page_u16,
        direct_page_x -> inc_direct_page_x_u16,
        absolute -> inc_absolute_u16,
        absolute_x -> inc_absolute_x_u16,
    ]
    (|registers, value| {
        impl_increment!(registers, value, wrapping_add)
    })
);

// DEC: Decrement (8-bit)
impl_modify_op_u8!(
    [
        accumulator -> dec_accumulator_u8,
        direct_page -> dec_direct_page_u8,
        direct_page_x -> dec_direct_page_x_u8,
        absolute -> dec_absolute_u8,
        absolute_x -> dec_absolute_x_u8,
    ]
    (|registers, value| {
        impl_increment!(registers, value, wrapping_sub)
    })
);

// DEC: Decrement (16-bit)
impl_modify_op_u16!(
    [
        accumulator -> dec_accumulator_u16,
        direct_page -> dec_direct_page_u16,
        direct_page_x -> dec_direct_page_x_u16,
        absolute -> dec_absolute_u16,
        absolute_x -> dec_absolute_x_u16,
    ]
    (|registers, value| {
        impl_increment!(registers, value, wrapping_sub)
    })
);

// INX: Increment X register
impl_registers_op!(inx_u8, |registers| {
    let result = impl_increment!(registers, registers.x as u8, wrapping_add);
    registers.x = result.into();
});
impl_registers_op!(inx_u16, |registers| {
    let result = impl_increment!(registers, registers.x, wrapping_add);
    registers.x = result;
});

// INY: Increment Y register
impl_registers_op!(iny_u8, |registers| {
    let result = impl_increment!(registers, registers.y as u8, wrapping_add);
    registers.y = result.into();
});
impl_registers_op!(iny_u16, |registers| {
    let result = impl_increment!(registers, registers.y, wrapping_add);
    registers.y = result;
});

// DEX: Decrement X register
impl_registers_op!(dex_u8, |registers| {
    let result = impl_increment!(registers, registers.x as u8, wrapping_sub);
    registers.x = result.into();
});
impl_registers_op!(dex_u16, |registers| {
    let result = impl_increment!(registers, registers.x, wrapping_sub);
    registers.x = result;
});

// DEY: Decrement Y register
impl_registers_op!(dey_u8, |registers| {
    let result = impl_increment!(registers, registers.y as u8, wrapping_sub);
    registers.y = result.into();
});
impl_registers_op!(dey_u16, |registers| {
    let result = impl_increment!(registers, registers.y, wrapping_sub);
    registers.y = result;
});
