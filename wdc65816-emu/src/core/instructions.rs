mod alu;
mod bits;
mod disassemble;
mod flags;
mod flow;
mod load;

use crate::core::{InterruptType, Registers, SizeBits, Wdc65816};
use crate::traits::BusInterface;
use jgenesis_traits::num::{GetBit, SignBit};

fn u24_address(bank: u8, address: u16) -> u32 {
    (u32::from(bank) << 16) | u32::from(address)
}

fn fetch_operand<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) -> u8 {
    let operand = bus.read(u24_address(cpu.registers.pbr, cpu.registers.pc));
    cpu.registers.pc = cpu.registers.pc.wrapping_add(1);

    log::trace!(
        "Fetched operand {operand:02X} from PBR {:02X} / PC {:04X}",
        cpu.registers.pbr,
        cpu.registers.pc.wrapping_sub(1)
    );

    operand
}

fn poll_interrupt_lines<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    let nmi = bus.nmi();
    cpu.state.nmi_triggered |= !cpu.state.last_nmi && nmi;
    cpu.state.last_nmi = nmi;

    let irq = bus.irq();
    cpu.state.irq_triggered = !cpu.registers.p.irq_disabled && irq;
    cpu.state.last_irq = irq;
}

fn final_cycle<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    poll_interrupt_lines(cpu, bus);
    cpu.state.cycle = 0;
}

fn ensure_page_1_stack(registers: &mut Registers) {
    registers.s = 0x0100 | (registers.s & 0x00FF);
}

macro_rules! invalid_cycle {
    ($cpu:expr) => {
        panic!("Invalid cycle for opcode {:02X}: {}", $cpu.state.opcode, $cpu.state.cycle)
    };
}

use invalid_cycle;

macro_rules! impl_read_immediate_u8 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            final_cycle(cpu, bus);

            let $value = fetch_operand(cpu, bus);
            let $registers = &mut cpu.registers;
            $body
        }
    };
}

use impl_read_immediate_u8;

macro_rules! impl_read_immediate_u16 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    final_cycle(cpu, bus);

                    let msb = fetch_operand(cpu, bus);
                    let $value = u16::from_le_bytes([cpu.state.t0, msb]);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_immediate_u16;

fn check_direct_page_low_byte(cpu: &mut Wdc65816) {
    // Low byte of D being non-zero causes an extra cycle for direct page access; emulate this by
    // skipping a cycle if it is zero
    if cpu.registers.d & 0x00FF == 0 {
        cpu.state.cycle += 1;
    }
}

macro_rules! impl_read_direct_page_u8 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    let $value = bus.read(address.into());
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_direct_page_u8;

