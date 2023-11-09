#[allow(clippy::wildcard_imports)]
use super::*;

impl_read_op!(
    [
        immediate -> mov_a_imm,
        x_indirect -> mov_a_xind,
        x_indirect_postinc -> mov_a_xind_postinc,
        direct_page -> mov_a_dp,
        direct_page_x -> mov_a_dpx,
        absolute -> mov_a_abs,
        absolute_x -> mov_a_absx,
        absolute_y -> mov_a_absy,
        indirect_x -> mov_a_indx,
        indirect_y -> mov_a_indy,
    ]
    (|registers, value| {
        registers.a = value;
        registers.psw.zero = value == 0;
        registers.psw.negative = value.sign_bit();
    })
);

impl_read_op!(
    [
        immediate -> mov_x_imm,
        direct_page -> mov_x_dp,
        direct_page_y -> mov_x_dpy,
        absolute -> mov_x_abs,
    ]
    (|registers, value| {
        registers.x = value;
        registers.psw.zero = value == 0;
        registers.psw.negative = value.sign_bit();
    })
);

impl_read_op!(
    [
        immediate -> mov_y_imm,
        direct_page -> mov_y_dp,
        direct_page_x -> mov_y_dpx,
        absolute -> mov_y_abs,
    ]
    (|registers, value| {
        registers.y = value;
        registers.psw.zero = value == 0;
        registers.psw.negative = value.sign_bit();
    })
);

