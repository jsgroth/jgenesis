use crate::bus;
use crate::bus::CpuBus;
use crate::cpu::instructions::InstructionState;
use crate::cpu::{CpuRegisters, StatusFlags, StatusReadContext};
use crate::num::GetBit;

macro_rules! poll_interrupt_lines {
    ($state:expr, $registers:expr, $bus:expr) => {
        $state.pending_interrupt |= $bus.interrupt_lines().nmi_triggered()
            || (!$registers.status.interrupt_disable && $bus.interrupt_lines().irq_triggered());
    };
}

macro_rules! final_cycle {
    ($state:expr, $registers:expr, $bus:expr) => {{
        poll_interrupt_lines!($state, $registers, $bus);
        $state.instruction_complete = true;
    }};
}

macro_rules! fetch_operand {
    ($registers:expr, $bus:expr) => {{
        let operand = $bus.read_address($registers.pc);
        $registers.pc += 1;
        operand
    }};
}

macro_rules! impl_read_immediate {
    ($state:expr, $registers:expr, $bus:expr, $operand:ident, $body:block) => {{
        final_cycle!($state, $registers, $bus);

        let $operand = fetch_operand!($registers, $bus);
        $body
    }};
}

macro_rules! impl_read_zero_page {
    ($state:expr, $registers:expr, $bus:expr, $operand:ident, $body:block) => {
        match $state.cycle {
            0 => {
                $state.operand_first_byte = fetch_operand!($registers, $bus);
            }
            1 => {
                final_cycle!($state, $registers, $bus);

                let $operand = $bus.read_address($state.operand_first_byte.into());
                $body
            }
            _ => panic!("invalid cycle: {}", $state.cycle),
        }
    };
}

macro_rules! impl_read_zero_page_indexed {
    (index: $index:ident, $state:expr, $registers:expr, $bus:expr, $operand:ident, $body:block) => {
        match $state.cycle {
            0 => {
                $state.operand_first_byte = fetch_operand!($registers, $bus);
            }
            1 => {
                $bus.read_address($state.operand_first_byte.into());
            }
            2 => {
                final_cycle!($state, $registers, $bus);

                let index = $registers.$index;
                let address = $state.operand_first_byte.wrapping_add(index);
                let $operand = $bus.read_address(address.into());
                $body
            }
            _ => panic!("invalid cycle: {}", $state.cycle),
        }
    };
}

macro_rules! impl_read_absolute {
    ($state:expr, $registers:expr, $bus:expr, $operand:ident, $body:block) => {
        match $state.cycle {
            0 => {
                $state.operand_first_byte = fetch_operand!($registers, $bus);
            }
            1 => {
                $state.operand_second_byte = fetch_operand!($registers, $bus);
            }
            2 => {
                final_cycle!($state, $registers, $bus);

                let address =
                    u16::from_le_bytes([$state.operand_first_byte, $state.operand_second_byte]);
                let $operand = $bus.read_address(address);
                $body
            }
            _ => panic!("invalid cycle: {}", $state.cycle),
        }
    };
}

macro_rules! impl_read_absolute_indexed {
    (index: $index:ident, $state:expr, $registers:expr, $bus:expr, $operand:ident, $body:block) => {
        match $state.cycle {
            0 => {
                $state.operand_first_byte = fetch_operand!($registers, $bus);
            }
            1 => {
                $state.operand_second_byte = fetch_operand!($registers, $bus);
            }
            2 => {
                poll_interrupt_lines!($state, $registers, $bus);

                let (address_lsb, overflowed) =
                    $state.operand_first_byte.overflowing_add($registers.$index);
                let address = u16::from_le_bytes([address_lsb, $state.operand_second_byte]);
                let $operand = $bus.read_address(address);

                if !overflowed {
                    $body
                    $state.instruction_complete = true;
                }
            }
            3 => {
                final_cycle!($state, $registers, $bus);

                let address = u16::from_le_bytes([$state.operand_first_byte, $state.operand_second_byte])
                    .wrapping_add($registers.$index.into());
                let $operand = $bus.read_address(address);
                $body
            }
            _ => panic!("invalid cycle: {}", $state.cycle)
        }
    }
}

macro_rules! impl_read_indirect_x {
    ($state:expr, $registers:expr, $bus:expr, $operand:ident, $body:block) => {
        match $state.cycle {
            0 => {
                $state.operand_first_byte = fetch_operand!($registers, $bus);
            }
            1 => {
                $bus.read_address($state.operand_first_byte.into());
            }
            2 => {
                let address = $state.operand_first_byte.wrapping_add($registers.x);
                $state.target_first_byte = $bus.read_address(address.into());
            }
            3 => {
                let address = $state
                    .operand_first_byte
                    .wrapping_add($registers.x)
                    .wrapping_add(1);
                $state.target_second_byte = $bus.read_address(address.into());
            }
            4 => {
                final_cycle!($state, $registers, $bus);

                let address =
                    u16::from_le_bytes([$state.target_first_byte, $state.target_second_byte]);
                let $operand = $bus.read_address(address);
                $body
            }
            _ => panic!("invalid cycle: {}", $state.cycle),
        }
    };
}

macro_rules! impl_read_indirect_y {
    ($state:expr, $registers:expr, $bus:expr, $operand:ident, $body:block) => {
        match $state.cycle {
            0 => {
                $state.operand_first_byte = fetch_operand!($registers, $bus);
            }
            1 => {
                $state.target_first_byte = $bus.read_address($state.operand_first_byte.into());
            }
            2 => {
                $state.target_second_byte =
                    $bus.read_address($state.operand_first_byte.wrapping_add(1).into());
            }
            3 => {
                poll_interrupt_lines!($state, $registers, $bus);

                let (address_lsb, overflowed) = $state.target_first_byte.overflowing_add($registers.y);
                let address = u16::from_le_bytes([address_lsb, $state.target_second_byte]);
                let $operand = $bus.read_address(address);

                if !overflowed {
                    $body
                    $state.instruction_complete = true;
                }
            }
            4 => {
                final_cycle!($state, $registers, $bus);

                let address = u16::from_le_bytes([$state.target_first_byte, $state.target_second_byte])
                    .wrapping_add($registers.y.into());
                let $operand = $bus.read_address(address);
                $body
            }
            _ => panic!("invalid cycle: {}", $state.cycle)
        }
    }
}

macro_rules! impl_read_instruction {
    (immediate, $state:expr, $registers:expr, $bus:expr, |$operand:ident| $body:block) => {
        impl_read_immediate!($state, $registers, $bus, $operand, $body)
    };
    (zero_page, $state:expr, $registers:expr, $bus:expr, |$operand:ident| $body:block) => {
        impl_read_zero_page!($state, $registers, $bus, $operand, $body)
    };
    (zero_page_x, $state:expr, $registers:expr, $bus:expr, |$operand:ident| $body:block) => {
        impl_read_zero_page_indexed!(index: x, $state, $registers, $bus, $operand, $body)
    };
    (zero_page_y, $state:expr, $registers:expr, $bus:expr, |$operand:ident| $body:block) => {
        impl_read_zero_page_indexed!(index: y, $state, $registers, $bus, $operand, $body)
    };
    (absolute, $state:expr, $registers:expr, $bus:expr, |$operand:ident| $body:block) => {
        impl_read_absolute!($state, $registers, $bus, $operand, $body)
    };
    (absolute_x, $state:expr, $registers:expr, $bus:expr, |$operand:ident| $body:block) => {
        impl_read_absolute_indexed!(index: x, $state, $registers, $bus, $operand, $body)
    };
    (absolute_y, $state:expr, $registers:expr, $bus:expr, |$operand:ident| $body:block) => {
        impl_read_absolute_indexed!(index: y, $state, $registers, $bus, $operand, $body)
    };
    (indirect_x, $state:expr, $registers:expr, $bus:expr, |$operand:ident| $body:block) => {
        impl_read_indirect_x!($state, $registers, $bus, $operand, $body)
    };
    (indirect_y, $state:expr, $registers:expr, $bus:expr, |$operand:ident| $body:block) => {
        impl_read_indirect_y!($state, $registers, $bus, $operand, $body)
    };
}

macro_rules! impl_store_zero_page {
    ($state:expr, $registers:expr, $bus:expr, $register:expr) => {
        match $state.cycle {
            0 => {
                $state.operand_first_byte = fetch_operand!($registers, $bus);
            }
            1 => {
                final_cycle!($state, $registers, $bus);

                let address = $state.operand_first_byte.into();
                $bus.write_address(address, $register);
            }
            _ => panic!("invalid cycle: {}", $state.cycle),
        }
    };
}

