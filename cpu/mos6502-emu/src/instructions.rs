use crate::bus::BusInterface;
use crate::{CpuRegisters, Mos6502, StatusFlags, StatusReadContext, IRQ_VECTOR, NMI_VECTOR};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Encode, Decode)]
pub struct InstructionState {
    pub opcode: u8,
    pub executing_interrupt: bool,
    pub cycle: u8,
    pub operand_first_byte: u8,
    pub operand_second_byte: u8,
    pub target_first_byte: u8,
    pub target_second_byte: u8,
    pub indirect_byte: u8,
    pub interrupt_vector: u16,
    pub pending_interrupt: bool,
    pub instruction_complete: bool,
}

impl Default for InstructionState {
    fn default() -> Self {
        Self {
            opcode: 0,
            executing_interrupt: false,
            cycle: 0,
            operand_first_byte: 0,
            operand_second_byte: 0,
            target_first_byte: 0,
            target_second_byte: 0,
            indirect_byte: 0,
            interrupt_vector: 0,
            pending_interrupt: false,
            instruction_complete: true,
        }
    }
}

#[inline]
fn poll_interrupt_lines<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
    cpu.state.pending_interrupt |=
        bus.nmi() || (!cpu.registers.status.interrupt_disable && bus.irq());
}

#[inline]
fn final_cycle<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
    poll_interrupt_lines(cpu, bus);
    cpu.state.instruction_complete = true;
}

#[inline]
fn fetch_operand<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) -> u8 {
    let operand = bus.read(cpu.registers.pc);
    cpu.registers.pc = cpu.registers.pc.wrapping_add(1);
    operand
}

macro_rules! invalid_cycle {
    ($cpu:expr) => {
        panic!("Invalid cycle: {}", $cpu.state.cycle)
    };
}

macro_rules! impl_read_immediate {
    ($cpu:expr, $bus:expr, |$operand:ident, $registers_param:ident| $body:block) => {{
        final_cycle($cpu, $bus);

        let $operand = fetch_operand($cpu, $bus);
        let $registers_param = &mut $cpu.registers;
        $body
    }};
}

macro_rules! impl_read_zero_page {
    ($cpu:expr, $bus:expr, |$operand:ident, $registers_param:ident| $body:block) => {
        match $cpu.state.cycle {
            0 => {
                $cpu.state.operand_first_byte = fetch_operand($cpu, $bus);
            }
            1 => {
                final_cycle($cpu, $bus);

                let $operand = $bus.read($cpu.state.operand_first_byte.into());
                let $registers_param = &mut $cpu.registers;
                $body
            }
            _ => invalid_cycle!($cpu),
        }
    };
}

macro_rules! impl_read_zero_page_indexed {
    (index: $index:ident, $cpu:expr, $bus:expr, |$operand:ident, $registers_param:ident| $body:block) => {
        match $cpu.state.cycle {
            0 => {
                $cpu.state.operand_first_byte = fetch_operand($cpu, $bus);
            }
            1 => {
                $bus.read($cpu.state.operand_first_byte.into());
            }
            2 => {
                final_cycle($cpu, $bus);

                let index = $cpu.registers.$index;
                let address = $cpu.state.operand_first_byte.wrapping_add(index);
                let $operand = $bus.read(address.into());
                let $registers_param = &mut $cpu.registers;
                $body
            }
            _ => invalid_cycle!($cpu),
        }
    };
}

macro_rules! impl_read_absolute {
    ($cpu:expr, $bus:expr, |$operand:ident, $registers_param:ident| $body:block) => {
        match $cpu.state.cycle {
            0 => {
                $cpu.state.operand_first_byte = fetch_operand($cpu, $bus);
            }
            1 => {
                $cpu.state.operand_second_byte = fetch_operand($cpu, $bus);
            }
            2 => {
                final_cycle($cpu, $bus);

                let address = u16::from_le_bytes([
                    $cpu.state.operand_first_byte,
                    $cpu.state.operand_second_byte,
                ]);
                let $operand = $bus.read(address);
                let $registers_param = &mut $cpu.registers;
                $body
            }
            _ => invalid_cycle!($cpu),
        }
    };
}

macro_rules! impl_read_absolute_indexed {
    (index: $index:ident, $cpu:expr, $bus:expr, |$operand:ident, $registers_param:ident| $body:block) => {
        match $cpu.state.cycle {
            0 => {
                $cpu.state.operand_first_byte = fetch_operand($cpu, $bus);
            }
            1 => {
                $cpu.state.operand_second_byte = fetch_operand($cpu, $bus);
            }
            2 => {
                poll_interrupt_lines($cpu, $bus);

                let (address_lsb, overflowed) =
                    $cpu.state.operand_first_byte.overflowing_add($cpu.registers.$index);
                let address = u16::from_le_bytes([address_lsb, $cpu.state.operand_second_byte]);
                let $operand = $bus.read(address);

                if !overflowed {
                    let $registers_param = &mut $cpu.registers;
                    $body
                    $cpu.state.instruction_complete = true;
                }
            }
            3 => {
                final_cycle($cpu, $bus);

                let address = u16::from_le_bytes([$cpu.state.operand_first_byte, $cpu.state.operand_second_byte])
                    .wrapping_add($cpu.registers.$index.into());
                let $operand = $bus.read(address);
                let $registers_param = &mut $cpu.registers;
                $body
            }
            _ => invalid_cycle!($cpu)
        }
    }
}

macro_rules! impl_read_indirect_x {
    ($cpu:expr, $bus:expr, |$operand:ident, $registers_param:ident| $body:block) => {
        match $cpu.state.cycle {
            0 => {
                $cpu.state.operand_first_byte = fetch_operand($cpu, $bus);
            }
            1 => {
                $bus.read($cpu.state.operand_first_byte.into());
            }
            2 => {
                let address = $cpu.state.operand_first_byte.wrapping_add($cpu.registers.x);
                $cpu.state.target_first_byte = $bus.read(address.into());
            }
            3 => {
                let address =
                    $cpu.state.operand_first_byte.wrapping_add($cpu.registers.x).wrapping_add(1);
                $cpu.state.target_second_byte = $bus.read(address.into());
            }
            4 => {
                final_cycle($cpu, $bus);

                let address = u16::from_le_bytes([
                    $cpu.state.target_first_byte,
                    $cpu.state.target_second_byte,
                ]);
                let $operand = $bus.read(address);
                let $registers_param = &mut $cpu.registers;
                $body
            }
            _ => invalid_cycle!($cpu),
        }
    };
}

macro_rules! impl_read_indirect_y {
    ($cpu:expr, $bus:expr, |$operand:ident, $registers_param:ident| $body:block) => {
        match $cpu.state.cycle {
            0 => {
                $cpu.state.operand_first_byte = fetch_operand($cpu, $bus);
            }
            1 => {
                $cpu.state.target_first_byte = $bus.read($cpu.state.operand_first_byte.into());
            }
            2 => {
                $cpu.state.target_second_byte =
                    $bus.read($cpu.state.operand_first_byte.wrapping_add(1).into());
            }
            3 => {
                poll_interrupt_lines($cpu, $bus);

                let (address_lsb, overflowed) = $cpu.state.target_first_byte.overflowing_add($cpu.registers.y);
                let address = u16::from_le_bytes([address_lsb, $cpu.state.target_second_byte]);
                let $operand = $bus.read(address);

                if !overflowed {
                    let $registers_param = &mut $cpu.registers;
                    $body
                    $cpu.state.instruction_complete = true;
                }
            }
            4 => {
                final_cycle($cpu, $bus);

                let address = u16::from_le_bytes([$cpu.state.target_first_byte, $cpu.state.target_second_byte])
                    .wrapping_add($cpu.registers.y.into());
                let $operand = $bus.read(address);
                let $registers_param = &mut $cpu.registers;
                $body
            }
            _ => invalid_cycle!($cpu)
        }
    }
}

macro_rules! impl_read_instruction {
    (immediate, $($rest:tt)*) => {
        impl_read_immediate!($($rest)*)
    };
    (zero_page, $($rest:tt)*) => {
        impl_read_zero_page!($($rest)*)
    };
    (zero_page_x, $($rest:tt)*) => {
        impl_read_zero_page_indexed!(index: x, $($rest)*)
    };
    (zero_page_y, $($rest:tt)*) => {
        impl_read_zero_page_indexed!(index: y, $($rest)*)
    };
    (absolute, $($rest:tt)*) => {
        impl_read_absolute!($($rest)*)
    };
    (absolute_x, $($rest:tt)*) => {
        impl_read_absolute_indexed!(index: x, $($rest)*)
    };
    (absolute_y, $($rest:tt)*) => {
        impl_read_absolute_indexed!(index: y, $($rest)*)
    };
    (indirect_x, $($rest:tt)*) => {
        impl_read_indirect_x!($($rest)*)
    };
    (indirect_y, $($rest:tt)*) => {
        impl_read_indirect_y!($($rest)*)
    };
}

macro_rules! impl_read_fn {
    ($name:ident, $addressing_mode:tt, |$operand:ident, $registers:ident| $body:block) => {
        fn $name<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
            impl_read_instruction!($addressing_mode, cpu, bus, |$operand, $registers| $body);
        }
    };
}

macro_rules! impl_store_zero_page {
    ($cpu:expr, $bus:expr, $register:expr) => {
        match $cpu.state.cycle {
            0 => {
                $cpu.state.operand_first_byte = fetch_operand($cpu, $bus);
            }
            1 => {
                final_cycle($cpu, $bus);

                let address = $cpu.state.operand_first_byte.into();
                $bus.write(address, $register);
            }
            _ => invalid_cycle!($cpu),
        }
    };
}

