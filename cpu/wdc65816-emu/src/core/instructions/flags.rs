#[allow(clippy::wildcard_imports)]
use super::*;
use std::mem;

macro_rules! impl_flag_op {
    ($name:ident, $flag:ident = $value:expr) => {
        impl_registers_op!($name, |registers| {
            registers.p.$flag = $value;
        });
    };
}

// CLC: Clear carry flag
impl_flag_op!(clc, carry = false);

// CLD: Clear decimal mode flag
impl_flag_op!(cld, decimal_mode = false);

// CLI: Clear interrupt disable flag
impl_flag_op!(cli, irq_disabled = false);

// CLV: Clear overflow flag
impl_flag_op!(clv, overflow = false);

// SEC: Set carry flag
impl_flag_op!(sec, carry = true);

// SED: Set decimal mode flag
impl_flag_op!(sed, decimal_mode = true);

// SEI: Set interrupt disable flag
impl_flag_op!(sei, irq_disabled = true);

// XCE: Exchange carry and emulation mode flags
impl_registers_op!(xce, |registers| {
    mem::swap(&mut registers.p.carry, &mut registers.emulation_mode);

    if registers.emulation_mode {
        // Force on m and x flags, and force stack to page 1
        registers.p.accumulator_size = SizeBits::Eight;
        registers.p.index_size = SizeBits::Eight;
        ensure_page_1_stack(registers);

        // Force index registers to 8 bits
        registers.x &= 0x00FF;
        registers.y &= 0x00FF;
    }
});

// REP: Reset processor status bits
pub(crate) fn rep<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            final_cycle(cpu, bus);

            bus.idle();

            let mask = if cpu.registers.emulation_mode {
                // Cannot clear m or x flag while in emulation mode
                !cpu.state.t0 | 0x30
            } else {
                !cpu.state.t0
            };
            cpu.registers.p = (u8::from(cpu.registers.p) & mask).into();
        }
        _ => invalid_cycle!(cpu),
    }
}

// SEP: Set processor status bits
pub(crate) fn sep<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            final_cycle(cpu, bus);

            bus.idle();

            let mask = cpu.state.t0;
            cpu.registers.p = (u8::from(cpu.registers.p) | mask).into();

            if cpu.registers.p.index_size == SizeBits::Eight {
                // Force X and Y registers to 8 bits
                cpu.registers.x &= 0x00FF;
                cpu.registers.y &= 0x00FF;
            }
        }
        _ => invalid_cycle!(cpu),
    }
}