macro_rules! impl_store_zero_page_indexed {
    (index: $index:ident, $state:expr, $registers:expr, $bus:expr, $register:expr) => {
        match $state.cycle {
            0 => {
                $state.operand_first_byte = fetch_operand!($registers, $bus);
            }
            1 => {
                $bus.read_address($state.operand_first_byte.into());
            }
            2 => {
                final_cycle!($state, $registers, $bus);

                let address = $state
                    .operand_first_byte
                    .wrapping_add($registers.$index)
                    .into();
                $bus.write_address(address, $register);
            }
            _ => panic!("invalid cycle: {}", $state.cycle),
        }
    };
}

macro_rules! impl_store_absolute {
    ($state:expr, $registers:expr, $bus:expr, $register:expr) => {
        match $state.cycle {
            0 => {
                $state.operand_first_byte = fetch_operand!($registers, $bus);
            }
            1 => {
                $state.operand_second_byte = fetch_operand!($registers, $bus);
            }
            2 => {
                final_cycle!($state, $registers, $bus);

                let address =
                    u16::from_le_bytes([$state.operand_first_byte, $state.operand_second_byte]);
                $bus.write_address(address, $register);
            }
            _ => panic!("invalid cycle: {}", $state.cycle),
        }
    };
}

macro_rules! impl_store_absolute_indexed {
    (index: $index:ident, $state:expr, $registers:expr, $bus:expr, $register:expr) => {
        match $state.cycle {
            0 => {
                $state.operand_first_byte = fetch_operand!($registers, $bus);
            }
            1 => {
                $state.operand_second_byte = fetch_operand!($registers, $bus);
            }
            2 => {
                let address_lsb = $state.operand_first_byte.wrapping_add($registers.$index);
                let address = u16::from_le_bytes([address_lsb, $state.operand_second_byte]);
                $bus.read_address(address);
            }
            3 => {
                final_cycle!($state, $registers, $bus);

                let address =
                    u16::from_le_bytes([$state.operand_first_byte, $state.operand_second_byte])
                        .wrapping_add($registers.$index.into());
                $bus.write_address(address, $register);
            }
            _ => panic!("invalid cycle: {}", $state.cycle),
        }
    };
}

macro_rules! impl_store_indirect_x {
    ($state:expr, $registers:expr, $bus:expr, $register:expr) => {
        match $state.cycle {
            0 => {
                $state.operand_first_byte = fetch_operand!($registers, $bus);
            }
            1 => {
                $bus.read_address($state.operand_first_byte.into());
            }
            2 => {
                let address = $state.operand_first_byte.wrapping_add($registers.x);
                $state.target_first_byte = $bus.read_address(address.into());
            }
            3 => {
                let address = $state
                    .operand_first_byte
                    .wrapping_add($registers.x)
                    .wrapping_add(1);
                $state.target_second_byte = $bus.read_address(address.into());
            }
            4 => {
                final_cycle!($state, $registers, $bus);

                let address =
                    u16::from_le_bytes([$state.target_first_byte, $state.target_second_byte]);
                $bus.write_address(address, $register);
            }
            _ => panic!("invalid cycle: {}", $state.cycle),
        }
    };
}

macro_rules! impl_store_indirect_y {
    ($state:expr, $registers:expr, $bus:expr, $register:expr) => {
        match $state.cycle {
            0 => {
                $state.operand_first_byte = fetch_operand!($registers, $bus);
            }
            1 => {
                $state.target_first_byte = $bus.read_address($state.operand_first_byte.into());
            }
            2 => {
                let address = $state.operand_first_byte.wrapping_add(1);
                $state.target_second_byte = $bus.read_address(address.into());
            }
            3 => {
                let address_lsb = $state.target_first_byte.wrapping_add($registers.y);
                let address = u16::from_le_bytes([address_lsb, $state.target_second_byte]);
                $bus.read_address(address);
            }
            4 => {
                final_cycle!($state, $registers, $bus);

                let address =
                    u16::from_le_bytes([$state.target_first_byte, $state.target_second_byte])
                        .wrapping_add($registers.y.into());
                $bus.write_address(address, $register);
            }
            _ => panic!("invalid cycle: {}", $state.cycle),
        }
    };
}

macro_rules! impl_store {
    (zero_page, $state:expr, $registers:expr, $bus:expr, $register:expr) => {
        impl_store_zero_page!($state, $registers, $bus, $register)
    };
    (zero_page_x, $state:expr, $registers:expr, $bus:expr, $register:expr) => {
        impl_store_zero_page_indexed!(index: x, $state, $registers, $bus, $register)
    };
    (zero_page_y, $state:expr, $registers:expr, $bus:expr, $register:expr) => {
        impl_store_zero_page_indexed!(index: y, $state, $registers, $bus, $register)
    };
    (absolute, $state:expr, $registers:expr, $bus:expr, $register:expr) => {
        impl_store_absolute!($state, $registers, $bus, $register)
    };
    (absolute_x, $state:expr, $registers:expr, $bus:expr, $register:expr) => {
        impl_store_absolute_indexed!(index: x, $state, $registers, $bus, $register)
    };
    (absolute_y, $state:expr, $registers:expr, $bus:expr, $register:expr) => {
        impl_store_absolute_indexed!(index: y, $state, $registers, $bus, $register)
    };
    (indirect_x, $state:expr, $registers:expr, $bus:expr, $register:expr) => {
        impl_store_indirect_x!($state, $registers, $bus, $register)
    };
    (indirect_y, $state:expr, $registers:expr, $bus:expr, $register:expr) => {
        impl_store_indirect_y!($state, $registers, $bus, $register)
    };
}

macro_rules! impl_modify_accumulator {
    ($state:expr, $registers:expr, $bus:expr, $operand:ident, $body:block) => {{
        final_cycle!($state, $registers, $bus);

        $bus.read_address($registers.pc);

        let $operand = $registers.accumulator;
        $registers.accumulator = $body;
    }};
}

macro_rules! impl_modify_zero_page {
    ($state:expr, $registers:expr, $bus:expr, $operand:ident, $body:block) => {
        match $state.cycle {
            0 => {
                $state.operand_first_byte = fetch_operand!($registers, $bus);
            }
            1 => {
                $state.target_first_byte = $bus.read_address($state.operand_first_byte.into());
            }
            2 => {
                $bus.write_address($state.operand_first_byte.into(), $state.target_first_byte);
            }
            3 => {
                final_cycle!($state, $registers, $bus);

                let $operand = $state.target_first_byte;
                let value = $body;
                $bus.write_address($state.operand_first_byte.into(), value);
            }
            _ => panic!("invalid cycle: {}", $state.cycle),
        }
    };
}

macro_rules! impl_modify_zero_page_x {
    ($state:expr, $registers:expr, $bus:expr, $operand:ident, $body:block) => {
        match $state.cycle {
            0 => {
                $state.operand_first_byte = fetch_operand!($registers, $bus);
            }
            1 => {
                $bus.read_address($state.operand_first_byte.into());
            }
            2 => {
                let address = $state.operand_first_byte.wrapping_add($registers.x).into();
                $state.target_first_byte = $bus.read_address(address);
            }
            3 => {
                let address = $state.operand_first_byte.wrapping_add($registers.x).into();
                $bus.write_address(address, $state.target_first_byte);
            }
            4 => {
                final_cycle!($state, $registers, $bus);

                let $operand = $state.target_first_byte;
                let value = $body;

                let address = $state.operand_first_byte.wrapping_add($registers.x).into();
                $bus.write_address(address, value);
            }
            _ => panic!("invalid cycle: {}", $state.cycle),
        }
    };
}

macro_rules! impl_modify_absolute {
    ($state:expr, $registers:expr, $bus:expr, $operand:ident, $body:block) => {
        match $state.cycle {
            0 => {
                $state.operand_first_byte = fetch_operand!($registers, $bus);
            }
            1 => {
                $state.operand_second_byte = fetch_operand!($registers, $bus);
            }
            2 => {
                let address =
                    u16::from_le_bytes([$state.operand_first_byte, $state.operand_second_byte]);
                $state.target_first_byte = $bus.read_address(address);
            }
            3 => {
                let address =
                    u16::from_le_bytes([$state.operand_first_byte, $state.operand_second_byte]);
                $bus.write_address(address, $state.target_first_byte);
            }
            4 => {
                final_cycle!($state, $registers, $bus);

                let $operand = $state.target_first_byte;
                let value = $body;

                let address =
                    u16::from_le_bytes([$state.operand_first_byte, $state.operand_second_byte]);
                $bus.write_address(address, value);
            }
            _ => panic!("invalid cycle: {}", $state.cycle),
        }
    };
}