macro_rules! impl_store_zero_page_indexed {
    (index: $index:ident, $cpu:expr, $bus:expr, $register:expr) => {
        match $cpu.state.cycle {
            0 => {
                $cpu.state.operand_first_byte = fetch_operand($cpu, $bus);
            }
            1 => {
                $bus.read($cpu.state.operand_first_byte.into());
            }
            2 => {
                final_cycle($cpu, $bus);

                let address =
                    $cpu.state.operand_first_byte.wrapping_add($cpu.registers.$index).into();
                $bus.write(address, $register);
            }
            _ => invalid_cycle!($cpu),
        }
    };
}

macro_rules! impl_store_absolute {
    ($cpu:expr, $bus:expr, $register:expr) => {
        match $cpu.state.cycle {
            0 => {
                $cpu.state.operand_first_byte = fetch_operand($cpu, $bus);
            }
            1 => {
                $cpu.state.operand_second_byte = fetch_operand($cpu, $bus);
            }
            2 => {
                final_cycle($cpu, $bus);

                let address = u16::from_le_bytes([
                    $cpu.state.operand_first_byte,
                    $cpu.state.operand_second_byte,
                ]);
                $bus.write(address, $register);
            }
            _ => invalid_cycle!($cpu),
        }
    };
}

macro_rules! impl_store_absolute_indexed {
    (index: $index:ident, $cpu:expr, $bus:expr, $register:expr) => {
        match $cpu.state.cycle {
            0 => {
                $cpu.state.operand_first_byte = fetch_operand($cpu, $bus);
            }
            1 => {
                $cpu.state.operand_second_byte = fetch_operand($cpu, $bus);
            }
            2 => {
                let address_lsb = $cpu.state.operand_first_byte.wrapping_add($cpu.registers.$index);
                let address = u16::from_le_bytes([address_lsb, $cpu.state.operand_second_byte]);
                $bus.read(address);
            }
            3 => {
                final_cycle($cpu, $bus);

                let address = u16::from_le_bytes([
                    $cpu.state.operand_first_byte,
                    $cpu.state.operand_second_byte,
                ])
                .wrapping_add($cpu.registers.$index.into());
                $bus.write(address, $register);
            }
            _ => invalid_cycle!($cpu),
        }
    };
}

macro_rules! impl_store_indirect_x {
    ($cpu:expr, $bus:expr, $register:expr) => {
        match $cpu.state.cycle {
            0 => {
                $cpu.state.operand_first_byte = fetch_operand($cpu, $bus);
            }
            1 => {
                $bus.read($cpu.state.operand_first_byte.into());
            }
            2 => {
                let address = $cpu.state.operand_first_byte.wrapping_add($cpu.registers.x);
                $cpu.state.target_first_byte = $bus.read(address.into());
            }
            3 => {
                let address =
                    $cpu.state.operand_first_byte.wrapping_add($cpu.registers.x).wrapping_add(1);
                $cpu.state.target_second_byte = $bus.read(address.into());
            }
            4 => {
                final_cycle($cpu, $bus);

                let address = u16::from_le_bytes([
                    $cpu.state.target_first_byte,
                    $cpu.state.target_second_byte,
                ]);
                $bus.write(address, $register);
            }
            _ => invalid_cycle!($cpu),
        }
    };
}

macro_rules! impl_store_indirect_y {
    ($cpu:expr, $bus:expr, $register:expr) => {
        match $cpu.state.cycle {
            0 => {
                $cpu.state.operand_first_byte = fetch_operand($cpu, $bus);
            }
            1 => {
                $cpu.state.target_first_byte = $bus.read($cpu.state.operand_first_byte.into());
            }
            2 => {
                let address = $cpu.state.operand_first_byte.wrapping_add(1);
                $cpu.state.target_second_byte = $bus.read(address.into());
            }
            3 => {
                let address_lsb = $cpu.state.target_first_byte.wrapping_add($cpu.registers.y);
                let address = u16::from_le_bytes([address_lsb, $cpu.state.target_second_byte]);
                $bus.read(address);
            }
            4 => {
                final_cycle($cpu, $bus);

                let address = u16::from_le_bytes([
                    $cpu.state.target_first_byte,
                    $cpu.state.target_second_byte,
                ])
                .wrapping_add($cpu.registers.y.into());
                $bus.write(address, $register);
            }
            _ => invalid_cycle!($cpu),
        }
    };
}

macro_rules! impl_store {
    (zero_page, $($rest:tt)*) => {
        impl_store_zero_page!($($rest)*)
    };
    (zero_page_x, $($rest:tt)*) => {
        impl_store_zero_page_indexed!(index: x, $($rest)*)
    };
    (zero_page_y, $($rest:tt)*) => {
        impl_store_zero_page_indexed!(index: y, $($rest)*)
    };
    (absolute, $($rest:tt)*) => {
        impl_store_absolute!($($rest)*)
    };
    (absolute_x, $($rest:tt)*) => {
        impl_store_absolute_indexed!(index: x, $($rest)*)
    };
    (absolute_y, $($rest:tt)*) => {
        impl_store_absolute_indexed!(index: y, $($rest)*)
    };
    (indirect_x, $($rest:tt)*) => {
        impl_store_indirect_x!($($rest)*)
    };
    (indirect_y, $($rest:tt)*) => {
        impl_store_indirect_y!($($rest)*)
    };
}

// STA, STX, STY, unofficial SAX
macro_rules! impl_store_fn {
    ($name:ident, ax, $addressing_mode:tt) => {
        fn $name<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
            impl_store!($addressing_mode, cpu, bus, cpu.registers.accumulator & cpu.registers.x);
        }
    };
    ($name:ident, $register:ident, $addressing_mode:tt) => {
        fn $name<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
            impl_store!($addressing_mode, cpu, bus, cpu.registers.$register);
        }
    };
}

impl_store_fn!(sta_zero_page, accumulator, zero_page);
impl_store_fn!(sta_zero_page_x, accumulator, zero_page_x);
impl_store_fn!(sta_absolute, accumulator, absolute);
impl_store_fn!(sta_absolute_x, accumulator, absolute_x);
impl_store_fn!(sta_absolute_y, accumulator, absolute_y);
impl_store_fn!(sta_indirect_x, accumulator, indirect_x);
impl_store_fn!(sta_indirect_y, accumulator, indirect_y);

impl_store_fn!(stx_zero_page, x, zero_page);
impl_store_fn!(stx_zero_page_y, x, zero_page_y);
impl_store_fn!(stx_absolute, x, absolute);

impl_store_fn!(sty_zero_page, y, zero_page);
impl_store_fn!(sty_zero_page_x, y, zero_page_x);
impl_store_fn!(sty_absolute, y, absolute);

impl_store_fn!(sax_zero_page, ax, zero_page);
impl_store_fn!(sax_zero_page_y, ax, zero_page_y);
impl_store_fn!(sax_absolute, ax, absolute);
impl_store_fn!(sax_indirect_x, ax, indirect_x);

macro_rules! impl_modify_accumulator {
    ($cpu:expr, $bus:expr, |$operand:ident, $registers_param:ident| $body:block) => {{
        final_cycle($cpu, $bus);

        $bus.read($cpu.registers.pc);

        let $operand = $cpu.registers.accumulator;
        let $registers_param = &mut $cpu.registers;
        $registers_param.accumulator = $body;
    }};
}

macro_rules! impl_modify_zero_page {
    ($cpu:expr, $bus:expr, |$operand:ident, $registers_param:ident| $body:block) => {
        match $cpu.state.cycle {
            0 => {
                $cpu.state.operand_first_byte = fetch_operand($cpu, $bus);
            }
            1 => {
                $cpu.state.target_first_byte = $bus.read($cpu.state.operand_first_byte.into());
            }
            2 => {
                $bus.write($cpu.state.operand_first_byte.into(), $cpu.state.target_first_byte);
            }
            3 => {
                final_cycle($cpu, $bus);

                let $operand = $cpu.state.target_first_byte;
                let $registers_param = &mut $cpu.registers;
                let value = $body;
                $bus.write($cpu.state.operand_first_byte.into(), value);
            }
            _ => invalid_cycle!($cpu),
        }
    };
}

macro_rules! impl_modify_zero_page_x {
    ($cpu:expr, $bus:expr, |$operand:ident, $registers_param:ident| $body:block) => {
        match $cpu.state.cycle {
            0 => {
                $cpu.state.operand_first_byte = fetch_operand($cpu, $bus);
            }
            1 => {
                $bus.read($cpu.state.operand_first_byte.into());
            }
            2 => {
                let address = $cpu.state.operand_first_byte.wrapping_add($cpu.registers.x).into();
                $cpu.state.target_first_byte = $bus.read(address);
            }
            3 => {
                let address = $cpu.state.operand_first_byte.wrapping_add($cpu.registers.x).into();
                $bus.write(address, $cpu.state.target_first_byte);
            }
            4 => {
                final_cycle($cpu, $bus);

                let address = $cpu.state.operand_first_byte.wrapping_add($cpu.registers.x).into();

                let $operand = $cpu.state.target_first_byte;
                let $registers_param = &mut $cpu.registers;
                let value = $body;

                $bus.write(address, value);
            }
            _ => invalid_cycle!($cpu),
        }
    };
}

macro_rules! impl_modify_absolute {
    ($cpu:expr, $bus:expr, |$operand:ident, $registers_param:ident| $body:block) => {
        match $cpu.state.cycle {
            0 => {
                $cpu.state.operand_first_byte = fetch_operand($cpu, $bus);
            }
            1 => {
                $cpu.state.operand_second_byte = fetch_operand($cpu, $bus);
            }
            2 => {
                let address = u16::from_le_bytes([
                    $cpu.state.operand_first_byte,
                    $cpu.state.operand_second_byte,
                ]);
                $cpu.state.target_first_byte = $bus.read(address);
            }
            3 => {
                let address = u16::from_le_bytes([
                    $cpu.state.operand_first_byte,
                    $cpu.state.operand_second_byte,
                ]);
                $bus.write(address, $cpu.state.target_first_byte);
            }
            4 => {
                final_cycle($cpu, $bus);

                let $operand = $cpu.state.target_first_byte;
                let $registers_param = &mut $cpu.registers;
                let value = $body;

                let address = u16::from_le_bytes([
                    $cpu.state.operand_first_byte,
                    $cpu.state.operand_second_byte,
                ]);
                $bus.write(address, value);
            }
            _ => invalid_cycle!($cpu),
        }
    };
}