macro_rules! impl_read_direct_page_u16 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    cpu.state.t1 = bus.read(address.into());
                }
                4 => {
                    final_cycle(cpu, bus);

                    let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
                    let value_msb = bus.read(address.wrapping_add(1).into());
                    let $value = u16::from_le_bytes([cpu.state.t1, value_msb]);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_direct_page_u16;

fn index_direct_page(cpu: &Wdc65816, operand: u8, index: u16) -> u16 {
    if cpu.registers.emulation_mode && cpu.registers.d & 0x00FF == 0 {
        // If in emulation mode with D LSB equal to zero, indexing wraps at page boundary
        (cpu.registers.d.wrapping_add(operand.into()).wrapping_add(index) & 0x00FF)
            | (cpu.registers.d & 0xFF00)
    } else {
        cpu.registers.d.wrapping_add(operand.into()).wrapping_add(index)
    }
}

macro_rules! impl_read_direct_page_indexed_u8 {
    ($index:ident, $name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    // Idle cycle for adding index
                    bus.idle();
                }
                4 => {
                    final_cycle(cpu, bus);

                    let address = index_direct_page(cpu, cpu.state.t0, cpu.registers.$index);
                    let $value = bus.read(address.into());
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_direct_page_indexed_u8;

macro_rules! impl_read_direct_page_indexed_u16 {
    ($index:ident, $name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    // Idle cycle for adding index
                    bus.idle();
                }
                4 => {
                    let address = index_direct_page(cpu, cpu.state.t0, cpu.registers.$index);
                    cpu.state.t1 = bus.read(address.into());
                }
                5 => {
                    final_cycle(cpu, bus);

                    let address = index_direct_page(cpu, cpu.state.t0, cpu.registers.$index);
                    let value_msb = bus.read(address.wrapping_add(1).into());
                    let $value = u16::from_le_bytes([cpu.state.t1, value_msb]);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_direct_page_indexed_u16;

macro_rules! impl_read_direct_page_indirect_u8 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    cpu.state.t1 = bus.read(address.into());
                }
                4 => {
                    let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
                    cpu.state.t2 = bus.read(address.wrapping_add(1).into());
                }
                5 => {
                    final_cycle(cpu, bus);

                    let address = u16::from_le_bytes([cpu.state.t1, cpu.state.t2]);
                    let $value = bus.read(u24_address(cpu.registers.dbr, address));
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_direct_page_indirect_u8;

macro_rules! impl_read_direct_page_indirect_u16 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    cpu.state.t1 = bus.read(address.into());
                }
                4 => {
                    let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
                    cpu.state.t2 = bus.read(address.wrapping_add(1).into());
                }
                5 => {
                    let address = u16::from_le_bytes([cpu.state.t1, cpu.state.t2]);
                    cpu.state.t3 = bus.read(u24_address(cpu.registers.dbr, address));
                }
                6 => {
                    final_cycle(cpu, bus);

                    let base_addr = u16::from_le_bytes([cpu.state.t1, cpu.state.t2]);
                    let address = (u24_address(cpu.registers.dbr, base_addr) + 1) & B::ADDRESS_MASK;
                    let value_msb = bus.read(address);
                    let $value = u16::from_le_bytes([cpu.state.t3, value_msb]);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_direct_page_indirect_u16;

macro_rules! impl_read_direct_page_indexed_indirect_u8 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    // Idle cycle to add index
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

                    let base_addr = u16::from_le_bytes([cpu.state.t1, cpu.state.t2]);
                    let address = u24_address(cpu.registers.dbr, base_addr);
                    let $value = bus.read(address);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_direct_page_indexed_indirect_u8;

macro_rules! impl_read_direct_page_indexed_indirect_u16 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    // Idle cycle to add index
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
                    let base_addr = u16::from_le_bytes([cpu.state.t1, cpu.state.t2]);
                    let address = u24_address(cpu.registers.dbr, base_addr);
                    cpu.state.t3 = bus.read(address);
                }
                7 => {
                    final_cycle(cpu, bus);

                    let base_addr = u16::from_le_bytes([cpu.state.t1, cpu.state.t2]);
                    let address = (u24_address(cpu.registers.dbr, base_addr) + 1) & B::ADDRESS_MASK;
                    let value_msb = bus.read(address);
                    let $value = u16::from_le_bytes([cpu.state.t3, value_msb]);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_direct_page_indexed_indirect_u16;

macro_rules! impl_read_direct_page_indirect_indexed_u8 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    let address = index_direct_page(cpu, cpu.state.t0, 0);
                    cpu.state.t1 = bus.read(address.into());
                }
                4 => {
                    let address = index_direct_page(cpu, cpu.state.t0, 0);
                    cpu.state.t2 = bus.read(address.wrapping_add(1).into());
                    check_absolute_index(
                        cpu,
                        u16::from_le_bytes([cpu.state.t1, cpu.state.t2]),
                        cpu.registers.y,
                    );
                }
                5 => {
                    // Idle cycle for adding index
                    bus.idle();
                }
                6 => {
                    final_cycle(cpu, bus);

                    let base_addr = u24_address(
                        cpu.registers.dbr,
                        u16::from_le_bytes([cpu.state.t1, cpu.state.t2]),
                    );
                    let index: u32 = cpu.registers.y.into();
                    let address = (base_addr + index) & B::ADDRESS_MASK;
                    let $value = bus.read(address);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_direct_page_indirect_indexed_u8;

macro_rules! impl_read_direct_page_indirect_indexed_u16 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    cpu.state.t1 = bus.read(address.into());
                }
                4 => {
                    let address = cpu.registers.d.wrapping_add(cpu.state.t0.into()).wrapping_add(1);
                    cpu.state.t2 = bus.read(address.into());
                    check_absolute_index(
                        cpu,
                        u16::from_le_bytes([cpu.state.t1, cpu.state.t2]),
                        cpu.registers.y,
                    );
                }
                5 => {
                    // Idle cycle for adding index
                    bus.idle();
                }
                6 => {
                    let base_addr = u24_address(
                        cpu.registers.dbr,
                        u16::from_le_bytes([cpu.state.t1, cpu.state.t2]),
                    );
                    let index: u32 = cpu.registers.y.into();
                    let address = (base_addr + index) & B::ADDRESS_MASK;
                    cpu.state.t3 = bus.read(address);
                }
                7 => {
                    final_cycle(cpu, bus);

                    let base_addr = u24_address(
                        cpu.registers.dbr,
                        u16::from_le_bytes([cpu.state.t1, cpu.state.t2]),
                    );
                    let index: u32 = cpu.registers.y.into();
                    let address = (base_addr + index + 1) & B::ADDRESS_MASK;
                    let value_msb = bus.read(address);
                    let $value = u16::from_le_bytes([cpu.state.t3, value_msb]);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_direct_page_indirect_indexed_u16;

macro_rules! impl_read_direct_page_indirect_long_u8 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    let $value = bus.read(address);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_direct_page_indirect_long_u8;

macro_rules! impl_read_direct_page_indirect_long_u16 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    cpu.state.t4 = bus.read(address);
                }
                7 => {
                    final_cycle(cpu, bus);

                    let address = u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.state.t3, 0]);
                    let value_msb = bus.read((address + 1) & B::ADDRESS_MASK);
                    let $value = u16::from_le_bytes([cpu.state.t4, value_msb]);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_direct_page_indirect_long_u16;

macro_rules! impl_read_direct_page_indirect_long_indexed_u8 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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

                    let base_addr =
                        u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.state.t3, 0]);
                    let index: u32 = cpu.registers.y.into();
                    let address = (base_addr + index) & B::ADDRESS_MASK;
                    let $value = bus.read(address);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_direct_page_indirect_long_indexed_u8;

macro_rules! impl_read_direct_page_indirect_long_indexed_u16 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    let base_addr =
                        u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.state.t3, 0]);
                    let index: u32 = cpu.registers.y.into();
                    let address = (base_addr + index) & B::ADDRESS_MASK;
                    cpu.state.t4 = bus.read(address);
                }
                7 => {
                    final_cycle(cpu, bus);

                    let base_addr =
                        u32::from_le_bytes([cpu.state.t1, cpu.state.t2, cpu.state.t3, 0]);
                    let index: u32 = cpu.registers.y.into();
                    let address = (base_addr + index + 1) & B::ADDRESS_MASK;
                    let value_msb = bus.read(address);
                    let $value = u16::from_le_bytes([cpu.state.t4, value_msb]);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_direct_page_indirect_long_indexed_u16;

macro_rules! impl_read_absolute_u8 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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

                    let address = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
                    let $value = bus.read(u24_address(cpu.registers.dbr, address));
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_absolute_u8;

macro_rules! impl_read_absolute_u16 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    cpu.state.t1 = fetch_operand(cpu, bus);
                }
                3 => {
                    let base_addr = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
                    cpu.state.t2 = bus.read(u24_address(cpu.registers.dbr, base_addr));
                }
                4 => {
                    final_cycle(cpu, bus);

                    let base_addr = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
                    let address = (u24_address(cpu.registers.dbr, base_addr) + 1) & B::ADDRESS_MASK;
                    let value_msb = bus.read(address);
                    let $value = u16::from_le_bytes([cpu.state.t2, value_msb]);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_absolute_u16;

fn check_absolute_index(cpu: &mut Wdc65816, absolute_addr: u16, index: u16) {
    // Absolute X/Y reads skip the idle cycle if index is 8-bit and indexing does not cross a page
    // boundary
    if cpu.registers.p.index_size == SizeBits::Eight
        && absolute_addr.wrapping_add(index) & 0xFF00 == absolute_addr & 0xFF00
    {
        cpu.state.cycle += 1;
    }
}

macro_rules! impl_read_absolute_indexed_u8 {
    ($index:ident, $name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    cpu.state.t1 = fetch_operand(cpu, bus);
                    check_absolute_index(
                        cpu,
                        u16::from_le_bytes([cpu.state.t0, cpu.state.t1]),
                        cpu.registers.$index,
                    );
                }
                3 => {
                    // Idle cycle for adding index, taken if either of the following is true:
                    // - x flag is set (8-bit index) and indexing crosses a page boundary
                    // - x flag is not set (16-bit index)
                    bus.idle();
                }
                4 => {
                    final_cycle(cpu, bus);

                    let base_addr = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
                    let index: u32 = cpu.registers.$index.into();
                    let address =
                        (u24_address(cpu.registers.dbr, base_addr) + index) & B::ADDRESS_MASK;
                    let $value = bus.read(address);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_absolute_indexed_u8;

macro_rules! impl_read_absolute_indexed_u16 {
    ($index:ident, $name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    cpu.state.t1 = fetch_operand(cpu, bus);
                    check_absolute_index(
                        cpu,
                        u16::from_le_bytes([cpu.state.t0, cpu.state.t1]),
                        cpu.registers.$index,
                    );
                }
                3 => {
                    // Idle cycle for adding index
                    bus.idle();
                }
                4 => {
                    let base_addr = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
                    let index: u32 = cpu.registers.$index.into();
                    let address =
                        (u24_address(cpu.registers.dbr, base_addr) + index) & B::ADDRESS_MASK;
                    cpu.state.t2 = bus.read(address);
                }
                5 => {
                    final_cycle(cpu, bus);

                    let base_addr = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
                    let index: u32 = cpu.registers.$index.into();
                    let address =
                        (u24_address(cpu.registers.dbr, base_addr) + index + 1) & B::ADDRESS_MASK;
                    let value_msb = bus.read(address);
                    let $value = u16::from_le_bytes([cpu.state.t2, value_msb]);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_absolute_indexed_u16;

macro_rules! impl_read_absolute_long_u8 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
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
                    let $value = bus.read(address);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_absolute_long_u8;

macro_rules! impl_read_absolute_long_u16 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
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
                    cpu.state.t3 = bus.read(address);
                }
                5 => {
                    final_cycle(cpu, bus);

                    let address = u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.state.t2, 0]);
                    let value_msb = bus.read((address + 1) & B::ADDRESS_MASK);
                    let $value = u16::from_le_bytes([cpu.state.t3, value_msb]);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_absolute_long_u16;

macro_rules! impl_read_absolute_long_indexed_u8 {
    ($index:ident, $name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
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

                    let base_addr =
                        u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.state.t2, 0]);
                    let index: u32 = cpu.registers.$index.into();
                    let address = (base_addr + index) & B::ADDRESS_MASK;
                    let $value = bus.read(address);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_absolute_long_indexed_u8;

macro_rules! impl_read_absolute_long_indexed_u16 {
    ($index:ident, $name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
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
                    let base_addr =
                        u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.state.t2, 0]);
                    let index: u32 = cpu.registers.$index.into();
                    let address = (base_addr + index) & B::ADDRESS_MASK;
                    cpu.state.t3 = bus.read(address);
                }
                5 => {
                    final_cycle(cpu, bus);

                    let base_addr =
                        u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.state.t2, 0]);
                    let index: u32 = cpu.registers.$index.into();
                    let address = (base_addr + index + 1) & B::ADDRESS_MASK;
                    let value_msb = bus.read(address);
                    let $value = u16::from_le_bytes([cpu.state.t3, value_msb]);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_absolute_long_indexed_u16;

macro_rules! impl_read_stack_relative_u8 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    // Idle cycle for indexing into stack
                    bus.idle();
                }
                3 => {
                    final_cycle(cpu, bus);

                    let address = cpu.registers.s.wrapping_add(cpu.state.t0.into());
                    let $value = bus.read(address.into());
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_stack_relative_u8;

macro_rules! impl_read_stack_relative_u16 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    // Idle cycle for indexing into stack
                    bus.idle();
                }
                3 => {
                    let address = cpu.registers.s.wrapping_add(cpu.state.t0.into());
                    cpu.state.t1 = bus.read(address.into());
                }
                4 => {
                    final_cycle(cpu, bus);

                    let address = cpu.registers.s.wrapping_add(cpu.state.t0.into());
                    let value_msb = bus.read(address.wrapping_add(1).into());
                    let $value = u16::from_le_bytes([cpu.state.t1, value_msb]);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_stack_relative_u16;

macro_rules! impl_read_stack_relative_indirect_indexed_u8 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    // Idle cycle for indexing into stack
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
                5 => {
                    // Idle cycle for indexing with Y
                    bus.idle();
                }
                6 => {
                    final_cycle(cpu, bus);

                    let base_addr = u24_address(
                        cpu.registers.dbr,
                        u16::from_le_bytes([cpu.state.t1, cpu.state.t2]),
                    );
                    let index: u32 = cpu.registers.y.into();
                    let address = (base_addr + index) & B::ADDRESS_MASK;
                    let $value = bus.read(address);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_stack_relative_indirect_indexed_u8;

macro_rules! impl_read_stack_relative_indirect_indexed_u16 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    // Idle cycle for indexing into stack
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
                5 => {
                    // Idle cycle for indexing with Y
                    bus.idle();
                }
                6 => {
                    let base_addr = u24_address(
                        cpu.registers.dbr,
                        u16::from_le_bytes([cpu.state.t1, cpu.state.t2]),
                    );
                    let index: u32 = cpu.registers.y.into();
                    let address = (base_addr + index) & B::ADDRESS_MASK;
                    cpu.state.t3 = bus.read(address);
                }
                7 => {
                    final_cycle(cpu, bus);

                    let base_addr = u24_address(
                        cpu.registers.dbr,
                        u16::from_le_bytes([cpu.state.t1, cpu.state.t2]),
                    );
                    let index: u32 = cpu.registers.y.into();
                    let address = (base_addr + index + 1) & B::ADDRESS_MASK;
                    let value_msb = bus.read(address);
                    let $value = u16::from_le_bytes([cpu.state.t3, value_msb]);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_stack_relative_indirect_indexed_u16;

macro_rules! impl_read_op_u8 {
    (@inner immediate, $($rest:tt)*) => {
        impl_read_immediate_u8!($($rest)*);
    };
    (@inner direct_page, $($rest:tt)*) => {
        impl_read_direct_page_u8!($($rest)*);
    };
    (@inner direct_page_x, $($rest:tt)*) => {
        impl_read_direct_page_indexed_u8!(x, $($rest)*);
    };
    (@inner direct_page_y, $($rest:tt)*) => {
        impl_read_direct_page_indexed_u8!(y, $($rest)*);
    };
    (@inner direct_page_indirect, $($rest:tt)*) => {
        impl_read_direct_page_indirect_u8!($($rest)*);
    };
    (@inner direct_page_indexed_indirect, $($rest:tt)*) => {
        impl_read_direct_page_indexed_indirect_u8!($($rest)*);
    };
    (@inner direct_page_indirect_indexed, $($rest:tt)*) => {
        impl_read_direct_page_indirect_indexed_u8!($($rest)*);
    };
    (@inner direct_page_indirect_long, $($rest:tt)*) => {
        impl_read_direct_page_indirect_long_u8!($($rest)*);
    };
    (@inner direct_page_indirect_long_indexed, $($rest:tt)*) => {
        impl_read_direct_page_indirect_long_indexed_u8!($($rest)*);
    };
    (@inner absolute, $($rest:tt)*) => {
        impl_read_absolute_u8!($($rest)*);
    };
    (@inner absolute_x, $($rest:tt)*) => {
        impl_read_absolute_indexed_u8!(x, $($rest)*);
    };
    (@inner absolute_y, $($rest:tt)*) => {
        impl_read_absolute_indexed_u8!(y, $($rest)*);
    };
    (@inner absolute_long, $($rest:tt)*) => {
        impl_read_absolute_long_u8!($($rest)*);
    };
    (@inner absolute_long_x, $($rest:tt)*) => {
        impl_read_absolute_long_indexed_u8!(x, $($rest)*);
    };
    (@inner stack_relative, $($rest:tt)*) => {
        impl_read_stack_relative_u8!($($rest)*);
    };
    (@inner stack_relative_indirect_indexed, $($rest:tt)*) => {
        impl_read_stack_relative_indirect_indexed_u8!($($rest)*);
    };
    ([$($am:tt -> $name:ident),* $(,)?] $rest:tt) => {
        $(
            impl_read_op_u8!(@inner $am, $name, $rest);
        )*
    };
}

use impl_read_op_u8;

macro_rules! impl_read_op_u16 {
    (@inner immediate, $($rest:tt)*) => {
        impl_read_immediate_u16!($($rest)*);
    };
    (@inner direct_page, $($rest:tt)*) => {
        impl_read_direct_page_u16!($($rest)*);
    };
    (@inner direct_page_x, $($rest:tt)*) => {
        impl_read_direct_page_indexed_u16!(x, $($rest)*);
    };
    (@inner direct_page_y, $($rest:tt)*) => {
        impl_read_direct_page_indexed_u16!(y, $($rest)*);
    };
    (@inner direct_page_indirect, $($rest:tt)*) => {
        impl_read_direct_page_indirect_u16!($($rest)*);
    };
    (@inner direct_page_indexed_indirect, $($rest:tt)*) => {
        impl_read_direct_page_indexed_indirect_u16!($($rest)*);
    };
    (@inner direct_page_indirect_indexed, $($rest:tt)*) => {
        impl_read_direct_page_indirect_indexed_u16!($($rest)*);
    };
    (@inner direct_page_indirect_long, $($rest:tt)*) => {
        impl_read_direct_page_indirect_long_u16!($($rest)*);
    };
    (@inner direct_page_indirect_long_indexed, $($rest:tt)*) => {
        impl_read_direct_page_indirect_long_indexed_u16!($($rest)*);
    };
    (@inner absolute, $($rest:tt)*) => {
        impl_read_absolute_u16!($($rest)*);
    };
    (@inner absolute_x, $($rest:tt)*) => {
        impl_read_absolute_indexed_u16!(x, $($rest)*);
    };
    (@inner absolute_y, $($rest:tt)*) => {
        impl_read_absolute_indexed_u16!(y, $($rest)*);
    };
    (@inner absolute_long, $($rest:tt)*) => {
        impl_read_absolute_long_u16!($($rest)*);
    };
    (@inner absolute_long_x, $($rest:tt)*) => {
        impl_read_absolute_long_indexed_u16!(x, $($rest)*);
    };
    (@inner stack_relative, $($rest:tt)*) => {
        impl_read_stack_relative_u16!($($rest)*);
    };
    (@inner stack_relative_indirect_indexed, $($rest:tt)*) => {
        impl_read_stack_relative_indirect_indexed_u16!($($rest)*);
    };
    ([$($am:tt -> $name:ident),* $(,)?] $rest:tt) => {
        $(
            impl_read_op_u16!(@inner $am, $name, $rest);
        )*
    }
}

use impl_read_op_u16;

macro_rules! impl_modify_accumulator_u8 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            final_cycle(cpu, bus);

            bus.idle();

            let $value = cpu.registers.a as u8;
            let $registers = &mut cpu.registers;
            let value = $body;
            cpu.registers.set_a_u8(value);
        }
    };
}