macro_rules! impl_modify_absolute_indexed {
    (index: $index:ident, $state:expr, $registers:expr, $bus:expr, $operand:ident, $body:block) => {
        match $state.cycle {
            0 => {
                $state.operand_first_byte = fetch_operand!($registers, $bus);
            }
            1 => {
                $state.operand_second_byte = fetch_operand!($registers, $bus);
            }
            2 => {
                let address_lsb = $state.operand_first_byte.wrapping_add($registers.$index);
                let address = u16::from_le_bytes([address_lsb, $state.operand_second_byte]);
                $bus.read_address(address);
            }
            3 => {
                let address =
                    u16::from_le_bytes([$state.operand_first_byte, $state.operand_second_byte])
                        .wrapping_add($registers.$index.into());
                $state.target_first_byte = $bus.read_address(address);
            }
            4 => {
                let address =
                    u16::from_le_bytes([$state.operand_first_byte, $state.operand_second_byte])
                        .wrapping_add($registers.$index.into());
                $bus.write_address(address, $state.target_first_byte);
            }
            5 => {
                final_cycle!($state, $registers, $bus);

                let $operand = $state.target_first_byte;
                let value = $body;

                let address =
                    u16::from_le_bytes([$state.operand_first_byte, $state.operand_second_byte])
                        .wrapping_add($registers.$index.into());
                $bus.write_address(address, value);
            }
            _ => panic!("invalid cycle: {}", $state.cycle),
        }
    };
}

macro_rules! impl_modify_indirect_x {
    ($state:expr, $registers:expr, $bus:expr, $operand:ident, $body:block) => {
        match $state.cycle {
            0 => {
                $state.operand_first_byte = fetch_operand!($registers, $bus);
            }
            1 => {
                $bus.read_address($state.operand_first_byte.into());
            }
            2 => {
                let address = $state.operand_first_byte.wrapping_add($registers.x);
                $state.target_first_byte = $bus.read_address(address.into());
            }
            3 => {
                let address = $state
                    .operand_first_byte
                    .wrapping_add($registers.x)
                    .wrapping_add(1);
                $state.target_second_byte = $bus.read_address(address.into());
            }
            4 => {
                let address =
                    u16::from_le_bytes([$state.target_first_byte, $state.target_second_byte]);
                $state.indirect_byte = $bus.read_address(address);
            }
            5 => {
                let address =
                    u16::from_le_bytes([$state.target_first_byte, $state.target_second_byte]);
                $bus.write_address(address, $state.indirect_byte);
            }
            6 => {
                final_cycle!($state, $registers, $bus);

                let $operand = $state.indirect_byte;
                let value = $body;

                let address =
                    u16::from_le_bytes([$state.target_first_byte, $state.target_second_byte]);
                $bus.write_address(address, value);
            }
            _ => panic!("invalid cycle: {}", $state.cycle),
        }
    };
}

macro_rules! impl_modify_indirect_y {
    ($state:expr, $registers:expr, $bus:expr, $operand:ident, $body:block) => {
        match $state.cycle {
            0 => {
                $state.operand_first_byte = fetch_operand!($registers, $bus);
            }
            1 => {
                $state.target_first_byte = $bus.read_address($state.operand_first_byte.into());
            }
            2 => {
                let address = $state.operand_first_byte.wrapping_add(1).into();
                $state.target_second_byte = $bus.read_address(address);
            }
            3 => {
                let address_lsb = $state.target_first_byte.wrapping_add($registers.y);
                let address = u16::from_le_bytes([address_lsb, $state.target_second_byte]);
                $bus.read_address(address);
            }
            4 => {
                let address =
                    u16::from_le_bytes([$state.target_first_byte, $state.target_second_byte])
                        .wrapping_add($registers.y.into());
                $state.indirect_byte = $bus.read_address(address);
            }
            5 => {
                let address =
                    u16::from_le_bytes([$state.target_first_byte, $state.target_second_byte])
                        .wrapping_add($registers.y.into());
                $bus.write_address(address, $state.indirect_byte);
            }
            6 => {
                final_cycle!($state, $registers, $bus);

                let $operand = $state.indirect_byte;
                let value = $body;

                let address =
                    u16::from_le_bytes([$state.target_first_byte, $state.target_second_byte])
                        .wrapping_add($registers.y.into());
                $bus.write_address(address, value);
            }
            _ => panic!("invalid cycle: {}", $state.cycle),
        }
    };
}

macro_rules! impl_modify_instruction {
    (accumulator, $state:expr, $registers:expr, $bus:expr, |$operand:ident| $body:block) => {
        impl_modify_accumulator!($state, $registers, $bus, $operand, $body)
    };
    (zero_page, $state:expr, $registers:expr, $bus:expr, |$operand:ident| $body:block) => {
        impl_modify_zero_page!($state, $registers, $bus, $operand, $body)
    };
    (zero_page_x, $state:expr, $registers:expr, $bus:expr, |$operand:ident| $body:block) => {
        impl_modify_zero_page_x!($state, $registers, $bus, $operand, $body)
    };
    (absolute, $state:expr, $registers:expr, $bus:expr, |$operand:ident| $body:block) => {
        impl_modify_absolute!($state, $registers, $bus, $operand, $body)
    };
    (absolute_x, $state:expr, $registers:expr, $bus:expr, |$operand:ident| $body:block) => {
        impl_modify_absolute_indexed!(index: x, $state, $registers, $bus, $operand, $body)
    };
    (absolute_y, $state:expr, $registers:expr, $bus:expr, |$operand:ident| $body:block) => {
        impl_modify_absolute_indexed!(index: y, $state, $registers, $bus, $operand, $body)
    };
    (indirect_x, $state:expr, $registers:expr, $bus:expr, |$operand:ident| $body:block) => {
        impl_modify_indirect_x!($state, $registers, $bus, $operand, $body)
    };
    (indirect_y, $state:expr, $registers:expr, $bus:expr, |$operand:ident| $body:block) => {
        impl_modify_indirect_y!($state, $registers, $bus, $operand, $body)
    };
}

macro_rules! impl_registers_instruction {
    ($state:expr, $registers:expr, $bus:expr, || $body:block) => {{
        final_cycle!($state, $registers, $bus);

        $bus.read_address($registers.pc);

        $body
    }};
}

// LDA, LDX, LDY
macro_rules! load {
    ($addressing_mode:tt, register: $register:ident, $state:expr, $registers:expr, $bus:expr) => {
        impl_read_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            $registers.$register = operand;
            $registers
                .status
                .set_negative(operand.bit(7))
                .set_zero(operand == 0);
        })
    };
}

// STA, STX, STY
macro_rules! store {
    ($addressing_mode:tt, register: ax, $state:expr, $registers:expr, $bus:expr) => {
        impl_store!(
            $addressing_mode,
            $state,
            $registers,
            $bus,
            $registers.accumulator & $registers.x
        )
    };
    ($addressing_mode:tt, register: $register:ident, $state:expr, $registers:expr, $bus:expr) => {
        impl_store!(
            $addressing_mode,
            $state,
            $registers,
            $bus,
            $registers.$register
        )
    };
}

fn add(accumulator: u8, value: u8, flags: &mut StatusFlags) -> u8 {
    let existing_carry = flags.carry;

    let (result, new_carry) = match accumulator.overflowing_add(value) {
        (sum, true) => (sum + u8::from(existing_carry), true),
        (sum, false) => sum.overflowing_add(u8::from(existing_carry)),
    };

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
macro_rules! add_with_carry {
    ($addressing_mode:tt, $state:expr, $registers:expr, $bus:expr) => {
        impl_read_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            $registers.accumulator = add($registers.accumulator, operand, &mut $registers.status);
        })
    };
}

fn and(accumulator: u8, value: u8, flags: &mut StatusFlags) -> u8 {
    let result = accumulator & value;
    flags.set_negative(result.bit(7)).set_zero(result == 0);
    result
}

// AND
macro_rules! and {
    ($addressing_mode:tt, $state:expr, $registers:expr, $bus:expr) => {
        impl_read_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            $registers.accumulator = and($registers.accumulator, operand, &mut $registers.status);
        })
    };
}

fn bit_test(accumulator: u8, value: u8, flags: &mut StatusFlags) {
    flags
        .set_negative(value.bit(7))
        .set_overflow(value.bit(6))
        .set_zero(accumulator & value == 0);
}

// BIT
macro_rules! bit_test {
    ($addressing_mode:tt, $state:expr, $registers:expr, $bus:expr) => {
        impl_read_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            bit_test($registers.accumulator, operand, &mut $registers.status);
        })
    };
}

fn compare(register: u8, value: u8, flags: &mut StatusFlags) {
    flags
        .set_negative(register.wrapping_sub(value).bit(7))
        .set_zero(register == value)
        .set_carry(register >= value);
}