macro_rules! impl_store_direct_page {
    ($name:ident, $register:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    // Dummy read
                    let address = u16::from_le_bytes([cpu.state.t0, cpu.direct_page_msb()]);
                    bus.read(address);
                }
                3 => {
                    cpu.final_cycle();

                    let address = u16::from_le_bytes([cpu.state.t0, cpu.direct_page_msb()]);
                    bus.write(address, cpu.registers.$register);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

macro_rules! impl_store_direct_page_indexed {
    ($name:ident, $register:ident, index: $index:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    bus.idle();
                }
                3 => {
                    // Dummy read
                    let address_lsb = cpu.state.t0.wrapping_add(cpu.registers.$index);
                    let address = u16::from_le_bytes([address_lsb, cpu.direct_page_msb()]);
                    bus.read(address);
                }
                4 => {
                    cpu.final_cycle();

                    let address_lsb = cpu.state.t0.wrapping_add(cpu.registers.$index);
                    let address = u16::from_le_bytes([address_lsb, cpu.direct_page_msb()]);
                    bus.write(address, cpu.registers.$register);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

macro_rules! impl_store_absolute {
    ($name:ident, $register:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    cpu.state.t1 = fetch_operand(cpu, bus);
                }
                3 => {
                    // Dummy read
                    let address = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
                    bus.read(address);
                }
                4 => {
                    cpu.final_cycle();

                    let address = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
                    bus.write(address, cpu.registers.$register);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

macro_rules! impl_store_a_absolute_indexed {
    ($name:ident, index: $index:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    cpu.state.t1 = fetch_operand(cpu, bus);
                }
                3 => {
                    bus.idle();
                }
                4 => {
                    // Dummy read
                    let address = u16::from_le_bytes([cpu.state.t0, cpu.state.t1])
                        .wrapping_add(cpu.registers.$index.into());
                    bus.read(address);
                }
                5 => {
                    cpu.final_cycle();

                    let address = u16::from_le_bytes([cpu.state.t0, cpu.state.t1])
                        .wrapping_add(cpu.registers.$index.into());
                    bus.write(address, cpu.registers.a);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

impl_store_direct_page!(mov_dp_a, a);
impl_store_direct_page_indexed!(mov_dpx_a, a, index: x);
impl_store_absolute!(mov_abs_a, a);
impl_store_a_absolute_indexed!(mov_absx_a, index: x);
impl_store_a_absolute_indexed!(mov_absy_a, index: y);

impl_store_direct_page!(mov_dp_x, x);
impl_store_direct_page_indexed!(mov_dpy_x, x, index: y);
impl_store_absolute!(mov_abs_x, x);

impl_store_direct_page!(mov_dp_y, y);
impl_store_direct_page_indexed!(mov_dpx_y, y, index: x);
impl_store_absolute!(mov_abs_y, y);

pub(crate) fn mov_xind_a<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            bus.idle();
        }
        2 => {
            // Dummy read
            let address = u16::from_le_bytes([cpu.registers.x, cpu.direct_page_msb()]);
            bus.read(address);
        }
        3 => {
            cpu.final_cycle();

            let address = u16::from_le_bytes([cpu.registers.x, cpu.direct_page_msb()]);
            bus.write(address, cpu.registers.a);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn mov_xind_postinc_a<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 | 2 => {
            bus.idle();
        }
        3 => {
            cpu.final_cycle();

            let address = u16::from_le_bytes([cpu.registers.x, cpu.direct_page_msb()]);
            bus.write(address, cpu.registers.a);
            cpu.registers.x = cpu.registers.x.wrapping_add(1);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn mov_indx_a<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            bus.idle();
        }
        3 => {
            let address_lsb = cpu.state.t0.wrapping_add(cpu.registers.x);
            let address = u16::from_le_bytes([address_lsb, cpu.direct_page_msb()]);
            cpu.state.t1 = bus.read(address);
        }
        4 => {
            let address_lsb = cpu.state.t0.wrapping_add(cpu.registers.x).wrapping_add(1);
            let address = u16::from_le_bytes([address_lsb, cpu.direct_page_msb()]);
            cpu.state.t2 = bus.read(address);
        }
        5 => {
            // Dummy read
            let address = u16::from_le_bytes([cpu.state.t1, cpu.state.t2]);
            bus.read(address);
        }
        6 => {
            cpu.final_cycle();

            let address = u16::from_le_bytes([cpu.state.t1, cpu.state.t2]);
            bus.write(address, cpu.registers.a);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn mov_indy_a<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            let address = u16::from_le_bytes([cpu.state.t0, cpu.direct_page_msb()]);
            cpu.state.t1 = bus.read(address);
        }
        3 => {
            let address_lsb = cpu.state.t0.wrapping_add(1);
            let address = u16::from_le_bytes([address_lsb, cpu.direct_page_msb()]);
            cpu.state.t2 = bus.read(address);
        }
        4 => {
            bus.idle();
        }
        5 => {
            // Dummy read
            let address = u16::from_le_bytes([cpu.state.t1, cpu.state.t2])
                .wrapping_add(cpu.registers.y.into());
            bus.read(address);
        }
        6 => {
            cpu.final_cycle();

            let address = u16::from_le_bytes([cpu.state.t1, cpu.state.t2])
                .wrapping_add(cpu.registers.y.into());
            bus.write(address, cpu.registers.a);
        }
        _ => invalid_cycle!(cpu),
    }
}

macro_rules! impl_transfer_op {
    (@set_flags sp, $registers:expr) => {};
    (@set_flags $to:ident, $registers:expr) => {
        $registers.psw.zero = $registers.$to == 0;
        $registers.psw.negative = $registers.$to.sign_bit();
    };
    ($name:ident, $to:ident <- $from:ident) => {
        impl_registers_op!($name, |registers| {
            registers.$to = registers.$from;
            impl_transfer_op!(@set_flags $to, registers);
        });
    }
}

impl_transfer_op!(mov_a_x, a <- x);
impl_transfer_op!(mov_a_y, a <- y);
impl_transfer_op!(mov_x_a, x <- a);
impl_transfer_op!(mov_y_a, y <- a);
impl_transfer_op!(mov_x_sp, x <- sp);
impl_transfer_op!(mov_sp_x, sp <- x);

pub(crate) fn mov_dp_dp<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            let address = u16::from_le_bytes([cpu.state.t0, cpu.direct_page_msb()]);
            cpu.state.t1 = bus.read(address);
        }
        3 => {
            cpu.state.t2 = fetch_operand(cpu, bus);
        }
        4 => {
            cpu.final_cycle();

            let address = u16::from_le_bytes([cpu.state.t2, cpu.direct_page_msb()]);
            bus.write(address, cpu.state.t1);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn mov_dp_imm<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            cpu.state.t1 = fetch_operand(cpu, bus);
        }
        3 => {
            // No-op read
            let address = u16::from_le_bytes([cpu.state.t1, cpu.direct_page_msb()]);
            bus.read(address);
        }
        4 => {
            cpu.final_cycle();

            let address = u16::from_le_bytes([cpu.state.t1, cpu.direct_page_msb()]);
            bus.write(address, cpu.state.t0);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn movw_ya_dp<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
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
            let value_msb = bus.read(address);

            cpu.registers.a = cpu.state.t1;
            cpu.registers.y = value_msb;

            cpu.registers.psw.zero = cpu.state.t1 == 0 && value_msb == 0;
            cpu.registers.psw.negative = value_msb.sign_bit();
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn movw_dp_ya<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            // No-op read
            let address = u16::from_le_bytes([cpu.state.t0, cpu.direct_page_msb()]);
            bus.read(address);
        }
        3 => {
            let address = u16::from_le_bytes([cpu.state.t0, cpu.direct_page_msb()]);
            bus.write(address, cpu.registers.a);
        }
        4 => {
            cpu.final_cycle();

            let address_lsb = cpu.state.t0.wrapping_add(1);
            let address = u16::from_le_bytes([address_lsb, cpu.direct_page_msb()]);
            bus.write(address, cpu.registers.y);
        }
        _ => invalid_cycle!(cpu),
    }
}

macro_rules! impl_push {
    ($name:ident, $register:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    bus.idle();
                }
                2 => {
                    bus.write(cpu.stack_pointer(), cpu.registers.$register.into());
                    cpu.registers.sp = cpu.registers.sp.wrapping_sub(1);
                }
                3 => {
                    cpu.final_cycle();
                    bus.idle();
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

impl_push!(push_a, a);
impl_push!(push_x, x);
impl_push!(push_y, y);
impl_push!(push_psw, psw);

macro_rules! impl_pop {
    ($name:ident, $register:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 | 2 => {
                    bus.idle();
                }
                3 => {
                    cpu.final_cycle();

                    cpu.registers.sp = cpu.registers.sp.wrapping_add(1);
                    cpu.registers.$register = bus.read(cpu.stack_pointer()).into();
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

impl_pop!(pop_a, a);
impl_pop!(pop_x, x);
impl_pop!(pop_y, y);
impl_pop!(pop_psw, psw);