macro_rules! impl_modify_absolute_indexed {
    (index: $index:ident, $cpu:expr, $bus:expr, |$operand:ident, $registers_param:ident| $body:block) => {
        match $cpu.state.cycle {
            0 => {
                $cpu.state.operand_first_byte = fetch_operand($cpu, $bus);
            }
            1 => {
                $cpu.state.operand_second_byte = fetch_operand($cpu, $bus);
            }
            2 => {
                let address_lsb = $cpu.state.operand_first_byte.wrapping_add($cpu.registers.$index);
                let address = u16::from_le_bytes([address_lsb, $cpu.state.operand_second_byte]);
                $bus.read(address);
            }
            3 => {
                let address = u16::from_le_bytes([
                    $cpu.state.operand_first_byte,
                    $cpu.state.operand_second_byte,
                ])
                .wrapping_add($cpu.registers.$index.into());
                $cpu.state.target_first_byte = $bus.read(address);
            }
            4 => {
                let address = u16::from_le_bytes([
                    $cpu.state.operand_first_byte,
                    $cpu.state.operand_second_byte,
                ])
                .wrapping_add($cpu.registers.$index.into());
                $bus.write(address, $cpu.state.target_first_byte);
            }
            5 => {
                final_cycle($cpu, $bus);

                let address = u16::from_le_bytes([
                    $cpu.state.operand_first_byte,
                    $cpu.state.operand_second_byte,
                ])
                .wrapping_add($cpu.registers.$index.into());

                let $operand = $cpu.state.target_first_byte;
                let $registers_param = &mut $cpu.registers;
                let value = $body;

                $bus.write(address, value);
            }
            _ => invalid_cycle!($cpu),
        }
    };
}

macro_rules! impl_modify_indirect_x {
    ($cpu:expr, $bus:expr, |$operand:ident, $registers_param:ident| $body:block) => {
        match $cpu.state.cycle {
            0 => {
                $cpu.state.operand_first_byte = fetch_operand($cpu, $bus);
            }
            1 => {
                $bus.read($cpu.state.operand_first_byte.into());
            }
            2 => {
                let address = $cpu.state.operand_first_byte.wrapping_add($cpu.registers.x);
                $cpu.state.target_first_byte = $bus.read(address.into());
            }
            3 => {
                let address =
                    $cpu.state.operand_first_byte.wrapping_add($cpu.registers.x).wrapping_add(1);
                $cpu.state.target_second_byte = $bus.read(address.into());
            }
            4 => {
                let address = u16::from_le_bytes([
                    $cpu.state.target_first_byte,
                    $cpu.state.target_second_byte,
                ]);
                $cpu.state.indirect_byte = $bus.read(address);
            }
            5 => {
                let address = u16::from_le_bytes([
                    $cpu.state.target_first_byte,
                    $cpu.state.target_second_byte,
                ]);
                $bus.write(address, $cpu.state.indirect_byte);
            }
            6 => {
                final_cycle($cpu, $bus);

                let $operand = $cpu.state.indirect_byte;
                let $registers_param = &mut $cpu.registers;
                let value = $body;

                let address = u16::from_le_bytes([
                    $cpu.state.target_first_byte,
                    $cpu.state.target_second_byte,
                ]);
                $bus.write(address, value);
            }
            _ => invalid_cycle!($cpu),
        }
    };
}

macro_rules! impl_modify_indirect_y {
    ($cpu:expr, $bus:expr, |$operand:ident, $registers_param:ident| $body:block) => {
        match $cpu.state.cycle {
            0 => {
                $cpu.state.operand_first_byte = fetch_operand($cpu, $bus);
            }
            1 => {
                $cpu.state.target_first_byte = $bus.read($cpu.state.operand_first_byte.into());
            }
            2 => {
                let address = $cpu.state.operand_first_byte.wrapping_add(1).into();
                $cpu.state.target_second_byte = $bus.read(address);
            }
            3 => {
                let address_lsb = $cpu.state.target_first_byte.wrapping_add($cpu.registers.y);
                let address = u16::from_le_bytes([address_lsb, $cpu.state.target_second_byte]);
                $bus.read(address);
            }
            4 => {
                let address = u16::from_le_bytes([
                    $cpu.state.target_first_byte,
                    $cpu.state.target_second_byte,
                ])
                .wrapping_add($cpu.registers.y.into());
                $cpu.state.indirect_byte = $bus.read(address);
            }
            5 => {
                let address = u16::from_le_bytes([
                    $cpu.state.target_first_byte,
                    $cpu.state.target_second_byte,
                ])
                .wrapping_add($cpu.registers.y.into());
                $bus.write(address, $cpu.state.indirect_byte);
            }
            6 => {
                final_cycle($cpu, $bus);

                let address = u16::from_le_bytes([
                    $cpu.state.target_first_byte,
                    $cpu.state.target_second_byte,
                ])
                .wrapping_add($cpu.registers.y.into());

                let $operand = $cpu.state.indirect_byte;
                let $registers_param = &mut $cpu.registers;
                let value = $body;

                $bus.write(address, value);
            }
            _ => invalid_cycle!($cpu),
        }
    };
}

macro_rules! impl_modify_instruction {
    (accumulator, $($rest:tt)*) => {
        impl_modify_accumulator!($($rest)*)
    };
    (zero_page, $($rest:tt)*) => {
        impl_modify_zero_page!($($rest)*)
    };
    (zero_page_x, $($rest:tt)*) => {
        impl_modify_zero_page_x!($($rest)*)
    };
    (absolute, $($rest:tt)*) => {
        impl_modify_absolute!($($rest)*)
    };
    (absolute_x, $($rest:tt)*) => {
        impl_modify_absolute_indexed!(index: x, $($rest)*)
    };
    (absolute_y, $($rest:tt)*) => {
        impl_modify_absolute_indexed!(index: y, $($rest)*)
    };
    (indirect_x, $($rest:tt)*) => {
        impl_modify_indirect_x!($($rest)*)
    };
    (indirect_y, $($rest:tt)*) => {
        impl_modify_indirect_y!($($rest)*)
    };
}

macro_rules! impl_modify_fn {
    ($name:ident, $addressing_mode:tt, |$operand:ident, $registers:ident| $body:block) => {
        fn $name<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
            impl_modify_instruction!($addressing_mode, cpu, bus, |$operand, $registers| $body);
        }
    };
}

macro_rules! impl_registers_only_fn {
    ($name:ident, |$registers:ident| $body:block) => {
        fn $name<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
            final_cycle(cpu, bus);

            // Spurious operand read
            bus.read(cpu.registers.pc);

            let $registers = &mut cpu.registers;
            $body
        }
    };
}

// LDA, LDX, LDY
macro_rules! impl_load {
    ($name:ident, $register:ident, $addressing_mode:tt) => {
        impl_read_fn!($name, $addressing_mode, |operand, registers| {
            registers.$register = operand;
            registers.status.set_negative(operand.bit(7)).set_zero(operand == 0);
        });
    };
}

impl_load!(lda_immediate, accumulator, immediate);
impl_load!(lda_zero_page, accumulator, zero_page);
impl_load!(lda_zero_page_x, accumulator, zero_page_x);
impl_load!(lda_absolute, accumulator, absolute);
impl_load!(lda_absolute_x, accumulator, absolute_x);
impl_load!(lda_absolute_y, accumulator, absolute_y);
impl_load!(lda_indirect_x, accumulator, indirect_x);
impl_load!(lda_indirect_y, accumulator, indirect_y);

impl_load!(ldx_immediate, x, immediate);
impl_load!(ldx_zero_page, x, zero_page);
impl_load!(ldx_zero_page_y, x, zero_page_y);
impl_load!(ldx_absolute, x, absolute);
impl_load!(ldx_absolute_y, x, absolute_y);

impl_load!(ldy_immediate, y, immediate);
impl_load!(ldy_zero_page, y, zero_page);
impl_load!(ldy_zero_page_x, y, zero_page_x);
impl_load!(ldy_absolute, y, absolute);
impl_load!(ldy_absolute_x, y, absolute_x);

fn add(accumulator: u8, value: u8, flags: &mut StatusFlags) -> u8 {
    let existing_carry = flags.carry;

    let (result, carry1) = accumulator.overflowing_add(value);
    let (result, carry2) = result.overflowing_add(existing_carry.into());
    let new_carry = carry1 || carry2;

    let bit_6_carry = (accumulator & 0x7F) + (value & 0x7F) + u8::from(existing_carry) >= 0x80;
    let overflow = new_carry ^ bit_6_carry;

    flags
        .set_negative(result.bit(7))
        .set_overflow(overflow)
        .set_zero(result == 0)
        .set_carry(new_carry);

    result
}

// ADC
macro_rules! impl_add_with_carry {
    ($name:ident, $addressing_mode:tt) => {
        impl_read_fn!($name, $addressing_mode, |operand, registers| {
            registers.accumulator = add(registers.accumulator, operand, &mut registers.status);
        });
    };
}

impl_add_with_carry!(adc_immediate, immediate);
impl_add_with_carry!(adc_zero_page, zero_page);
impl_add_with_carry!(adc_zero_page_x, zero_page_x);
impl_add_with_carry!(adc_absolute, absolute);
impl_add_with_carry!(adc_absolute_x, absolute_x);
impl_add_with_carry!(adc_absolute_y, absolute_y);
impl_add_with_carry!(adc_indirect_x, indirect_x);
impl_add_with_carry!(adc_indirect_y, indirect_y);

fn and(accumulator: u8, value: u8, flags: &mut StatusFlags) -> u8 {
    let result = accumulator & value;
    flags.set_negative(result.bit(7)).set_zero(result == 0);
    result
}