// CMP, CPX, CPY
macro_rules! compare {
    ($addressing_mode:tt, register: $register:ident, $state:expr, $registers:expr, $bus:expr) => {
        impl_read_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            compare($registers.$register, operand, &mut $registers.status);
        })
    };
}

fn xor(accumulator: u8, value: u8, flags: &mut StatusFlags) -> u8 {
    let result = accumulator ^ value;
    flags.set_negative(result.bit(7)).set_zero(result == 0);
    result
}

// EOR
macro_rules! xor {
    ($addressing_mode:tt, $state:expr, $registers:expr, $bus:expr) => {
        impl_read_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            $registers.accumulator = xor($registers.accumulator, operand, &mut $registers.status);
        })
    };
}

fn or(accumulator: u8, value: u8, flags: &mut StatusFlags) -> u8 {
    let result = accumulator | value;
    flags.set_negative(result.bit(7)).set_zero(result == 0);
    result
}

// ORA
macro_rules! or {
    ($addressing_mode:tt, $state:expr, $registers:expr, $bus:expr) => {
        impl_read_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            $registers.accumulator = or($registers.accumulator, operand, &mut $registers.status);
        })
    };
}

fn subtract(accumulator: u8, value: u8, flags: &mut StatusFlags) -> u8 {
    // Carry flag is inverted in subtraction
    let existing_borrow = u8::from(!flags.carry);

    let (result, borrowed) = match accumulator.overflowing_sub(value) {
        (difference, true) => (difference - existing_borrow, true),
        (difference, false) => difference.overflowing_sub(existing_borrow),
    };

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
macro_rules! subtract_with_carry {
    ($addressing_mode:tt, $state:expr, $registers:expr, $bus:expr) => {
        impl_read_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            $registers.accumulator =
                subtract($registers.accumulator, operand, &mut $registers.status);
        })
    };
}

fn shift_left(value: u8, flags: &mut StatusFlags) -> u8 {
    let shifted = value << 1;
    flags
        .set_carry(value.bit(7))
        .set_negative(shifted.bit(7))
        .set_zero(shifted == 0);
    shifted
}

// ASL
macro_rules! shift_left {
    ($addressing_mode:tt, $state:expr, $registers:expr, $bus:expr) => {
        impl_modify_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            shift_left(operand, &mut $registers.status)
        })
    };
}

fn decrement(value: u8, flags: &mut StatusFlags) -> u8 {
    let decremented = value.wrapping_sub(1);
    flags
        .set_negative(decremented.bit(7))
        .set_zero(decremented == 0);
    decremented
}

// DEC
macro_rules! decrement {
    ($addressing_mode:tt, $state:expr, $registers:expr, $bus:expr) => {
        impl_modify_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            decrement(operand, &mut $registers.status)
        })
    };
}

fn increment(value: u8, flags: &mut StatusFlags) -> u8 {
    let incremented = value.wrapping_add(1);
    flags
        .set_negative(incremented.bit(7))
        .set_zero(incremented == 0);
    incremented
}

// INC
macro_rules! increment {
    ($addressing_mode:tt, $state:expr, $registers:expr, $bus:expr) => {
        impl_modify_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            increment(operand, &mut $registers.status)
        })
    };
}

fn logical_shift_right(value: u8, flags: &mut StatusFlags) -> u8 {
    let shifted = value >> 1;
    flags
        .set_carry(value.bit(0))
        .set_negative(false)
        .set_zero(shifted == 0);
    shifted
}

// LSR
macro_rules! logical_shift_right {
    ($addressing_mode:tt, $state:expr, $registers:expr, $bus:expr) => {
        impl_modify_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            logical_shift_right(operand, &mut $registers.status)
        })
    };
}

fn rotate_left(value: u8, flags: &mut StatusFlags) -> u8 {
    let rotated = (value << 1) | u8::from(flags.carry);
    flags
        .set_carry(value.bit(7))
        .set_negative(rotated.bit(7))
        .set_zero(rotated == 0);
    rotated
}

// ROL
macro_rules! rotate_left {
    ($addressing_mode:tt, $state:expr, $registers:expr, $bus:expr) => {
        impl_modify_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            rotate_left(operand, &mut $registers.status)
        })
    };
}

fn rotate_right(value: u8, flags: &mut StatusFlags) -> u8 {
    let rotated = (value >> 1) | (u8::from(flags.carry) << 7);
    flags
        .set_carry(value.bit(0))
        .set_negative(rotated.bit(7))
        .set_zero(rotated == 0);
    rotated
}

// ROR
macro_rules! rotate_right {
    ($addressing_mode:tt, $state:expr, $registers:expr, $bus:expr) => {
        impl_modify_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            rotate_right(operand, &mut $registers.status)
        })
    };
}

// SLO (unofficial)
macro_rules! shift_left_or {
    ($addressing_mode:tt, $state:expr, $registers:expr, $bus:expr) => {
        impl_modify_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            let shifted = shift_left(operand, &mut $registers.status);
            $registers.accumulator = or($registers.accumulator, shifted, &mut $registers.status);
            shifted
        })
    };
}

// RLA (unofficial)
macro_rules! rotate_left_and {
    ($addressing_mode:tt, $state:expr, $registers:expr, $bus:expr) => {
        impl_modify_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            let rotated = rotate_left(operand, &mut $registers.status);
            $registers.accumulator = and($registers.accumulator, rotated, &mut $registers.status);
            rotated
        })
    };
}

// SRE (unofficial)
macro_rules! shift_right_xor {
    ($addressing_mode:tt, $state:expr, $registers:expr, $bus:expr) => {
        impl_modify_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            let shifted = logical_shift_right(operand, &mut $registers.status);
            $registers.accumulator = xor($registers.accumulator, shifted, &mut $registers.status);
            shifted
        })
    };
}

// RRA (unofficial)
macro_rules! rotate_right_add {
    ($addressing_mode:tt, $state:expr, $registers:expr, $bus:expr) => {
        impl_modify_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            let rotated = rotate_right(operand, &mut $registers.status);
            $registers.accumulator = add($registers.accumulator, rotated, &mut $registers.status);
            rotated
        })
    };
}

// DCP (unofficial)
macro_rules! decrement_compare {
    ($addressing_mode:tt, $state:expr, $registers:expr, $bus:expr) => {
        impl_modify_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            let decremented = decrement(operand, &mut $registers.status);
            compare($registers.accumulator, decremented, &mut $registers.status);
            decremented
        })
    };
}

// ISC (unofficial)
macro_rules! increment_subtract {
    ($addressing_mode:tt, $state:expr, $registers:expr, $bus:expr) => {
        impl_modify_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            let incremented = increment(operand, &mut $registers.status);
            $registers.accumulator =
                subtract($registers.accumulator, incremented, &mut $registers.status);
            incremented
        })
    };
}

// CLC, CLD, CLI, CLV, SEC, SED, SEI
macro_rules! set_status_flag {
    ($flag:ident, $value:expr, $state:expr, $registers:expr, $bus:expr) => {
        impl_registers_instruction!($state, $registers, $bus, || {
            $registers.status.$flag = $value;
        })
    };
}

// INX, INY
macro_rules! increment_register {
    (register: $register:ident, $state:expr, $registers:expr, $bus:expr) => {
        impl_registers_instruction!($state, $registers, $bus, || {
            let value = $registers.$register.wrapping_add(1);
            $registers.$register = value;
            $registers
                .status
                .set_negative(value.bit(7))
                .set_zero(value == 0);
        })
    };
}

// DEX, DEY
macro_rules! decrement_register {
    (register: $register:ident, $state:expr, $registers:expr, $bus:expr) => {
        impl_registers_instruction!($state, $registers, $bus, || {
            let value = $registers.$register.wrapping_sub(1);
            $registers.$register = value;
            $registers
                .status
                .set_negative(value.bit(7))
                .set_zero(value == 0);
        })
    };
}

macro_rules! set_transfer_flags {
    (sp, $registers:expr, $value:expr) => {};
    ($to:ident, $registers:expr, $value:expr) => {
        $registers
            .status
            .set_negative($value.bit(7))
            .set_zero($value == 0);
    };
}

// TAX, TAY, TSX, TXA, TXS, TYA
macro_rules! register_transfer {
    (to: $to:ident, from: $from:ident, $state:expr, $registers:expr, $bus:expr) => {
        impl_registers_instruction!($state, $registers, $bus, || {
            let value = $registers.$from;
            $registers.$to = value;
            set_transfer_flags!($to, $registers, value);
        })
    };
}

// NOP
macro_rules! noop {
    ($state:expr, $registers:expr, $bus:expr) => {
        impl_registers_instruction!($state, $registers, $bus, || {})
    };
}

