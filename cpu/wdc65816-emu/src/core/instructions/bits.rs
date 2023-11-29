#[allow(clippy::wildcard_imports)]
use super::*;

// BIT: Test bits (8-bit)
// Immediate gets a separate implementation because it doesn't set the N and V flags
impl_read_op_u8!(
    [immediate -> bit_immediate_u8]
    (|registers, value| {
        registers.p.zero = (registers.a as u8) & value == 0;
    })
);

// BIT: Test bits (16-bit)
// Immediate gets a separate implementation because it doesn't set the N and V flags
impl_read_op_u16!(
    [immediate -> bit_immediate_u16]
    (|registers, value| {
        registers.p.zero = registers.a & value == 0;
    })
);

// BIT: Test bits (8-bit)
impl_read_op_u8!(
    [
        direct_page -> bit_direct_page_u8,
        direct_page_x -> bit_direct_page_x_u8,
        absolute -> bit_absolute_u8,
        absolute_x -> bit_absolute_x_u8,
    ]
    (|registers, value| {
        registers.p.zero = (registers.a as u8) & value == 0;
        registers.p.negative = value.sign_bit();
        registers.p.overflow = value.bit(6);
    })
);

// BIT: Test bits (16-bit)
impl_read_op_u16!(
    [
        direct_page -> bit_direct_page_u16,
        direct_page_x -> bit_direct_page_x_u16,
        absolute -> bit_absolute_u16,
        absolute_x -> bit_absolute_x_u16,
    ]
    (|registers, value| {
        registers.p.zero = registers.a & value == 0;
        registers.p.negative = value.sign_bit();
        registers.p.overflow = value.bit(14);
    })
);

// TRB: Test and reset bits (8-bit)
impl_modify_op_u8!(
    [
        direct_page -> trb_direct_page_u8,
        absolute -> trb_absolute_u8,
    ]
    (|registers, value| {
        let a_lsb = registers.a as u8;
        let test = a_lsb & value;
        registers.p.zero = test == 0;

        value & !a_lsb
    })
);

// TRB: Test and reset bits (16-bit)
impl_modify_op_u16!(
    [
        direct_page -> trb_direct_page_u16,
        absolute -> trb_absolute_u16,
    ]
    (|registers, value| {
        registers.p.zero = registers.a & value == 0;
        value & !registers.a
    })
);

// TSB: Test and set bits (8-bit)
impl_modify_op_u8!(
    [
        direct_page -> tsb_direct_page_u8,
        absolute -> tsb_absolute_u8,
    ]
    (|registers, value| {
        let a_lsb = registers.a as u8;
        let test = a_lsb & value;
        registers.p.zero = test == 0;

        value | a_lsb
    })
);

// TSB: Test and set bits (16-bit)
impl_modify_op_u16!(
    [
        direct_page -> tsb_direct_page_u16,
        absolute -> tsb_absolute_u16,
    ]
    (|registers, value| {
        registers.p.zero = registers.a & value == 0;
        value | registers.a
    })
);

// ASL: Arithmetic shift left (8-bit)
impl_modify_op_u8!(
    [
        accumulator -> asl_accumulator_u8,
        direct_page -> asl_direct_page_u8,
        direct_page_x -> asl_direct_page_x_u8,
        absolute -> asl_absolute_u8,
        absolute_x -> asl_absolute_x_u8,
    ]
    (|registers, value| {
        let result = value << 1;
        registers.p.zero = result == 0;
        registers.p.negative = result.sign_bit();
        registers.p.carry = value.sign_bit();

        result
    })
);

// ASL: Arithmetic shift left (16-bit)
impl_modify_op_u16!(
    [
        accumulator -> asl_accumulator_u16,
        direct_page -> asl_direct_page_u16,
        direct_page_x -> asl_direct_page_x_u16,
        absolute -> asl_absolute_u16,
        absolute_x -> asl_absolute_x_u16,
    ]
    (|registers, value| {
        let result = value << 1;
        registers.p.zero = result == 0;
        registers.p.negative = result.sign_bit();
        registers.p.carry = value.sign_bit();

        result
    })
);

// LSR: Logical shift right (8-bit)
impl_modify_op_u8!(
    [
        accumulator -> lsr_accumulator_u8,
        direct_page -> lsr_direct_page_u8,
        direct_page_x -> lsr_direct_page_x_u8,
        absolute -> lsr_absolute_u8,
        absolute_x -> lsr_absolute_x_u8,
    ]
    (|registers, value| {
        let result = value >> 1;
        registers.p.zero = result == 0;
        registers.p.negative = result.sign_bit();
        registers.p.carry = value.bit(0);

        result
    })
);

// LSR: Logical shift right (16-bit)
impl_modify_op_u16!(
    [
        accumulator -> lsr_accumulator_u16,
        direct_page -> lsr_direct_page_u16,
        direct_page_x -> lsr_direct_page_x_u16,
        absolute -> lsr_absolute_u16,
        absolute_x -> lsr_absolute_x_u16,
    ]
    (|registers, value| {
        let result = value >> 1;
        registers.p.zero = result == 0;
        registers.p.negative = result.sign_bit();
        registers.p.carry = value.bit(0);

        result
    })
);

// ROL: Rotate left (8-bit)
impl_modify_op_u8!(
    [
        accumulator -> rol_accumulator_u8,
        direct_page -> rol_direct_page_u8,
        direct_page_x -> rol_direct_page_x_u8,
        absolute -> rol_absolute_u8,
        absolute_x -> rol_absolute_x_u8,
    ]
    (|registers, value| {
        let result = (value << 1) | u8::from(registers.p.carry);
        registers.p.zero = result == 0;
        registers.p.negative = result.sign_bit();
        registers.p.carry = value.sign_bit();

        result
    })
);

// ROL: Rotate left (16-bit)
impl_modify_op_u16!(
    [
        accumulator -> rol_accumulator_u16,
        direct_page -> rol_direct_page_u16,
        direct_page_x -> rol_direct_page_x_u16,
        absolute -> rol_absolute_u16,
        absolute_x -> rol_absolute_x_u16,
    ]
    (|registers, value| {
        let result = (value << 1) | u16::from(registers.p.carry);
        registers.p.zero = result == 0;
        registers.p.negative = result.sign_bit();
        registers.p.carry = value.sign_bit();

        result
    })
);

// ROR: Rotate right (8-bit)
impl_modify_op_u8!(
    [
        accumulator -> ror_accumulator_u8,
        direct_page -> ror_direct_page_u8,
        direct_page_x -> ror_direct_page_x_u8,
        absolute -> ror_absolute_u8,
        absolute_x -> ror_absolute_x_u8,
    ]
    (|registers, value| {
        let result = (value >> 1) | (u8::from(registers.p.carry) << 7);
        registers.p.zero = result == 0;
        registers.p.negative = result.sign_bit();
        registers.p.carry = value.bit(0);

        result
    })
);

// ROR: Rotate right (16-bit)
impl_modify_op_u16!(
    [
        accumulator -> ror_accumulator_u16,
        direct_page -> ror_direct_page_u16,
        direct_page_x -> ror_direct_page_x_u16,
        absolute -> ror_absolute_u16,
        absolute_x -> ror_absolute_x_u16,
    ]
    (|registers, value| {
        let result = (value >> 1) | (u16::from(registers.p.carry) << 15);
        registers.p.zero = result == 0;
        registers.p.negative = result.sign_bit();
        registers.p.carry = value.bit(0);

        result
    })
);
