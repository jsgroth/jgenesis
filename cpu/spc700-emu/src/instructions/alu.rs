#[allow(clippy::wildcard_imports)]
use super::*;
use crate::StatusRegister;
use jgenesis_common::num::U16Ext;

fn adc(operand_l: u8, operand_r: u8, psw: &mut StatusRegister) -> u8 {
    let existing_carry: u8 = psw.carry.into();

    let (partial_sum, carry1) = operand_l.overflowing_add(operand_r);
    let (sum, carry2) = partial_sum.overflowing_add(existing_carry);
    let carry = carry1 || carry2;

    let bit_6_carry = (operand_l & 0x7F) + (operand_r & 0x7F) + existing_carry >= 0x80;
    let overflow = bit_6_carry != carry;

    let half_carry = (operand_l & 0x0F) + (operand_r & 0x0F) + existing_carry >= 0x10;

    psw.zero = sum == 0;
    psw.negative = sum.sign_bit();
    psw.carry = carry;
    psw.half_carry = half_carry;
    psw.overflow = overflow;

    sum
}

impl_read_op!(
    [
        immediate -> adc_imm,
        x_indirect -> adc_xind,
        direct_page -> adc_dp,
        direct_page_x -> adc_dpx,
        absolute -> adc_abs,
        absolute_x -> adc_absx,
        absolute_y -> adc_absy,
        indirect_x -> adc_indx,
        indirect_y -> adc_indy,
    ]
    (|registers, value| {
        registers.a = adc(registers.a, value, &mut registers.psw);
    })
);

impl_memory_to_memory_op!(
    [
        x_y -> adc_x_y_ind,
        dp_dp -> adc_dp_dp,
        dp_imm -> adc_dp_imm,
    ]
    (|registers, operand_l, operand_r| {
        adc(operand_l, operand_r, &mut registers.psw)
    })
);

fn sbc(operand_l: u8, operand_r: u8, psw: &mut StatusRegister) -> u8 {
    let existing_borrow: u8 = (!psw.carry).into();

    let (partial_diff, borrow1) = operand_l.overflowing_sub(operand_r);
    let (difference, borrow2) = partial_diff.overflowing_sub(existing_borrow);
    let borrow = borrow1 || borrow2;

    let bit_6_borrow = operand_l & 0x7F < (operand_r & 0x7F) + existing_borrow;
    let overflow = bit_6_borrow != borrow;

    let half_borrow = operand_l & 0x0F < (operand_r & 0x0F) + existing_borrow;

    psw.zero = difference == 0;
    psw.negative = difference.sign_bit();
    psw.carry = !borrow;
    psw.half_carry = !half_borrow;
    psw.overflow = overflow;

    difference
}

impl_read_op!(
    [
        immediate -> sbc_imm,
        x_indirect -> sbc_xind,
        direct_page -> sbc_dp,
        direct_page_x -> sbc_dpx,
        absolute -> sbc_abs,
        absolute_x -> sbc_absx,
        absolute_y -> sbc_absy,
        indirect_x -> sbc_indx,
        indirect_y -> sbc_indy,
    ]
    (|registers, value| {
        registers.a = sbc(registers.a, value, &mut registers.psw);
    })
);

impl_memory_to_memory_op!(
    [
        x_y -> sbc_x_y_ind,
        dp_dp -> sbc_dp_dp,
        dp_imm -> sbc_dp_imm,
    ]
    (|registers, operand_l, operand_r| {
        sbc(operand_l, operand_r, &mut registers.psw)
    })
);

fn cmp(operand_l: u8, operand_r: u8, psw: &mut StatusRegister) {
    let (difference, borrow) = operand_l.overflowing_sub(operand_r);
    psw.zero = difference == 0;
    psw.negative = difference.sign_bit();
    psw.carry = !borrow;
}

impl_read_op!(
    [
        immediate -> cmp_a_imm,
        x_indirect -> cmp_a_xind,
        direct_page -> cmp_a_dp,
        direct_page_x -> cmp_a_dpx,
        absolute -> cmp_a_abs,
        absolute_x -> cmp_a_absx,
        absolute_y -> cmp_a_absy,
        indirect_x -> cmp_a_indx,
        indirect_y -> cmp_a_indy,
    ]
    (|registers, value| {
        cmp(registers.a, value, &mut registers.psw);
    })
);