// AND
macro_rules! impl_and {
    ($name:ident, $addressing_mode:tt) => {
        impl_read_fn!($name, $addressing_mode, |operand, registers| {
            registers.accumulator = and(registers.accumulator, operand, &mut registers.status);
        });
    };
}

impl_and!(and_immediate, immediate);
impl_and!(and_zero_page, zero_page);
impl_and!(and_zero_page_x, zero_page_x);
impl_and!(and_absolute, absolute);
impl_and!(and_absolute_x, absolute_x);
impl_and!(and_absolute_y, absolute_y);
impl_and!(and_indirect_x, indirect_x);
impl_and!(and_indirect_y, indirect_y);

fn bit_test(accumulator: u8, value: u8, flags: &mut StatusFlags) {
    flags.set_negative(value.bit(7)).set_overflow(value.bit(6)).set_zero(accumulator & value == 0);
}

// BIT
macro_rules! impl_bit_test {
    ($name:ident, $addressing_mode:tt) => {
        impl_read_fn!($name, $addressing_mode, |operand, registers| {
            bit_test(registers.accumulator, operand, &mut registers.status);
        });
    };
}

impl_bit_test!(bit_zero_page, zero_page);
impl_bit_test!(bit_absolute, absolute);

fn compare(register: u8, value: u8, flags: &mut StatusFlags) {
    flags
        .set_negative(register.wrapping_sub(value).bit(7))
        .set_zero(register == value)
        .set_carry(register >= value);
}

// CMP, CPX, CPY
macro_rules! impl_compare {
    ($name:ident, $register:ident, $addressing_mode:tt) => {
        impl_read_fn!($name, $addressing_mode, |operand, registers| {
            compare(registers.$register, operand, &mut registers.status);
        });
    };
}

impl_compare!(cmp_immediate, accumulator, immediate);
impl_compare!(cmp_zero_page, accumulator, zero_page);
impl_compare!(cmp_zero_page_x, accumulator, zero_page_x);
impl_compare!(cmp_absolute, accumulator, absolute);
impl_compare!(cmp_absolute_x, accumulator, absolute_x);
impl_compare!(cmp_absolute_y, accumulator, absolute_y);
impl_compare!(cmp_indirect_x, accumulator, indirect_x);
impl_compare!(cmp_indirect_y, accumulator, indirect_y);

impl_compare!(cpx_immediate, x, immediate);
impl_compare!(cpx_zero_page, x, zero_page);
impl_compare!(cpx_absolute, x, absolute);

impl_compare!(cpy_immediate, y, immediate);
impl_compare!(cpy_zero_page, y, zero_page);
impl_compare!(cpy_absolute, y, absolute);

fn xor(accumulator: u8, value: u8, flags: &mut StatusFlags) -> u8 {
    let result = accumulator ^ value;
    flags.set_negative(result.bit(7)).set_zero(result == 0);
    result
}

// EOR
macro_rules! impl_xor {
    ($name:ident, $addressing_mode:tt) => {
        impl_read_fn!($name, $addressing_mode, |operand, registers| {
            registers.accumulator = xor(registers.accumulator, operand, &mut registers.status);
        });
    };
}

impl_xor!(eor_immediate, immediate);
impl_xor!(eor_zero_page, zero_page);
impl_xor!(eor_zero_page_x, zero_page_x);
impl_xor!(eor_absolute, absolute);
impl_xor!(eor_absolute_x, absolute_x);
impl_xor!(eor_absolute_y, absolute_y);
impl_xor!(eor_indirect_x, indirect_x);
impl_xor!(eor_indirect_y, indirect_y);

fn or(accumulator: u8, value: u8, flags: &mut StatusFlags) -> u8 {
    let result = accumulator | value;
    flags.set_negative(result.bit(7)).set_zero(result == 0);
    result
}

// ORA
macro_rules! impl_or {
    ($name:ident, $addressing_mode:tt) => {
        impl_read_fn!($name, $addressing_mode, |operand, registers| {
            registers.accumulator = or(registers.accumulator, operand, &mut registers.status);
        });
    };
}

impl_or!(ora_immediate, immediate);
impl_or!(ora_zero_page, zero_page);
impl_or!(ora_zero_page_x, zero_page_x);
impl_or!(ora_absolute, absolute);
impl_or!(ora_absolute_x, absolute_x);
impl_or!(ora_absolute_y, absolute_y);
impl_or!(ora_indirect_x, indirect_x);
impl_or!(ora_indirect_y, indirect_y);

fn subtract(accumulator: u8, value: u8, flags: &mut StatusFlags) -> u8 {
    // Carry flag is inverted in subtraction
    let existing_borrow = u8::from(!flags.carry);

    let (result, borrowed1) = accumulator.overflowing_sub(value);
    let (result, borrowed2) = result.overflowing_sub(existing_borrow);
    let borrowed = borrowed1 || borrowed2;

    let bit_6_borrowed = accumulator & 0x7F < (value & 0x7F) + existing_borrow;
    let overflow = borrowed ^ bit_6_borrowed;

    flags
        .set_negative(result.bit(7))
        .set_overflow(overflow)
        .set_zero(result == 0)
        .set_carry(!borrowed);

    result
}

// SBC
macro_rules! impl_subtract_with_carry {
    ($name:ident, $addressing_mode:tt) => {
        impl_read_fn!($name, $addressing_mode, |operand, registers| {
            registers.accumulator = subtract(registers.accumulator, operand, &mut registers.status);
        });
    };
}

impl_subtract_with_carry!(sbc_immediate, immediate);
impl_subtract_with_carry!(sbc_zero_page, zero_page);
impl_subtract_with_carry!(sbc_zero_page_x, zero_page_x);
impl_subtract_with_carry!(sbc_absolute, absolute);
impl_subtract_with_carry!(sbc_absolute_x, absolute_x);
impl_subtract_with_carry!(sbc_absolute_y, absolute_y);
impl_subtract_with_carry!(sbc_indirect_x, indirect_x);
impl_subtract_with_carry!(sbc_indirect_y, indirect_y);

fn shift_left(value: u8, flags: &mut StatusFlags) -> u8 {
    let shifted = value << 1;
    flags.set_carry(value.bit(7)).set_negative(shifted.bit(7)).set_zero(shifted == 0);
    shifted
}

// ASL
macro_rules! impl_shift_left {
    ($name:ident, $addressing_mode:tt) => {
        impl_modify_fn!($name, $addressing_mode, |operand, registers| {
            shift_left(operand, &mut registers.status)
        });
    };
}

impl_shift_left!(asl_accumulator, accumulator);
impl_shift_left!(asl_zero_page, zero_page);
impl_shift_left!(asl_zero_page_x, zero_page_x);
impl_shift_left!(asl_absolute, absolute);
impl_shift_left!(asl_absolute_x, absolute_x);

fn decrement(value: u8, flags: &mut StatusFlags) -> u8 {
    let decremented = value.wrapping_sub(1);
    flags.set_negative(decremented.bit(7)).set_zero(decremented == 0);
    decremented
}

// DEC
macro_rules! impl_decrement {
    ($name:ident, $addressing_mode:tt) => {
        impl_modify_fn!($name, $addressing_mode, |operand, registers| {
            decrement(operand, &mut registers.status)
        });
    };
}

impl_decrement!(dec_zero_page, zero_page);
impl_decrement!(dec_zero_page_x, zero_page_x);
impl_decrement!(dec_absolute, absolute);
impl_decrement!(dec_absolute_x, absolute_x);

fn increment(value: u8, flags: &mut StatusFlags) -> u8 {
    let incremented = value.wrapping_add(1);
    flags.set_negative(incremented.bit(7)).set_zero(incremented == 0);
    incremented
}

// INC
macro_rules! impl_increment {
    ($name:ident, $addressing_mode:tt) => {
        impl_modify_fn!($name, $addressing_mode, |operand, registers| {
            increment(operand, &mut registers.status)
        });
    };
}

impl_increment!(inc_zero_page, zero_page);
impl_increment!(inc_zero_page_x, zero_page_x);
impl_increment!(inc_absolute, absolute);
impl_increment!(inc_absolute_x, absolute_x);

fn logical_shift_right(value: u8, flags: &mut StatusFlags) -> u8 {
    let shifted = value >> 1;
    flags.set_carry(value.bit(0)).set_negative(false).set_zero(shifted == 0);
    shifted
}

// LSR
macro_rules! impl_logical_shift_right {
    ($name:ident, $addressing_mode:tt) => {
        impl_modify_fn!($name, $addressing_mode, |operand, registers| {
            logical_shift_right(operand, &mut registers.status)
        });
    };
}

impl_logical_shift_right!(lsr_accumulator, accumulator);
impl_logical_shift_right!(lsr_zero_page, zero_page);
impl_logical_shift_right!(lsr_zero_page_x, zero_page_x);
impl_logical_shift_right!(lsr_absolute, absolute);
impl_logical_shift_right!(lsr_absolute_x, absolute_x);

fn rotate_left(value: u8, flags: &mut StatusFlags) -> u8 {
    let rotated = (value << 1) | u8::from(flags.carry);
    flags.set_carry(value.bit(7)).set_negative(rotated.bit(7)).set_zero(rotated == 0);
    rotated
}

// ROL
macro_rules! impl_rotate_left {
    ($name:ident, $addressing_mode:tt) => {
        impl_modify_fn!($name, $addressing_mode, |operand, registers| {
            rotate_left(operand, &mut registers.status)
        });
    };
}

impl_rotate_left!(rol_accumulator, accumulator);
impl_rotate_left!(rol_zero_page, zero_page);
impl_rotate_left!(rol_zero_page_x, zero_page_x);
impl_rotate_left!(rol_absolute, absolute);
impl_rotate_left!(rol_absolute_x, absolute_x);

