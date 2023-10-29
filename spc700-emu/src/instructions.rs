mod alu;
mod bits;
mod flags;
mod flow;
mod load;

use crate::traits::BusInterface;
use crate::Spc700;
use jgenesis_traits::num::{GetBit, SignBit};

fn fetch_operand<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) -> u8 {
    let operand = bus.read(cpu.registers.pc);
    cpu.registers.pc = cpu.registers.pc.wrapping_add(1);
    operand
}

macro_rules! invalid_cycle {
    ($cpu:expr) => {
        panic!("Invalid cycle for opcode {:02X}: {}", $cpu.state.opcode, $cpu.state.cycle)
    };
}

use invalid_cycle;

macro_rules! impl_read_immediate {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            cpu.final_cycle();

            let $value = fetch_operand(cpu, bus);
            let $registers = &mut cpu.registers;
            $body
        }
    };
}

use impl_read_immediate;

macro_rules! impl_read_register_indirect {
    ($index:ident, $name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    bus.idle();
                }
                2 => {
                    cpu.final_cycle();

                    let address = u16::from_le_bytes([cpu.registers.$index, cpu.direct_page_msb()]);
                    let $value = bus.read(address);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_register_indirect;

macro_rules! impl_read_register_indirect_postinc {
    ($index:ident, $name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    bus.idle();
                }
                2 => {
                    let address = u16::from_le_bytes([cpu.registers.$index, cpu.direct_page_msb()]);
                    let $value = bus.read(address);
                    let $registers = &mut cpu.registers;
                    $body
                }
                3 => {
                    cpu.final_cycle();

                    bus.idle();
                    cpu.registers.$index = cpu.registers.$index.wrapping_add(1);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_register_indirect_postinc;

macro_rules! impl_read_direct_page {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    cpu.final_cycle();

                    let address = u16::from_le_bytes([cpu.state.t0, cpu.direct_page_msb()]);
                    let $value = bus.read(address);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_direct_page;

macro_rules! impl_read_direct_page_indexed {
    ($index:ident, $name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    bus.idle();
                }
                3 => {
                    cpu.final_cycle();

                    let address_lsb = cpu.state.t0.wrapping_add(cpu.registers.$index);
                    let address = u16::from_le_bytes([address_lsb, cpu.direct_page_msb()]);
                    let $value = bus.read(address);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_direct_page_indexed;

macro_rules! impl_read_absolute {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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

                    let address = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
                    let $value = bus.read(address);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_absolute;

macro_rules! impl_read_absolute_indexed {
    ($index:ident, $name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    cpu.final_cycle();

                    let address = u16::from_le_bytes([cpu.state.t0, cpu.state.t1])
                        .wrapping_add(cpu.registers.$index.into());
                    let $value = bus.read(address);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_absolute_indexed;

macro_rules! impl_read_indexed_indirect {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
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
                    cpu.final_cycle();

                    let address = u16::from_le_bytes([cpu.state.t1, cpu.state.t2]);
                    let $value = bus.read(address);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_indexed_indirect;

macro_rules! impl_read_indirect_indexed {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    bus.idle();
                }
                3 => {
                    let address = u16::from_le_bytes([cpu.state.t0, cpu.direct_page_msb()]);
                    cpu.state.t1 = bus.read(address);
                }
                4 => {
                    let address_lsb = cpu.state.t0.wrapping_add(1);
                    let address = u16::from_le_bytes([address_lsb, cpu.direct_page_msb()]);
                    cpu.state.t2 = bus.read(address);
                }
                5 => {
                    cpu.final_cycle();

                    let address = u16::from_le_bytes([cpu.state.t1, cpu.state.t2])
                        .wrapping_add(cpu.registers.y.into());
                    let $value = bus.read(address);
                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_read_indirect_indexed;

macro_rules! impl_read_op {
    (@inner immediate, $($rest:tt)*) => {
        impl_read_immediate!($($rest)*);
    };
    (@inner x_indirect, $($rest:tt)*) => {
        impl_read_register_indirect!(x, $($rest)*);
    };
    (@inner x_indirect_postinc, $($rest:tt)*) => {
        impl_read_register_indirect_postinc!(x, $($rest)*);
    };
    (@inner direct_page, $($rest:tt)*) => {
        impl_read_direct_page!($($rest)*);
    };
    (@inner direct_page_x, $($rest:tt)*) => {
        impl_read_direct_page_indexed!(x, $($rest)*);
    };
    (@inner direct_page_y, $($rest:tt)*) => {
        impl_read_direct_page_indexed!(y, $($rest)*);
    };
    (@inner absolute, $($rest:tt)*) => {
        impl_read_absolute!($($rest)*);
    };
    (@inner absolute_x, $($rest:tt)*) => {
        impl_read_absolute_indexed!(x, $($rest)*);
    };
    (@inner absolute_y, $($rest:tt)*) => {
        impl_read_absolute_indexed!(y, $($rest)*);
    };
    (@inner indirect_x, $($rest:tt)*) => {
        impl_read_indexed_indirect!($($rest)*);
    };
    (@inner indirect_y, $($rest:tt)*) => {
        impl_read_indirect_indexed!($($rest)*);
    };
    ([$($am:tt -> $name:ident),* $(,)?] $rest:tt) => {
        $(
            impl_read_op!(@inner $am, $name, $rest);
        )*
    }
}

use impl_read_op;

macro_rules! impl_x_y {
    ($write:expr, $name:ident, (|$registers:ident, $operand_l:ident, $operand_r:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    bus.idle();
                }
                2 => {
                    let address = u16::from_le_bytes([cpu.registers.y, cpu.direct_page_msb()]);
                    cpu.state.t0 = bus.read(address);
                }
                3 => {
                    let address = u16::from_le_bytes([cpu.registers.x, cpu.direct_page_msb()]);
                    cpu.state.t1 = bus.read(address);
                }
                4 => {
                    cpu.final_cycle();

                    let $operand_l = cpu.state.t1;
                    let $operand_r = cpu.state.t0;
                    let $registers = &mut cpu.registers;
                    let value = $body;

                    if $write {
                        let address = u16::from_le_bytes([cpu.registers.x, cpu.direct_page_msb()]);
                        bus.write(address, value);
                    } else {
                        bus.idle();
                    }
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_x_y;

macro_rules! impl_dp_dp {
    ($write:expr, $name:ident, (|$registers:ident, $operand_l:ident, $operand_r:ident| $body:block)) => {
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
                    cpu.state.t2 = fetch_operand(cpu, bus);
                }
                4 => {
                    let address = u16::from_le_bytes([cpu.state.t2, cpu.direct_page_msb()]);
                    cpu.state.t3 = bus.read(address);
                }
                5 => {
                    cpu.final_cycle();

                    let $operand_l = cpu.state.t3;
                    let $operand_r = cpu.state.t1;
                    let $registers = &mut cpu.registers;
                    let value = $body;

                    if $write {
                        let address = u16::from_le_bytes([cpu.state.t2, cpu.direct_page_msb()]);
                        bus.write(address, value);
                    } else {
                        bus.idle();
                    }
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_dp_dp;

macro_rules! impl_dp_imm {
    ($write:expr, $name:ident, (|$registers:ident, $operand_l:ident, $operand_r:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    cpu.state.t1 = fetch_operand(cpu, bus);
                }
                3 => {
                    let address = u16::from_le_bytes([cpu.state.t1, cpu.direct_page_msb()]);
                    cpu.state.t2 = bus.read(address);
                }
                4 => {
                    cpu.final_cycle();

                    let $operand_l = cpu.state.t2;
                    let $operand_r = cpu.state.t0;
                    let $registers = &mut cpu.registers;
                    let value = $body;

                    if $write {
                        let address = u16::from_le_bytes([cpu.state.t1, cpu.direct_page_msb()]);
                        bus.write(address, value);
                    } else {
                        bus.idle();
                    }
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_dp_imm;

macro_rules! impl_memory_to_memory_op {
    (@inner x_y, $($rest:tt)*) => {
        impl_x_y!(true, $($rest)*);
    };
    (@inner x_y_no_write, $($rest:tt)*) => {
        impl_x_y!(false, $($rest)*);
    };
    (@inner dp_dp, $($rest:tt)*) => {
        impl_dp_dp!(true, $($rest)*);
    };
    (@inner dp_dp_no_write, $($rest:tt)*) => {
        impl_dp_dp!(false, $($rest)*);
    };
    (@inner dp_imm, $($rest:tt)*) => {
        impl_dp_imm!(true, $($rest)*);
    };
    (@inner dp_imm_no_write, $($rest:tt)*) => {
        impl_dp_imm!(false, $($rest)*);
    };
    ([$($am:tt -> $name:ident),* $(,)?] $rest:tt) => {
        $(
            impl_memory_to_memory_op!(@inner $am, $name, $rest);
        )*
    }
}

use impl_memory_to_memory_op;

macro_rules! impl_modify_accumulator {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            cpu.final_cycle();
            bus.idle();

            let $value = cpu.registers.a;
            let $registers = &mut cpu.registers;
            cpu.registers.a = $body;
        }
    };
}

use impl_modify_accumulator;

macro_rules! impl_modify_direct_page {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
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
                    cpu.final_cycle();

                    let $value = cpu.state.t1;
                    let $registers = &mut cpu.registers;
                    let value = $body;

                    let address = u16::from_le_bytes([cpu.state.t0, cpu.direct_page_msb()]);
                    bus.write(address, value);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_modify_direct_page;

macro_rules! impl_modify_direct_page_indexed {
    ($index:ident, $name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    bus.idle();
                }
                3 => {
                    let address_lsb = cpu.state.t0.wrapping_add(cpu.registers.$index);
                    let address = u16::from_le_bytes([address_lsb, cpu.direct_page_msb()]);
                    cpu.state.t1 = bus.read(address);
                }
                4 => {
                    cpu.final_cycle();

                    let $value = cpu.state.t1;
                    let $registers = &mut cpu.registers;
                    let value = $body;

                    let address_lsb = cpu.state.t0.wrapping_add(cpu.registers.$index);
                    let address = u16::from_le_bytes([address_lsb, cpu.direct_page_msb()]);
                    bus.write(address, value);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_modify_direct_page_indexed;

macro_rules! impl_modify_absolute {
    ($name:ident, (|$registers:ident, $value:ident| $body:block)) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    cpu.state.t1 = fetch_operand(cpu, bus);
                }
                3 => {
                    let address = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
                    cpu.state.t2 = bus.read(address);
                }
                4 => {
                    cpu.final_cycle();

                    let $value = cpu.state.t2;
                    let $registers = &mut cpu.registers;
                    let value = $body;

                    let address = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
                    bus.write(address, value);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_modify_absolute;

macro_rules! impl_modify_op {
    (@inner accumulator, $($rest:tt)*) => {
        impl_modify_accumulator!($($rest)*);
    };
    (@inner direct_page, $($rest:tt)*) => {
        impl_modify_direct_page!($($rest)*);
    };
    (@inner direct_page_x, $($rest:tt)*) => {
        impl_modify_direct_page_indexed!(x, $($rest)*);
    };
    (@inner absolute, $($rest:tt)*) => {
        impl_modify_absolute!($($rest)*);
    };
    ([$($am:tt -> $name:ident),* $(,)?] $rest:tt) => {
        $(
            impl_modify_op!(@inner $am, $name, $rest);
        )*
    }
}

use impl_modify_op;

macro_rules! impl_registers_op {
    ($name:ident, |$registers:ident| $body:block) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            cpu.final_cycle();
            bus.idle();

            let $registers = &mut cpu.registers;
            $body
        }
    };
}

use impl_registers_op;

macro_rules! impl_long_registers_op {
    ($name:ident, |$registers:ident| $body:block) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    bus.idle();
                }
                2 => {
                    cpu.final_cycle();
                    bus.idle();

                    let $registers = &mut cpu.registers;
                    $body
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

use impl_long_registers_op;

impl_registers_op!(nop, |_registers| {});

pub fn execute<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    if cpu.state.stopped {
        bus.idle();
        return;
    }

    if cpu.state.cycle != 0 {
        execute_cycle(cpu, bus);

        if cpu.state.cycle != 0 {
            cpu.state.cycle += 1;
        }

        return;
    }

    // Cycle 0: Fetch operand
    cpu.state.opcode = fetch_operand(cpu, bus);
    cpu.state.cycle = 1;
}

macro_rules! opcode_table {
    (
        $cpu:expr,
        $bus:expr,
        { $($opcode:literal => $($module:ident)?::$op_fn:ident $(($arg:expr))?),* $(,)? }
    ) => {
        match $cpu.state.opcode {
            $(
                $opcode => $($module::)?$op_fn($cpu, $bus $(, $arg)?),
            )*
        }
    }
}

fn execute_cycle<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    opcode_table!(cpu, bus, {
        0x00 => ::nop,
        0x01 => flow::tcall(0),
        0x02 => bits::set1(0),
        0x03 => flow::bbs(0),
        0x04 => alu::or_dp,
        0x05 => alu::or_abs,
        0x06 => alu::or_xind,
        0x07 => alu::or_indx,
        0x08 => alu::or_imm,
        0x09 => alu::or_dp_dp,
        0x0A => bits::or1,
        0x0B => bits::asl_dp,
        0x0C => bits::asl_abs,
        0x0D => load::push_psw,
        0x0E => bits::tset1,
        0x0F => flow::brk,
        0x10 => flow::bpl,
        0x11 => flow::tcall(1),
        0x12 => bits::clr1(0),
        0x13 => flow::bbc(0),
        0x14 => alu::or_dpx,
        0x15 => alu::or_absx,
        0x16 => alu::or_absy,
        0x17 => alu::or_indy,
        0x18 => alu::or_dp_imm,
        0x19 => alu::or_x_y_ind,
        0x1A => alu::decw,
        0x1B => bits::asl_dpx,
        0x1C => bits::asl_a,
        0x1D => alu::dec_x,
        0x1E => alu::cmp_x_abs,
        0x1F => flow::jmp_absx_ind,
        0x20 => flags::clrp,
        0x21 => flow::tcall(2),
        0x22 => bits::set1(1),
        0x23 => flow::bbs(1),
        0x24 => alu::and_dp,
        0x25 => alu::and_abs,
        0x26 => alu::and_xind,
        0x27 => alu::and_indx,
        0x28 => alu::and_imm,
        0x29 => alu::and_dp_dp,
        0x2A => bits::or1_cpl,
        0x2B => bits::rol_dp,
        0x2C => bits::rol_abs,
        0x2D => load::push_a,
        0x2E => flow::cbne_dp,
        0x2F => flow::bra,
        0x30 => flow::bmi,
        0x31 => flow::tcall(3),
        0x32 => bits::clr1(1),
        0x33 => flow::bbc(1),
        0x34 => alu::and_dpx,
        0x35 => alu::and_absx,
        0x36 => alu::and_absy,
        0x37 => alu::and_indy,
        0x38 => alu::and_dp_imm,
        0x39 => alu::and_x_y_ind,
        0x3A => alu::incw,
        0x3B => bits::rol_dpx,
        0x3C => bits::rol_a,
        0x3D => alu::inc_x,
        0x3E => alu::cmp_x_dp,
        0x3F => flow::call,
        0x40 => flags::setp,
        0x41 => flow::tcall(4),
        0x42 => bits::set1(2),
        0x43 => flow::bbs(2),
        0x44 => alu::eor_dp,
        0x45 => alu::eor_abs,
        0x46 => alu::eor_xind,
        0x47 => alu::eor_indx,
        0x48 => alu::eor_imm,
        0x49 => alu::eor_dp_dp,
        0x4A => bits::and1,
        0x4B => bits::lsr_dp,
        0x4C => bits::lsr_abs,
        0x4D => load::push_x,
        0x4E => bits::tclr1,
        0x4F => flow::pcall,
        0x50 => flow::bvc,
        0x51 => flow::tcall(5),
        0x52 => bits::clr1(2),
        0x53 => flow::bbc(2),
        0x54 => alu::eor_dpx,
        0x55 => alu::eor_absx,
        0x56 => alu::eor_absy,
        0x57 => alu::eor_indy,
        0x58 => alu::eor_dp_imm,
        0x59 => alu::eor_x_y_ind,
        0x5A => alu::cmpw,
        0x5B => bits::lsr_dpx,
        0x5C => bits::lsr_a,
        0x5D => load::mov_x_a,
        0x5E => alu::cmp_y_abs,
        0x5F => flow::jmp_abs,
        0x60 => flags::clrc,
        0x61 => flow::tcall(6),
        0x62 => bits::set1(3),
        0x63 => flow::bbs(3),
        0x64 => alu::cmp_a_dp,
        0x65 => alu::cmp_a_abs,
        0x66 => alu::cmp_a_xind,
        0x67 => alu::cmp_a_indx,
        0x68 => alu::cmp_a_imm,
        0x69 => alu::cmp_dp_dp,
        0x6A => bits::and1_cpl,
        0x6B => bits::ror_dp,
        0x6C => bits::ror_abs,
        0x6D => load::push_y,
        0x6E => flow::dbnz_dp,
        0x6F => flow::ret,
        0x70 => flow::bvs,
        0x71 => flow::tcall(7),
        0x72 => bits::clr1(3),
        0x73 => flow::bbc(3),
        0x74 => alu::cmp_a_dpx,
        0x75 => alu::cmp_a_absx,
        0x76 => alu::cmp_a_absy,
        0x77 => alu::cmp_a_indy,
        0x78 => alu::cmp_dp_imm,
        0x79 => alu::cmp_x_y_ind,
        0x7A => alu::addw,
        0x7B => bits::ror_dpx,
        0x7C => bits::ror_a,
        0x7D => load::mov_a_x,
        0x7E => alu::cmp_y_dp,
        0x7F => flow::reti,
        0x80 => flags::setc,
        0x81 => flow::tcall(8),
        0x82 => bits::set1(4),
        0x83 => flow::bbs(4),
        0x84 => alu::adc_dp,
        0x85 => alu::adc_abs,
        0x86 => alu::adc_xind,
        0x87 => alu::adc_indx,
        0x88 => alu::adc_imm,
        0x89 => alu::adc_dp_dp,
        0x8A => bits::eor1,
        0x8B => alu::dec_dp,
        0x8C => alu::dec_abs,
        0x8D => load::mov_y_imm,
        0x8E => load::pop_psw,
        0x8F => load::mov_dp_imm,
        0x90 => flow::bcc,
        0x91 => flow::tcall(9),
        0x92 => bits::clr1(4),
        0x93 => flow::bbc(4),
        0x94 => alu::adc_dpx,
        0x95 => alu::adc_absx,
        0x96 => alu::adc_absy,
        0x97 => alu::adc_indy,
        0x98 => alu::adc_dp_imm,
        0x99 => alu::adc_x_y_ind,
        0x9A => alu::subw,
        0x9B => alu::dec_dpx,
        0x9C => alu::dec_a,
        0x9D => load::mov_x_sp,
        0x9E => alu::div,
        0x9F => bits::xcn,
        0xA0 => flags::ei,
        0xA1 => flow::tcall(10),
        0xA2 => bits::set1(5),
        0xA3 => flow::bbs(5),
        0xA4 => alu::sbc_dp,
        0xA5 => alu::sbc_abs,
        0xA6 => alu::sbc_xind,
        0xA7 => alu::sbc_indx,
        0xA8 => alu::sbc_imm,
        0xA9 => alu::sbc_dp_dp,
        0xAA => bits::mov1_c_bit,
        0xAB => alu::inc_dp,
        0xAC => alu::inc_abs,
        0xAD => alu::cmp_y_imm,
        0xAE => load::pop_a,
        0xAF => load::mov_xind_postinc_a,
        0xB0 => flow::bcs,
        0xB1 => flow::tcall(11),
        0xB2 => bits::clr1(5),
        0xB3 => flow::bbc(5),
        0xB4 => alu::sbc_dpx,
        0xB5 => alu::sbc_absx,
        0xB6 => alu::sbc_absy,
        0xB7 => alu::sbc_indy,
        0xB8 => alu::sbc_dp_imm,
        0xB9 => alu::sbc_x_y_ind,
        0xBA => load::movw_ya_dp,
        0xBB => alu::inc_dpx,
        0xBC => alu::inc_a,
        0xBD => load::mov_sp_x,
        0xBE => alu::das,
        0xBF => load::mov_a_xind_postinc,
        0xC0 => flags::di,
        0xC1 => flow::tcall(12),
        0xC2 => bits::set1(6),
        0xC3 => flow::bbs(6),
        0xC4 => load::mov_dp_a,
        0xC5 => load::mov_abs_a,
        0xC6 => load::mov_xind_a,
        0xC7 => load::mov_indx_a,
        0xC8 => alu::cmp_x_imm,
        0xC9 => load::mov_abs_x,
        0xCA => bits::mov1_bit_c,
        0xCB => load::mov_dp_y,
        0xCC => load::mov_abs_y,
        0xCD => load::mov_x_imm,
        0xCE => load::pop_x,
        0xCF => alu::mul,
        0xD0 => flow::bne,
        0xD1 => flow::tcall(13),
        0xD2 => bits::clr1(6),
        0xD3 => flow::bbc(6),
        0xD4 => load::mov_dpx_a,
        0xD5 => load::mov_absx_a,
        0xD6 => load::mov_absy_a,
        0xD7 => load::mov_indy_a,
        0xD8 => load::mov_dp_x,
        0xD9 => load::mov_dpy_x,
        0xDA => load::movw_dp_ya,
        0xDB => load::mov_dpx_y,
        0xDC => alu::dec_y,
        0xDD => load::mov_a_y,
        0xDE => flow::cbne_dpx,
        0xDF => alu::daa,
        0xE0 => flags::clrv,
        0xE1 => flow::tcall(14),
        0xE2 => bits::set1(7),
        0xE3 => flow::bbs(7),
        0xE4 => load::mov_a_dp,
        0xE5 => load::mov_a_abs,
        0xE6 => load::mov_a_xind,
        0xE7 => load::mov_a_indx,
        0xE8 => load::mov_a_imm,
        0xE9 => load::mov_x_abs,
        0xEA => bits::not1,
        0xEB => load::mov_y_dp,
        0xEC => load::mov_y_abs,
        0xED => flags::notc,
        0xEE => load::pop_y,
        0xEF => flow::stop,
        0xF0 => flow::beq,
        0xF1 => flow::tcall(15),
        0xF2 => bits::clr1(7),
        0xF3 => flow::bbc(7),
        0xF4 => load::mov_a_dpx,
        0xF5 => load::mov_a_absx,
        0xF6 => load::mov_a_absy,
        0xF7 => load::mov_a_indy,
        0xF8 => load::mov_x_dp,
        0xF9 => load::mov_x_dpy,
        0xFA => load::mov_dp_dp,
        0xFB => load::mov_y_dpx,
        0xFC => alu::inc_y,
        0xFD => load::mov_y_a,
        0xFE => flow::dbnz_y,
        0xFF => flow::stop,
    });
}