use impl_modify_accumulator_u8;

macro_rules! impl_modify_accumulator_u16 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            final_cycle(cpu, bus);

            bus.idle();

            let $value = cpu.registers.a;
            let $registers = &mut cpu.registers;
            let value = $body;
            cpu.registers.a = value;
        }
    };
}

use impl_modify_accumulator_u16;

macro_rules! impl_modify_direct_page_u8 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    cpu.state.t1 = bus.read(address.into());
                }
                4 => {
                    if cpu.registers.emulation_mode {
                        // Write value back to address
                        let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
                        bus.write(address.into(), cpu.state.t1);
                    } else {
                        // Idle cycle for computation
                        bus.idle();
                    }
                }
                5 => {
                    final_cycle(cpu, bus);

                    let $value = cpu.state.t1;
                    let $registers = &mut cpu.registers;
                    let value = $body;

                    let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
                    bus.write(address.into(), value);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_modify_direct_page_u8;

macro_rules! impl_modify_direct_page_u16 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    cpu.state.t1 = bus.read(address.into());
                }
                4 => {
                    let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
                    cpu.state.t2 = bus.read(address.wrapping_add(1).into());
                }
                5 => {
                    // Idle cycle for computation
                    bus.idle();
                }
                6 => {
                    let $value = u16::from_le_bytes([cpu.state.t1, cpu.state.t2]);
                    let $registers = &mut cpu.registers;
                    let value = $body;
                    let [value_lsb, value_msb] = value.to_le_bytes();

                    let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
                    bus.write(address.wrapping_add(1).into(), value_msb);
                    cpu.state.t3 = value_lsb;
                }
                7 => {
                    final_cycle(cpu, bus);

                    let address = cpu.registers.d.wrapping_add(cpu.state.t0.into());
                    bus.write(address.into(), cpu.state.t3);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_modify_direct_page_u16;

macro_rules! impl_modify_direct_page_indexed_u8 {
    ($index:ident, $name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    cpu.state.t1 = bus.read(address.into());
                }
                5 => {
                    if cpu.registers.emulation_mode {
                        // Write value back to address
                        let address = index_direct_page(cpu, cpu.state.t0, cpu.registers.$index);
                        bus.write(address.into(), cpu.state.t1);
                    } else {
                        // Idle cycle for computation
                        bus.idle();
                    }
                }
                6 => {
                    final_cycle(cpu, bus);

                    let $value = cpu.state.t1;
                    let $registers = &mut cpu.registers;
                    let value = $body;

                    let address = index_direct_page(cpu, cpu.state.t0, cpu.registers.$index);
                    bus.write(address.into(), value);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_modify_direct_page_indexed_u8;

macro_rules! impl_modify_direct_page_indexed_u16 {
    ($index:ident, $name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    cpu.state.t1 = bus.read(address.into());
                }
                5 => {
                    let address = index_direct_page(cpu, cpu.state.t0, cpu.registers.$index);
                    cpu.state.t2 = bus.read(address.wrapping_add(1).into());
                }
                6 => {
                    // Idle cycle for computation
                    bus.idle();
                }
                7 => {
                    let $value = u16::from_le_bytes([cpu.state.t1, cpu.state.t2]);
                    let $registers = &mut cpu.registers;
                    let value = $body;
                    let [value_lsb, value_msb] = value.to_le_bytes();

                    let address = index_direct_page(cpu, cpu.state.t0, cpu.registers.$index);
                    bus.write(address.wrapping_add(1).into(), value_msb);
                    cpu.state.t3 = value_lsb;
                }
                8 => {
                    final_cycle(cpu, bus);

                    let address = index_direct_page(cpu, cpu.state.t0, cpu.registers.$index);
                    bus.write(address.into(), cpu.state.t3);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_modify_direct_page_indexed_u16;

macro_rules! impl_modify_absolute_u8 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    cpu.state.t2 = bus.read(address);
                }
                4 => {
                    if cpu.registers.emulation_mode {
                        // Write back to address
                        let address =
                            u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.registers.dbr, 0]);
                        bus.write(address, cpu.state.t2);
                    } else {
                        // Idle cycle for computation
                        bus.idle();
                    }
                }
                5 => {
                    final_cycle(cpu, bus);

                    let $value = cpu.state.t2;
                    let $registers = &mut cpu.registers;
                    let value = $body;

                    let address =
                        u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.registers.dbr, 0]);
                    bus.write(address, value);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_modify_absolute_u8;

macro_rules! impl_modify_absolute_u16 {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    cpu.state.t2 = bus.read(address);
                }
                4 => {
                    let address =
                        u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.registers.dbr, 0]);
                    cpu.state.t3 = bus.read((address + 1) & B::ADDRESS_MASK);
                }
                5 => {
                    // Idle cycle for computation
                    bus.idle();
                }
                6 => {
                    let $value = u16::from_le_bytes([cpu.state.t2, cpu.state.t3]);
                    let $registers = &mut cpu.registers;
                    let value = $body;
                    let [value_lsb, value_msb] = value.to_le_bytes();

                    let address =
                        u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.registers.dbr, 0]);
                    bus.write((address + 1) & B::ADDRESS_MASK, value_msb);
                    cpu.state.t4 = value_lsb;
                }
                7 => {
                    final_cycle(cpu, bus);

                    let address =
                        u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.registers.dbr, 0]);
                    bus.write(address, cpu.state.t4);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_modify_absolute_u16;

macro_rules! impl_modify_absolute_indexed_u8 {
    ($index:ident, $name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    let address = (base_addr + u32::from(cpu.registers.$index)) & B::ADDRESS_MASK;
                    cpu.state.t2 = bus.read(address);
                }
                5 => {
                    if cpu.registers.emulation_mode {
                        // Write back to address
                        let base_addr =
                            u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.registers.dbr, 0]);
                        let address =
                            (base_addr + u32::from(cpu.registers.$index)) & B::ADDRESS_MASK;
                        bus.write(address, cpu.state.t2);
                    } else {
                        // Idle cycle for computation
                        bus.idle();
                    }
                }
                6 => {
                    final_cycle(cpu, bus);

                    let $value = cpu.state.t2;
                    let $registers = &mut cpu.registers;
                    let value = $body;

                    let base_addr =
                        u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.registers.dbr, 0]);
                    let address = (base_addr + u32::from(cpu.registers.$index)) & B::ADDRESS_MASK;
                    bus.write(address, value);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_modify_absolute_indexed_u8;

