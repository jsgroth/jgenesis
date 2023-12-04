#[allow(clippy::wildcard_imports)]
use super::*;

// LDA: Load accumulator (8-bit)
impl_read_op_u8!(
    [
        immediate -> lda_immediate_u8,
        direct_page -> lda_direct_page_u8,
        direct_page_x -> lda_direct_page_x_u8,
        direct_page_indirect -> lda_direct_page_indirect_u8,
        direct_page_indexed_indirect -> lda_direct_page_indexed_indirect_u8,
        direct_page_indirect_indexed -> lda_direct_page_indirect_indexed_u8,
        direct_page_indirect_long -> lda_direct_page_indirect_long_u8,
        direct_page_indirect_long_indexed -> lda_direct_page_indirect_long_indexed_u8,
        absolute -> lda_absolute_u8,
        absolute_x -> lda_absolute_x_u8,
        absolute_y -> lda_absolute_y_u8,
        absolute_long -> lda_absolute_long_u8,
        absolute_long_x -> lda_absolute_long_x_u8,
        stack_relative -> lda_stack_relative_u8,
        stack_relative_indirect_indexed -> lda_stack_relative_indirect_indexed_u8,
    ]
    (|registers, value| {
        registers.a.set_lsb(value);

        registers.p.zero = value == 0;
        registers.p.negative = value.sign_bit();
    })
);

// LDA: Load accumulator (16-bit)
impl_read_op_u16!(
    [
        immediate -> lda_immediate_u16,
        direct_page -> lda_direct_page_u16,
        direct_page_x -> lda_direct_page_x_u16,
        direct_page_indirect -> lda_direct_page_indirect_u16,
        direct_page_indexed_indirect -> lda_direct_page_indexed_indirect_u16,
        direct_page_indirect_indexed -> lda_direct_page_indirect_indexed_u16,
        direct_page_indirect_long -> lda_direct_page_indirect_long_u16,
        direct_page_indirect_long_indexed -> lda_direct_page_indirect_long_indexed_u16,
        absolute -> lda_absolute_u16,
        absolute_x -> lda_absolute_x_u16,
        absolute_y -> lda_absolute_y_u16,
        absolute_long -> lda_absolute_long_u16,
        absolute_long_x -> lda_absolute_long_x_u16,
        stack_relative -> lda_stack_relative_u16,
        stack_relative_indirect_indexed -> lda_stack_relative_indirect_indexed_u16,
    ]
    (|registers, value| {
        registers.a = value;

        registers.p.zero = value == 0;
        registers.p.negative = value.sign_bit();
    })
);

// LDX: Load X register (8-bit)
impl_read_op_u8!(
    [
        immediate -> ldx_immediate_u8,
        direct_page -> ldx_direct_page_u8,
        direct_page_y -> ldx_direct_page_y_u8,
        absolute -> ldx_absolute_u8,
        absolute_y -> ldx_absolute_y_u8,
    ]
    (|registers, value| {
        registers.x = value.into();

        registers.p.zero = value == 0;
        registers.p.negative = value.sign_bit();
    })
);

// LDX: Load X register (16-bit)
impl_read_op_u16!(
    [
        immediate -> ldx_immediate_u16,
        direct_page -> ldx_direct_page_u16,
        direct_page_y -> ldx_direct_page_y_u16,
        absolute -> ldx_absolute_u16,
        absolute_y -> ldx_absolute_y_u16,
    ]
    (|registers, value| {
        registers.x = value;

        registers.p.zero = value == 0;
        registers.p.negative = value.sign_bit();
    })
);

// LDY: Load Y register (8-bit)
impl_read_op_u8!(
    [
        immediate -> ldy_immediate_u8,
        direct_page -> ldy_direct_page_u8,
        direct_page_x -> ldy_direct_page_x_u8,
        absolute -> ldy_absolute_u8,
        absolute_x -> ldy_absolute_x_u8,
    ]
    (|registers, value| {
        registers.y = value.into();

        registers.p.zero = value == 0;
        registers.p.negative = value.sign_bit();
    })
);

// LDY: Load Y register (16-bit)
impl_read_op_u16!(
    [
        immediate -> ldy_immediate_u16,
        direct_page -> ldy_direct_page_u16,
        direct_page_x -> ldy_direct_page_x_u16,
        absolute -> ldy_absolute_u16,
        absolute_x -> ldy_absolute_x_u16,
    ]
    (|registers, value| {
        registers.y = value;

        registers.p.zero = value == 0;
        registers.p.negative = value.sign_bit();
    })
);

