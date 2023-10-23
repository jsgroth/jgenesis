#[allow(clippy::wildcard_imports)]
use super::*;
use crate::core::InterruptType;

// JMP: Jump
pub(crate) fn jmp_absolute<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            final_cycle(cpu, bus);

            let pc_msb = fetch_operand(cpu, bus);
            cpu.registers.pc = u16::from_le_bytes([cpu.state.t0, pc_msb]);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn jmp_absolute_long<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            cpu.state.t1 = fetch_operand(cpu, bus);
        }
        3 => {
            final_cycle(cpu, bus);

            cpu.registers.pbr = fetch_operand(cpu, bus);

            cpu.registers.pc = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn jmp_indirect<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            cpu.state.t1 = fetch_operand(cpu, bus);
        }
        3 => {
            let address = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
            cpu.state.t2 = bus.read(address.into());
        }
        4 => {
            final_cycle(cpu, bus);

            let address = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
            let pc_msb = bus.read(address.wrapping_add(1).into());
            cpu.registers.pc = u16::from_le_bytes([cpu.state.t2, pc_msb]);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn jmp_indexed_indirect<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
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
            let bank_addr =
                u16::from_le_bytes([cpu.state.t0, cpu.state.t1]).wrapping_add(cpu.registers.x);
            let address = u24_address(cpu.registers.pbr, bank_addr);
            cpu.state.t2 = bus.read(address);
        }
        5 => {
            final_cycle(cpu, bus);

            let bank_addr = u16::from_le_bytes([cpu.state.t0, cpu.state.t1])
                .wrapping_add(cpu.registers.x)
                .wrapping_add(1);
            let address = u24_address(cpu.registers.pbr, bank_addr);
            let pc_msb = bus.read(address);
            cpu.registers.pc = u16::from_le_bytes([cpu.state.t2, pc_msb]);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn jmp_indirect_long<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            cpu.state.t1 = fetch_operand(cpu, bus);
        }
        3 => {
            let address = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
            cpu.state.t2 = bus.read(address.into());
        }
        4 => {
            let address = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
            cpu.state.t3 = bus.read(address.wrapping_add(1).into());
        }
        5 => {
            final_cycle(cpu, bus);

            let address = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
            cpu.registers.pbr = bus.read(address.wrapping_add(2).into());

            cpu.registers.pc = u16::from_le_bytes([cpu.state.t2, cpu.state.t3]);
        }
        _ => invalid_cycle!(cpu),
    }
}

// JSR: Jump to subroutine
pub(crate) fn jsr_absolute<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
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
            let push_pc = cpu.registers.pc.wrapping_sub(1);
            bus.write(cpu.registers.s.into(), (push_pc >> 8) as u8);
            cpu.registers.s = cpu.registers.s.wrapping_sub(1);

            if cpu.registers.emulation_mode {
                ensure_page_1_stack(&mut cpu.registers);
            }
        }
        5 => {
            final_cycle(cpu, bus);

            let push_pc = cpu.registers.pc.wrapping_sub(1);
            bus.write(cpu.registers.s.into(), push_pc as u8);
            cpu.registers.s = cpu.registers.s.wrapping_sub(1);

            cpu.registers.pc = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);

            if cpu.registers.emulation_mode {
                ensure_page_1_stack(&mut cpu.registers);
            }
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn jsr_indirect_indexed<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            bus.write(cpu.registers.s.into(), (cpu.registers.pc >> 8) as u8);
            cpu.registers.s = cpu.registers.s.wrapping_sub(1);

            if cpu.registers.emulation_mode {
                ensure_page_1_stack(&mut cpu.registers);
            }
        }
        3 => {
            bus.write(cpu.registers.s.into(), cpu.registers.pc as u8);
            cpu.registers.s = cpu.registers.s.wrapping_sub(1);

            if cpu.registers.emulation_mode {
                ensure_page_1_stack(&mut cpu.registers);
            }
        }
        4 => {
            cpu.state.t1 = fetch_operand(cpu, bus);
        }
        5 => {
            bus.idle();
        }
        6 => {
            let bank_addr =
                u16::from_le_bytes([cpu.state.t0, cpu.state.t1]).wrapping_add(cpu.registers.x);
            let address = u24_address(cpu.registers.pbr, bank_addr);
            cpu.state.t2 = bus.read(address);
        }
        7 => {
            final_cycle(cpu, bus);

            let bank_addr = u16::from_le_bytes([cpu.state.t0, cpu.state.t1])
                .wrapping_add(cpu.registers.x)
                .wrapping_add(1);
            let address = u24_address(cpu.registers.pbr, bank_addr);
            let pc_msb = bus.read(address);
            cpu.registers.pc = u16::from_le_bytes([cpu.state.t2, pc_msb]);
        }
        _ => invalid_cycle!(cpu),
    }
}