impl_memory_to_memory_op!(
    [
        x_y_no_write -> cmp_x_y_ind,
        dp_dp_no_write -> cmp_dp_dp,
        dp_imm_no_write -> cmp_dp_imm,
    ]
    (|registers, operand_l, operand_r| {
        cmp(operand_l, operand_r, &mut registers.psw);
        operand_l
    })
);

impl_read_op!(
    [
        immediate -> cmp_x_imm,
        direct_page -> cmp_x_dp,
        absolute -> cmp_x_abs,
    ]
    (|registers, value| {
        cmp(registers.x, value, &mut registers.psw);
    })
);

impl_read_op!(
    [
        immediate -> cmp_y_imm,
        direct_page -> cmp_y_dp,
        absolute -> cmp_y_abs,
    ]
    (|registers, value| {
        cmp(registers.y, value, &mut registers.psw);
    })
);

impl_read_op!(
    [
        immediate -> and_imm,
        x_indirect -> and_xind,
        direct_page -> and_dp,
        direct_page_x -> and_dpx,
        absolute -> and_abs,
        absolute_x -> and_absx,
        absolute_y -> and_absy,
        indirect_x -> and_indx,
        indirect_y -> and_indy,
    ]
    (|registers, value| {
        registers.a &= value;
        registers.psw.zero = registers.a == 0;
        registers.psw.negative = registers.a.sign_bit();
    })
);

impl_memory_to_memory_op!(
    [
        x_y -> and_x_y_ind,
        dp_dp -> and_dp_dp,
        dp_imm -> and_dp_imm,
    ]
    (|registers, operand_l, operand_r| {
        let result = operand_l & operand_r;
        registers.psw.zero = result == 0;
        registers.psw.negative = result.sign_bit();
        result
    })
);

impl_read_op!(
    [
        immediate -> or_imm,
        x_indirect -> or_xind,
        direct_page -> or_dp,
        direct_page_x -> or_dpx,
        absolute -> or_abs,
        absolute_x -> or_absx,
        absolute_y -> or_absy,
        indirect_x -> or_indx,
        indirect_y -> or_indy,
    ]
    (|registers, value| {
        registers.a |= value;
        registers.psw.zero = registers.a == 0;
        registers.psw.negative = registers.a.sign_bit();
    })
);

impl_memory_to_memory_op!(
    [
        x_y -> or_x_y_ind,
        dp_dp -> or_dp_dp,
        dp_imm -> or_dp_imm,
    ]
    (|registers, operand_l, operand_r| {
        let result = operand_l | operand_r;
        registers.psw.zero = result == 0;
        registers.psw.negative = result.sign_bit();
        result
    })
);

impl_read_op!(
    [
        immediate -> eor_imm,
        x_indirect -> eor_xind,
        direct_page -> eor_dp,
        direct_page_x -> eor_dpx,
        absolute -> eor_abs,
        absolute_x -> eor_absx,
        absolute_y -> eor_absy,
        indirect_x -> eor_indx,
        indirect_y -> eor_indy,
    ]
    (|registers, value| {
        registers.a ^= value;
        registers.psw.zero = registers.a == 0;
        registers.psw.negative = registers.a.sign_bit();
    })
);

impl_memory_to_memory_op!(
    [
        x_y -> eor_x_y_ind,
        dp_dp -> eor_dp_dp,
        dp_imm -> eor_dp_imm,
    ]
    (|registers, operand_l, operand_r| {
        let result = operand_l ^ operand_r;
        registers.psw.zero = result == 0;
        registers.psw.negative = result.sign_bit();
        result
    })
);

impl_modify_op!(
    [
        accumulator -> inc_a,
        direct_page -> inc_dp,
        direct_page_x -> inc_dpx,
        absolute -> inc_abs,
    ]
    (|registers, value| {
        let result = value.wrapping_add(1);
        registers.psw.zero = result == 0;
        registers.psw.negative = result.sign_bit();
        result
    })
);

impl_modify_op!(
    [
        accumulator -> dec_a,
        direct_page -> dec_dp,
        direct_page_x -> dec_dpx,
        absolute -> dec_abs,
    ]
    (|registers, value| {
        let result = value.wrapping_sub(1);
        registers.psw.zero = result == 0;
        registers.psw.negative = result.sign_bit();
        result
    })
);