macro_rules! impl_modify_absolute_indexed_u16 {
    ($index:ident, $name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    let address = (base_addr + u32::from(cpu.registers.$index)) & B::ADDRESS_MASK;
                    cpu.state.t2 = bus.read(address);
                }
                5 => {
                    let base_addr =
                        u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.registers.dbr, 0]);
                    let address =
                        (base_addr + u32::from(cpu.registers.$index) + 1) & B::ADDRESS_MASK;
                    cpu.state.t3 = bus.read(address);
                }
                6 => {
                    // Idle cycle for computation
                    bus.idle();
                }
                7 => {
                    let $value = u16::from_le_bytes([cpu.state.t2, cpu.state.t3]);
                    let $registers = &mut cpu.registers;
                    let value = $body;
                    let [value_lsb, value_msb] = value.to_le_bytes();

                    let base_addr =
                        u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.registers.dbr, 0]);
                    let address =
                        (base_addr + u32::from(cpu.registers.$index) + 1) & B::ADDRESS_MASK;
                    bus.write(address, value_msb);
                    cpu.state.t4 = value_lsb;
                }
                8 => {
                    final_cycle(cpu, bus);

                    let base_addr =
                        u32::from_le_bytes([cpu.state.t0, cpu.state.t1, cpu.registers.dbr, 0]);
                    let address = (base_addr + u32::from(cpu.registers.$index)) & B::ADDRESS_MASK;
                    bus.write(address, cpu.state.t4);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_modify_absolute_indexed_u16;

macro_rules! impl_modify_op_u8 {
    (@inner accumulator, $($rest:tt)*) => {
        impl_modify_accumulator_u8!($($rest)*);
    };
    (@inner direct_page, $($rest:tt)*) => {
        impl_modify_direct_page_u8!($($rest)*);
    };
    (@inner direct_page_x, $($rest:tt)*) => {
        impl_modify_direct_page_indexed_u8!(x, $($rest)*);
    };
    (@inner absolute, $($rest:tt)*) => {
        impl_modify_absolute_u8!($($rest)*);
    };
    (@inner absolute_x, $($rest:tt)*) => {
        impl_modify_absolute_indexed_u8!(x, $($rest)*);
    };
    ([$($am:tt -> $name:ident),* $(,)?] $rest:tt) => {
        $(
            impl_modify_op_u8!(@inner $am, $name, $rest);
        )*
    }
}

use impl_modify_op_u8;

macro_rules! impl_modify_op_u16 {
    (@inner accumulator, $($rest:tt)*) => {
        impl_modify_accumulator_u16!($($rest)*);
    };
    (@inner direct_page, $($rest:tt)*) => {
        impl_modify_direct_page_u16!($($rest)*);
    };
    (@inner direct_page_x, $($rest:tt)*) => {
        impl_modify_direct_page_indexed_u16!(x, $($rest)*);
    };
    (@inner absolute, $($rest:tt)*) => {
        impl_modify_absolute_u16!($($rest)*);
    };
    (@inner absolute_x, $($rest:tt)*) => {
        impl_modify_absolute_indexed_u16!(x, $($rest)*);
    };
    ([$($am:tt -> $name:ident),* $(,)?] $rest:tt) => {
        $(
            impl_modify_op_u16!(@inner $am, $name, $rest);
        )*
    }
}

use impl_modify_op_u16;

macro_rules! impl_registers_op {
    ($name:ident, |$registers:ident| $body:block) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            final_cycle(cpu, bus);

            bus.idle();
            let $registers = &mut cpu.registers;
            $body
        }
    };
}

use impl_registers_op;

// NOP: No-op
impl_registers_op!(nop, |_registers| {});

// WDM: William D. Mensch, Jr. (2-byte no-op)
fn wdm<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    final_cycle(cpu, bus);

    bus.idle();

    cpu.registers.pc = cpu.registers.pc.wrapping_add(1);
}

pub fn execute<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    if cpu.state.cycle != 0 {
        // If mid-instruction, continue to execute that instruction
        execute_cycle(cpu, bus);

        // Increment only if the instruction didn't finish
        if cpu.state.cycle != 0 {
            cpu.state.cycle += 1;
        }

        return;
    }

    // If stopped, the CPU is halted until reset
    if cpu.state.stopped {
        bus.idle();
        return;
    }

    // If waiting, the CPU is halted until an NMI or IRQ triggers; check last_irq instead of irq_triggered
    // because IRQs will unhalt the CPU even if the I flag is set (though the IRQ won't be handled in that case)
    if cpu.state.waiting && !cpu.state.nmi_triggered && !cpu.state.last_irq {
        poll_interrupt_lines(cpu, bus);
        bus.idle();
        return;
    }
    cpu.state.waiting = false;

    if cpu.state.nmi_triggered {
        log::trace!("Handling NMI");

        cpu.state.nmi_triggered = false;
        bus.acknowledge_nmi();

        flow::handle_interrupt(cpu, bus, InterruptType::Nmi);
        cpu.state.handling_interrupt = Some(InterruptType::Nmi);
        cpu.state.cycle = 1;
        return;
    }

    if cpu.state.irq_triggered {
        log::trace!("Handling IRQ");

        flow::handle_interrupt(cpu, bus, InterruptType::Irq);
        cpu.state.handling_interrupt = Some(InterruptType::Irq);
        cpu.state.cycle = 1;
        return;
    }

    // Cycle 0 and not handling an interrupt is always opcode fetch
    cpu.state.opcode = fetch_operand(cpu, bus);
    cpu.state.cycle = 1;

    if log::log_enabled!(log::Level::Trace) {
        let instr_name = disassemble::instruction_str(cpu.state.opcode);
        log::trace!(
            "Fetched opcode from PBR={:02X} / PC={:04X}: {:02X} ({instr_name}); m={}, x={}, e={}",
            cpu.registers.pbr,
            cpu.registers.pc.wrapping_sub(1),
            cpu.state.opcode,
            u8::from(cpu.registers.p.accumulator_size == SizeBits::Eight),
            u8::from(cpu.registers.p.index_size == SizeBits::Eight),
            u8::from(cpu.registers.emulation_mode),
        );
    }
}