macro_rules! impl_transfer_op {
    (u8: $u8_name:ident, u16: $u16_name:ident, $from:ident -> $to:ident) => {
        impl_registers_op!($u8_name, |registers| {
            let value = registers.$from as u8;
            registers.$to.set_lsb(value);

            registers.p.zero = value == 0;
            registers.p.negative = value.sign_bit();
        });

        impl_registers_op!($u16_name, |registers| {
            let value = registers.$from;
            registers.$to = value;

            registers.p.zero = value == 0;
            registers.p.negative = value.sign_bit();
        });
    };
}

// TAX: Transfer A to X
impl_transfer_op!(u8: tax_u8, u16: tax_u16, a -> x);

// TAY: Transfer A to Y
impl_transfer_op!(u8: tay_u8, u16: tay_u16, a -> y);

// TSX: Transfer S to X
impl_transfer_op!(u8: tsx_u8, u16: tsx_u16, s -> x);

// TXA: Transfer X to A
impl_transfer_op!(u8: txa_u8, u16: txa_u16, x -> a);

// TXY: Transfer X to Y
impl_transfer_op!(u8: txy_u8, u16: txy_u16, x -> y);

// TYA: Transfer Y to A
impl_transfer_op!(u8: tya_u8, u16: tya_u16, y -> a);

// TYX: Transfer Y to X
impl_transfer_op!(u8: tyx_u8, u16: tyx_u16, y -> x);

// TXS: Transfer X to S
// Does not use impl_transfer_op! because it is always a 16-bit operation unless in emulation mode,
// and because it does not set flags
impl_registers_op!(txs, |registers| {
    if registers.emulation_mode {
        // S MSB is forced to $01 in emulation mode
        registers.s = 0x0100 | (registers.x & 0x00FF);
    } else {
        registers.s = registers.x;
    }
});

// TCD: Transfer C (16-bit accumulator) to D
impl_registers_op!(tcd, |registers| {
    let value = registers.a;
    registers.d = value;

    registers.p.zero = value == 0;
    registers.p.negative = value.sign_bit();
});

// TCS: Transfer C (16-bit accumulator) to S
impl_registers_op!(tcs, |registers| {
    if registers.emulation_mode {
        // TCS is an 8-bit operation when in emulation mode because S MSB is forced to $01
        registers.s = 0x0100 | (registers.a & 0x00FF);
    } else {
        registers.s = registers.a;
    }
});

// TDC: Transfer D to C (16-bit accumulator)
impl_registers_op!(tdc, |registers| {
    let value = registers.d;
    registers.a = value;

    registers.p.zero = value == 0;
    registers.p.negative = value.sign_bit();
});

// TSC: Transfer S to C (16-bit accumulator)
impl_registers_op!(tsc, |registers| {
    let value = registers.s;
    registers.a = value;

    registers.p.zero = value == 0;
    registers.p.negative = value.sign_bit();
});

// XBA: Exchange B and A accumulator bytes
// Not using impl_registers_op! because this instruction takes 3 cycles, not 2
pub(crate) fn xba<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            bus.idle();
        }
        2 => {
            final_cycle(cpu, bus);

            bus.idle();

            cpu.registers.a = cpu.registers.a.swap_bytes();

            // Flags are always based on low byte
            cpu.registers.p.zero = cpu.registers.a & 0x00FF == 0;
            cpu.registers.p.negative = cpu.registers.a.bit(7);
        }
        _ => invalid_cycle!(cpu),
    }
}

macro_rules! store_register {
    ($registers:expr, zero) => {
        0_u16
    };
    ($registers:expr, $register:ident) => {
        $registers.$register
    };
}