macro_rules! impl_register_increment_op {
    ($name:ident, $register:ident, $inc_method:ident) => {
        impl_registers_op!($name, |registers| {
            let value = registers.$register.$inc_method(1);
            registers.$register = value;

            registers.psw.zero = value == 0;
            registers.psw.negative = value.sign_bit();
        });
    };
}

impl_register_increment_op!(inc_x, x, wrapping_add);
impl_register_increment_op!(inc_y, y, wrapping_add);
impl_register_increment_op!(dec_x, x, wrapping_sub);
impl_register_increment_op!(dec_y, y, wrapping_sub);

pub(crate) fn addw<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            let address = u16::from_le_bytes([cpu.state.t0, cpu.direct_page_msb()]);
            cpu.state.t1 = bus.read(address);
        }
        3 => {
            bus.idle();
        }
        4 => {
            cpu.final_cycle();

            let address_lsb = cpu.state.t0.wrapping_add(1);
            let address = u16::from_le_bytes([address_lsb, cpu.direct_page_msb()]);
            let operand_msb = bus.read(address);
            let operand = u16::from_le_bytes([cpu.state.t1, operand_msb]);

            let ya = cpu.registers.ya();
            let (sum, carry) = ya.overflowing_add(operand);

            let bit_14_carry = (ya & 0x7FFF) + (operand & 0x7FFF) >= 0x8000;
            let overflow = bit_14_carry != carry;

            let half_carry = (ya & 0x0FFF) + (operand & 0x0FFF) >= 0x1000;

            cpu.registers.set_ya(sum);
            cpu.registers.psw.zero = sum == 0;
            cpu.registers.psw.negative = sum.sign_bit();
            cpu.registers.psw.carry = carry;
            cpu.registers.psw.half_carry = half_carry;
            cpu.registers.psw.overflow = overflow;
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn subw<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            let address = u16::from_le_bytes([cpu.state.t0, cpu.direct_page_msb()]);
            cpu.state.t1 = bus.read(address);
        }
        3 => {
            bus.idle();
        }
        4 => {
            cpu.final_cycle();

            let address_lsb = cpu.state.t0.wrapping_add(1);
            let address = u16::from_le_bytes([address_lsb, cpu.direct_page_msb()]);
            let operand_msb = bus.read(address);
            let operand = u16::from_le_bytes([cpu.state.t1, operand_msb]);

            let ya = cpu.registers.ya();
            let (difference, borrow) = ya.overflowing_sub(operand);

            let bit_14_borrow = ya & 0x7FFF < operand & 0x7FFF;
            let overflow = bit_14_borrow != borrow;

            let half_borrow = ya & 0x0FFF < operand & 0x0FFF;

            cpu.registers.set_ya(difference);
            cpu.registers.psw.zero = difference == 0;
            cpu.registers.psw.negative = difference.sign_bit();
            cpu.registers.psw.carry = !borrow;
            cpu.registers.psw.half_carry = !half_borrow;
            cpu.registers.psw.overflow = overflow;
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn cmpw<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            let address = u16::from_le_bytes([cpu.state.t0, cpu.direct_page_msb()]);
            cpu.state.t1 = bus.read(address);
        }
        3 => {
            cpu.final_cycle();

            let address_lsb = cpu.state.t0.wrapping_add(1);
            let address = u16::from_le_bytes([address_lsb, cpu.direct_page_msb()]);
            let operand_msb = bus.read(address);
            let operand = u16::from_le_bytes([cpu.state.t1, operand_msb]);

            let ya = cpu.registers.ya();
            let (difference, borrow) = ya.overflowing_sub(operand);

            cpu.registers.psw.zero = difference == 0;
            cpu.registers.psw.negative = difference.sign_bit();
            cpu.registers.psw.carry = !borrow;
        }
        _ => invalid_cycle!(cpu),
    }
}