// BCC, BCS, BEQ, BMI, BNE, BPL
macro_rules! branch {
    ($flag:ident == $flag_value:expr, $state:expr, $registers:expr, $bus:expr) => {
        match $state.cycle {
            0 => {
                poll_interrupt_lines!($state, $registers, $bus);

                $state.operand_first_byte = fetch_operand!($registers, $bus);

                if $registers.status.$flag != $flag_value {
                    $state.instruction_complete = true;
                }
            }
            1 => {
                $bus.read_address($registers.pc);

                let offset = $state.operand_first_byte as i8;
                let pc = (i32::from($registers.pc) + i32::from(offset)) as u16;

                if $registers.pc & 0xFF00 == pc & 0xFF00 {
                    $registers.pc = pc;
                    $state.instruction_complete = true;
                }
            }
            2 => {
                final_cycle!($state, $registers, $bus);

                let offset = $state.operand_first_byte as i8;
                let pc = (i32::from($registers.pc) + i32::from(offset)) as u16;

                $bus.read_address(($registers.pc & 0xFF00) | (pc & 0x00FF));

                $registers.pc = pc;
            }
            _ => panic!("invalid cycle: {}", $state.cycle),
        }
    };
}

// JMP
fn jump_absolute(state: &mut InstructionState, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) {
    match state.cycle {
        0 => {
            state.operand_first_byte = fetch_operand!(registers, bus);
        }
        1 => {
            final_cycle!(state, registers, bus);

            let address_msb = bus.read_address(registers.pc);
            registers.pc = u16::from_le_bytes([state.operand_first_byte, address_msb]);
        }
        _ => panic!("invalid cycle: {}", state.cycle),
    }
}

// JMP
fn jump_indirect(state: &mut InstructionState, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) {
    match state.cycle {
        0 => {
            state.operand_first_byte = fetch_operand!(registers, bus);
        }
        1 => {
            state.operand_second_byte = fetch_operand!(registers, bus);
        }
        2 => {
            let address = u16::from_le_bytes([state.operand_first_byte, state.operand_second_byte]);
            state.target_first_byte = bus.read_address(address);
        }
        3 => {
            final_cycle!(state, registers, bus);

            let address_lsb = state.operand_first_byte.wrapping_add(1);
            let address = u16::from_le_bytes([address_lsb, state.operand_second_byte]);
            let pc_msb = bus.read_address(address);

            registers.pc = u16::from_le_bytes([state.target_first_byte, pc_msb]);
        }
        _ => panic!("invalid cycle: {}", state.cycle),
    }
}

macro_rules! impl_push_stack {
    ($state:expr, $registers:expr, $bus:expr, $register:expr) => {
        match $state.cycle {
            0 => {
                $bus.read_address($registers.pc);
            }
            1 => {
                final_cycle!($state, $registers, $bus);

                let address = u16::from_be_bytes([0x01, $registers.sp]);
                $bus.write_address(address, $register);
                $registers.sp = $registers.sp.wrapping_sub(1);
            }
            _ => panic!("invalid cycle: {}", $state.cycle),
        }
    };
}

// PHA, PHP
macro_rules! push_stack {
    (register: accumulator, $state:expr, $registers:expr, $bus:expr) => {
        impl_push_stack!($state, $registers, $bus, $registers.accumulator)
    };
    (register: p, $state:expr, $registers:expr, $bus:expr) => {
        impl_push_stack!(
            $state,
            $registers,
            $bus,
            $registers.status.to_byte(StatusReadContext::PushStack)
        )
    };
}

macro_rules! impl_pull_set {
    (register: accumulator, $registers:expr, $value:expr) => {{
        let value = $value;
        $registers.accumulator = value;
        $registers
            .status
            .set_negative(value.bit(7))
            .set_zero(value == 0);
    }};
    (register: p, $registers:expr, $value:expr) => {
        $registers.status = StatusFlags::from_byte($value);
    };
}

// PLA, PLP
macro_rules! pull_stack {
    (register: $register:tt, $state:expr, $registers:expr, $bus:expr) => {
        match $state.cycle {
            0 => {
                $bus.read_address($registers.pc);
            }
            1 => {
                $bus.read_address(u16::from_be_bytes([0x01, $registers.sp]));
            }
            2 => {
                final_cycle!($state, $registers, $bus);

                $registers.sp = $registers.sp.wrapping_add(1);
                let value = $bus.read_address(u16::from_be_bytes([0x01, $registers.sp]));
                impl_pull_set!(register: $register, $registers, value);
            }
            _ => panic!("invalid cycle: {}", $state.cycle)
        }
    }
}

#[inline]
fn push_pc_msb(registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) {
    let address = u16::from_be_bytes([0x01, registers.sp]);
    bus.write_address(address, (registers.pc >> 8) as u8);
    registers.sp = registers.sp.wrapping_sub(1);
}

#[inline]
fn push_pc_lsb(registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) {
    let address = u16::from_be_bytes([0x01, registers.sp]);
    bus.write_address(address, registers.pc as u8);
    registers.sp = registers.sp.wrapping_sub(1);
}

#[inline]
fn pull_pc_lsb(registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) {
    registers.sp = registers.sp.wrapping_add(1);
    registers.pc = bus
        .read_address(u16::from_be_bytes([0x01, registers.sp]))
        .into();
}

#[inline]
fn pull_pc_msb(registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) {
    registers.sp = registers.sp.wrapping_add(1);
    let pc_msb = bus.read_address(u16::from_be_bytes([0x01, registers.sp]));
    registers.pc |= u16::from(pc_msb) << 8;
}

// JSR
fn jump_to_subroutine(
    state: &mut InstructionState,
    registers: &mut CpuRegisters,
    bus: &mut CpuBus<'_>,
) {
    match state.cycle {
        0 => {
            state.operand_first_byte = fetch_operand!(registers, bus);
        }
        1 => {
            // Spurious stack read
            bus.read_address(u16::from_be_bytes([0x01, registers.sp]));
        }
        2 => {
            push_pc_msb(registers, bus);
        }
        3 => {
            push_pc_lsb(registers, bus);
        }
        4 => {
            final_cycle!(state, registers, bus);

            let address_msb = bus.read_address(registers.pc);
            registers.pc = u16::from_le_bytes([state.operand_first_byte, address_msb]);
        }
        _ => panic!("invalid cycle: {}", state.cycle),
    }
}

// RTS
fn return_from_subroutine(
    state: &mut InstructionState,
    registers: &mut CpuRegisters,
    bus: &mut CpuBus<'_>,
) {
    match state.cycle {
        0 => {
            // Spurious operand read
            bus.read_address(registers.pc);
        }
        1 => {
            // Spurious stack read
            bus.read_address(u16::from_be_bytes([0x01, registers.sp]));
        }
        2 => {
            pull_pc_lsb(registers, bus);
        }
        3 => {
            pull_pc_msb(registers, bus);
        }
        4 => {
            final_cycle!(state, registers, bus);

            // Fetch operand and increment PC, ignore fetch result
            fetch_operand!(registers, bus);
        }
        _ => panic!("invalid cycle: {}", state.cycle),
    }
}

// RTI
fn return_from_interrupt(
    state: &mut InstructionState,
    registers: &mut CpuRegisters,
    bus: &mut CpuBus<'_>,
) {
    match state.cycle {
        0 => {
            // Spurious operand read
            bus.read_address(registers.pc);
        }
        1 => {
            // Spurious stack read
            bus.read_address(u16::from_be_bytes([0x01, registers.sp]));
        }
        2 => {
            registers.sp = registers.sp.wrapping_add(1);
            let value = bus.read_address(u16::from_be_bytes([0x01, registers.sp]));
            registers.status = StatusFlags::from_byte(value);
        }
        3 => {
            pull_pc_lsb(registers, bus);
        }
        4 => {
            final_cycle!(state, registers, bus);

            pull_pc_msb(registers, bus);
        }
        _ => panic!("invalid cycle: {}", state.cycle),
    }
}

#[inline]
fn interrupt_push_status(
    state: &mut InstructionState,
    registers: &mut CpuRegisters,
    bus: &mut CpuBus<'_>,
    read_ctx: StatusReadContext,
) {
    let stack_address = u16::from_be_bytes([0x01, registers.sp]);
    bus.write_address(stack_address, registers.status.to_byte(read_ctx));
    registers.sp = registers.sp.wrapping_sub(1);

    state.interrupt_vector = if bus.interrupt_lines().nmi_triggered() {
        bus.interrupt_lines().clear_nmi_triggered();
        bus::CPU_NMI_VECTOR
    } else {
        bus::CPU_IRQ_VECTOR
    };
}

#[inline]
fn interrupt_pull_pc_lsb(
    state: &InstructionState,
    registers: &mut CpuRegisters,
    bus: &mut CpuBus<'_>,
) {
    registers.pc = bus.read_address(state.interrupt_vector).into();
    registers.status.interrupt_disable = true;
}