fn execute_cycle<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    use SizeBits::{Eight, Sixteen};

    if let Some(interrupt) = cpu.state.handling_interrupt {
        flow::handle_interrupt(cpu, bus, interrupt);

        if cpu.state.cycle == 0 {
            cpu.state.handling_interrupt = None;
        }

        return;
    }

    match cpu.state.opcode {
        0x00 => flow::handle_interrupt(cpu, bus, InterruptType::Breakpoint),
        0x01 => match cpu.registers.p.accumulator_size {
            Eight => alu::ora_direct_page_indexed_indirect_u8(cpu, bus),
            Sixteen => alu::ora_direct_page_indexed_indirect_u16(cpu, bus),
        },
        0x02 => flow::handle_interrupt(cpu, bus, InterruptType::Coprocessor),
        0x03 => match cpu.registers.p.accumulator_size {
            Eight => alu::ora_stack_relative_u8(cpu, bus),
            Sixteen => alu::ora_stack_relative_u16(cpu, bus),
        },
        0x04 => match cpu.registers.p.accumulator_size {
            Eight => bits::tsb_direct_page_u8(cpu, bus),
            Sixteen => bits::tsb_direct_page_u16(cpu, bus),
        },
        0x05 => match cpu.registers.p.accumulator_size {
            Eight => alu::ora_direct_page_u8(cpu, bus),
            Sixteen => alu::ora_direct_page_u16(cpu, bus),
        },
        0x06 => match cpu.registers.p.accumulator_size {
            Eight => bits::asl_direct_page_u8(cpu, bus),
            Sixteen => bits::asl_direct_page_u16(cpu, bus),
        },
        0x07 => match cpu.registers.p.accumulator_size {
            Eight => alu::ora_direct_page_indirect_long_u8(cpu, bus),
            Sixteen => alu::ora_direct_page_indirect_long_u16(cpu, bus),
        },
        0x08 => load::php(cpu, bus),
        0x09 => match cpu.registers.p.accumulator_size {
            Eight => alu::ora_immediate_u8(cpu, bus),
            Sixteen => alu::ora_immediate_u16(cpu, bus),
        },
        0x0A => match cpu.registers.p.accumulator_size {
            Eight => bits::asl_accumulator_u8(cpu, bus),
            Sixteen => bits::asl_accumulator_u16(cpu, bus),
        },
        0x0B => load::phd(cpu, bus),
        0x0C => match cpu.registers.p.accumulator_size {
            Eight => bits::tsb_absolute_u8(cpu, bus),
            Sixteen => bits::tsb_absolute_u16(cpu, bus),
        },
        0x0D => match cpu.registers.p.accumulator_size {
            Eight => alu::ora_absolute_u8(cpu, bus),
            Sixteen => alu::ora_absolute_u16(cpu, bus),
        },
        0x0E => match cpu.registers.p.accumulator_size {
            Eight => bits::asl_absolute_u8(cpu, bus),
            Sixteen => bits::asl_absolute_u16(cpu, bus),
        },
        0x0F => match cpu.registers.p.accumulator_size {
            Eight => alu::ora_absolute_long_u8(cpu, bus),
            Sixteen => alu::ora_absolute_long_u16(cpu, bus),
        },
        0x10 => flow::bpl(cpu, bus),
        0x11 => match cpu.registers.p.accumulator_size {
            Eight => alu::ora_direct_page_indirect_indexed_u8(cpu, bus),
            Sixteen => alu::ora_direct_page_indirect_indexed_u16(cpu, bus),
        },
        0x12 => match cpu.registers.p.accumulator_size {
            Eight => alu::ora_direct_page_indirect_u8(cpu, bus),
            Sixteen => alu::ora_direct_page_indirect_u16(cpu, bus),
        },
        0x13 => match cpu.registers.p.accumulator_size {
            Eight => alu::ora_stack_relative_indirect_indexed_u8(cpu, bus),
            Sixteen => alu::ora_stack_relative_indirect_indexed_u16(cpu, bus),
        },
        0x14 => match cpu.registers.p.accumulator_size {
            Eight => bits::trb_direct_page_u8(cpu, bus),
            Sixteen => bits::trb_direct_page_u16(cpu, bus),
        },
        0x15 => match cpu.registers.p.accumulator_size {
            Eight => alu::ora_direct_page_x_u8(cpu, bus),
            Sixteen => alu::ora_direct_page_x_u16(cpu, bus),
        },
        0x16 => match cpu.registers.p.accumulator_size {
            Eight => bits::asl_direct_page_x_u8(cpu, bus),
            Sixteen => bits::asl_direct_page_x_u16(cpu, bus),
        },
        0x17 => match cpu.registers.p.accumulator_size {
            Eight => alu::ora_direct_page_indirect_long_indexed_u8(cpu, bus),
            Sixteen => alu::ora_direct_page_indirect_long_indexed_u16(cpu, bus),
        },
        0x18 => flags::clc(cpu, bus),
        0x19 => match cpu.registers.p.accumulator_size {
            Eight => alu::ora_absolute_y_u8(cpu, bus),
            Sixteen => alu::ora_absolute_y_u16(cpu, bus),
        },
        0x1A => match cpu.registers.p.accumulator_size {
            Eight => alu::inc_accumulator_u8(cpu, bus),
            Sixteen => alu::inc_accumulator_u16(cpu, bus),
        },
        0x1B => load::tcs(cpu, bus),
        0x1C => match cpu.registers.p.accumulator_size {
            Eight => bits::trb_absolute_u8(cpu, bus),
            Sixteen => bits::trb_absolute_u16(cpu, bus),
        },
        0x1D => match cpu.registers.p.accumulator_size {
            Eight => alu::ora_absolute_x_u8(cpu, bus),
            Sixteen => alu::ora_absolute_x_u16(cpu, bus),
        },
        0x1E => match cpu.registers.p.accumulator_size {
            Eight => bits::asl_absolute_x_u8(cpu, bus),
            Sixteen => bits::asl_absolute_x_u16(cpu, bus),
        },
        0x1F => match cpu.registers.p.accumulator_size {
            Eight => alu::ora_absolute_long_x_u8(cpu, bus),
            Sixteen => alu::ora_absolute_long_x_u16(cpu, bus),
        },
        0x20 => flow::jsr_absolute(cpu, bus),
        0x21 => match cpu.registers.p.accumulator_size {
            Eight => alu::and_direct_page_indexed_indirect_u8(cpu, bus),
            Sixteen => alu::and_direct_page_indexed_indirect_u16(cpu, bus),
        },
        0x22 => flow::jsl(cpu, bus),
        0x23 => match cpu.registers.p.accumulator_size {
            Eight => alu::and_stack_relative_u8(cpu, bus),
            Sixteen => alu::and_stack_relative_u16(cpu, bus),
        },
        0x24 => match cpu.registers.p.accumulator_size {
            Eight => bits::bit_direct_page_u8(cpu, bus),
            Sixteen => bits::bit_direct_page_u16(cpu, bus),
        },
        0x25 => match cpu.registers.p.accumulator_size {
            Eight => alu::and_direct_page_u8(cpu, bus),
            Sixteen => alu::and_direct_page_u16(cpu, bus),
        },
        0x26 => match cpu.registers.p.accumulator_size {
            Eight => bits::rol_direct_page_u8(cpu, bus),
            Sixteen => bits::rol_direct_page_u16(cpu, bus),
        },
        0x27 => match cpu.registers.p.accumulator_size {
            Eight => alu::and_direct_page_indirect_long_u8(cpu, bus),
            Sixteen => alu::and_direct_page_indirect_long_u16(cpu, bus),
        },
        0x28 => load::plp(cpu, bus),
        0x29 => match cpu.registers.p.accumulator_size {
            Eight => alu::and_immediate_u8(cpu, bus),
            Sixteen => alu::and_immediate_u16(cpu, bus),
        },
        0x2A => match cpu.registers.p.accumulator_size {
            Eight => bits::rol_accumulator_u8(cpu, bus),
            Sixteen => bits::rol_accumulator_u16(cpu, bus),
        },
        0x2B => load::pld(cpu, bus),
        0x2C => match cpu.registers.p.accumulator_size {
            Eight => bits::bit_absolute_u8(cpu, bus),
            Sixteen => bits::bit_absolute_u16(cpu, bus),
        },
        0x2D => match cpu.registers.p.accumulator_size {
            Eight => alu::and_absolute_u8(cpu, bus),
            Sixteen => alu::and_absolute_u16(cpu, bus),
        },
        0x2E => match cpu.registers.p.accumulator_size {
            Eight => bits::rol_absolute_u8(cpu, bus),
            Sixteen => bits::rol_absolute_u16(cpu, bus),
        },
        0x2F => match cpu.registers.p.accumulator_size {
            Eight => alu::and_absolute_long_u8(cpu, bus),
            Sixteen => alu::and_absolute_long_u16(cpu, bus),
        },
        0x30 => flow::bmi(cpu, bus),
        0x31 => match cpu.registers.p.accumulator_size {
            Eight => alu::and_direct_page_indirect_indexed_u8(cpu, bus),
            Sixteen => alu::and_direct_page_indirect_indexed_u16(cpu, bus),
        },
        0x32 => match cpu.registers.p.accumulator_size {
            Eight => alu::and_direct_page_indirect_u8(cpu, bus),
            Sixteen => alu::and_direct_page_indirect_u16(cpu, bus),
        },
        0x33 => match cpu.registers.p.accumulator_size {
            Eight => alu::and_stack_relative_indirect_indexed_u8(cpu, bus),
            Sixteen => alu::and_stack_relative_indirect_indexed_u16(cpu, bus),
        },
        0x34 => match cpu.registers.p.accumulator_size {
            Eight => bits::bit_direct_page_x_u8(cpu, bus),
            Sixteen => bits::bit_direct_page_x_u16(cpu, bus),
        },
        0x35 => match cpu.registers.p.accumulator_size {
            Eight => alu::and_direct_page_x_u8(cpu, bus),
            Sixteen => alu::and_direct_page_x_u16(cpu, bus),
        },
        0x36 => match cpu.registers.p.accumulator_size {
            Eight => bits::rol_direct_page_x_u8(cpu, bus),
            Sixteen => bits::rol_direct_page_x_u16(cpu, bus),
        },
        0x37 => match cpu.registers.p.accumulator_size {
            Eight => alu::and_direct_page_indirect_long_indexed_u8(cpu, bus),
            Sixteen => alu::and_direct_page_indirect_long_indexed_u16(cpu, bus),
        },
        0x38 => flags::sec(cpu, bus),
        0x39 => match cpu.registers.p.accumulator_size {
            Eight => alu::and_absolute_y_u8(cpu, bus),
            Sixteen => alu::and_absolute_y_u16(cpu, bus),
        },
        0x3A => match cpu.registers.p.accumulator_size {
            Eight => alu::dec_accumulator_u8(cpu, bus),
            Sixteen => alu::dec_accumulator_u16(cpu, bus),
        },
        0x3B => load::tsc(cpu, bus),
        0x3C => match cpu.registers.p.accumulator_size {
            Eight => bits::bit_absolute_x_u8(cpu, bus),
            Sixteen => bits::bit_absolute_x_u16(cpu, bus),
        },
        0x3D => match cpu.registers.p.accumulator_size {
            Eight => alu::and_absolute_x_u8(cpu, bus),
            Sixteen => alu::and_absolute_x_u16(cpu, bus),
        },
        0x3E => match cpu.registers.p.accumulator_size {
            Eight => bits::rol_absolute_x_u8(cpu, bus),
            Sixteen => bits::rol_absolute_x_u16(cpu, bus),
        },
        0x3F => match cpu.registers.p.accumulator_size {
            Eight => alu::and_absolute_long_x_u8(cpu, bus),
            Sixteen => alu::and_absolute_long_x_u16(cpu, bus),
        },
        0x40 => flow::rti(cpu, bus),
        0x41 => match cpu.registers.p.accumulator_size {
            Eight => alu::eor_direct_page_indexed_indirect_u8(cpu, bus),
            Sixteen => alu::eor_direct_page_indexed_indirect_u16(cpu, bus),
        },
        0x42 => wdm(cpu, bus),
        0x43 => match cpu.registers.p.accumulator_size {
            Eight => alu::eor_stack_relative_u8(cpu, bus),
            Sixteen => alu::eor_stack_relative_u16(cpu, bus),
        },
        0x44 => load::mvp(cpu, bus),
        0x45 => match cpu.registers.p.accumulator_size {
            Eight => alu::eor_direct_page_u8(cpu, bus),
            Sixteen => alu::eor_direct_page_u16(cpu, bus),
        },
        0x46 => match cpu.registers.p.accumulator_size {
            Eight => bits::lsr_direct_page_u8(cpu, bus),
            Sixteen => bits::lsr_direct_page_u16(cpu, bus),
        },
        0x47 => match cpu.registers.p.accumulator_size {
            Eight => alu::eor_direct_page_indirect_long_u8(cpu, bus),
            Sixteen => alu::eor_direct_page_indirect_long_u16(cpu, bus),
        },
        0x48 => match cpu.registers.p.accumulator_size {
            Eight => load::pha_u8(cpu, bus),
            Sixteen => load::pha_u16(cpu, bus),
        },
        0x49 => match cpu.registers.p.accumulator_size {
            Eight => alu::eor_immediate_u8(cpu, bus),
            Sixteen => alu::eor_immediate_u16(cpu, bus),
        },
        0x4A => match cpu.registers.p.accumulator_size {
            Eight => bits::lsr_accumulator_u8(cpu, bus),
            Sixteen => bits::lsr_accumulator_u16(cpu, bus),
        },
        0x4B => load::phk(cpu, bus),
        0x4C => flow::jmp_absolute(cpu, bus),
        0x4D => match cpu.registers.p.accumulator_size {
            Eight => alu::eor_absolute_u8(cpu, bus),
            Sixteen => alu::eor_absolute_u16(cpu, bus),
        },
        0x4E => match cpu.registers.p.accumulator_size {
            Eight => bits::lsr_absolute_u8(cpu, bus),
            Sixteen => bits::lsr_absolute_u16(cpu, bus),
        },
        0x4F => match cpu.registers.p.accumulator_size {
            Eight => alu::eor_absolute_long_u8(cpu, bus),
            Sixteen => alu::eor_absolute_long_u16(cpu, bus),
        },
        0x50 => flow::bvc(cpu, bus),
        0x51 => match cpu.registers.p.accumulator_size {
            Eight => alu::eor_direct_page_indirect_indexed_u8(cpu, bus),
            Sixteen => alu::eor_direct_page_indirect_indexed_u16(cpu, bus),
        },
        0x52 => match cpu.registers.p.accumulator_size {
            Eight => alu::eor_direct_page_indirect_u8(cpu, bus),
            Sixteen => alu::eor_direct_page_indirect_u16(cpu, bus),
        },
        0x53 => match cpu.registers.p.accumulator_size {
            Eight => alu::eor_stack_relative_indirect_indexed_u8(cpu, bus),
            Sixteen => alu::eor_stack_relative_indirect_indexed_u16(cpu, bus),
        },
        0x54 => load::mvn(cpu, bus),
        0x55 => match cpu.registers.p.accumulator_size {
            Eight => alu::eor_direct_page_x_u8(cpu, bus),
            Sixteen => alu::eor_direct_page_x_u16(cpu, bus),
        },
        0x56 => match cpu.registers.p.accumulator_size {
            Eight => bits::lsr_direct_page_x_u8(cpu, bus),
            Sixteen => bits::lsr_direct_page_x_u16(cpu, bus),
        },
        0x57 => match cpu.registers.p.accumulator_size {
            Eight => alu::eor_direct_page_indirect_long_indexed_u8(cpu, bus),
            Sixteen => alu::eor_direct_page_indirect_long_indexed_u16(cpu, bus),
        },
        0x58 => flags::cli(cpu, bus),
        0x59 => match cpu.registers.p.accumulator_size {
            Eight => alu::eor_absolute_y_u8(cpu, bus),
            Sixteen => alu::eor_absolute_y_u16(cpu, bus),
        },
        0x5A => match cpu.registers.p.index_size {
            Eight => load::phy_u8(cpu, bus),
            Sixteen => load::phy_u16(cpu, bus),
        },
        0x5B => load::tcd(cpu, bus),
        0x5C => flow::jmp_absolute_long(cpu, bus),
        0x5D => match cpu.registers.p.accumulator_size {
            Eight => alu::eor_absolute_x_u8(cpu, bus),
            Sixteen => alu::eor_absolute_x_u16(cpu, bus),
        },
        0x5E => match cpu.registers.p.accumulator_size {
            Eight => bits::lsr_absolute_x_u8(cpu, bus),
            Sixteen => bits::lsr_absolute_x_u16(cpu, bus),
        },
        0x5F => match cpu.registers.p.accumulator_size {
            Eight => alu::eor_absolute_long_x_u8(cpu, bus),
            Sixteen => alu::eor_absolute_long_x_u16(cpu, bus),
        },
        0x60 => flow::rts(cpu, bus),
        0x61 => match cpu.registers.p.accumulator_size {
            Eight => alu::adc_direct_page_indexed_indirect_u8(cpu, bus),
            Sixteen => alu::adc_direct_page_indexed_indirect_u16(cpu, bus),
        },
        0x62 => flow::per(cpu, bus),
        0x63 => match cpu.registers.p.accumulator_size {
            Eight => alu::adc_stack_relative_u8(cpu, bus),
            Sixteen => alu::adc_stack_relative_u16(cpu, bus),
        },
        0x64 => match cpu.registers.p.accumulator_size {
            Eight => load::stz_direct_page_u8(cpu, bus),
            Sixteen => load::stz_direct_page_u16(cpu, bus),
        },
        0x65 => match cpu.registers.p.accumulator_size {
            Eight => alu::adc_direct_page_u8(cpu, bus),
            Sixteen => alu::adc_direct_page_u16(cpu, bus),
        },
        0x66 => match cpu.registers.p.accumulator_size {
            Eight => bits::ror_direct_page_u8(cpu, bus),
            Sixteen => bits::ror_direct_page_u16(cpu, bus),
        },
        0x67 => match cpu.registers.p.accumulator_size {
            Eight => alu::adc_direct_page_indirect_long_u8(cpu, bus),
            Sixteen => alu::adc_direct_page_indirect_long_u16(cpu, bus),
        },
        0x68 => match cpu.registers.p.accumulator_size {
            Eight => load::pla_u8(cpu, bus),
            Sixteen => load::pla_u16(cpu, bus),
        },
        0x69 => match cpu.registers.p.accumulator_size {
            Eight => alu::adc_immediate_u8(cpu, bus),
            Sixteen => alu::adc_immediate_u16(cpu, bus),
        },
        0x6A => match cpu.registers.p.accumulator_size {
            Eight => bits::ror_accumulator_u8(cpu, bus),
            Sixteen => bits::ror_accumulator_u16(cpu, bus),
        },
        0x6B => flow::rtl(cpu, bus),
        0x6C => flow::jmp_indirect(cpu, bus),
        0x6D => match cpu.registers.p.accumulator_size {
            Eight => alu::adc_absolute_u8(cpu, bus),
            Sixteen => alu::adc_absolute_u16(cpu, bus),
        },
        0x6E => match cpu.registers.p.accumulator_size {
            Eight => bits::ror_absolute_u8(cpu, bus),
            Sixteen => bits::ror_absolute_u16(cpu, bus),
        },
        0x6F => match cpu.registers.p.accumulator_size {
            Eight => alu::adc_absolute_long_u8(cpu, bus),
            Sixteen => alu::adc_absolute_long_u16(cpu, bus),
        },
        0x70 => flow::bvs(cpu, bus),
        0x71 => match cpu.registers.p.accumulator_size {
            Eight => alu::adc_direct_page_indirect_indexed_u8(cpu, bus),
            Sixteen => alu::adc_direct_page_indirect_indexed_u16(cpu, bus),
        },
        0x72 => match cpu.registers.p.accumulator_size {
            Eight => alu::adc_direct_page_indirect_u8(cpu, bus),
            Sixteen => alu::adc_direct_page_indirect_u16(cpu, bus),
        },
        0x73 => match cpu.registers.p.accumulator_size {
            Eight => alu::adc_stack_relative_indirect_indexed_u8(cpu, bus),
            Sixteen => alu::adc_stack_relative_indirect_indexed_u16(cpu, bus),
        },
        0x74 => match cpu.registers.p.accumulator_size {
            Eight => load::stz_direct_page_x_u8(cpu, bus),
            Sixteen => load::stz_direct_page_x_u16(cpu, bus),
        },
        0x75 => match cpu.registers.p.accumulator_size {
            Eight => alu::adc_direct_page_x_u8(cpu, bus),
            Sixteen => alu::adc_direct_page_x_u16(cpu, bus),
        },
        0x76 => match cpu.registers.p.accumulator_size {
            Eight => bits::ror_direct_page_x_u8(cpu, bus),
            Sixteen => bits::ror_direct_page_x_u16(cpu, bus),
        },
        0x77 => match cpu.registers.p.accumulator_size {
            Eight => alu::adc_direct_page_indirect_long_indexed_u8(cpu, bus),
            Sixteen => alu::adc_direct_page_indirect_long_indexed_u16(cpu, bus),
        },
        0x78 => flags::sei(cpu, bus),
        0x79 => match cpu.registers.p.accumulator_size {
            Eight => alu::adc_absolute_y_u8(cpu, bus),
            Sixteen => alu::adc_absolute_y_u16(cpu, bus),
        },
        0x7A => match cpu.registers.p.index_size {
            Eight => load::ply_u8(cpu, bus),
            Sixteen => load::ply_u16(cpu, bus),
        },
        0x7B => load::tdc(cpu, bus),
        0x7C => flow::jmp_indexed_indirect(cpu, bus),
        0x7D => match cpu.registers.p.accumulator_size {
            Eight => alu::adc_absolute_x_u8(cpu, bus),
            Sixteen => alu::adc_absolute_x_u16(cpu, bus),
        },
        0x7E => match cpu.registers.p.accumulator_size {
            Eight => bits::ror_absolute_x_u8(cpu, bus),
            Sixteen => bits::ror_absolute_x_u16(cpu, bus),
        },
        0x7F => match cpu.registers.p.accumulator_size {
            Eight => alu::adc_absolute_long_x_u8(cpu, bus),
            Sixteen => alu::adc_absolute_long_x_u16(cpu, bus),
        },
        0x80 => flow::bra(cpu, bus),
        0x81 => match cpu.registers.p.accumulator_size {
            Eight => load::sta_direct_page_indexed_indirect_u8(cpu, bus),
            Sixteen => load::sta_direct_page_indexed_indirect_u16(cpu, bus),
        },
        0x82 => flow::brl(cpu, bus),
        0x83 => match cpu.registers.p.accumulator_size {
            Eight => load::sta_stack_relative_u8(cpu, bus),
            Sixteen => load::sta_stack_relative_u16(cpu, bus),
        },
        0x84 => match cpu.registers.p.index_size {
            Eight => load::sty_direct_page_u8(cpu, bus),
            Sixteen => load::sty_direct_page_u16(cpu, bus),
        },
        0x85 => match cpu.registers.p.accumulator_size {
            Eight => load::sta_direct_page_u8(cpu, bus),
            Sixteen => load::sta_direct_page_u16(cpu, bus),
        },
        0x86 => match cpu.registers.p.index_size {
            Eight => load::stx_direct_page_u8(cpu, bus),
            Sixteen => load::stx_direct_page_u16(cpu, bus),
        },
        0x87 => match cpu.registers.p.accumulator_size {
            Eight => load::sta_direct_page_indirect_long_u8(cpu, bus),
            Sixteen => load::sta_direct_page_indirect_long_u16(cpu, bus),
        },
        0x88 => match cpu.registers.p.index_size {
            Eight => alu::dey_u8(cpu, bus),
            Sixteen => alu::dey_u16(cpu, bus),
        },
        0x89 => match cpu.registers.p.accumulator_size {
            Eight => bits::bit_immediate_u8(cpu, bus),
            Sixteen => bits::bit_immediate_u16(cpu, bus),
        },
        0x8A => match cpu.registers.p.accumulator_size {
            Eight => load::txa_u8(cpu, bus),
            Sixteen => load::txa_u16(cpu, bus),
        },
        0x8B => load::phb(cpu, bus),
        0x8C => match cpu.registers.p.index_size {
            Eight => load::sty_absolute_u8(cpu, bus),
            Sixteen => load::sty_absolute_u16(cpu, bus),
        },
        0x8D => match cpu.registers.p.accumulator_size {
            Eight => load::sta_absolute_u8(cpu, bus),
            Sixteen => load::sta_absolute_u16(cpu, bus),
        },
        0x8E => match cpu.registers.p.index_size {
            Eight => load::stx_absolute_u8(cpu, bus),
            Sixteen => load::stx_absolute_u16(cpu, bus),
        },
        0x8F => match cpu.registers.p.accumulator_size {
            Eight => load::sta_absolute_long_u8(cpu, bus),
            Sixteen => load::sta_absolute_long_u16(cpu, bus),
        },
        0x90 => flow::bcc(cpu, bus),
        0x91 => match cpu.registers.p.accumulator_size {
            Eight => load::sta_direct_page_indirect_indexed_u8(cpu, bus),
            Sixteen => load::sta_direct_page_indirect_indexed_u16(cpu, bus),
        },
        0x92 => match cpu.registers.p.accumulator_size {
            Eight => load::sta_direct_page_indirect_u8(cpu, bus),
            Sixteen => load::sta_direct_page_indirect_u16(cpu, bus),
        },
        0x93 => match cpu.registers.p.accumulator_size {
            Eight => load::sta_stack_relative_indirect_indexed_u8(cpu, bus),
            Sixteen => load::sta_stack_relative_indirect_indexed_u16(cpu, bus),
        },
        0x94 => match cpu.registers.p.index_size {
            Eight => load::sty_direct_page_x_u8(cpu, bus),
            Sixteen => load::sty_direct_page_x_u16(cpu, bus),
        },
        0x95 => match cpu.registers.p.accumulator_size {
            Eight => load::sta_direct_page_x_u8(cpu, bus),
            Sixteen => load::sta_direct_page_x_u16(cpu, bus),
        },
        0x96 => match cpu.registers.p.index_size {
            Eight => load::stx_direct_page_y_u8(cpu, bus),
            Sixteen => load::stx_direct_page_y_u16(cpu, bus),
        },
        0x97 => match cpu.registers.p.accumulator_size {
            Eight => load::sta_direct_page_indirect_long_indexed_u8(cpu, bus),
            Sixteen => load::sta_direct_page_indirect_long_indexed_u16(cpu, bus),
        },
        0x98 => match cpu.registers.p.accumulator_size {
            Eight => load::tya_u8(cpu, bus),
            Sixteen => load::tya_u16(cpu, bus),
        },
        0x99 => match cpu.registers.p.accumulator_size {
            Eight => load::sta_absolute_y_u8(cpu, bus),
            Sixteen => load::sta_absolute_y_u16(cpu, bus),
        },
        0x9A => load::txs(cpu, bus),
        0x9B => match cpu.registers.p.index_size {
            Eight => load::txy_u8(cpu, bus),
            Sixteen => load::txy_u16(cpu, bus),
        },
        0x9C => match cpu.registers.p.accumulator_size {
            Eight => load::stz_absolute_u8(cpu, bus),
            Sixteen => load::stz_absolute_u16(cpu, bus),
        },
        0x9D => match cpu.registers.p.accumulator_size {
            Eight => load::sta_absolute_x_u8(cpu, bus),
            Sixteen => load::sta_absolute_x_u16(cpu, bus),
        },
        0x9E => match cpu.registers.p.accumulator_size {
            Eight => load::stz_absolute_x_u8(cpu, bus),
            Sixteen => load::stz_absolute_x_u16(cpu, bus),
        },
        0x9F => match cpu.registers.p.accumulator_size {
            Eight => load::sta_absolute_long_x_u8(cpu, bus),
            Sixteen => load::sta_absolute_long_x_u16(cpu, bus),
        },
        0xA0 => match cpu.registers.p.index_size {
            Eight => load::ldy_immediate_u8(cpu, bus),
            Sixteen => load::ldy_immediate_u16(cpu, bus),
        },
        0xA1 => match cpu.registers.p.accumulator_size {
            Eight => load::lda_direct_page_indexed_indirect_u8(cpu, bus),
            Sixteen => load::lda_direct_page_indexed_indirect_u16(cpu, bus),
        },
        0xA2 => match cpu.registers.p.index_size {
            Eight => load::ldx_immediate_u8(cpu, bus),
            Sixteen => load::ldx_immediate_u16(cpu, bus),
        },
        0xA3 => match cpu.registers.p.accumulator_size {
            Eight => load::lda_stack_relative_u8(cpu, bus),
            Sixteen => load::lda_stack_relative_u16(cpu, bus),
        },
        0xA4 => match cpu.registers.p.index_size {
            Eight => load::ldy_direct_page_u8(cpu, bus),
            Sixteen => load::ldy_direct_page_u16(cpu, bus),
        },
        0xA5 => match cpu.registers.p.accumulator_size {
            Eight => load::lda_direct_page_u8(cpu, bus),
            Sixteen => load::lda_direct_page_u16(cpu, bus),
        },
        0xA6 => match cpu.registers.p.index_size {
            Eight => load::ldx_direct_page_u8(cpu, bus),
            Sixteen => load::ldx_direct_page_u16(cpu, bus),
        },
        0xA7 => match cpu.registers.p.accumulator_size {
            Eight => load::lda_direct_page_indirect_long_u8(cpu, bus),
            Sixteen => load::lda_direct_page_indirect_long_u16(cpu, bus),
        },
        0xA8 => match cpu.registers.p.index_size {
            Eight => load::tay_u8(cpu, bus),
            Sixteen => load::tay_u16(cpu, bus),
        },
        0xA9 => match cpu.registers.p.accumulator_size {
            Eight => load::lda_immediate_u8(cpu, bus),
            Sixteen => load::lda_immediate_u16(cpu, bus),
        },
        0xAA => match cpu.registers.p.index_size {
            Eight => load::tax_u8(cpu, bus),
            Sixteen => load::tax_u16(cpu, bus),
        },
        0xAB => load::plb(cpu, bus),
        0xAC => match cpu.registers.p.index_size {
            Eight => load::ldy_absolute_u8(cpu, bus),
            Sixteen => load::ldy_absolute_u16(cpu, bus),
        },
        0xAD => match cpu.registers.p.accumulator_size {
            Eight => load::lda_absolute_u8(cpu, bus),
            Sixteen => load::lda_absolute_u16(cpu, bus),
        },
        0xAE => match cpu.registers.p.index_size {
            Eight => load::ldx_absolute_u8(cpu, bus),
            Sixteen => load::ldx_absolute_u16(cpu, bus),
        },
        0xAF => match cpu.registers.p.accumulator_size {
            Eight => load::lda_absolute_long_u8(cpu, bus),
            Sixteen => load::lda_absolute_long_u16(cpu, bus),
        },
        0xB0 => flow::bcs(cpu, bus),
        0xB1 => match cpu.registers.p.accumulator_size {
            Eight => load::lda_direct_page_indirect_indexed_u8(cpu, bus),
            Sixteen => load::lda_direct_page_indirect_indexed_u16(cpu, bus),
        },
        0xB2 => match cpu.registers.p.accumulator_size {
            Eight => load::lda_direct_page_indirect_u8(cpu, bus),
            Sixteen => load::lda_direct_page_indirect_u16(cpu, bus),
        },
        0xB3 => match cpu.registers.p.accumulator_size {
            Eight => load::lda_stack_relative_indirect_indexed_u8(cpu, bus),
            Sixteen => load::lda_stack_relative_indirect_indexed_u16(cpu, bus),
        },
        0xB4 => match cpu.registers.p.index_size {
            Eight => load::ldy_direct_page_x_u8(cpu, bus),
            Sixteen => load::ldy_direct_page_x_u16(cpu, bus),
        },
        0xB5 => match cpu.registers.p.accumulator_size {
            Eight => load::lda_direct_page_x_u8(cpu, bus),
            Sixteen => load::lda_direct_page_x_u16(cpu, bus),
        },
        0xB6 => match cpu.registers.p.index_size {
            Eight => load::ldx_direct_page_y_u8(cpu, bus),
            Sixteen => load::ldx_direct_page_y_u16(cpu, bus),
        },
        0xB7 => match cpu.registers.p.accumulator_size {
            Eight => load::lda_direct_page_indirect_long_indexed_u8(cpu, bus),
            Sixteen => load::lda_direct_page_indirect_long_indexed_u16(cpu, bus),
        },
        0xB8 => flags::clv(cpu, bus),
        0xB9 => match cpu.registers.p.accumulator_size {
            Eight => load::lda_absolute_y_u8(cpu, bus),
            Sixteen => load::lda_absolute_y_u16(cpu, bus),
        },
        0xBA => match cpu.registers.p.index_size {
            Eight => load::tsx_u8(cpu, bus),
            Sixteen => load::tsx_u16(cpu, bus),
        },
        0xBB => match cpu.registers.p.index_size {
            Eight => load::tyx_u8(cpu, bus),
            Sixteen => load::tyx_u16(cpu, bus),
        },
        0xBC => match cpu.registers.p.index_size {
            Eight => load::ldy_absolute_x_u8(cpu, bus),
            Sixteen => load::ldy_absolute_x_u16(cpu, bus),
        },
        0xBD => match cpu.registers.p.accumulator_size {
            Eight => load::lda_absolute_x_u8(cpu, bus),
            Sixteen => load::lda_absolute_x_u16(cpu, bus),
        },
        0xBE => match cpu.registers.p.index_size {
            Eight => load::ldx_absolute_y_u8(cpu, bus),
            Sixteen => load::ldx_absolute_y_u16(cpu, bus),
        },
        0xBF => match cpu.registers.p.accumulator_size {
            Eight => load::lda_absolute_long_x_u8(cpu, bus),
            Sixteen => load::lda_absolute_long_x_u16(cpu, bus),
        },
        0xC0 => match cpu.registers.p.index_size {
            Eight => alu::cpy_immediate_u8(cpu, bus),
            Sixteen => alu::cpy_immediate_u16(cpu, bus),
        },
        0xC1 => match cpu.registers.p.accumulator_size {
            Eight => alu::cmp_direct_page_indexed_indirect_u8(cpu, bus),
            Sixteen => alu::cmp_direct_page_indexed_indirect_u16(cpu, bus),
        },
        0xC2 => flags::rep(cpu, bus),
        0xC3 => match cpu.registers.p.accumulator_size {
            Eight => alu::cmp_stack_relative_u8(cpu, bus),
            Sixteen => alu::cmp_stack_relative_u16(cpu, bus),
        },
        0xC4 => match cpu.registers.p.index_size {
            Eight => alu::cpy_direct_page_u8(cpu, bus),
            Sixteen => alu::cpy_direct_page_u16(cpu, bus),
        },
        0xC5 => match cpu.registers.p.accumulator_size {
            Eight => alu::cmp_direct_page_u8(cpu, bus),
            Sixteen => alu::cmp_direct_page_u16(cpu, bus),
        },
        0xC6 => match cpu.registers.p.accumulator_size {
            Eight => alu::dec_direct_page_u8(cpu, bus),
            Sixteen => alu::dec_direct_page_u16(cpu, bus),
        },
        0xC7 => match cpu.registers.p.accumulator_size {
            Eight => alu::cmp_direct_page_indirect_long_u8(cpu, bus),
            Sixteen => alu::cmp_direct_page_indirect_long_u16(cpu, bus),
        },
        0xC8 => match cpu.registers.p.index_size {
            Eight => alu::iny_u8(cpu, bus),
            Sixteen => alu::iny_u16(cpu, bus),
        },
        0xC9 => match cpu.registers.p.accumulator_size {
            Eight => alu::cmp_immediate_u8(cpu, bus),
            Sixteen => alu::cmp_immediate_u16(cpu, bus),
        },
        0xCA => match cpu.registers.p.index_size {
            Eight => alu::dex_u8(cpu, bus),
            Sixteen => alu::dex_u16(cpu, bus),
        },
        0xCB => flow::wai(cpu, bus),
        0xCC => match cpu.registers.p.index_size {
            Eight => alu::cpy_absolute_u8(cpu, bus),
            Sixteen => alu::cpy_absolute_u16(cpu, bus),
        },
        0xCD => match cpu.registers.p.accumulator_size {
            Eight => alu::cmp_absolute_u8(cpu, bus),
            Sixteen => alu::cmp_absolute_u16(cpu, bus),
        },
        0xCE => match cpu.registers.p.accumulator_size {
            Eight => alu::dec_absolute_u8(cpu, bus),
            Sixteen => alu::dec_absolute_u16(cpu, bus),
        },
        0xCF => match cpu.registers.p.accumulator_size {
            Eight => alu::cmp_absolute_long_u8(cpu, bus),
            Sixteen => alu::cmp_absolute_long_u16(cpu, bus),
        },
        0xD0 => flow::bne(cpu, bus),
        0xD1 => match cpu.registers.p.accumulator_size {
            Eight => alu::cmp_direct_page_indirect_indexed_u8(cpu, bus),
            Sixteen => alu::cmp_direct_page_indirect_indexed_u16(cpu, bus),
        },
        0xD2 => match cpu.registers.p.accumulator_size {
            Eight => alu::cmp_direct_page_indirect_u8(cpu, bus),
            Sixteen => alu::cmp_direct_page_indirect_u16(cpu, bus),
        },
        0xD3 => match cpu.registers.p.accumulator_size {
            Eight => alu::cmp_stack_relative_indirect_indexed_u8(cpu, bus),
            Sixteen => alu::cmp_stack_relative_indirect_indexed_u16(cpu, bus),
        },
        0xD4 => flow::pei(cpu, bus),
        0xD5 => match cpu.registers.p.accumulator_size {
            Eight => alu::cmp_direct_page_x_u8(cpu, bus),
            Sixteen => alu::cmp_direct_page_x_u16(cpu, bus),
        },
        0xD6 => match cpu.registers.p.accumulator_size {
            Eight => alu::dec_direct_page_x_u8(cpu, bus),
            Sixteen => alu::dec_direct_page_x_u16(cpu, bus),
        },
        0xD7 => match cpu.registers.p.accumulator_size {
            Eight => alu::cmp_direct_page_indirect_long_indexed_u8(cpu, bus),
            Sixteen => alu::cmp_direct_page_indirect_long_indexed_u16(cpu, bus),
        },
        0xD8 => flags::cld(cpu, bus),
        0xD9 => match cpu.registers.p.accumulator_size {
            Eight => alu::cmp_absolute_y_u8(cpu, bus),
            Sixteen => alu::cmp_absolute_y_u16(cpu, bus),
        },
        0xDA => match cpu.registers.p.index_size {
            Eight => load::phx_u8(cpu, bus),
            Sixteen => load::phx_u16(cpu, bus),
        },
        0xDB => flow::stp(cpu, bus),
        0xDC => flow::jmp_indirect_long(cpu, bus),
        0xDD => match cpu.registers.p.accumulator_size {
            Eight => alu::cmp_absolute_x_u8(cpu, bus),
            Sixteen => alu::cmp_absolute_x_u16(cpu, bus),
        },
        0xDE => match cpu.registers.p.accumulator_size {
            Eight => alu::dec_absolute_x_u8(cpu, bus),
            Sixteen => alu::dec_absolute_x_u16(cpu, bus),
        },
        0xDF => match cpu.registers.p.accumulator_size {
            Eight => alu::cmp_absolute_long_x_u8(cpu, bus),
            Sixteen => alu::cmp_absolute_long_x_u16(cpu, bus),
        },
        0xE0 => match cpu.registers.p.index_size {
            Eight => alu::cpx_immediate_u8(cpu, bus),
            Sixteen => alu::cpx_immediate_u16(cpu, bus),
        },
        0xE1 => match cpu.registers.p.accumulator_size {
            Eight => alu::sbc_direct_page_indexed_indirect_u8(cpu, bus),
            Sixteen => alu::sbc_direct_page_indexed_indirect_u16(cpu, bus),
        },
        0xE2 => flags::sep(cpu, bus),
        0xE3 => match cpu.registers.p.accumulator_size {
            Eight => alu::sbc_stack_relative_u8(cpu, bus),
            Sixteen => alu::sbc_stack_relative_u16(cpu, bus),
        },
        0xE4 => match cpu.registers.p.index_size {
            Eight => alu::cpx_direct_page_u8(cpu, bus),
            Sixteen => alu::cpx_direct_page_u16(cpu, bus),
        },
        0xE5 => match cpu.registers.p.accumulator_size {
            Eight => alu::sbc_direct_page_u8(cpu, bus),
            Sixteen => alu::sbc_direct_page_u16(cpu, bus),
        },
        0xE6 => match cpu.registers.p.accumulator_size {
            Eight => alu::inc_direct_page_u8(cpu, bus),
            Sixteen => alu::inc_direct_page_u16(cpu, bus),
        },
        0xE7 => match cpu.registers.p.accumulator_size {
            Eight => alu::sbc_direct_page_indirect_long_u8(cpu, bus),
            Sixteen => alu::sbc_direct_page_indirect_long_u16(cpu, bus),
        },
        0xE8 => match cpu.registers.p.index_size {
            Eight => alu::inx_u8(cpu, bus),
            Sixteen => alu::inx_u16(cpu, bus),
        },
        0xE9 => match cpu.registers.p.accumulator_size {
            Eight => alu::sbc_immediate_u8(cpu, bus),
            Sixteen => alu::sbc_immediate_u16(cpu, bus),
        },
        0xEA => nop(cpu, bus),
        0xEB => load::xba(cpu, bus),
        0xEC => match cpu.registers.p.index_size {
            Eight => alu::cpx_absolute_u8(cpu, bus),
            Sixteen => alu::cpx_absolute_u16(cpu, bus),
        },
        0xED => match cpu.registers.p.accumulator_size {
            Eight => alu::sbc_absolute_u8(cpu, bus),
            Sixteen => alu::sbc_absolute_u16(cpu, bus),
        },
        0xEE => match cpu.registers.p.accumulator_size {
            Eight => alu::inc_absolute_u8(cpu, bus),
            Sixteen => alu::inc_absolute_u16(cpu, bus),
        },
        0xEF => match cpu.registers.p.accumulator_size {
            Eight => alu::sbc_absolute_long_u8(cpu, bus),
            Sixteen => alu::sbc_absolute_long_u16(cpu, bus),
        },
        0xF0 => flow::beq(cpu, bus),
        0xF1 => match cpu.registers.p.accumulator_size {
            Eight => alu::sbc_direct_page_indirect_indexed_u8(cpu, bus),
            Sixteen => alu::sbc_direct_page_indirect_indexed_u16(cpu, bus),
        },
        0xF2 => match cpu.registers.p.accumulator_size {
            Eight => alu::sbc_direct_page_indirect_u8(cpu, bus),
            Sixteen => alu::sbc_direct_page_indirect_u16(cpu, bus),
        },
        0xF3 => match cpu.registers.p.accumulator_size {
            Eight => alu::sbc_stack_relative_indirect_indexed_u8(cpu, bus),
            Sixteen => alu::sbc_stack_relative_indirect_indexed_u16(cpu, bus),
        },
        0xF4 => flow::pea(cpu, bus),
        0xF5 => match cpu.registers.p.accumulator_size {
            Eight => alu::sbc_direct_page_x_u8(cpu, bus),
            Sixteen => alu::sbc_direct_page_x_u16(cpu, bus),
        },
        0xF6 => match cpu.registers.p.accumulator_size {
            Eight => alu::inc_direct_page_x_u8(cpu, bus),
            Sixteen => alu::inc_direct_page_x_u16(cpu, bus),
        },
        0xF7 => match cpu.registers.p.accumulator_size {
            Eight => alu::sbc_direct_page_indirect_long_indexed_u8(cpu, bus),
            Sixteen => alu::sbc_direct_page_indirect_long_indexed_u16(cpu, bus),
        },
        0xF8 => flags::sed(cpu, bus),
        0xF9 => match cpu.registers.p.accumulator_size {
            Eight => alu::sbc_absolute_y_u8(cpu, bus),
            Sixteen => alu::sbc_absolute_y_u16(cpu, bus),
        },
        0xFA => match cpu.registers.p.index_size {
            Eight => load::plx_u8(cpu, bus),
            Sixteen => load::plx_u16(cpu, bus),
        },
        0xFB => flags::xce(cpu, bus),
        0xFC => flow::jsr_indirect_indexed(cpu, bus),
        0xFD => match cpu.registers.p.accumulator_size {
            Eight => alu::sbc_absolute_x_u8(cpu, bus),
            Sixteen => alu::sbc_absolute_x_u16(cpu, bus),
        },
        0xFE => match cpu.registers.p.accumulator_size {
            Eight => alu::inc_absolute_x_u8(cpu, bus),
            Sixteen => alu::inc_absolute_x_u16(cpu, bus),
        },
        0xFF => match cpu.registers.p.accumulator_size {
            Eight => alu::sbc_absolute_long_x_u8(cpu, bus),
            Sixteen => alu::sbc_absolute_long_x_u16(cpu, bus),
        },
    }
}
