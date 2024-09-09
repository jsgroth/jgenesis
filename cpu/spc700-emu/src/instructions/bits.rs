#[allow(clippy::wildcard_imports)]
use super::*;

impl_modify_op!(
    [
        accumulator -> asl_a,
        direct_page -> asl_dp,
        direct_page_x -> asl_dpx,
        absolute -> asl_abs,
    ]
    (|registers, value| {
        registers.psw.carry = value.sign_bit();

        let result = value << 1;
        registers.psw.zero = result == 0;
        registers.psw.negative = result.sign_bit();
        result
    })
);

impl_modify_op!(
    [
        accumulator -> lsr_a,
        direct_page -> lsr_dp,
        direct_page_x -> lsr_dpx,
        absolute -> lsr_abs,
    ]
    (|registers, value| {
        registers.psw.carry = value.bit(0);

        let result = value >> 1;
        registers.psw.zero = result == 0;
        registers.psw.negative = result.sign_bit();
        result
    })
);

impl_modify_op!(
    [
        accumulator -> rol_a,
        direct_page -> rol_dp,
        direct_page_x -> rol_dpx,
        absolute -> rol_abs,
    ]
    (|registers, value| {
        let result = (value << 1) | u8::from(registers.psw.carry);
        registers.psw.zero = result == 0;
        registers.psw.negative = result.sign_bit();
        registers.psw.carry = value.sign_bit();
        result
    })
);

impl_modify_op!(
    [
        accumulator -> ror_a,
        direct_page -> ror_dp,
        direct_page_x -> ror_dpx,
        absolute -> ror_abs,
    ]
    (|registers, value| {
        let result = (value >> 1) | (u8::from(registers.psw.carry) << 7);
        registers.psw.zero = result == 0;
        registers.psw.negative = result.sign_bit();
        registers.psw.carry = value.bit(0);
        result
    })
);

macro_rules! bit_op {
    (set $value:expr, $bit:expr) => {
        $value | (1 << $bit)
    };
    (clear $value:expr, $bit:expr) => {
        $value & !(1 << $bit)
    };
}

macro_rules! impl_set_bit {
    ($name:ident, $op:tt) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B, bit: u8) {
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

                    let value = bit_op!($op cpu.state.t1, bit);

                    let address = u16::from_le_bytes([cpu.state.t0, cpu.direct_page_msb()]);
                    bus.write(address, value);
                }
                _ => invalid_cycle!(cpu)
            }
        }
    };
}

impl_set_bit!(set1, set);
impl_set_bit!(clr1, clear);

macro_rules! impl_and1 {
    ($name:ident $($not:tt)?) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    cpu.state.t1 = fetch_operand(cpu, bus);
                }
                3 => {
                    cpu.final_cycle();

                    let address_msb = cpu.state.t1 & 0x1F;
                    let address = u16::from_le_bytes([cpu.state.t0, address_msb]);
                    let value = bus.read(address);

                    let bit_index = cpu.state.t1 >> 5;
                    let bit = value.bit(bit_index);

                    cpu.registers.psw.carry &= $($not)? bit;
                }
                _ => invalid_cycle!(cpu)
            }
        }
    }
}

impl_and1!(and1);
impl_and1!(and1_cpl !);

macro_rules! impl_or1 {
    ($name:ident, $op:tt $(, $not:tt)?) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    cpu.state.t1 = fetch_operand(cpu, bus);
                }
                3 => {
                    let address_msb = cpu.state.t1 & 0x1F;
                    let address = u16::from_le_bytes([cpu.state.t0, address_msb]);
                    cpu.state.t2 = bus.read(address);
                }
                4 => {
                    cpu.final_cycle();
                    bus.idle();

                    let bit_index = cpu.state.t1 >> 5;
                    let bit = cpu.state.t2.bit(bit_index);

                    cpu.registers.psw.carry $op $($not)? bit;
                }
                _ => invalid_cycle!(cpu)
            }
        }
    }
}

impl_or1!(or1, |=);
impl_or1!(or1_cpl, |=, !);
impl_or1!(eor1, ^=);

pub(crate) fn not1<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            cpu.state.t1 = fetch_operand(cpu, bus);
        }
        3 => {
            let address_msb = cpu.state.t1 & 0x1F;
            let address = u16::from_le_bytes([cpu.state.t0, address_msb]);
            cpu.state.t2 = bus.read(address);
        }
        4 => {
            cpu.final_cycle();

            let bit_index = cpu.state.t1 >> 5;
            let value = cpu.state.t2 ^ (1 << bit_index);

            let address_msb = cpu.state.t1 & 0x1F;
            let address = u16::from_le_bytes([cpu.state.t0, address_msb]);
            bus.write(address, value);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn mov1_c_bit<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            cpu.state.t1 = fetch_operand(cpu, bus);
        }
        3 => {
            cpu.final_cycle();

            let address_msb = cpu.state.t1 & 0x1F;
            let address = u16::from_le_bytes([cpu.state.t0, address_msb]);
            let value = bus.read(address);

            let bit_index = cpu.state.t1 >> 5;
            cpu.registers.psw.carry = value.bit(bit_index);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn mov1_bit_c<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            cpu.state.t1 = fetch_operand(cpu, bus);
        }
        3 => {
            let address_msb = cpu.state.t1 & 0x1F;
            let address = u16::from_le_bytes([cpu.state.t0, address_msb]);
            cpu.state.t2 = bus.read(address);
        }
        4 => {
            bus.idle();
        }
        5 => {
            cpu.final_cycle();

            let bit_index = cpu.state.t1 >> 5;
            let value = (cpu.state.t2 & !(1 << bit_index))
                | (u8::from(cpu.registers.psw.carry) << bit_index);

            let address_msb = cpu.state.t1 & 0x1F;
            let address = u16::from_le_bytes([cpu.state.t0, address_msb]);
            bus.write(address, value);
        }
        _ => invalid_cycle!(cpu),
    }
}

macro_rules! bits_op {
    (set $value:expr, $a:expr) => {
        $value | $a
    };
    (clear $value:expr, $a:expr) => {
        $value & !$a
    };
}

macro_rules! impl_test_and_set_bits {
    ($name:ident, $op:tt) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    cpu.state.t1 = fetch_operand(cpu, bus);
                }
                3 => {
                    // No-op read
                    let address = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
                    bus.read(address);
                }
                4 => {
                    let address = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
                    cpu.state.t2 = bus.read(address);
                }
                5 => {
                    cpu.final_cycle();

                    // Z and N flags are set based on (A - value)
                    let difference = cpu.registers.a.wrapping_sub(cpu.state.t2);
                    cpu.registers.psw.zero = difference == 0;
                    cpu.registers.psw.negative = difference.sign_bit();

                    let address = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
                    let value = bits_op!($op cpu.state.t2, cpu.registers.a);
                    bus.write(address, value);
                }
                _ => invalid_cycle!(cpu)
            }
        }
    }
}

impl_test_and_set_bits!(tset1, set);
impl_test_and_set_bits!(tclr1, clear);

pub(crate) fn xcn<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1..=3 => {
            bus.idle();
        }
        4 => {
            cpu.final_cycle();
            bus.idle();

            cpu.registers.a = cpu.registers.a.rotate_left(4);
            cpu.registers.psw.zero = cpu.registers.a == 0;
            cpu.registers.psw.negative = cpu.registers.a.sign_bit();
        }
        _ => invalid_cycle!(cpu),
    }
}