// JSL: Jump to subroutine long
pub(crate) fn jsl<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            cpu.state.t1 = fetch_operand(cpu, bus);
        }
        3 => {
            bus.write(cpu.registers.s.into(), cpu.registers.pbr);
            cpu.registers.s = cpu.registers.s.wrapping_sub(1);
        }
        4 => {
            bus.idle();
        }
        5 => {
            cpu.registers.pbr = fetch_operand(cpu, bus);
        }
        6 => {
            let push_pc = cpu.registers.pc.wrapping_sub(1);
            bus.write(cpu.registers.s.into(), (push_pc >> 8) as u8);
            cpu.registers.s = cpu.registers.s.wrapping_sub(1);
        }
        7 => {
            final_cycle(cpu, bus);

            let push_pc = cpu.registers.pc.wrapping_sub(1);
            bus.write(cpu.registers.s.into(), push_pc as u8);
            cpu.registers.s = cpu.registers.s.wrapping_sub(1);

            cpu.registers.pc = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);

            if cpu.registers.emulation_mode {
                ensure_page_1_stack(&mut cpu.registers);
            }
        }
        _ => invalid_cycle!(cpu),
    }
}

macro_rules! impl_branch {
    ($name:ident $(, $flag:ident == $value:expr)?) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    poll_interrupt_lines(cpu, bus);

                    cpu.state.t0 = fetch_operand(cpu, bus);

                    // Check whether to take branch
                    $(
                        if cpu.registers.p.$flag != $value {
                            cpu.state.cycle = 0;
                        }
                    )?
                }
                2 => {
                    poll_interrupt_lines(cpu, bus);

                    bus.idle();

                    let offset = cpu.state.t0 as i8 as u16;
                    let original_page = cpu.registers.pc & 0xFF00;
                    cpu.registers.pc = cpu.registers.pc.wrapping_add(offset);

                    // In emulation mode, crossing a page adds an extra cycle
                    if !cpu.registers.emulation_mode || original_page == cpu.registers.pc & 0xFF00 {
                        cpu.state.cycle = 0;
                    }
                }
                3 => {
                    final_cycle(cpu, bus);

                    // Penalty cycle for page crossing in emulation mode
                    bus.idle();
                }
                _ => invalid_cycle!(cpu)
            }
        }
    };
}

// BCC: Branch if carry clear
impl_branch!(bcc, carry == false);

// BCS: Branch if carry set
impl_branch!(bcs, carry == true);

// BEQ: Branch if equal
impl_branch!(beq, zero == true);

// BMI: Branch if minus
impl_branch!(bmi, negative == true);

// BNE: Branch if not equal
impl_branch!(bne, zero == false);

// BPL: Branch if plus
impl_branch!(bpl, negative == false);

// BRA: Branch always
impl_branch!(bra);

// BVC: Branch if overflow clear
impl_branch!(bvc, overflow == false);

// BVS: Branch if overflow set
impl_branch!(bvs, overflow == true);

// BRL: Branch long
pub(crate) fn brl<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            cpu.state.t1 = fetch_operand(cpu, bus);
        }
        3 => {
            final_cycle(cpu, bus);

            bus.idle();

            let offset = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
            cpu.registers.pc = cpu.registers.pc.wrapping_add(offset);
        }
        _ => invalid_cycle!(cpu),
    }
}