macro_rules! impl_16bit_increment {
    ($name:ident, $inc_method:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    let address = u16::from_le_bytes([cpu.state.t0, cpu.direct_page_msb()]);
                    cpu.state.t1 = bus.read(address);
                }
                3 => {
                    let address = u16::from_le_bytes([cpu.state.t0, cpu.direct_page_msb()]);
                    bus.write(address, cpu.state.t1.$inc_method(1));
                }
                4 => {
                    let address_lsb = cpu.state.t0.wrapping_add(1);
                    let address = u16::from_le_bytes([address_lsb, cpu.direct_page_msb()]);
                    cpu.state.t2 = bus.read(address);
                }
                5 => {
                    cpu.final_cycle();

                    let value = u16::from_le_bytes([cpu.state.t1, cpu.state.t2]).$inc_method(1);

                    let address_lsb = cpu.state.t0.wrapping_add(1);
                    let address = u16::from_le_bytes([address_lsb, cpu.direct_page_msb()]);
                    bus.write(address, value.msb());

                    cpu.registers.psw.zero = value == 0;
                    cpu.registers.psw.negative = value.sign_bit();
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

impl_16bit_increment!(incw, wrapping_add);
impl_16bit_increment!(decw, wrapping_sub);

pub(crate) fn mul<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    // MUL takes 9 cycles, but every cycle is a bus idle cycle except for the opcode fetch
    match cpu.state.cycle {
        1..=7 => {
            bus.idle();
        }
        8 => {
            cpu.final_cycle();
            bus.idle();

            let result = u16::from(cpu.registers.y) * u16::from(cpu.registers.a);
            cpu.registers.set_ya(result);

            // Z flag is set based on the MSB (Y), not the full 16-bit result
            cpu.registers.psw.zero = cpu.registers.y == 0;
            cpu.registers.psw.negative = cpu.registers.y.sign_bit();
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn div<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    // DIV takes 12 cycles, but every cycle is a bus idle cycle except for the opcode fetch
    match cpu.state.cycle {
        1..=10 => {
            bus.idle();
        }
        11 => {
            cpu.final_cycle();
            bus.idle();

            // Algorithm from https://board.zsnes.com/phpBB3/viewtopic.php?t=3753

            cpu.registers.psw.overflow = cpu.registers.y >= cpu.registers.x;
            cpu.registers.psw.half_carry = cpu.registers.y & 0x0F >= cpu.registers.x & 0x0F;

            let x: u16 = cpu.registers.x.into();
            let y: u16 = cpu.registers.y.into();
            let ya = cpu.registers.ya();
            if y < 2 * x {
                // Normal division
                cpu.registers.a = (ya / x) as u8;
                cpu.registers.y = (ya % x) as u8;
            } else {
                // Overflow (including divide by zero)
                cpu.registers.a = (255 - (ya - x * 0x200) / (256 - x)) as u8;
                cpu.registers.y = (x + (ya - x * 0x200) % (256 - x)) as u8;
            }

            cpu.registers.psw.zero = cpu.registers.a == 0;
            cpu.registers.psw.negative = cpu.registers.a.sign_bit();
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn daa<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            bus.idle();
        }
        2 => {
            cpu.final_cycle();
            bus.idle();

            // Algorithm from https://board.zsnes.com/phpBB3/viewtopic.php?t=3753

            if cpu.registers.psw.carry || cpu.registers.a > 0x99 {
                cpu.registers.psw.carry = true;
                cpu.registers.a = cpu.registers.a.wrapping_add(0x60);
            }

            if cpu.registers.psw.half_carry || (cpu.registers.a & 0x0F) > 0x09 {
                cpu.registers.a = cpu.registers.a.wrapping_add(0x06);
            }

            cpu.registers.psw.zero = cpu.registers.a == 0;
            cpu.registers.psw.negative = cpu.registers.a.sign_bit();
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn das<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            bus.idle();
        }
        2 => {
            cpu.final_cycle();
            bus.idle();

            // Algorithm from https://board.zsnes.com/phpBB3/viewtopic.php?t=3753

            if !cpu.registers.psw.carry || cpu.registers.a > 0x99 {
                cpu.registers.psw.carry = false;
                cpu.registers.a = cpu.registers.a.wrapping_sub(0x60);
            }

            if !cpu.registers.psw.half_carry || (cpu.registers.a & 0x0F) > 0x09 {
                cpu.registers.a = cpu.registers.a.wrapping_sub(0x06);
            }

            cpu.registers.psw.zero = cpu.registers.a == 0;
            cpu.registers.psw.negative = cpu.registers.a.sign_bit();
        }
        _ => invalid_cycle!(cpu),
    }
}