fn rotate_right(value: u8, flags: &mut StatusFlags) -> u8 {
    let rotated = (value >> 1) | (u8::from(flags.carry) << 7);
    flags.set_carry(value.bit(0)).set_negative(rotated.bit(7)).set_zero(rotated == 0);
    rotated
}

// ROR
macro_rules! impl_rotate_right {
    ($name:ident, $addressing_mode:tt) => {
        impl_modify_fn!($name, $addressing_mode, |operand, registers| {
            rotate_right(operand, &mut registers.status)
        });
    };
}

impl_rotate_right!(ror_accumulator, accumulator);
impl_rotate_right!(ror_zero_page, zero_page);
impl_rotate_right!(ror_zero_page_x, zero_page_x);
impl_rotate_right!(ror_absolute, absolute);
impl_rotate_right!(ror_absolute_x, absolute_x);

// SLO (unofficial; combination of ASL and ORA)
macro_rules! impl_shift_left_or {
    ($name:ident, $addressing_mode:tt) => {
        impl_modify_fn!($name, $addressing_mode, |operand, registers| {
            let shifted = shift_left(operand, &mut registers.status);
            registers.accumulator = or(registers.accumulator, shifted, &mut registers.status);
            shifted
        });
    };
}

impl_shift_left_or!(slo_zero_page, zero_page);
impl_shift_left_or!(slo_zero_page_x, zero_page_x);
impl_shift_left_or!(slo_absolute, absolute);
impl_shift_left_or!(slo_absolute_x, absolute_x);
impl_shift_left_or!(slo_absolute_y, absolute_y);
impl_shift_left_or!(slo_indirect_x, indirect_x);
impl_shift_left_or!(slo_indirect_y, indirect_y);

// RLA (unofficial; combination of ROL and AND)
macro_rules! impl_rotate_left_and {
    ($name:ident, $addressing_mode:tt) => {
        impl_modify_fn!($name, $addressing_mode, |operand, registers| {
            let rotated = rotate_left(operand, &mut registers.status);
            registers.accumulator = and(registers.accumulator, rotated, &mut registers.status);
            rotated
        });
    };
}

impl_rotate_left_and!(rla_zero_page, zero_page);
impl_rotate_left_and!(rla_zero_page_x, zero_page_x);
impl_rotate_left_and!(rla_absolute, absolute);
impl_rotate_left_and!(rla_absolute_x, absolute_x);
impl_rotate_left_and!(rla_absolute_y, absolute_y);
impl_rotate_left_and!(rla_indirect_x, indirect_x);
impl_rotate_left_and!(rla_indirect_y, indirect_y);

// SRE (unofficial; combination of LSR and EOR)
macro_rules! impl_shift_right_xor {
    ($name:ident, $addressing_mode:tt) => {
        impl_modify_fn!($name, $addressing_mode, |operand, registers| {
            let shifted = logical_shift_right(operand, &mut registers.status);
            registers.accumulator = xor(registers.accumulator, shifted, &mut registers.status);
            shifted
        });
    };
}

impl_shift_right_xor!(sre_zero_page, zero_page);
impl_shift_right_xor!(sre_zero_page_x, zero_page_x);
impl_shift_right_xor!(sre_absolute, absolute);
impl_shift_right_xor!(sre_absolute_x, absolute_x);
impl_shift_right_xor!(sre_absolute_y, absolute_y);
impl_shift_right_xor!(sre_indirect_x, indirect_x);
impl_shift_right_xor!(sre_indirect_y, indirect_y);

// RRA (unofficial; combination of ROR and ADC)
macro_rules! impl_rotate_right_add {
    ($name:ident, $addressing_mode:tt) => {
        impl_modify_fn!($name, $addressing_mode, |operand, registers| {
            let rotated = rotate_right(operand, &mut registers.status);
            registers.accumulator = add(registers.accumulator, rotated, &mut registers.status);
            rotated
        });
    };
}

impl_rotate_right_add!(rra_zero_page, zero_page);
impl_rotate_right_add!(rra_zero_page_x, zero_page_x);
impl_rotate_right_add!(rra_absolute, absolute);
impl_rotate_right_add!(rra_absolute_x, absolute_x);
impl_rotate_right_add!(rra_absolute_y, absolute_y);
impl_rotate_right_add!(rra_indirect_x, indirect_x);
impl_rotate_right_add!(rra_indirect_y, indirect_y);

// DCP (unofficial; combination of DEC and CMP)
macro_rules! impl_decrement_compare {
    ($name:ident, $addressing_mode:tt) => {
        impl_modify_fn!($name, $addressing_mode, |operand, registers| {
            let decremented = decrement(operand, &mut registers.status);
            compare(registers.accumulator, decremented, &mut registers.status);
            decremented
        });
    };
}

impl_decrement_compare!(dcp_zero_page, zero_page);
impl_decrement_compare!(dcp_zero_page_x, zero_page_x);
impl_decrement_compare!(dcp_absolute, absolute);
impl_decrement_compare!(dcp_absolute_x, absolute_x);
impl_decrement_compare!(dcp_absolute_y, absolute_y);
impl_decrement_compare!(dcp_indirect_x, indirect_x);
impl_decrement_compare!(dcp_indirect_y, indirect_y);

// ISC (unofficial; combination of INC and SBC)
macro_rules! impl_increment_subtract {
    ($name:ident, $addressing_mode:tt) => {
        impl_modify_fn!($name, $addressing_mode, |operand, registers| {
            let incremented = increment(operand, &mut registers.status);
            registers.accumulator =
                subtract(registers.accumulator, incremented, &mut registers.status);
            incremented
        });
    };
}

impl_increment_subtract!(isc_zero_page, zero_page);
impl_increment_subtract!(isc_zero_page_x, zero_page_x);
impl_increment_subtract!(isc_absolute, absolute);
impl_increment_subtract!(isc_absolute_x, absolute_x);
impl_increment_subtract!(isc_absolute_y, absolute_y);
impl_increment_subtract!(isc_indirect_x, indirect_x);
impl_increment_subtract!(isc_indirect_y, indirect_y);

// CLC, CLD, CLI, CLV, SEC, SED, SEI
macro_rules! impl_set_status_flag {
    ($name:ident, $flag:ident = $value:expr) => {
        impl_registers_only_fn!($name, |registers| {
            registers.status.$flag = $value;
        });
    };
}

impl_set_status_flag!(clc, carry = false);
impl_set_status_flag!(cld, decimal = false);
impl_set_status_flag!(cli, interrupt_disable = false);
impl_set_status_flag!(clv, overflow = false);
impl_set_status_flag!(sec, carry = true);
impl_set_status_flag!(sed, decimal = true);
impl_set_status_flag!(sei, interrupt_disable = true);

// INX, INY
macro_rules! impl_increment_register {
    ($name:ident, $register:ident) => {
        impl_registers_only_fn!($name, |registers| {
            let value = registers.$register.wrapping_add(1);
            registers.$register = value;
            registers.status.set_negative(value.bit(7)).set_zero(value == 0);
        });
    };
}

impl_increment_register!(inx, x);
impl_increment_register!(iny, y);

// DEX, DEY
macro_rules! impl_decrement_register {
    ($name:ident, $register:ident) => {
        impl_registers_only_fn!($name, |registers| {
            let value = registers.$register.wrapping_sub(1);
            registers.$register = value;
            registers.status.set_negative(value.bit(7)).set_zero(value == 0);
        });
    };
}

impl_decrement_register!(dex, x);
impl_decrement_register!(dey, y);

macro_rules! set_transfer_flags {
    (sp, $registers:expr, $value:expr) => {};
    ($to:ident, $registers:expr, $value:expr) => {
        $registers.status.set_negative($value.bit(7)).set_zero($value == 0);
    };
}

// TAX, TAY, TSX, TXA, TXS, TYA
macro_rules! impl_register_transfer {
    ($name:ident, $from:ident -> $to:ident) => {
        impl_registers_only_fn!($name, |registers| {
            let value = registers.$from;
            registers.$to = value;
            set_transfer_flags!($to, registers, value);
        });
    };
}

impl_register_transfer!(tax, accumulator -> x);
impl_register_transfer!(tay, accumulator -> y);
impl_register_transfer!(tsx, sp -> x);
impl_register_transfer!(txa, x -> accumulator);
impl_register_transfer!(txs, x -> sp);
impl_register_transfer!(tya, y -> accumulator);

// NOP
impl_registers_only_fn!(nop, |_registers| {});