macro_rules! impl_store_direct_page_u8 {
    ($name:ident, $register:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                    check_direct_page_low_byte(cpu);
                }
                2 => {
                    // Idle cycle if D LSB is non-zero
                    bus.idle();
                }
                3 => {
                    final_cycle(cpu, bus);

                    let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
                    let value = store_register!(cpu.registers, $register);
                    bus.write(address.into(), value as u8);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

macro_rules! impl_store_direct_page_u16 {
    ($name:ident, $register:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                    check_direct_page_low_byte(cpu);
                }
                2 => {
                    // Idle cycle if D LSB is non-zero
                    bus.idle();
                }
                3 => {
                    let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
                    let value = store_register!(cpu.registers, $register);
                    bus.write(address.into(), value.lsb());
                }
                4 => {
                    final_cycle(cpu, bus);

                    let address = cpu.registers.d.wrapping_add(cpu.state.t0.into()).wrapping_add(1);
                    let value = store_register!(cpu.registers, $register);
                    bus.write(address.into(), value.msb());
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

macro_rules! impl_store_direct_page_indexed_u8 {
    ($name:ident, $register:ident, index: $index:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                    check_direct_page_low_byte(cpu);
                }
                2 => {
                    // Idle cycle if D LSB is non-zero
                    bus.idle();
                }
                3 => {
                    // Idle cycle for indexing
                    bus.idle();
                }
                4 => {
                    final_cycle(cpu, bus);

                    let address = index_direct_page(cpu, cpu.state.t0, cpu.registers.$index);
                    let value = store_register!(cpu.registers, $register);
                    bus.write(address.into(), value as u8);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

macro_rules! impl_store_direct_page_indexed_u16 {
    ($name:ident, $register:ident, index: $index:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                    check_direct_page_low_byte(cpu);
                }
                2 => {
                    // Idle cycle if D LSB is non-zero
                    bus.idle();
                }
                3 => {
                    // Idle cycle for indexing
                    bus.idle();
                }
                4 => {
                    let address = index_direct_page(cpu, cpu.state.t0, cpu.registers.$index);
                    let value = store_register!(cpu.registers, $register);
                    bus.write(address.into(), value.lsb());
                }
                5 => {
                    final_cycle(cpu, bus);

                    let address = index_direct_page(cpu, cpu.state.t0, cpu.registers.$index);
                    let value = store_register!(cpu.registers, $register);
                    bus.write(address.wrapping_add(1).into(), value.msb());
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

macro_rules! impl_store_absolute_u8 {
    ($name:ident, $register:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    cpu.state.t1 = fetch_operand(cpu, bus);
                }
                3 => {
                    final_cycle(cpu, bus);

                    let address =
                        u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.registers.dbr, 0]);
                    let value = store_register!(cpu.registers, $register);
                    bus.write(address, value as u8);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

macro_rules! impl_store_absolute_u16 {
    ($name:ident, $register:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    cpu.state.t1 = fetch_operand(cpu, bus);
                }
                3 => {
                    let address =
                        u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.registers.dbr, 0]);
                    let value = store_register!(cpu.registers, $register);
                    bus.write(address, value.lsb());
                }
                4 => {
                    final_cycle(cpu, bus);

                    let base_addr =
                        u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.registers.dbr, 0]);
                    let address = (base_addr + 1) & B::ADDRESS_MASK;
                    let value = store_register!(cpu.registers, $register);
                    bus.write(address, value.msb());
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

macro_rules! impl_store_absolute_indexed_u8 {
    ($name:ident, $register:ident, index: $index:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    cpu.state.t1 = fetch_operand(cpu, bus);
                }
                3 => {
                    // Idle cycle for indexing
                    bus.idle();
                }
                4 => {
                    final_cycle(cpu, bus);

                    let base_addr =
                        u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.registers.dbr, 0]);
                    let index: u32 = cpu.registers.$index.into();
                    let address = (base_addr + index) & B::ADDRESS_MASK;
                    let value = store_register!(cpu.registers, $register);
                    bus.write(address, value as u8);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

macro_rules! impl_store_absolute_indexed_u16 {
    ($name:ident, $register:ident, index: $index:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    cpu.state.t1 = fetch_operand(cpu, bus);
                }
                3 => {
                    // Idle cycle for indexing
                    bus.idle();
                }
                4 => {
                    let base_addr =
                        u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.registers.dbr, 0]);
                    let index: u32 = cpu.registers.$index.into();
                    let address = (base_addr + index) & B::ADDRESS_MASK;
                    let value = store_register!(cpu.registers, $register);
                    bus.write(address, value.lsb());
                }
                5 => {
                    final_cycle(cpu, bus);

                    let base_addr =
                        u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.registers.dbr, 0]);
                    let index: u32 = cpu.registers.$index.into();
                    let address = (base_addr + index + 1) & B::ADDRESS_MASK;
                    let value = store_register!(cpu.registers, $register);
                    bus.write(address, value.msb());
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

// STA: Store accumulator
impl_store_direct_page_u8!(sta_direct_page_u8, a);
impl_store_direct_page_u16!(sta_direct_page_u16, a);
impl_store_direct_page_indexed_u8!(sta_direct_page_x_u8, a, index: x);
impl_store_direct_page_indexed_u16!(sta_direct_page_x_u16, a, index: x);
impl_store_absolute_u8!(sta_absolute_u8, a);
impl_store_absolute_u16!(sta_absolute_u16, a);
impl_store_absolute_indexed_u8!(sta_absolute_x_u8, a, index: x);
impl_store_absolute_indexed_u16!(sta_absolute_x_u16, a, index: x);
impl_store_absolute_indexed_u8!(sta_absolute_y_u8, a, index: y);
impl_store_absolute_indexed_u16!(sta_absolute_y_u16, a, index: y);

// Remaining STA addressing modes do not use macros because they're unique to STA

pub(crate) fn sta_direct_page_indirect_u8<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
            check_direct_page_low_byte(cpu);
        }
        2 => {
            // Idle cycle if D LSB is non-zero
            bus.idle();
        }
        3 => {
            let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
            cpu.state.t1 = bus.read(address.into());
        }
        4 => {
            let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
            cpu.state.t2 = bus.read(address.wrapping_add(1).into());
        }
        5 => {
            final_cycle(cpu, bus);

            let address = u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.registers.dbr, 0]);
            bus.write(address, cpu.registers.a as u8);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn sta_direct_page_indirect_u16<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
            check_direct_page_low_byte(cpu);
        }
        2 => {
            // Idle cycle if D LSB is non-zero
            bus.idle();
        }
        3 => {
            let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
            cpu.state.t1 = bus.read(address.into());
        }
        4 => {
            let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
            cpu.state.t2 = bus.read(address.wrapping_add(1).into());
        }
        5 => {
            let address = u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.registers.dbr, 0]);
            bus.write(address, cpu.registers.a.lsb());
        }
        6 => {
            final_cycle(cpu, bus);

            let address = u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.registers.dbr, 0]);
            bus.write((address + 1) & B::ADDRESS_MASK, cpu.registers.a.msb());
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn sta_direct_page_indexed_indirect_u8<B: BusInterface>(
    cpu: &mut Wdc65816,
    bus: &mut B,
) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
            check_direct_page_low_byte(cpu);
        }
        2 | 3 => {
            // Idle cycles if D LSB is non-zero (2) and for indexing (3)
            bus.idle();
        }
        4 => {
            let address = index_direct_page(cpu, cpu.state.t0, cpu.registers.x);
            cpu.state.t1 = bus.read(address.into());
        }
        5 => {
            let address = index_direct_page(cpu, cpu.state.t0, cpu.registers.x);
            cpu.state.t2 = bus.read(address.wrapping_add(1).into());
        }
        6 => {
            final_cycle(cpu, bus);

            let address = u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.registers.dbr, 0]);
            bus.write(address, cpu.registers.a as u8);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn sta_direct_page_indexed_indirect_u16<B: BusInterface>(
    cpu: &mut Wdc65816,
    bus: &mut B,
) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
            check_direct_page_low_byte(cpu);
        }
        2 | 3 => {
            // Idle cycles if D LSB is non-zero (2) and for indexing (3)
            bus.idle();
        }
        4 => {
            let address = index_direct_page(cpu, cpu.state.t0, cpu.registers.x);
            cpu.state.t1 = bus.read(address.into());
        }
        5 => {
            let address = index_direct_page(cpu, cpu.state.t0, cpu.registers.x);
            cpu.state.t2 = bus.read(address.wrapping_add(1).into());
        }
        6 => {
            let address = u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.registers.dbr, 0]);
            bus.write(address, cpu.registers.a.lsb());
        }
        7 => {
            final_cycle(cpu, bus);

            let address = u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.registers.dbr, 0]);
            bus.write((address + 1) & B::ADDRESS_MASK, cpu.registers.a.msb());
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn sta_direct_page_indirect_indexed_u8<B: BusInterface>(
    cpu: &mut Wdc65816,
    bus: &mut B,
) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
            check_direct_page_low_byte(cpu);
        }
        2 | 5 => {
            // Idle cycles for if D LSB is non-zero (2) and for indexing (5)
            bus.idle();
        }
        3 => {
            let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
            cpu.state.t1 = bus.read(address.into());
        }
        4 => {
            let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
            cpu.state.t2 = bus.read(address.wrapping_add(1).into());
        }
        6 => {
            final_cycle(cpu, bus);

            let base_addr = u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.registers.dbr, 0]);
            let address = (base_addr + u32::from(cpu.registers.y)) & B::ADDRESS_MASK;
            bus.write(address, cpu.registers.a as u8);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn sta_direct_page_indirect_indexed_u16<B: BusInterface>(
    cpu: &mut Wdc65816,
    bus: &mut B,
) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
            check_direct_page_low_byte(cpu);
        }
        2 | 5 => {
            // Idle cycles for if D LSB is non-zero (2) and for indexing (5)
            bus.idle();
        }
        3 => {
            let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
            cpu.state.t1 = bus.read(address.into());
        }
        4 => {
            let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
            cpu.state.t2 = bus.read(address.wrapping_add(1).into());
        }
        6 => {
            let base_addr = u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.registers.dbr, 0]);
            let address = (base_addr + u32::from(cpu.registers.y)) & B::ADDRESS_MASK;
            bus.write(address, cpu.registers.a.lsb());
        }
        7 => {
            final_cycle(cpu, bus);

            let base_addr = u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.registers.dbr, 0]);
            let address = (base_addr + u32::from(cpu.registers.y) + 1) & B::ADDRESS_MASK;
            bus.write(address, cpu.registers.a.msb());
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn sta_direct_page_indirect_long_u8<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
            check_direct_page_low_byte(cpu);
        }
        2 => {
            // Idle cycle if D LSB is non-zero
            bus.idle();
        }
        3 => {
            let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
            cpu.state.t1 = bus.read(address.into());
        }
        4 => {
            let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
            cpu.state.t2 = bus.read(address.wrapping_add(1).into());
        }
        5 => {
            let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
            cpu.state.t3 = bus.read(address.wrapping_add(2).into());
        }
        6 => {
            final_cycle(cpu, bus);

            let address = u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.state.t3, 0]);
            bus.write(address, cpu.registers.a as u8);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn sta_direct_page_indirect_long_u16<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
            check_direct_page_low_byte(cpu);
        }
        2 => {
            // Idle cycle if D LSB is non-zero
            bus.idle();
        }
        3 => {
            let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
            cpu.state.t1 = bus.read(address.into());
        }
        4 => {
            let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
            cpu.state.t2 = bus.read(address.wrapping_add(1).into());
        }
        5 => {
            let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
            cpu.state.t3 = bus.read(address.wrapping_add(2).into());
        }
        6 => {
            let address = u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.state.t3, 0]);
            bus.write(address, cpu.registers.a.lsb());
        }
        7 => {
            final_cycle(cpu, bus);

            let address = u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.state.t3, 0]);
            bus.write((address + 1) & B::ADDRESS_MASK, cpu.registers.a.msb());
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn sta_direct_page_indirect_long_indexed_u8<B: BusInterface>(
    cpu: &mut Wdc65816,
    bus: &mut B,
) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
            check_direct_page_low_byte(cpu);
        }
        2 => {
            // Idle cycle if D LSB is non-zero
            bus.idle();
        }
        3 => {
            let address = index_direct_page(cpu, cpu.state.t0, 0);
            cpu.state.t1 = bus.read(address.into());
        }
        4 => {
            let address = index_direct_page(cpu, cpu.state.t0, 1);
            cpu.state.t2 = bus.read(address.into());
        }
        5 => {
            let address = index_direct_page(cpu, cpu.state.t0, 2);
            cpu.state.t3 = bus.read(address.into());
        }
        6 => {
            final_cycle(cpu, bus);

            let base_addr = u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.state.t3, 0]);
            let address = (base_addr + u32::from(cpu.registers.y)) & B::ADDRESS_MASK;
            bus.write(address, cpu.registers.a as u8);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn sta_direct_page_indirect_long_indexed_u16<B: BusInterface>(
    cpu: &mut Wdc65816,
    bus: &mut B,
) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
            check_direct_page_low_byte(cpu);
        }
        2 => {
            // Idle cycle if D LSB is non-zero
            bus.idle();
        }
        3 => {
            let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
            cpu.state.t1 = bus.read(address.into());
        }
        4 => {
            let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
            cpu.state.t2 = bus.read(address.wrapping_add(1).into());
        }
        5 => {
            let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
            cpu.state.t3 = bus.read(address.wrapping_add(2).into());
        }
        6 => {
            let base_addr = u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.state.t3, 0]);
            let address = (base_addr + u32::from(cpu.registers.y)) & B::ADDRESS_MASK;
            bus.write(address, cpu.registers.a.lsb());
        }
        7 => {
            final_cycle(cpu, bus);

            let base_addr = u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.state.t3, 0]);
            let address = (base_addr + u32::from(cpu.registers.y) + 1) & B::ADDRESS_MASK;
            bus.write(address, cpu.registers.a.msb());
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn sta_absolute_long_u8<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            cpu.state.t1 = fetch_operand(cpu, bus);
        }
        3 => {
            cpu.state.t2 = fetch_operand(cpu, bus);
        }
        4 => {
            final_cycle(cpu, bus);

            let address = u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.state.t2, 0]);
            bus.write(address, cpu.registers.a as u8);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn sta_absolute_long_u16<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            cpu.state.t1 = fetch_operand(cpu, bus);
        }
        3 => {
            cpu.state.t2 = fetch_operand(cpu, bus);
        }
        4 => {
            let address = u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.state.t2, 0]);
            bus.write(address, cpu.registers.a.lsb());
        }
        5 => {
            final_cycle(cpu, bus);

            let base_addr = u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.state.t2, 0]);
            let address = (base_addr + 1) & B::ADDRESS_MASK;
            bus.write(address, cpu.registers.a.msb());
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn sta_absolute_long_x_u8<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            cpu.state.t1 = fetch_operand(cpu, bus);
        }
        3 => {
            cpu.state.t2 = fetch_operand(cpu, bus);
        }
        4 => {
            final_cycle(cpu, bus);

            let base_addr = u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.state.t2, 0]);
            let index: u32 = cpu.registers.x.into();
            let address = (base_addr + index) & B::ADDRESS_MASK;
            bus.write(address, cpu.registers.a as u8);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn sta_absolute_long_x_u16<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            cpu.state.t1 = fetch_operand(cpu, bus);
        }
        3 => {
            cpu.state.t2 = fetch_operand(cpu, bus);
        }
        4 => {
            let base_addr = u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.state.t2, 0]);
            let index: u32 = cpu.registers.x.into();
            let address = (base_addr + index) & B::ADDRESS_MASK;
            bus.write(address, cpu.registers.a.lsb());
        }
        5 => {
            final_cycle(cpu, bus);

            let base_addr = u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.state.t2, 0]);
            let index: u32 = cpu.registers.x.into();
            let address = (base_addr + index + 1) & B::ADDRESS_MASK;
            bus.write(address, cpu.registers.a.msb());
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn sta_stack_relative_u8<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            // Idle cycle for indexing
            bus.idle();
        }
        3 => {
            final_cycle(cpu, bus);

            let address = cpu.registers.s.wrapping_add(cpu.state.t0.into());
            bus.write(address.into(), cpu.registers.a as u8);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn sta_stack_relative_u16<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            // Idle cycle for indexing
            bus.idle();
        }
        3 => {
            let address = cpu.registers.s.wrapping_add(cpu.state.t0.into());
            bus.write(address.into(), cpu.registers.a.lsb());
        }
        4 => {
            final_cycle(cpu, bus);

            let address = cpu.registers.s.wrapping_add(cpu.state.t0.into());
            bus.write(address.wrapping_add(1).into(), cpu.registers.a.msb());
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn sta_stack_relative_indirect_indexed_u8<B: BusInterface>(
    cpu: &mut Wdc65816,
    bus: &mut B,
) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 | 5 => {
            // Idle cycles for stack indexing (2) and for address indexing (5)
            bus.idle();
        }
        3 => {
            let address = cpu.registers.s.wrapping_add(cpu.state.t0.into());
            cpu.state.t1 = bus.read(address.into());
        }
        4 => {
            let address = cpu.registers.s.wrapping_add(cpu.state.t0.into());
            cpu.state.t2 = bus.read(address.wrapping_add(1).into());
        }
        6 => {
            final_cycle(cpu, bus);

            let base_addr = u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.registers.dbr, 0]);
            let address = (base_addr + u32::from(cpu.registers.y)) & B::ADDRESS_MASK;
            bus.write(address, cpu.registers.a as u8);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn sta_stack_relative_indirect_indexed_u16<B: BusInterface>(
    cpu: &mut Wdc65816,
    bus: &mut B,
) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 | 5 => {
            // Idle cycles for stack indexing (2) and for address indexing (5)
            bus.idle();
        }
        3 => {
            let address = cpu.registers.s.wrapping_add(cpu.state.t0.into());
            cpu.state.t1 = bus.read(address.into());
        }
        4 => {
            let address = cpu.registers.s.wrapping_add(cpu.state.t0.into());
            cpu.state.t2 = bus.read(address.wrapping_add(1).into());
        }
        6 => {
            let base_addr = u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.registers.dbr, 0]);
            let address = (base_addr + u32::from(cpu.registers.y)) & B::ADDRESS_MASK;
            bus.write(address, cpu.registers.a.lsb());
        }
        7 => {
            final_cycle(cpu, bus);

            let base_addr = u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.registers.dbr, 0]);
            let address = (base_addr + u32::from(cpu.registers.y) + 1) & B::ADDRESS_MASK;
            bus.write(address, cpu.registers.a.msb());
        }
        _ => invalid_cycle!(cpu),
    }
}