// BRK + COP + hardware interrupt handler (NMI + IRQ)
// BRK: Breakpoint software interrupt
// COP: Coprocessor software interrupt
pub(crate) fn handle_interrupt<B: BusInterface>(
    cpu: &mut Wdc65816,
    bus: &mut B,
    interrupt: InterruptType,
) {
    match cpu.state.cycle {
        0 => {
            // For hardware interrupts only; read operand but don't increment PC
            bus.read(u24_address(cpu.registers.pbr, cpu.registers.pc));
        }
        1 => {
            if interrupt.is_software() {
                fetch_operand(cpu, bus);
            } else {
                // Read operand but don't increment PC
                bus.read(u24_address(cpu.registers.pbr, cpu.registers.pc));
            }

            if cpu.registers.emulation_mode {
                // Emulation mode skips pushing PBR
                cpu.state.cycle += 1;
            }
        }
        2 => {
            bus.write(cpu.registers.s.into(), cpu.registers.pbr);
            cpu.registers.s = cpu.registers.s.wrapping_sub(1);
        }
        3 => {
            bus.write(cpu.registers.s.into(), (cpu.registers.pc >> 8) as u8);
            cpu.registers.s = cpu.registers.s.wrapping_sub(1);

            if cpu.registers.emulation_mode {
                ensure_page_1_stack(&mut cpu.registers);
            }
        }
        4 => {
            bus.write(cpu.registers.s.into(), cpu.registers.pc as u8);
            cpu.registers.s = cpu.registers.s.wrapping_sub(1);

            if cpu.registers.emulation_mode {
                ensure_page_1_stack(&mut cpu.registers);
            }
        }
        5 => {
            let p_mask = if cpu.registers.emulation_mode && !interrupt.is_software() {
                // Mask out the x flag, which emulates the 6502's b flag in emulation mode
                !0x10
            } else {
                !0
            };

            bus.write(cpu.registers.s.into(), u8::from(cpu.registers.p) & p_mask);
            cpu.registers.s = cpu.registers.s.wrapping_sub(1);

            cpu.registers.p.irq_disabled = true;
            cpu.registers.p.decimal_mode = false;

            if cpu.registers.emulation_mode {
                ensure_page_1_stack(&mut cpu.registers);
            }
        }
        6 => {
            let vector = if cpu.registers.emulation_mode {
                interrupt.emulation_vector()
            } else {
                interrupt.native_vector()
            };
            cpu.state.t0 = bus.read(vector.into());
        }
        7 => {
            final_cycle(cpu, bus);

            let vector = if cpu.registers.emulation_mode {
                interrupt.emulation_vector()
            } else {
                interrupt.native_vector()
            };
            let pc_msb = bus.read(vector.wrapping_add(1).into());
            cpu.registers.pc = u16::from_le_bytes([cpu.state.t0, pc_msb]);
            cpu.registers.pbr = 0;
        }
        _ => invalid_cycle!(cpu),
    }
}

// RTS: Return from subroutine
pub(crate) fn rts<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 | 2 => {
            bus.idle();
        }
        3 => {
            cpu.registers.s = cpu.registers.s.wrapping_add(1);

            if cpu.registers.emulation_mode {
                ensure_page_1_stack(&mut cpu.registers);
            }

            cpu.state.t0 = bus.read(cpu.registers.s.into());
        }
        4 => {
            cpu.registers.s = cpu.registers.s.wrapping_add(1);

            if cpu.registers.emulation_mode {
                ensure_page_1_stack(&mut cpu.registers);
            }

            cpu.state.t1 = bus.read(cpu.registers.s.into());
        }
        5 => {
            final_cycle(cpu, bus);

            bus.idle();

            cpu.registers.pc = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]).wrapping_add(1);
        }
        _ => invalid_cycle!(cpu),
    }
}

// RTL: Return from subroutine long
pub(crate) fn rtl<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 | 2 => {
            bus.idle();
        }
        3 => {
            cpu.registers.s = cpu.registers.s.wrapping_add(1);
            cpu.state.t0 = bus.read(cpu.registers.s.into());
        }
        4 => {
            cpu.registers.s = cpu.registers.s.wrapping_add(1);
            cpu.state.t1 = bus.read(cpu.registers.s.into());
        }
        5 => {
            final_cycle(cpu, bus);

            cpu.registers.s = cpu.registers.s.wrapping_add(1);
            cpu.registers.pbr = bus.read(cpu.registers.s.into());

            cpu.registers.pc = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]).wrapping_add(1);

            if cpu.registers.emulation_mode {
                ensure_page_1_stack(&mut cpu.registers);
            }
        }
        _ => invalid_cycle!(cpu),
    }
}