#[inline]
fn interrupt_pull_pc_msb(
    state: &InstructionState,
    registers: &mut CpuRegisters,
    bus: &mut CpuBus<'_>,
) {
    let pc_msb = bus.read_address(state.interrupt_vector + 1);
    registers.pc |= u16::from(pc_msb) << 8;
}

// BRK
fn force_interrupt(
    state: &mut InstructionState,
    registers: &mut CpuRegisters,
    bus: &mut CpuBus<'_>,
) {
    match state.cycle {
        0 => {
            fetch_operand!(registers, bus);
        }
        1 => {
            push_pc_msb(registers, bus);
        }
        2 => {
            push_pc_lsb(registers, bus);
        }
        3 => {
            interrupt_push_status(state, registers, bus, StatusReadContext::Brk);
        }
        4 => {
            interrupt_pull_pc_lsb(state, registers, bus);
        }
        5 => {
            final_cycle!(state, registers, bus);

            interrupt_pull_pc_msb(state, registers, bus);
        }
        _ => panic!("invalid cycle: {}", state.cycle),
    }
}

// Hardware interrupt servicing routine
fn interrupt_service_routine(
    state: &mut InstructionState,
    registers: &mut CpuRegisters,
    bus: &mut CpuBus<'_>,
) {
    match state.cycle {
        0 => {
            // Spurious operand read
            bus.read_address(registers.pc);
        }
        1 => {
            push_pc_msb(registers, bus);
        }
        2 => {
            push_pc_lsb(registers, bus);
        }
        3 => {
            interrupt_push_status(
                state,
                registers,
                bus,
                StatusReadContext::HardwareInterruptHandler,
            );
        }
        4 => {
            interrupt_pull_pc_lsb(state, registers, bus);
        }
        5 => {
            final_cycle!(state, registers, bus);

            state.executing_interrupt = false;
            interrupt_pull_pc_msb(state, registers, bus);
        }
        _ => panic!("invalid cycle: {}", state.cycle),
    }
}

fn execute_unofficial_store(
    state: &InstructionState,
    bus: &mut CpuBus<'_>,
    register_value: u8,
    index: u8,
) {
    // This is a buggy instruction that is only implemented because CPU test ROMs test
    // it.
    // This implementation ANDs the X/Y register with the high byte of the address plus 1
    // and then stores that value, but only if the indexing did not overflow.

    let address = u16::from_le_bytes([state.operand_first_byte, state.operand_second_byte])
        .wrapping_add(index.into());
    let (_, overflowed) = state.operand_first_byte.overflowing_add(index);

    let value = register_value & state.operand_second_byte.wrapping_add(1);

    if !overflowed {
        bus.write_address(address, value);
    }
}

// SHX, SHY (unofficial)
macro_rules! unofficial_store {
    (register: $register:ident, index: $index:ident, $state:expr, $registers:expr, $bus:expr) => {
        match $state.cycle {
            0 => {
                $state.operand_first_byte = fetch_operand!($registers, $bus);
            }
            1 => {
                $state.operand_second_byte = fetch_operand!($registers, $bus);
            }
            2 => {
                let address_lsb = $state.operand_first_byte.wrapping_add($registers.$index);
                let address = u16::from_le_bytes([address_lsb, $state.operand_second_byte]);
                $bus.read_address(address);
            }
            3 => {
                final_cycle!($state, $registers, $bus);

                let value = $registers.$register;
                let index = $registers.$index;
                execute_unofficial_store($state, $bus, value, index);
            }
            _ => panic!("invalid cycle: {}", $state.cycle),
        }
    };
}

// AHX, TAS (unofficial) (not actually implemented because these opcodes are very unstable)
fn unimplemented_unofficial_store_absolute_y(
    state: &mut InstructionState,
    registers: &mut CpuRegisters,
    bus: &mut CpuBus<'_>,
) {
    match state.cycle {
        0 => {
            state.operand_first_byte = fetch_operand!(registers, bus);
        }
        1 => {
            state.operand_second_byte = fetch_operand!(registers, bus);
        }
        2 => {
            let address_lsb = state.operand_first_byte.wrapping_add(registers.y);
            let address = u16::from_le_bytes([address_lsb, state.operand_second_byte]);
            state.target_first_byte = bus.read_address(address);
        }
        3 => {
            final_cycle!(state, registers, bus);

            let address_lsb = state.operand_first_byte.wrapping_add(registers.y);
            let address = u16::from_le_bytes([address_lsb, state.operand_second_byte]);
            bus.write_address(address, state.target_first_byte);
        }
        _ => panic!("invalid cycle: {}", state.cycle),
    }
}

// AHX, TAS (unofficial) (not actually implemented because these opcodes are very unstable)
fn unimplemented_unofficial_store_indirect_y(
    state: &mut InstructionState,
    registers: &mut CpuRegisters,
    bus: &mut CpuBus<'_>,
) {
    match state.cycle {
        0 => {
            state.operand_first_byte = fetch_operand!(registers, bus);
        }
        1 => {
            state.target_first_byte = bus.read_address(state.operand_first_byte.into());
        }
        2 => {
            let address = state.operand_first_byte.wrapping_add(1).into();
            state.target_second_byte = bus.read_address(address);
        }
        3 => {
            let address_lsb = state.target_first_byte.wrapping_add(registers.y);
            let address = u16::from_le_bytes([address_lsb, state.target_second_byte]);
            state.indirect_byte = bus.read_address(address);
        }
        4 => {
            final_cycle!(state, registers, bus);

            let address_lsb = state.target_first_byte.wrapping_add(registers.y);
            let address = u16::from_le_bytes([address_lsb, state.target_second_byte]);
            bus.write_address(address, state.indirect_byte);
        }
        _ => panic!("invalid cycle: {}", state.cycle),
    }
}

// ANC (unofficial)
macro_rules! and_with_shift_left {
    ($state:expr, $registers:expr, $bus:expr) => {
        impl_read_instruction!(immediate, $state, $registers, $bus, |operand| {
            // ANC performs an AND and then sets the C flag the way that ASL does
            $registers.accumulator = and($registers.accumulator, operand, &mut $registers.status);
            $registers.status.carry = $registers.accumulator.bit(7);
        })
    };
}

// ALR (unofficial)
macro_rules! and_with_shift_right {
    ($state:expr, $registers:expr, $bus:expr) => {
        impl_read_instruction!(immediate, $state, $registers, $bus, |operand| {
            // ALR simply performs an AND followed by an LSR
            let and_value = and($registers.accumulator, operand, &mut $registers.status);
            $registers.accumulator = logical_shift_right(and_value, &mut $registers.status);
        })
    };
}

// ARR (unofficial)
macro_rules! and_with_rotate_right {
    ($state:expr, $registers:expr, $bus:expr) => {
        impl_read_instruction!(immediate, $state, $registers, $bus, |operand| {
            // ARR is like a mix of AND, ROR, and ADC; the accumulator is set to (A & #imm) rotated,
            // but the flags are set differently from ROR

            let and_value = and($registers.accumulator, operand, &mut StatusFlags::new());
            $registers.accumulator = (and_value >> 1) | (u8::from($registers.status.carry) << 7);

            // The overflow flag is set as if an ADC was performed between the AND and ROR, and
            // the carry flag is set based on what was bit 7 prior to the rotation
            let overflow = $registers.accumulator.bit(6) ^ $registers.accumulator.bit(5);
            $registers
                .status
                .set_negative($registers.accumulator.bit(7))
                .set_overflow(overflow)
                .set_carry($registers.accumulator.bit(6))
                .set_zero($registers.accumulator == 0);
        })
    };
}

// LAX (unofficial)
macro_rules! load_transfer_ax {
    ($addressing_mode:tt, $state:expr, $registers:expr, $bus:expr) => {
        impl_read_instruction!($addressing_mode, $state, $registers, $bus, |operand| {
            // LAX simply performs LDA and LDX simultaneously

            $registers.accumulator = operand;
            $registers.x = operand;

            $registers
                .status
                .set_negative(operand.bit(7))
                .set_zero(operand == 0);
        })
    };
}

// XAA (unofficial)
macro_rules! load_and_x_immediate {
    ($state:expr, $registers:expr, $bus:expr) => {
        impl_read_instruction!(immediate, $state, $registers, $bus, |operand| {
            $registers.accumulator = $registers.x & operand;
            $registers
                .status
                .set_negative($registers.accumulator.bit(7))
                .set_zero($registers.accumulator == 0);
        })
    };
}