// STX: Store X register
impl_store_direct_page_u8!(stx_direct_page_u8, x);
impl_store_direct_page_u16!(stx_direct_page_u16, x);
impl_store_direct_page_indexed_u8!(stx_direct_page_y_u8, x, index: y);
impl_store_direct_page_indexed_u16!(stx_direct_page_y_u16, x, index: y);
impl_store_absolute_u8!(stx_absolute_u8, x);
impl_store_absolute_u16!(stx_absolute_u16, x);

// STY: Store Y register
impl_store_direct_page_u8!(sty_direct_page_u8, y);
impl_store_direct_page_u16!(sty_direct_page_u16, y);
impl_store_direct_page_indexed_u8!(sty_direct_page_x_u8, y, index: x);
impl_store_direct_page_indexed_u16!(sty_direct_page_x_u16, y, index: x);
impl_store_absolute_u8!(sty_absolute_u8, y);
impl_store_absolute_u16!(sty_absolute_u16, y);

// STZ: Store zero
impl_store_direct_page_u8!(stz_direct_page_u8, zero);
impl_store_direct_page_u16!(stz_direct_page_u16, zero);
impl_store_direct_page_indexed_u8!(stz_direct_page_x_u8, zero, index: x);
impl_store_direct_page_indexed_u16!(stz_direct_page_x_u16, zero, index: x);
impl_store_absolute_u8!(stz_absolute_u8, zero);
impl_store_absolute_u16!(stz_absolute_u16, zero);
impl_store_absolute_indexed_u8!(stz_absolute_x_u8, zero, index: x);
impl_store_absolute_indexed_u16!(stz_absolute_x_u16, zero, index: x);