// RTI: Return from interrupt
pub(crate) fn rti<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 | 2 => {
            bus.idle();
        }
        3 => {
            cpu.registers.s = cpu.registers.s.wrapping_add(1);

            if cpu.registers.emulation_mode {
                ensure_page_1_stack(&mut cpu.registers);
            }

            let p = bus.read(cpu.registers.s.into());
            if cpu.registers.emulation_mode {
                // m and x bits are forced to 1 in emulation mode
                cpu.registers.p = (0x30 | p).into();
            } else {
                cpu.registers.p = p.into();

                // Immediately truncate X and Y if x bit was set
                if cpu.registers.p.index_size == SizeBits::Eight {
                    cpu.registers.x &= 0x00FF;
                    cpu.registers.y &= 0x00FF;
                }
            }
        }
        4 => {
            cpu.registers.s = cpu.registers.s.wrapping_add(1);

            if cpu.registers.emulation_mode {
                ensure_page_1_stack(&mut cpu.registers);
            }

            cpu.state.t0 = bus.read(cpu.registers.s.into());
        }
        5 => {
            cpu.registers.s = cpu.registers.s.wrapping_add(1);

            if cpu.registers.emulation_mode {
                // Emulation mode skips the last cycle (pull PBR)
                final_cycle(cpu, bus);

                ensure_page_1_stack(&mut cpu.registers);

                let pc_msb = bus.read(cpu.registers.s.into());
                cpu.registers.pc = u16::from_le_bytes([cpu.state.t0, pc_msb]);
            } else {
                cpu.state.t1 = bus.read(cpu.registers.s.into());
            }
        }
        6 => {
            final_cycle(cpu, bus);

            cpu.registers.s = cpu.registers.s.wrapping_add(1);
            cpu.registers.pbr = bus.read(cpu.registers.s.into());

            cpu.registers.pc = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
        }
        _ => invalid_cycle!(cpu),
    }
}

// PEA: Push effective address
pub(crate) fn pea<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            cpu.state.t1 = fetch_operand(cpu, bus);
        }
        3 => {
            bus.write(cpu.registers.s.into(), cpu.state.t1);
            cpu.registers.s = cpu.registers.s.wrapping_sub(1);
        }
        4 => {
            final_cycle(cpu, bus);

            bus.write(cpu.registers.s.into(), cpu.state.t0);
            cpu.registers.s = cpu.registers.s.wrapping_sub(1);

            if cpu.registers.emulation_mode {
                ensure_page_1_stack(&mut cpu.registers);
            }
        }
        _ => invalid_cycle!(cpu),
    }
}

// PEI: Push effective indirect address
pub(crate) fn pei<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
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
            let address = index_direct_page(cpu, cpu.state.t0, 1);
            cpu.state.t2 = bus.read(address.into());
        }
        5 => {
            bus.write(cpu.registers.s.into(), cpu.state.t2);
            cpu.registers.s = cpu.registers.s.wrapping_sub(1);
        }
        6 => {
            final_cycle(cpu, bus);

            bus.write(cpu.registers.s.into(), cpu.state.t1);
            cpu.registers.s = cpu.registers.s.wrapping_sub(1);

            if cpu.registers.emulation_mode {
                ensure_page_1_stack(&mut cpu.registers);
            }
        }
        _ => invalid_cycle!(cpu),
    }
}

// PER: Push effective relative address
pub(crate) fn per<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
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
            let offset = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
            let address = cpu.registers.pc.wrapping_add(offset);
            bus.write(cpu.registers.s.into(), (address >> 8) as u8);
            cpu.registers.s = cpu.registers.s.wrapping_sub(1);
        }
        5 => {
            final_cycle(cpu, bus);

            let offset = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
            let address = cpu.registers.pc.wrapping_add(offset);
            bus.write(cpu.registers.s.into(), address as u8);
            cpu.registers.s = cpu.registers.s.wrapping_sub(1);

            if cpu.registers.emulation_mode {
                ensure_page_1_stack(&mut cpu.registers);
            }
        }
        _ => invalid_cycle!(cpu),
    }
}

macro_rules! impl_halt {
    ($name:ident, $field:ident) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Wdc65816, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    bus.idle();
                }
                2 => {
                    final_cycle(cpu, bus);

                    bus.idle();

                    cpu.state.$field = true;
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

// WAI: Wait for interrupt
impl_halt!(wai, waiting);

// STP: Stop the clock
impl_halt!(stp, stopped);