// AXS (unofficial)
macro_rules! ax_subtract {
    ($state:expr, $registers:expr, $bus:expr) => {
        impl_read_instruction!(immediate, $state, $registers, $bus, |operand| {
            // AXS sets X to (A&X) - #imm, while ignoring the current carry flag. The flags
            // are set not from the subtraction operation but from a CMP between (A&X) and #imm

            let ax = $registers.accumulator & $registers.x;
            let mut flags = StatusFlags {
                // Set carry to true because SBC inverts the carry flag for borrowing
                carry: true,
                ..StatusFlags::new()
            };
            $registers.x = subtract(ax, operand, &mut flags);

            compare(ax, operand, &mut $registers.status);
        })
    };
}

// LAS (unofficial)
macro_rules! load_and_stack {
    ($state:expr, $registers:expr, $bus:expr) => {
        impl_read_instruction!(absolute_y, $state, $registers, $bus, |operand| {
            // LAS sets A, X, and S to (value & S)

            let new_value = operand & $registers.sp;

            $registers.accumulator = new_value;
            $registers.x = new_value;
            $registers.sp = new_value;

            $registers
                .status
                .set_negative(new_value.bit(7))
                .set_zero(new_value == 0);
        })
    };
}

// unofficial NOPs
macro_rules! multi_byte_noop {
    ($addressing_mode:tt, $state:expr, $registers:expr, $bus:expr) => {
        impl_read_instruction!($addressing_mode, $state, $registers, $bus, |_operand| {})
    };
}