macro_rules! impl_push_u8 {
    ($name:ident, $register:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    bus.idle();
                }
                2 => {
                    final_cycle(cpu, bus);

                    bus.write(cpu.registers.s.into(), cpu.registers.$register as u8);
                    cpu.registers.s = cpu.registers.s.wrapping_sub(1);

                    if cpu.registers.emulation_mode {
                        ensure_page_1_stack(&mut cpu.registers);
                    }
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

macro_rules! impl_push_u16 {
    ($name:ident, $register:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    bus.idle();
                }
                2 => {
                    bus.write(cpu.registers.s.into(), cpu.registers.$register.msb());
                    cpu.registers.s = cpu.registers.s.wrapping_sub(1);
                }
                3 => {
                    final_cycle(cpu, bus);

                    bus.write(cpu.registers.s.into(), cpu.registers.$register as u8);
                    cpu.registers.s = cpu.registers.s.wrapping_sub(1);

                    // Emulation mode check necessary for PHD
                    // The second write is allowed to go outside of page 1 in emulation mode, but
                    // the stack gets forced back into page 1 afterwards
                    if cpu.registers.emulation_mode {
                        ensure_page_1_stack(&mut cpu.registers);
                    }
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

// PHA: Push accumulator
impl_push_u8!(pha_u8, a);
impl_push_u16!(pha_u16, a);

// PHX: Push X register
impl_push_u8!(phx_u8, x);
impl_push_u16!(phx_u16, x);

// PHY: Push Y register
impl_push_u8!(phy_u8, y);
impl_push_u16!(phy_u16, y);

// PHB: Push data bank register
impl_push_u8!(phb, dbr);

// PHD: Push direct page register
impl_push_u16!(phd, d);

// PHK: Push K register (program bank register)
impl_push_u8!(phk, pbr);

// PHP: Push processor status register
pub(crate) fn php<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            bus.idle();
        }
        2 => {
            final_cycle(cpu, bus);

            bus.write(cpu.registers.s.into(), cpu.registers.p.into());
            cpu.registers.s = cpu.registers.s.wrapping_sub(1);

            if cpu.registers.emulation_mode {
                ensure_page_1_stack(&mut cpu.registers);
            }
        }
        _ => invalid_cycle!(cpu),
    }
}

macro_rules! impl_pull_u8 {
    (@write dbr, $registers:expr, $value:expr) => {
        $registers.dbr = $value;
    };
    (@write $register:ident, $registers:expr, $value:expr) => {
        $registers.$register.set_lsb($value);
    };
    ($name:ident, $register:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 | 2 => {
                    bus.idle();
                }
                3 => {
                    final_cycle(cpu, bus);

                    cpu.registers.s = cpu.registers.s.wrapping_add(1);

                    if cpu.registers.emulation_mode {
                        ensure_page_1_stack(&mut cpu.registers);
                    }

                    let value = bus.read(cpu.registers.s.into());
                    impl_pull_u8!(@write $register, cpu.registers, value);

                    cpu.registers.p.zero = value == 0;
                    cpu.registers.p.negative = value.sign_bit();
                }
                _ => invalid_cycle!(cpu)
            }
        }
    }
}