// BCC, BCS, BEQ, BMI, BNE, BPL, BVC, BVS
macro_rules! impl_branch {
    ($name:ident, $flag:ident == $flag_value:expr) => {
        fn $name<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
            match cpu.state.cycle {
                0 => {
                    poll_interrupt_lines(cpu, bus);

                    cpu.state.operand_first_byte = fetch_operand(cpu, bus);

                    if cpu.registers.status.$flag != $flag_value {
                        cpu.state.instruction_complete = true;
                    }
                }
                1 => {
                    bus.read(cpu.registers.pc);

                    let offset = cpu.state.operand_first_byte as i8;
                    let pc = cpu.registers.pc.wrapping_add_signed(offset.into());

                    if cpu.registers.pc & 0xFF00 == pc & 0xFF00 {
                        cpu.registers.pc = pc;
                        cpu.state.instruction_complete = true;
                    }
                }
                2 => {
                    final_cycle(cpu, bus);

                    let offset = cpu.state.operand_first_byte as i8;
                    let pc = cpu.registers.pc.wrapping_add_signed(offset.into());

                    bus.read((cpu.registers.pc & 0xFF00) | (pc & 0x00FF));

                    cpu.registers.pc = pc;
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

impl_branch!(bcc, carry == false);
impl_branch!(bcs, carry == true);
impl_branch!(beq, zero == true);
impl_branch!(bmi, negative == true);
impl_branch!(bne, zero == false);
impl_branch!(bpl, negative == false);
impl_branch!(bvc, overflow == false);
impl_branch!(bvs, overflow == true);

// JMP
fn jmp_absolute<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
    match cpu.state.cycle {
        0 => {
            cpu.state.operand_first_byte = fetch_operand(cpu, bus);
        }
        1 => {
            final_cycle(cpu, bus);

            let address_msb = bus.read(cpu.registers.pc);
            cpu.registers.pc = u16::from_le_bytes([cpu.state.operand_first_byte, address_msb]);
        }
        _ => invalid_cycle!(cpu),
    }
}

// JMP
fn jmp_indirect<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
    match cpu.state.cycle {
        0 => {
            cpu.state.operand_first_byte = fetch_operand(cpu, bus);
        }
        1 => {
            cpu.state.operand_second_byte = fetch_operand(cpu, bus);
        }
        2 => {
            let address =
                u16::from_le_bytes([cpu.state.operand_first_byte, cpu.state.operand_second_byte]);
            cpu.state.target_first_byte = bus.read(address);
        }
        3 => {
            final_cycle(cpu, bus);

            let address_lsb = cpu.state.operand_first_byte.wrapping_add(1);
            let address = u16::from_le_bytes([address_lsb, cpu.state.operand_second_byte]);
            let pc_msb = bus.read(address);

            cpu.registers.pc = u16::from_le_bytes([cpu.state.target_first_byte, pc_msb]);
        }
        _ => invalid_cycle!(cpu),
    }
}

macro_rules! read_register_for_push {
    (accumulator, $registers:expr) => {
        $registers.accumulator
    };
    (p, $registers:expr) => {
        $registers.status.to_byte(StatusReadContext::PushStack)
    };
}

// PHA, PHP
macro_rules! impl_push_stack {
    ($name:ident, $register:tt) => {
        fn $name<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
            match cpu.state.cycle {
                0 => {
                    bus.read(cpu.registers.pc);
                }
                1 => {
                    final_cycle(cpu, bus);

                    let address = u16::from_be_bytes([0x01, cpu.registers.sp]);
                    let value = read_register_for_push!($register, cpu.registers);
                    bus.write(address, value);
                    cpu.registers.sp = cpu.registers.sp.wrapping_sub(1);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

impl_push_stack!(pha, accumulator);
impl_push_stack!(php, p);

macro_rules! write_register_for_pull {
    (accumulator, $registers:expr, $value:expr) => {{
        let value = $value;
        $registers.accumulator = value;
        $registers.status.set_negative(value.bit(7)).set_zero(value == 0);
    }};
    (p, $registers:expr, $value:expr) => {
        $registers.status = StatusFlags::from_byte($value);
    };
}

// PLA, PLP
macro_rules! impl_pull_stack {
    ($name:ident, $register:tt) => {
        fn $name<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
            match cpu.state.cycle {
                0 => {
                    bus.read(cpu.registers.pc);
                }
                1 => {
                    bus.read(u16::from_be_bytes([0x01, cpu.registers.sp]));
                }
                2 => {
                    final_cycle(cpu, bus);

                    cpu.registers.sp = cpu.registers.sp.wrapping_add(1);
                    let value = bus.read(u16::from_be_bytes([0x01, cpu.registers.sp]));
                    write_register_for_pull!($register, cpu.registers, value);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

impl_pull_stack!(pla, accumulator);
impl_pull_stack!(plp, p);

#[inline]
fn push_pc_msb<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
    let address = u16::from_be_bytes([0x01, cpu.registers.sp]);
    bus.write(address, (cpu.registers.pc >> 8) as u8);
    cpu.registers.sp = cpu.registers.sp.wrapping_sub(1);
}

#[inline]
fn push_pc_lsb<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
    let address = u16::from_be_bytes([0x01, cpu.registers.sp]);
    bus.write(address, cpu.registers.pc as u8);
    cpu.registers.sp = cpu.registers.sp.wrapping_sub(1);
}

#[inline]
fn pull_pc_lsb<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
    cpu.registers.sp = cpu.registers.sp.wrapping_add(1);
    cpu.registers.pc = bus.read(u16::from_be_bytes([0x01, cpu.registers.sp])).into();
}

#[inline]
fn pull_pc_msb<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
    cpu.registers.sp = cpu.registers.sp.wrapping_add(1);
    let pc_msb = bus.read(u16::from_be_bytes([0x01, cpu.registers.sp]));
    cpu.registers.pc |= u16::from(pc_msb) << 8;
}

// JSR (jump to subroutine)
fn jsr<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
    match cpu.state.cycle {
        0 => {
            cpu.state.operand_first_byte = fetch_operand(cpu, bus);
        }
        1 => {
            // Spurious stack read
            bus.read(u16::from_be_bytes([0x01, cpu.registers.sp]));
        }
        2 => {
            push_pc_msb(cpu, bus);
        }
        3 => {
            push_pc_lsb(cpu, bus);
        }
        4 => {
            final_cycle(cpu, bus);

            let address_msb = bus.read(cpu.registers.pc);
            cpu.registers.pc = u16::from_le_bytes([cpu.state.operand_first_byte, address_msb]);
        }
        _ => invalid_cycle!(cpu),
    }
}

// RTS (return from subroutine)
fn rts<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
    match cpu.state.cycle {
        0 => {
            // Spurious operand read
            bus.read(cpu.registers.pc);
        }
        1 => {
            // Spurious stack read
            bus.read(u16::from_be_bytes([0x01, cpu.registers.sp]));
        }
        2 => {
            pull_pc_lsb(cpu, bus);
        }
        3 => {
            pull_pc_msb(cpu, bus);
        }
        4 => {
            final_cycle(cpu, bus);

            // Fetch operand and increment PC, ignore fetch result
            fetch_operand(cpu, bus);
        }
        _ => invalid_cycle!(cpu),
    }
}

// RTI (return from interrupt)
fn rti<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
    match cpu.state.cycle {
        0 => {
            // Spurious operand read
            bus.read(cpu.registers.pc);
        }
        1 => {
            // Spurious stack read
            bus.read(u16::from_be_bytes([0x01, cpu.registers.sp]));
        }
        2 => {
            cpu.registers.sp = cpu.registers.sp.wrapping_add(1);
            let value = bus.read(u16::from_be_bytes([0x01, cpu.registers.sp]));
            cpu.registers.status = StatusFlags::from_byte(value);
        }
        3 => {
            pull_pc_lsb(cpu, bus);
        }
        4 => {
            final_cycle(cpu, bus);

            pull_pc_msb(cpu, bus);
        }
        _ => invalid_cycle!(cpu),
    }
}

#[inline]
fn interrupt_push_status<B: BusInterface>(
    cpu: &mut Mos6502,
    bus: &mut B,
    read_ctx: StatusReadContext,
) {
    let stack_address = u16::from_be_bytes([0x01, cpu.registers.sp]);
    bus.write(stack_address, cpu.registers.status.to_byte(read_ctx));
    cpu.registers.sp = cpu.registers.sp.wrapping_sub(1);

    cpu.state.interrupt_vector = if bus.nmi() {
        bus.acknowledge_nmi();
        NMI_VECTOR
    } else {
        IRQ_VECTOR
    };
}

#[inline]
fn interrupt_pull_pc_lsb<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
    cpu.registers.pc = bus.read(cpu.state.interrupt_vector).into();
    cpu.registers.status.interrupt_disable = true;
}

#[inline]
fn interrupt_pull_pc_msb<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
    let pc_msb = bus.read(cpu.state.interrupt_vector + 1);
    cpu.registers.pc |= u16::from(pc_msb) << 8;
}

// BRK (force interrupt)
fn brk<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
    match cpu.state.cycle {
        0 => {
            fetch_operand(cpu, bus);
        }
        1 => {
            push_pc_msb(cpu, bus);
        }
        2 => {
            push_pc_lsb(cpu, bus);
        }
        3 => {
            interrupt_push_status(cpu, bus, StatusReadContext::Brk);
        }
        4 => {
            interrupt_pull_pc_lsb(cpu, bus);
        }
        5 => {
            final_cycle(cpu, bus);

            interrupt_pull_pc_msb(cpu, bus);
        }
        _ => invalid_cycle!(cpu),
    }
}

// Hardware interrupt servicing routine
fn interrupt_service_routine<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
    match cpu.state.cycle {
        0 => {
            // Spurious operand read
            bus.read(cpu.registers.pc);
        }
        1 => {
            push_pc_msb(cpu, bus);
        }
        2 => {
            push_pc_lsb(cpu, bus);
        }
        3 => {
            interrupt_push_status(cpu, bus, StatusReadContext::HardwareInterruptHandler);
        }
        4 => {
            interrupt_pull_pc_lsb(cpu, bus);
        }
        5 => {
            final_cycle(cpu, bus);

            cpu.state.executing_interrupt = false;
            interrupt_pull_pc_msb(cpu, bus);
        }
        _ => invalid_cycle!(cpu),
    }
}

fn execute_unofficial_store<B: BusInterface>(
    cpu: &mut Mos6502,
    bus: &mut B,
    register_value: u8,
    index: u8,
) {
    // This is a buggy instruction that is only implemented because CPU test ROMs test
    // it.
    // This implementation ANDs the X/Y register with the high byte of the address plus 1
    // and then stores that value, but only if the indexing did not overflow.

    let address = u16::from_le_bytes([cpu.state.operand_first_byte, cpu.state.operand_second_byte])
        .wrapping_add(index.into());
    let (_, overflowed) = cpu.state.operand_first_byte.overflowing_add(index);

    let value = register_value & cpu.state.operand_second_byte.wrapping_add(1);

    if !overflowed {
        bus.write(address, value);
    }
}

