use crate::bus;
use crate::bus::{Bus, CpuBus};
use crate::cpu::instructions::{BranchState, ExecutingInstruction};

mod instructions;

#[derive(Debug)]
pub struct CpuRegisters {
    pub accumulator: u8,
    pub x: u8,
    pub y: u8,
    pub status: u8,
    pub pc: u16,
    pub sp: u8,
}

impl CpuRegisters {
    pub fn new(bus: &mut CpuBus<'_>) -> Self {
        let pc_lsb = bus.read_address(bus::CPU_RESET_VECTOR);
        let pc_msb = bus.read_address(bus::CPU_RESET_VECTOR + 1);
        let pc = u16::from_le_bytes([pc_lsb, pc_msb]);

        Self {
            accumulator: 0,
            x: 0,
            y: 0,
            status: 0x34,
            pc,
            sp: 0xFD,
        }
    }

    fn status_flags(&mut self) -> StatusFlags<'_> {
        StatusFlags(&mut self.status)
    }
}

pub struct StatusFlags<'a>(&'a mut u8);

impl<'a> StatusFlags<'a> {
    fn negative(&self) -> bool {
        *self.0 & 0x80 != 0
    }

    fn set_negative(&mut self, value: bool) -> &mut Self {
        if value {
            *self.0 |= 0x80;
        } else {
            *self.0 &= !0x80;
        }
        self
    }

    fn overflow(&self) -> bool {
        *self.0 & 0x40 != 0
    }

    fn set_overflow(&mut self, value: bool) -> &mut Self {
        if value {
            *self.0 |= 0x40;
        } else {
            *self.0 &= !0x40;
        }
        self
    }

    fn set_decimal(&mut self, value: bool) -> &mut Self {
        if value {
            *self.0 |= 0x08;
        } else {
            *self.0 &= !0x08;
        }
        self
    }

    fn interrupt_disable(&self) -> bool {
        *self.0 & 0x04 != 0
    }

    fn set_interrupt_disable(&mut self, value: bool) -> &mut Self {
        if value {
            *self.0 |= 0x04;
        } else {
            *self.0 &= !0x04;
        }
        self
    }

    fn zero(&self) -> bool {
        *self.0 & 0x02 != 0
    }

    fn set_zero(&mut self, value: bool) -> &mut Self {
        if value {
            *self.0 |= 0x02;
        } else {
            *self.0 &= !0x02;
        }
        self
    }

    fn carry(&self) -> bool {
        *self.0 & 0x01 != 0
    }

    fn set_carry(&mut self, value: bool) -> &mut Self {
        if value {
            *self.0 |= 0x01;
        } else {
            *self.0 &= !0x01;
        }
        self
    }
}

enum HandlingInterruptState {
    Cycle0,
    Cycle1,
    Cycle2,
    Cycle3,
    Cycle4 { interrupt_vector: u16 },
    Cycle5 { interrupt_vector: u16 },
    Cycle6 { interrupt_vector: u16, pc_lsb: u8 },
}

impl HandlingInterruptState {
    fn next(self, registers: &mut CpuRegisters, bus: &mut CpuBus<'_>) -> Option<Self> {
        match self {
            Self::Cycle0 => {
                bus.read_address(registers.pc);
                Some(Self::Cycle1)
            }
            Self::Cycle1 => {
                bus.read_address(registers.pc);
                Some(Self::Cycle2)
            }
            Self::Cycle2 => {
                let stack_address = bus::CPU_STACK_START | u16::from(registers.sp);
                bus.write_address(stack_address, (registers.pc >> 8) as u8);
                registers.sp = registers.sp.wrapping_sub(1);

                Some(Self::Cycle3)
            }
            Self::Cycle3 => {
                let stack_address = bus::CPU_STACK_START | u16::from(registers.sp);
                bus.write_address(stack_address, (registers.pc & 0x00FF) as u8);
                registers.sp = registers.sp.wrapping_sub(1);

                let interrupt_vector = if bus.interrupt_lines().nmi_triggered() {
                    bus.interrupt_lines().clear_nmi_triggered();
                    bus::CPU_NMI_VECTOR
                } else {
                    bus::CPU_IRQ_VECTOR
                };

                Some(Self::Cycle4 { interrupt_vector })
            }
            Self::Cycle4 { interrupt_vector } => {
                let stack_address = bus::CPU_STACK_START | u16::from(registers.sp);
                // Write P register with B flag cleared
                bus.write_address(stack_address, (registers.status | 0x20) & 0xEF);
                registers.sp = registers.sp.wrapping_sub(1);

                Some(Self::Cycle5 { interrupt_vector })
            }
            Self::Cycle5 { interrupt_vector } => {
                let pc_lsb = bus.read_address(interrupt_vector);

                registers.status_flags().set_interrupt_disable(true);

                Some(Self::Cycle6 {
                    interrupt_vector,
                    pc_lsb,
                })
            }
            Self::Cycle6 {
                interrupt_vector,
                pc_lsb,
            } => {
                let pc_msb = bus.read_address(interrupt_vector + 1);

                registers.pc = u16::from_le_bytes([pc_lsb, pc_msb]);

                None
            }
        }
    }
}

enum State {
    InstructionStart,
    InstructionExecuting {
        executing_instruction: ExecutingInstruction,
        interrupt_pending: bool,
    },
    HandlingInterrupt(HandlingInterruptState),
}

pub struct CpuState {
    registers: CpuRegisters,
    state: State,
}

impl CpuState {
    pub fn new(registers: CpuRegisters) -> Self {
        Self {
            registers,
            state: State::InstructionStart,
        }
    }
}

pub fn tick(state: &mut CpuState, bus: &mut Bus) {
    let new_state = match std::mem::replace(&mut state.state, State::InstructionStart) {
        State::InstructionStart => {
            let executing_instruction =
                ExecutingInstruction::fetch(&mut state.registers, &mut bus.cpu());
            State::InstructionExecuting {
                executing_instruction,
                interrupt_pending: false,
            }
        }
        State::InstructionExecuting {
            executing_instruction,
            interrupt_pending,
        } => {
            // Read I flag before executing
            let irq_enabled = !state.registers.status_flags().interrupt_disable();

            let interrupt_pending = match &executing_instruction {
                // Special case branch instructions because they poll interrupts weirdly
                ExecutingInstruction::Branch(branch_state) => match branch_state {
                    BranchState::Cycle1(..) | BranchState::Cycle3 => {
                        interrupt_pending || is_interrupt_pending(irq_enabled, bus)
                    }
                    BranchState::Cycle2 { .. } => interrupt_pending,
                },
                // Non-branch instructions always poll on the last cycle so ignore previous value
                _ => is_interrupt_pending(irq_enabled, bus),
            };

            match executing_instruction.next(&mut state.registers, &mut bus.cpu()) {
                Some(executing_instruction) => State::InstructionExecuting {
                    executing_instruction,
                    interrupt_pending,
                },
                None => {
                    if interrupt_pending {
                        State::HandlingInterrupt(HandlingInterruptState::Cycle0)
                    } else {
                        State::InstructionStart
                    }
                }
            }
        }
        State::HandlingInterrupt(interrupt_state) => {
            match interrupt_state.next(&mut state.registers, &mut bus.cpu()) {
                Some(next_state) => State::HandlingInterrupt(next_state),
                None => State::InstructionStart,
            }
        }
    };
    state.state = new_state;
}

fn is_interrupt_pending(irq_enabled: bool, bus: &mut Bus) -> bool {
    let interrupt_lines = bus.interrupt_lines();
    interrupt_lines.nmi_triggered() || (irq_enabled && interrupt_lines.irq_triggered())
}