macro_rules! impl_pull_u16 {
    ($name:ident, $register:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 | 2 => {
                    bus.idle();
                }
                3 => {
                    cpu.registers.s = cpu.registers.s.wrapping_add(1);
                    cpu.state.t0 = bus.read(cpu.registers.s.into());
                }
                4 => {
                    final_cycle(cpu, bus);

                    cpu.registers.s = cpu.registers.s.wrapping_add(1);
                    let value_msb = bus.read(cpu.registers.s.into());
                    let value = u16::from_le_bytes([cpu.state.t0, value_msb]);
                    cpu.registers.$register = value;

                    cpu.registers.p.zero = value == 0;
                    cpu.registers.p.negative = value.sign_bit();

                    // Emulation mode check necessary for PLD, similar to PHD
                    if cpu.registers.emulation_mode {
                        ensure_page_1_stack(&mut cpu.registers);
                    }
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

// PLA: Pull accumulator
impl_pull_u8!(pla_u8, a);
impl_pull_u16!(pla_u16, a);

// PLX: Pull X register
impl_pull_u8!(plx_u8, x);
impl_pull_u16!(plx_u16, x);

// PLY: Pull Y register
impl_pull_u8!(ply_u8, y);
impl_pull_u16!(ply_u16, y);

// PLB: Pull data bank register
impl_pull_u8!(plb, dbr);

// PLD: Pull direct page register
impl_pull_u16!(pld, d);

// PLP: Pull processor status register
pub(crate) fn plp<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 | 2 => {
            bus.idle();
        }
        3 => {
            final_cycle(cpu, bus);

            cpu.registers.s = cpu.registers.s.wrapping_add(1);

            if cpu.registers.emulation_mode {
                ensure_page_1_stack(&mut cpu.registers);

                // m and x flags are forced to 1 in emulation mode
                let value = 0x30 | bus.read(cpu.registers.s.into());
                cpu.registers.p = value.into();
            } else {
                cpu.registers.p = bus.read(cpu.registers.s.into()).into();

                if cpu.registers.p.index_size == SizeBits::Eight {
                    // Immediately truncate index registers to 8 bits
                    cpu.registers.x &= 0x00FF;
                    cpu.registers.y &= 0x00FF;
                }
            }
        }
        _ => invalid_cycle!(cpu),
    }
}

macro_rules! impl_move {
    ($name:ident, $index_inc_method:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.registers.dbr = fetch_operand(cpu, bus);
                }
                2 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                3 => {
                    let address = u24_address(cpu.state.t0, cpu.registers.x);
                    cpu.state.t1 = bus.read(address);
                }
                4 => {
                    let address = u24_address(cpu.registers.dbr, cpu.registers.y);
                    bus.write(address, cpu.state.t1);
                }
                5 => {
                    bus.idle();
                }
                6 => {
                    final_cycle(cpu, bus);

                    bus.idle();

                    cpu.registers.a = cpu.registers.a.wrapping_sub(1);
                    if cpu.registers.a != 0xFFFF {
                        cpu.registers.pc = cpu.registers.pc.wrapping_sub(3);
                    }

                    cpu.registers.x = cpu.registers.x.$index_inc_method(1);
                    cpu.registers.y = cpu.registers.y.$index_inc_method(1);

                    if cpu.registers.p.index_size == SizeBits::Eight {
                        cpu.registers.x &= 0x00FF;
                        cpu.registers.y &= 0x00FF;
                    }
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

// MVN: Move memory negative
impl_move!(mvn, wrapping_add);

// MVP: Move memory positive
impl_move!(mvp, wrapping_sub);