// SHX, SHY (unofficial)
macro_rules! impl_unofficial_store {
    ($name:ident, register: $register:ident, index: $index:ident) => {
        fn $name<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
            match cpu.state.cycle {
                0 => {
                    cpu.state.operand_first_byte = fetch_operand(cpu, bus);
                }
                1 => {
                    cpu.state.operand_second_byte = fetch_operand(cpu, bus);
                }
                2 => {
                    let address_lsb =
                        cpu.state.operand_first_byte.wrapping_add(cpu.registers.$index);
                    let address = u16::from_le_bytes([address_lsb, cpu.state.operand_second_byte]);
                    bus.read(address);
                }
                3 => {
                    final_cycle(cpu, bus);

                    let value = cpu.registers.$register;
                    let index = cpu.registers.$index;
                    execute_unofficial_store(cpu, bus, value, index);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

impl_unofficial_store!(shy, register: y, index: x);
impl_unofficial_store!(shx, register: x, index: y);

// AHX, TAS (unofficial) (not actually implemented because these opcodes are very unstable)
fn unimplemented_unofficial_store_absolute_y<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
    match cpu.state.cycle {
        0 => {
            cpu.state.operand_first_byte = fetch_operand(cpu, bus);
        }
        1 => {
            cpu.state.operand_second_byte = fetch_operand(cpu, bus);
        }
        2 => {
            let address_lsb = cpu.state.operand_first_byte.wrapping_add(cpu.registers.y);
            let address = u16::from_le_bytes([address_lsb, cpu.state.operand_second_byte]);
            cpu.state.target_first_byte = bus.read(address);
        }
        3 => {
            final_cycle(cpu, bus);

            let address_lsb = cpu.state.operand_first_byte.wrapping_add(cpu.registers.y);
            let address = u16::from_le_bytes([address_lsb, cpu.state.operand_second_byte]);
            bus.write(address, cpu.state.target_first_byte);
        }
        _ => invalid_cycle!(cpu),
    }
}

// AHX, TAS (unofficial) (not actually implemented because these opcodes are very unstable)
fn unimplemented_unofficial_store_indirect_y<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
    match cpu.state.cycle {
        0 => {
            cpu.state.operand_first_byte = fetch_operand(cpu, bus);
        }
        1 => {
            cpu.state.target_first_byte = bus.read(cpu.state.operand_first_byte.into());
        }
        2 => {
            let address = cpu.state.operand_first_byte.wrapping_add(1).into();
            cpu.state.target_second_byte = bus.read(address);
        }
        3 => {
            let address_lsb = cpu.state.target_first_byte.wrapping_add(cpu.registers.y);
            let address = u16::from_le_bytes([address_lsb, cpu.state.target_second_byte]);
            cpu.state.indirect_byte = bus.read(address);
        }
        4 => {
            final_cycle(cpu, bus);

            let address_lsb = cpu.state.target_first_byte.wrapping_add(cpu.registers.y);
            let address = u16::from_le_bytes([address_lsb, cpu.state.target_second_byte]);
            bus.write(address, cpu.state.indirect_byte);
        }
        _ => invalid_cycle!(cpu),
    }
}

// ANC (unofficial; combination of AND and ASL)
impl_read_fn!(anc, immediate, |operand, registers| {
    // ANC performs an AND and then sets the C flag the way that ASL does
    registers.accumulator = and(registers.accumulator, operand, &mut registers.status);
    registers.status.carry = registers.accumulator.bit(7);
});

// ALR (unofficial; combination of AND and LSR)
impl_read_fn!(alr, immediate, |operand, registers| {
    // ALR simply performs an AND followed by an LSR
    let and_value = and(registers.accumulator, operand, &mut registers.status);
    registers.accumulator = logical_shift_right(and_value, &mut registers.status);
});

fn and_with_rotate_right(registers: &mut CpuRegisters, operand: u8) {
    // ARR is like a mix of AND, ROR, and ADC; the accumulator is set to (A & #imm) rotated,
    // but the flags are set differently from ROR

    let and_value = and(registers.accumulator, operand, &mut StatusFlags::new());
    registers.accumulator = (and_value >> 1) | (u8::from(registers.status.carry) << 7);

    // The overflow flag is set as if an ADC was performed between the AND and ROR, and
    // the carry flag is set based on what was bit 7 prior to the rotation
    let overflow = registers.accumulator.bit(6) ^ registers.accumulator.bit(5);
    registers
        .status
        .set_negative(registers.accumulator.bit(7))
        .set_overflow(overflow)
        .set_carry(registers.accumulator.bit(6))
        .set_zero(registers.accumulator == 0);
}

// ARR (unofficial; combination of AND, ROR, and ADC)
impl_read_fn!(arr, immediate, |operand, registers| {
    and_with_rotate_right(registers, operand);
});

// LAX (unofficial)
macro_rules! impl_load_transfer_ax {
    ($name:ident, $addressing_mode:tt) => {
        impl_read_fn!($name, $addressing_mode, |operand, registers| {
            // LAX simply performs LDA and LDX simultaneously

            registers.accumulator = operand;
            registers.x = operand;

            registers.status.set_negative(operand.bit(7)).set_zero(operand == 0);
        });
    };
}

impl_load_transfer_ax!(lax_immediate, immediate);
impl_load_transfer_ax!(lax_zero_page, zero_page);
impl_load_transfer_ax!(lax_zero_page_y, zero_page_y);
impl_load_transfer_ax!(lax_absolute, absolute);
impl_load_transfer_ax!(lax_absolute_y, absolute_y);
impl_load_transfer_ax!(lax_indirect_x, indirect_x);
impl_load_transfer_ax!(lax_indirect_y, indirect_y);

// XAA (unofficial; loads X & <imm> into A)
impl_read_fn!(xaa, immediate, |operand, registers| {
    registers.accumulator = registers.x & operand;
    registers
        .status
        .set_negative(registers.accumulator.bit(7))
        .set_zero(registers.accumulator == 0);
});

// AXS (unofficial)
impl_read_fn!(axs, immediate, |operand, registers| {
    // AXS sets X to (A&X) - #imm, while ignoring the current carry flag. The flags
    // are set not from the subtraction operation but from a CMP between (A&X) and #imm

    let ax = registers.accumulator & registers.x;
    let mut flags = StatusFlags {
        // Set carry to true because SBC inverts the carry flag for borrowing
        carry: true,
        ..StatusFlags::new()
    };
    registers.x = subtract(ax, operand, &mut flags);

    compare(ax, operand, &mut registers.status);
});

// LAS (unofficial; sets A, X, and S to S & value)
impl_read_fn!(las, absolute_y, |operand, registers| {
    let new_value = operand & registers.sp;

    registers.accumulator = new_value;
    registers.x = new_value;
    registers.sp = new_value;

    registers.status.set_negative(new_value.bit(7)).set_zero(new_value == 0);
});

// unofficial NOPs
macro_rules! impl_multi_byte_noop {
    ($name:ident, $addressing_mode:tt) => {
        impl_read_fn!($name, $addressing_mode, |_operand, _registers| {});
    };
}

impl_multi_byte_noop!(nop_immediate, immediate);
impl_multi_byte_noop!(nop_zero_page, zero_page);
impl_multi_byte_noop!(nop_zero_page_x, zero_page_x);
impl_multi_byte_noop!(nop_absolute, absolute);
impl_multi_byte_noop!(nop_absolute_x, absolute_x);

pub fn execute_cycle<B: BusInterface>(cpu: &mut Mos6502, bus: &mut B) {
    if cpu.state.executing_interrupt {
        interrupt_service_routine(cpu, bus);
        cpu.state.cycle += 1;
        return;
    }

    match cpu.state.opcode {
        0x00 => brk(cpu, bus),
        0x01 => ora_indirect_x(cpu, bus),
        0x03 => slo_indirect_x(cpu, bus),
        0x04 | 0x44 | 0x64 => nop_zero_page(cpu, bus),
        0x05 => ora_zero_page(cpu, bus),
        0x06 => asl_zero_page(cpu, bus),
        0x07 => slo_zero_page(cpu, bus),
        0x08 => php(cpu, bus),
        0x09 => ora_immediate(cpu, bus),
        0x0A => asl_accumulator(cpu, bus),
        0x0B | 0x2B => anc(cpu, bus),
        0x0C => nop_absolute(cpu, bus),
        0x0D => ora_absolute(cpu, bus),
        0x0E => asl_absolute(cpu, bus),
        0x0F => slo_absolute(cpu, bus),
        0x10 => bpl(cpu, bus),
        0x11 => ora_indirect_y(cpu, bus),
        0x13 => slo_indirect_y(cpu, bus),
        0x14 | 0x34 | 0x54 | 0x74 | 0xD4 | 0xF4 => nop_zero_page_x(cpu, bus),
        0x15 => ora_zero_page_x(cpu, bus),
        0x16 => asl_zero_page_x(cpu, bus),
        0x17 => slo_zero_page_x(cpu, bus),
        0x18 => clc(cpu, bus),
        0x19 => ora_absolute_y(cpu, bus),
        0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xEA | 0xFA => nop(cpu, bus),
        0x1B => slo_absolute_y(cpu, bus),
        0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC => nop_absolute_x(cpu, bus),
        0x1D => ora_absolute_x(cpu, bus),
        0x1E => asl_absolute_x(cpu, bus),
        0x1F => slo_absolute_x(cpu, bus),
        0x20 => jsr(cpu, bus),
        0x21 => and_indirect_x(cpu, bus),
        0x23 => rla_indirect_x(cpu, bus),
        0x24 => bit_zero_page(cpu, bus),
        0x25 => and_zero_page(cpu, bus),
        0x26 => rol_zero_page(cpu, bus),
        0x27 => rla_zero_page(cpu, bus),
        0x28 => plp(cpu, bus),
        0x29 => and_immediate(cpu, bus),
        0x2A => rol_accumulator(cpu, bus),
        0x2C => bit_absolute(cpu, bus),
        0x2D => and_absolute(cpu, bus),
        0x2E => rol_absolute(cpu, bus),
        0x2F => rla_absolute(cpu, bus),
        0x30 => bmi(cpu, bus),
        0x31 => and_indirect_y(cpu, bus),
        0x33 => rla_indirect_y(cpu, bus),
        0x35 => and_zero_page_x(cpu, bus),
        0x36 => rol_zero_page_x(cpu, bus),
        0x37 => rla_zero_page_x(cpu, bus),
        0x38 => sec(cpu, bus),
        0x39 => and_absolute_y(cpu, bus),
        0x3B => rla_absolute_y(cpu, bus),
        0x3D => and_absolute_x(cpu, bus),
        0x3E => rol_absolute_x(cpu, bus),
        0x3F => rla_absolute_x(cpu, bus),
        0x40 => rti(cpu, bus),
        0x41 => eor_indirect_x(cpu, bus),
        0x43 => sre_indirect_x(cpu, bus),
        0x45 => eor_zero_page(cpu, bus),
        0x46 => lsr_zero_page(cpu, bus),
        0x47 => sre_zero_page(cpu, bus),
        0x48 => pha(cpu, bus),
        0x49 => eor_immediate(cpu, bus),
        0x4A => lsr_accumulator(cpu, bus),
        0x4B => alr(cpu, bus),
        0x4C => jmp_absolute(cpu, bus),
        0x4D => eor_absolute(cpu, bus),
        0x4E => lsr_absolute(cpu, bus),
        0x4F => sre_absolute(cpu, bus),
        0x50 => bvc(cpu, bus),
        0x51 => eor_indirect_y(cpu, bus),
        0x53 => sre_indirect_y(cpu, bus),
        0x55 => eor_zero_page_x(cpu, bus),
        0x56 => lsr_zero_page_x(cpu, bus),
        0x57 => sre_zero_page_x(cpu, bus),
        0x58 => cli(cpu, bus),
        0x59 => eor_absolute_y(cpu, bus),
        0x5B => sre_absolute_y(cpu, bus),
        0x5D => eor_absolute_x(cpu, bus),
        0x5E => lsr_absolute_x(cpu, bus),
        0x5F => sre_absolute_x(cpu, bus),
        0x60 => rts(cpu, bus),
        0x61 => adc_indirect_x(cpu, bus),
        0x63 => rra_indirect_x(cpu, bus),
        0x65 => adc_zero_page(cpu, bus),
        0x66 => ror_zero_page(cpu, bus),
        0x67 => rra_zero_page(cpu, bus),
        0x68 => pla(cpu, bus),
        0x69 => adc_immediate(cpu, bus),
        0x6A => ror_accumulator(cpu, bus),
        0x6B => arr(cpu, bus),
        0x6C => jmp_indirect(cpu, bus),
        0x6D => adc_absolute(cpu, bus),
        0x6E => ror_absolute(cpu, bus),
        0x6F => rra_absolute(cpu, bus),
        0x70 => bvs(cpu, bus),
        0x71 => adc_indirect_y(cpu, bus),
        0x73 => rra_indirect_y(cpu, bus),
        0x75 => adc_zero_page_x(cpu, bus),
        0x76 => ror_zero_page_x(cpu, bus),
        0x77 => rra_zero_page_x(cpu, bus),
        0x78 => sei(cpu, bus),
        0x79 => adc_absolute_y(cpu, bus),
        0x7B => rra_absolute_y(cpu, bus),
        0x7D => adc_absolute_x(cpu, bus),
        0x7E => ror_absolute_x(cpu, bus),
        0x7F => rra_absolute_x(cpu, bus),
        0x80 | 0x82 | 0x89 | 0xC2 | 0xE2 => nop_immediate(cpu, bus),
        0x81 => sta_indirect_x(cpu, bus),
        0x83 => sax_indirect_x(cpu, bus),
        0x84 => sty_zero_page(cpu, bus),
        0x85 => sta_zero_page(cpu, bus),
        0x86 => stx_zero_page(cpu, bus),
        0x87 => sax_zero_page(cpu, bus),
        0x88 => dey(cpu, bus),
        0x8A => txa(cpu, bus),
        0x8B => xaa(cpu, bus),
        0x8C => sty_absolute(cpu, bus),
        0x8D => sta_absolute(cpu, bus),
        0x8E => stx_absolute(cpu, bus),
        0x8F => sax_absolute(cpu, bus),
        0x90 => bcc(cpu, bus),
        0x91 => sta_indirect_y(cpu, bus),
        0x93 => unimplemented_unofficial_store_indirect_y(cpu, bus),
        0x94 => sty_zero_page_x(cpu, bus),
        0x95 => sta_zero_page_x(cpu, bus),
        0x96 => stx_zero_page_y(cpu, bus),
        0x97 => sax_zero_page_y(cpu, bus),
        0x98 => tya(cpu, bus),
        0x99 => sta_absolute_y(cpu, bus),
        0x9A => txs(cpu, bus),
        0x9B | 0x9F => unimplemented_unofficial_store_absolute_y(cpu, bus),
        0x9C => shy(cpu, bus),
        0x9D => sta_absolute_x(cpu, bus),
        0x9E => shx(cpu, bus),
        0xA0 => ldy_immediate(cpu, bus),
        0xA1 => lda_indirect_x(cpu, bus),
        0xA2 => ldx_immediate(cpu, bus),
        0xA3 => lax_indirect_x(cpu, bus),
        0xA4 => ldy_zero_page(cpu, bus),
        0xA5 => lda_zero_page(cpu, bus),
        0xA6 => ldx_zero_page(cpu, bus),
        0xA7 => lax_zero_page(cpu, bus),
        0xA8 => tay(cpu, bus),
        0xA9 => lda_immediate(cpu, bus),
        0xAA => tax(cpu, bus),
        0xAB => lax_immediate(cpu, bus),
        0xAC => ldy_absolute(cpu, bus),
        0xAD => lda_absolute(cpu, bus),
        0xAE => ldx_absolute(cpu, bus),
        0xAF => lax_absolute(cpu, bus),
        0xB0 => bcs(cpu, bus),
        0xB1 => lda_indirect_y(cpu, bus),
        0xB3 => lax_indirect_y(cpu, bus),
        0xB4 => ldy_zero_page_x(cpu, bus),
        0xB5 => lda_zero_page_x(cpu, bus),
        0xB6 => ldx_zero_page_y(cpu, bus),
        0xB7 => lax_zero_page_y(cpu, bus),
        0xB8 => clv(cpu, bus),
        0xB9 => lda_absolute_y(cpu, bus),
        0xBA => tsx(cpu, bus),
        0xBB => las(cpu, bus),
        0xBC => ldy_absolute_x(cpu, bus),
        0xBD => lda_absolute_x(cpu, bus),
        0xBE => ldx_absolute_y(cpu, bus),
        0xBF => lax_absolute_y(cpu, bus),
        0xC0 => cpy_immediate(cpu, bus),
        0xC1 => cmp_indirect_x(cpu, bus),
        0xC3 => dcp_indirect_x(cpu, bus),
        0xC4 => cpy_zero_page(cpu, bus),
        0xC5 => cmp_zero_page(cpu, bus),
        0xC6 => dec_zero_page(cpu, bus),
        0xC7 => dcp_zero_page(cpu, bus),
        0xC8 => iny(cpu, bus),
        0xC9 => cmp_immediate(cpu, bus),
        0xCA => dex(cpu, bus),
        0xCB => axs(cpu, bus),
        0xCC => cpy_absolute(cpu, bus),
        0xCD => cmp_absolute(cpu, bus),
        0xCE => dec_absolute(cpu, bus),
        0xCF => dcp_absolute(cpu, bus),
        0xD0 => bne(cpu, bus),
        0xD1 => cmp_indirect_y(cpu, bus),
        0xD3 => dcp_indirect_y(cpu, bus),
        0xD5 => cmp_zero_page_x(cpu, bus),
        0xD6 => dec_zero_page_x(cpu, bus),
        0xD7 => dcp_zero_page_x(cpu, bus),
        0xD8 => cld(cpu, bus),
        0xD9 => cmp_absolute_y(cpu, bus),
        0xDB => dcp_absolute_y(cpu, bus),
        0xDD => cmp_absolute_x(cpu, bus),
        0xDE => dec_absolute_x(cpu, bus),
        0xDF => dcp_absolute_x(cpu, bus),
        0xE0 => cpx_immediate(cpu, bus),
        0xE1 => sbc_indirect_x(cpu, bus),
        0xE3 => isc_indirect_x(cpu, bus),
        0xE4 => cpx_zero_page(cpu, bus),
        0xE5 => sbc_zero_page(cpu, bus),
        0xE6 => inc_zero_page(cpu, bus),
        0xE7 => isc_zero_page(cpu, bus),
        0xE8 => inx(cpu, bus),
        0xE9 | 0xEB => sbc_immediate(cpu, bus),
        0xEC => cpx_absolute(cpu, bus),
        0xED => sbc_absolute(cpu, bus),
        0xEE => inc_absolute(cpu, bus),
        0xEF => isc_absolute(cpu, bus),
        0xF0 => beq(cpu, bus),
        0xF1 => sbc_indirect_y(cpu, bus),
        0xF3 => isc_indirect_y(cpu, bus),
        0xF5 => sbc_zero_page_x(cpu, bus),
        0xF6 => inc_zero_page_x(cpu, bus),
        0xF7 => isc_zero_page_x(cpu, bus),
        0xF8 => sed(cpu, bus),
        0xF9 => sbc_absolute_y(cpu, bus),
        0xFB => isc_absolute_y(cpu, bus),
        0xFD => sbc_absolute_x(cpu, bus),
        0xFE => inc_absolute_x(cpu, bus),
        0xFF => isc_absolute_x(cpu, bus),
        0x02 | 0x12 | 0x22 | 0x32 | 0x42 | 0x52 | 0x62 | 0x72 | 0x92 | 0xB2 | 0xD2 | 0xF2 => {
            // KIL unofficial opcodes; executing any of these halts the CPU until a reset or power cycle
            cpu.frozen = true;
        }
    }

    cpu.state.cycle += 1;
}