pub fn execute_cycle(
    state: &mut InstructionState,
    registers: &mut CpuRegisters,
    bus: &mut CpuBus<'_>,
) {
    if state.executing_interrupt {
        interrupt_service_routine(state, registers, bus);
        state.cycle += 1;
        return;
    }

    match state.opcode {
        0x00 => force_interrupt(state, registers, bus),
        0x01 => or!(indirect_x, state, registers, bus),
        0x03 => shift_left_or!(indirect_x, state, registers, bus),
        0x04 | 0x44 | 0x64 => multi_byte_noop!(zero_page, state, registers, bus),
        0x05 => or!(zero_page, state, registers, bus),
        0x06 => shift_left!(zero_page, state, registers, bus),
        0x07 => shift_left_or!(zero_page, state, registers, bus),
        0x08 => push_stack!(register: p, state, registers, bus),
        0x09 => or!(immediate, state, registers, bus),
        0x0A => shift_left!(accumulator, state, registers, bus),
        0x0B | 0x2B => and_with_shift_left!(state, registers, bus),
        0x0C => multi_byte_noop!(absolute, state, registers, bus),
        0x0D => or!(absolute, state, registers, bus),
        0x0E => shift_left!(absolute, state, registers, bus),
        0x0F => shift_left_or!(absolute, state, registers, bus),
        0x10 => branch!(negative == false, state, registers, bus),
        0x11 => or!(indirect_y, state, registers, bus),
        0x13 => shift_left_or!(indirect_y, state, registers, bus),
        0x14 | 0x34 | 0x54 | 0x74 | 0xD4 | 0xF4 => {
            multi_byte_noop!(zero_page_x, state, registers, bus);
        }
        0x15 => or!(zero_page_x, state, registers, bus),
        0x16 => shift_left!(zero_page_x, state, registers, bus),
        0x17 => shift_left_or!(zero_page_x, state, registers, bus),
        0x18 => set_status_flag!(carry, false, state, registers, bus),
        0x19 => or!(absolute_y, state, registers, bus),
        0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xEA | 0xFA => noop!(state, registers, bus),
        0x1B => shift_left_or!(absolute_y, state, registers, bus),
        0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC => {
            multi_byte_noop!(absolute_x, state, registers, bus);
        }
        0x1D => or!(absolute_x, state, registers, bus),
        0x1E => shift_left!(absolute_x, state, registers, bus),
        0x1F => shift_left_or!(absolute_x, state, registers, bus),
        0x20 => jump_to_subroutine(state, registers, bus),
        0x21 => and!(indirect_x, state, registers, bus),
        0x23 => rotate_left_and!(indirect_x, state, registers, bus),
        0x24 => bit_test!(zero_page, state, registers, bus),
        0x25 => and!(zero_page, state, registers, bus),
        0x26 => rotate_left!(zero_page, state, registers, bus),
        0x27 => rotate_left_and!(zero_page, state, registers, bus),
        0x28 => pull_stack!(register: p, state, registers, bus),
        0x29 => and!(immediate, state, registers, bus),
        0x2A => rotate_left!(accumulator, state, registers, bus),
        0x2C => bit_test!(absolute, state, registers, bus),
        0x2D => and!(absolute, state, registers, bus),
        0x2E => rotate_left!(absolute, state, registers, bus),
        0x2F => rotate_left_and!(absolute, state, registers, bus),
        0x30 => branch!(negative == true, state, registers, bus),
        0x31 => and!(indirect_y, state, registers, bus),
        0x33 => rotate_left_and!(indirect_y, state, registers, bus),
        0x35 => and!(zero_page_x, state, registers, bus),
        0x36 => rotate_left!(zero_page_x, state, registers, bus),
        0x37 => rotate_left_and!(zero_page_x, state, registers, bus),
        0x38 => set_status_flag!(carry, true, state, registers, bus),
        0x39 => and!(absolute_y, state, registers, bus),
        0x3B => rotate_left_and!(absolute_y, state, registers, bus),
        0x3D => and!(absolute_x, state, registers, bus),
        0x3E => rotate_left!(absolute_x, state, registers, bus),
        0x3F => rotate_left_and!(absolute_x, state, registers, bus),
        0x40 => return_from_interrupt(state, registers, bus),
        0x41 => xor!(indirect_x, state, registers, bus),
        0x43 => shift_right_xor!(indirect_x, state, registers, bus),
        0x45 => xor!(zero_page, state, registers, bus),
        0x46 => logical_shift_right!(zero_page, state, registers, bus),
        0x47 => shift_right_xor!(zero_page, state, registers, bus),
        0x48 => push_stack!(register: accumulator, state, registers, bus),
        0x49 => xor!(immediate, state, registers, bus),
        0x4A => logical_shift_right!(accumulator, state, registers, bus),
        0x4B => and_with_shift_right!(state, registers, bus),
        0x4C => jump_absolute(state, registers, bus),
        0x4D => xor!(absolute, state, registers, bus),
        0x4E => logical_shift_right!(absolute, state, registers, bus),
        0x4F => shift_right_xor!(absolute, state, registers, bus),
        0x50 => branch!(overflow == false, state, registers, bus),
        0x51 => xor!(indirect_y, state, registers, bus),
        0x53 => shift_right_xor!(indirect_y, state, registers, bus),
        0x55 => xor!(zero_page_x, state, registers, bus),
        0x56 => logical_shift_right!(zero_page_x, state, registers, bus),
        0x57 => shift_right_xor!(zero_page_x, state, registers, bus),
        0x58 => set_status_flag!(interrupt_disable, false, state, registers, bus),
        0x59 => xor!(absolute_y, state, registers, bus),
        0x5B => shift_right_xor!(absolute_y, state, registers, bus),
        0x5D => xor!(absolute_x, state, registers, bus),
        0x5E => logical_shift_right!(absolute_x, state, registers, bus),
        0x5F => shift_right_xor!(absolute_x, state, registers, bus),
        0x60 => return_from_subroutine(state, registers, bus),
        0x61 => add_with_carry!(indirect_x, state, registers, bus),
        0x63 => rotate_right_add!(indirect_x, state, registers, bus),
        0x65 => add_with_carry!(zero_page, state, registers, bus),
        0x66 => rotate_right!(zero_page, state, registers, bus),
        0x67 => rotate_right_add!(zero_page, state, registers, bus),
        0x68 => pull_stack!(register: accumulator, state, registers, bus),
        0x69 => add_with_carry!(immediate, state, registers, bus),
        0x6A => rotate_right!(accumulator, state, registers, bus),
        0x6B => and_with_rotate_right!(state, registers, bus),
        0x6C => jump_indirect(state, registers, bus),
        0x6D => add_with_carry!(absolute, state, registers, bus),
        0x6E => rotate_right!(absolute, state, registers, bus),
        0x6F => rotate_right_add!(absolute, state, registers, bus),
        0x70 => branch!(overflow == true, state, registers, bus),
        0x71 => add_with_carry!(indirect_y, state, registers, bus),
        0x73 => rotate_right_add!(indirect_y, state, registers, bus),
        0x75 => add_with_carry!(zero_page_x, state, registers, bus),
        0x76 => rotate_right!(zero_page_x, state, registers, bus),
        0x77 => rotate_right_add!(zero_page_x, state, registers, bus),
        0x78 => set_status_flag!(interrupt_disable, true, state, registers, bus),
        0x79 => add_with_carry!(absolute_y, state, registers, bus),
        0x7B => rotate_right_add!(absolute_y, state, registers, bus),
        0x7D => add_with_carry!(absolute_x, state, registers, bus),
        0x7E => rotate_right!(absolute_x, state, registers, bus),
        0x7F => rotate_right_add!(absolute_x, state, registers, bus),
        0x80 | 0x82 | 0x89 | 0xC2 | 0xE2 => multi_byte_noop!(immediate, state, registers, bus),
        0x81 => store!(indirect_x, register: accumulator, state, registers, bus),
        0x83 => store!(indirect_x, register: ax, state, registers, bus),
        0x84 => store!(zero_page, register: y, state, registers, bus),
        0x85 => store!(zero_page, register: accumulator, state, registers, bus),
        0x86 => store!(zero_page, register: x, state, registers, bus),
        0x87 => store!(zero_page, register: ax, state, registers, bus),
        0x88 => decrement_register!(register: y, state, registers, bus),
        0x8A => register_transfer!(to: accumulator, from: x, state, registers, bus),
        0x8B => load_and_x_immediate!(state, registers, bus),
        0x8C => store!(absolute, register: y, state, registers, bus),
        0x8D => store!(absolute, register: accumulator, state, registers, bus),
        0x8E => store!(absolute, register: x, state, registers, bus),
        0x8F => store!(absolute, register: ax, state, registers, bus),
        0x90 => branch!(carry == false, state, registers, bus),
        0x91 => store!(indirect_y, register: accumulator, state, registers, bus),
        0x93 => unimplemented_unofficial_store_indirect_y(state, registers, bus),
        0x94 => store!(zero_page_x, register: y, state, registers, bus),
        0x95 => store!(zero_page_x, register: accumulator, state, registers, bus),
        0x96 => store!(zero_page_y, register: x, state, registers, bus),
        0x97 => store!(zero_page_y, register: ax, state, registers, bus),
        0x98 => register_transfer!(to: accumulator, from: y, state, registers, bus),
        0x99 => store!(absolute_y, register: accumulator, state, registers, bus),
        0x9A => register_transfer!(to: sp, from: x, state, registers, bus),
        0x9B | 0x9F => unimplemented_unofficial_store_absolute_y(state, registers, bus),
        0x9C => unofficial_store!(register: y, index: x, state, registers, bus),
        0x9D => store!(absolute_x, register: accumulator, state, registers, bus),
        0x9E => unofficial_store!(register: x, index: y, state, registers, bus),
        0xA0 => load!(immediate, register: y, state, registers, bus),
        0xA1 => load!(indirect_x, register: accumulator, state, registers, bus),
        0xA2 => load!(immediate, register: x, state, registers, bus),
        0xA3 => load_transfer_ax!(indirect_x, state, registers, bus),
        0xA4 => load!(zero_page, register: y, state, registers, bus),
        0xA5 => load!(zero_page, register: accumulator, state, registers, bus),
        0xA6 => load!(zero_page, register: x, state, registers, bus),
        0xA7 => load_transfer_ax!(zero_page, state, registers, bus),
        0xA8 => register_transfer!(to: y, from: accumulator, state, registers, bus),
        0xA9 => load!(immediate, register: accumulator, state, registers, bus),
        0xAA => register_transfer!(to: x, from: accumulator, state, registers, bus),
        0xAB => load_transfer_ax!(immediate, state, registers, bus),
        0xAC => load!(absolute, register: y, state, registers, bus),
        0xAD => load!(absolute, register: accumulator, state, registers, bus),
        0xAE => load!(absolute, register: x, state, registers, bus),
        0xAF => load_transfer_ax!(absolute, state, registers, bus),
        0xB0 => branch!(carry == true, state, registers, bus),
        0xB1 => load!(indirect_y, register: accumulator, state, registers, bus),
        0xB3 => load_transfer_ax!(indirect_y, state, registers, bus),
        0xB4 => load!(zero_page_x, register: y, state, registers, bus),
        0xB5 => load!(zero_page_x, register: accumulator, state, registers, bus),
        0xB6 => load!(zero_page_y, register: x, state, registers, bus),
        0xB7 => load_transfer_ax!(zero_page_y, state, registers, bus),
        0xB8 => set_status_flag!(overflow, false, state, registers, bus),
        0xB9 => load!(absolute_y, register: accumulator, state, registers, bus),
        0xBA => register_transfer!(to: x, from: sp, state, registers, bus),
        0xBB => load_and_stack!(state, registers, bus),
        0xBC => load!(absolute_x, register: y, state, registers, bus),
        0xBD => load!(absolute_x, register: accumulator, state, registers, bus),
        0xBE => load!(absolute_y, register: x, state, registers, bus),
        0xBF => load_transfer_ax!(absolute_y, state, registers, bus),
        0xC0 => compare!(immediate, register: y, state, registers, bus),
        0xC1 => compare!(indirect_x, register: accumulator, state, registers, bus),
        0xC3 => decrement_compare!(indirect_x, state, registers, bus),
        0xC4 => compare!(zero_page, register: y, state, registers, bus),
        0xC5 => compare!(zero_page, register: accumulator, state, registers, bus),
        0xC6 => decrement!(zero_page, state, registers, bus),
        0xC7 => decrement_compare!(zero_page, state, registers, bus),
        0xC8 => increment_register!(register: y, state, registers, bus),
        0xC9 => compare!(immediate, register: accumulator, state, registers, bus),
        0xCA => decrement_register!(register: x, state, registers, bus),
        0xCB => ax_subtract!(state, registers, bus),
        0xCC => compare!(absolute, register: y, state, registers, bus),
        0xCD => compare!(absolute, register: accumulator, state, registers, bus),
        0xCE => decrement!(absolute, state, registers, bus),
        0xCF => decrement_compare!(absolute, state, registers, bus),
        0xD0 => branch!(zero == false, state, registers, bus),
        0xD1 => compare!(indirect_y, register: accumulator, state, registers, bus),
        0xD3 => decrement_compare!(indirect_y, state, registers, bus),
        0xD5 => compare!(zero_page_x, register: accumulator, state, registers, bus),
        0xD6 => decrement!(zero_page_x, state, registers, bus),
        0xD7 => decrement_compare!(zero_page_x, state, registers, bus),
        0xD8 => set_status_flag!(decimal, false, state, registers, bus),
        0xD9 => compare!(absolute_y, register: accumulator, state, registers, bus),
        0xDB => decrement_compare!(absolute_y, state, registers, bus),
        0xDD => compare!(absolute_x, register: accumulator, state, registers, bus),
        0xDE => decrement!(absolute_x, state, registers, bus),
        0xDF => decrement_compare!(absolute_x, state, registers, bus),
        0xE0 => compare!(immediate, register: x, state, registers, bus),
        0xE1 => subtract_with_carry!(indirect_x, state, registers, bus),
        0xE3 => increment_subtract!(indirect_x, state, registers, bus),
        0xE4 => compare!(zero_page, register: x, state, registers, bus),
        0xE5 => subtract_with_carry!(zero_page, state, registers, bus),
        0xE6 => increment!(zero_page, state, registers, bus),
        0xE7 => increment_subtract!(zero_page, state, registers, bus),
        0xE8 => increment_register!(register: x, state, registers, bus),
        0xE9 | 0xEB => subtract_with_carry!(immediate, state, registers, bus),
        0xEC => compare!(absolute, register: x, state, registers, bus),
        0xED => subtract_with_carry!(absolute, state, registers, bus),
        0xEE => increment!(absolute, state, registers, bus),
        0xEF => increment_subtract!(absolute, state, registers, bus),
        0xF0 => branch!(zero == true, state, registers, bus),
        0xF1 => subtract_with_carry!(indirect_y, state, registers, bus),
        0xF3 => increment_subtract!(indirect_y, state, registers, bus),
        0xF5 => subtract_with_carry!(zero_page_x, state, registers, bus),
        0xF6 => increment!(zero_page_x, state, registers, bus),
        0xF7 => increment_subtract!(zero_page_x, state, registers, bus),
        0xF8 => set_status_flag!(decimal, true, state, registers, bus),
        0xF9 => subtract_with_carry!(absolute_y, state, registers, bus),
        0xFB => increment_subtract!(absolute_y, state, registers, bus),
        0xFD => subtract_with_carry!(absolute_x, state, registers, bus),
        0xFE => increment!(absolute_x, state, registers, bus),
        0xFF => increment_subtract!(absolute_x, state, registers, bus),
        0x02 | 0x12 | 0x22 | 0x32 | 0x42 | 0x52 | 0x62 | 0x72 | 0x92 | 0xB2 | 0xD2 | 0xF2 => {
            // KIL unofficial opcodes; executing any of these halts the CPU until a reset or power cycle
            state.terminated = true;
        }
    }

    state.cycle += 1;
}
