#[allow(clippy::wildcard_imports)]
use super::*;
use jgenesis_traits::num::GetBit;

macro_rules! impl_branch {
    ($name:ident $(, $flag:ident == $value:expr)?) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);

                    // Check if branch should be taken; skip rest of instruction if not
                    $(
                        if cpu.registers.psw.$flag != $value {
                            cpu.state.cycle = 0;
                        }
                    )?
                }
                2 => {
                    bus.idle();
                }
                3 => {
                    cpu.final_cycle();
                    bus.idle();

                    cpu.registers.pc = cpu.registers.pc.wrapping_add(cpu.state.t0 as i8 as u16);
                }
                _ => invalid_cycle!(cpu)
            }
        }
    }
}

impl_branch!(bra);
impl_branch!(beq, zero == true);
impl_branch!(bne, zero == false);
impl_branch!(bcs, carry == true);
impl_branch!(bcc, carry == false);
impl_branch!(bvs, overflow == true);
impl_branch!(bvc, overflow == false);
impl_branch!(bmi, negative == true);
impl_branch!(bpl, negative == false);

macro_rules! impl_branch_memory_bit {
    ($name:ident, bit == $value:expr) => {
        pub(crate) fn $name<B: BusInterface>(cpu: &mut Spc700, bus: &mut B, bit: u8) {
            match cpu.state.cycle {
                1 => {
                    cpu.state.t0 = fetch_operand(cpu, bus);
                }
                2 => {
                    let address = u16::from_le_bytes([cpu.state.t0, cpu.direct_page_msb()]);
                    cpu.state.t1 = bus.read(address);
                }
                3 | 5 => {
                    bus.idle();
                }
                4 => {
                    cpu.state.t2 = fetch_operand(cpu, bus);

                    // Skip rest of instruction if specified bit is not set/clear
                    if cpu.state.t1.bit(bit) != $value {
                        cpu.state.cycle = 0;
                    }
                }
                6 => {
                    cpu.final_cycle();
                    bus.idle();

                    cpu.registers.pc = cpu.registers.pc.wrapping_add(cpu.state.t2 as i8 as u16);
                }
                _ => invalid_cycle!(cpu),
            }
        }
    };
}

impl_branch_memory_bit!(bbs, bit == true);
impl_branch_memory_bit!(bbc, bit == false);

pub(crate) fn cbne_dp<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            let address = u16::from_le_bytes([cpu.state.t0, cpu.direct_page_msb()]);
            cpu.state.t1 = bus.read(address);
        }
        3 | 5 => {
            bus.idle();
        }
        4 => {
            cpu.state.t2 = fetch_operand(cpu, bus);

            // Skip rest of instruction if A == (dp)
            if cpu.registers.a == cpu.state.t1 {
                cpu.state.cycle = 0;
            }
        }
        // 5: Idle
        6 => {
            cpu.final_cycle();
            bus.idle();

            cpu.registers.pc = cpu.registers.pc.wrapping_add(cpu.state.t2 as i8 as u16);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn cbne_dpx<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 | 4 | 6 => {
            bus.idle();
        }
        3 => {
            let address_lsb = cpu.state.t0.wrapping_add(cpu.registers.x);
            let address = u16::from_le_bytes([address_lsb, cpu.direct_page_msb()]);
            cpu.state.t1 = bus.read(address);
        }
        // 4: Idle
        5 => {
            cpu.state.t2 = fetch_operand(cpu, bus);

            // Skip rest of instruction if A == (dp+X)
            if cpu.registers.a == cpu.state.t1 {
                cpu.state.cycle = 0;
            }
        }
        // 6: Idle
        7 => {
            cpu.final_cycle();
            bus.idle();

            cpu.registers.pc = cpu.registers.pc.wrapping_add(cpu.state.t2 as i8 as u16);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn dbnz_dp<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            let address = u16::from_le_bytes([cpu.state.t0, cpu.direct_page_msb()]);
            cpu.state.t1 = bus.read(address);
        }
        3 => {
            let address = u16::from_le_bytes([cpu.state.t0, cpu.direct_page_msb()]);
            bus.write(address, cpu.state.t1.wrapping_sub(1));
        }
        4 => {
            cpu.state.t2 = fetch_operand(cpu, bus);

            // Skip rest of instruction if --(dp) == 0
            if cpu.state.t1 == 1 {
                cpu.state.cycle = 0;
            }
        }
        5 => {
            bus.idle();
        }
        6 => {
            cpu.final_cycle();
            bus.idle();

            cpu.registers.pc = cpu.registers.pc.wrapping_add(cpu.state.t2 as i8 as u16);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn dbnz_y<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 | 2 | 4 => {
            bus.idle();
        }
        3 => {
            cpu.state.t1 = fetch_operand(cpu, bus);

            // Skip rest of instruction if --Y == 0
            cpu.registers.y = cpu.registers.y.wrapping_sub(1);
            if cpu.registers.y == 0 {
                cpu.state.cycle = 0;
            }
        }
        // 4: Idle
        5 => {
            cpu.final_cycle();
            bus.idle();

            cpu.registers.pc = cpu.registers.pc.wrapping_add(cpu.state.t1 as i8 as u16);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn jmp_abs<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            cpu.final_cycle();

            let address_msb = fetch_operand(cpu, bus);
            cpu.registers.pc = u16::from_le_bytes([cpu.state.t0, address_msb]);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn jmp_absx_ind<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
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
            let address = u16::from_le_bytes([cpu.state.t0, cpu.state.t1])
                .wrapping_add(cpu.registers.x.into());
            cpu.state.t2 = bus.read(address);
        }
        5 => {
            cpu.final_cycle();

            let address = u16::from_le_bytes([cpu.state.t0, cpu.state.t1])
                .wrapping_add(cpu.registers.x.into())
                .wrapping_add(1);
            let jump_address_msb = bus.read(address);
            cpu.registers.pc = u16::from_le_bytes([cpu.state.t2, jump_address_msb]);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn call<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            cpu.state.t1 = fetch_operand(cpu, bus);
        }
        3 | 6 => {
            bus.idle();
        }
        4 => {
            bus.write(cpu.stack_pointer(), (cpu.registers.pc >> 8) as u8);
            cpu.registers.sp = cpu.registers.sp.wrapping_sub(1);
        }
        5 => {
            bus.write(cpu.stack_pointer(), cpu.registers.pc as u8);
            cpu.registers.sp = cpu.registers.sp.wrapping_sub(1);
        }
        // 6: Idle
        7 => {
            cpu.final_cycle();
            bus.idle();

            cpu.registers.pc = u16::from_le_bytes([cpu.state.t0, cpu.state.t1]);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn pcall<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 => {
            cpu.state.t0 = fetch_operand(cpu, bus);
        }
        2 => {
            bus.idle();
        }
        3 => {
            bus.write(cpu.stack_pointer(), (cpu.registers.pc >> 8) as u8);
            cpu.registers.sp = cpu.registers.sp.wrapping_sub(1);
        }
        4 => {
            bus.write(cpu.stack_pointer(), cpu.registers.pc as u8);
            cpu.registers.sp = cpu.registers.sp.wrapping_sub(1);
        }
        5 => {
            cpu.final_cycle();
            bus.idle();

            cpu.registers.pc = u16::from_le_bytes([cpu.state.t0, 0xFF]);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn tcall<B: BusInterface>(cpu: &mut Spc700, bus: &mut B, n: u16) {
    match cpu.state.cycle {
        1 => {
            // Dummy read
            bus.read(cpu.registers.pc);
        }
        2 | 5 => {
            bus.idle();
        }
        3 => {
            bus.write(cpu.stack_pointer(), (cpu.registers.pc >> 8) as u8);
            cpu.registers.sp = cpu.registers.sp.wrapping_sub(1);
        }
        4 => {
            bus.write(cpu.stack_pointer(), cpu.registers.pc as u8);
            cpu.registers.sp = cpu.registers.sp.wrapping_sub(1);
        }
        // 5: Idle
        6 => {
            let address = 0xFFC0 + 2 * (15 - n);
            cpu.state.t0 = bus.read(address);
        }
        7 => {
            cpu.final_cycle();

            let address = 0xFFC1 + 2 * (15 - n);
            let jump_address_msb = bus.read(address);
            cpu.registers.pc = u16::from_le_bytes([cpu.state.t0, jump_address_msb]);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn ret<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 | 2 => {
            bus.idle();
        }
        3 => {
            cpu.registers.sp = cpu.registers.sp.wrapping_add(1);
            cpu.state.t0 = bus.read(cpu.stack_pointer());
        }
        4 => {
            cpu.final_cycle();

            cpu.registers.sp = cpu.registers.sp.wrapping_add(1);
            let pc_msb = bus.read(cpu.stack_pointer());
            cpu.registers.pc = u16::from_le_bytes([cpu.state.t0, pc_msb]);
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn reti<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 | 2 => {
            bus.idle();
        }
        3 => {
            cpu.registers.sp = cpu.registers.sp.wrapping_add(1);
            cpu.registers.psw = bus.read(cpu.stack_pointer()).into();
        }
        4 => {
            cpu.registers.sp = cpu.registers.sp.wrapping_add(1);
            cpu.state.t0 = bus.read(cpu.stack_pointer());
        }
        5 => {
            cpu.final_cycle();

            cpu.registers.sp = cpu.registers.sp.wrapping_add(1);
            let pc_msb = bus.read(cpu.stack_pointer());
            cpu.registers.pc = u16::from_le_bytes([cpu.state.t0, pc_msb]);
        }
        _ => invalid_cycle!(cpu),
    }
}

const BRK_VECTOR: u16 = 0xFFDE;

pub(crate) fn brk<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    match cpu.state.cycle {
        1 | 5 => {
            bus.idle();
        }
        2 => {
            bus.write(cpu.stack_pointer(), (cpu.registers.pc >> 8) as u8);
            cpu.registers.sp = cpu.registers.sp.wrapping_sub(1);
        }
        3 => {
            bus.write(cpu.stack_pointer(), cpu.registers.pc as u8);
            cpu.registers.sp = cpu.registers.sp.wrapping_sub(1);
        }
        4 => {
            bus.write(cpu.stack_pointer(), cpu.registers.psw.into());
            cpu.registers.sp = cpu.registers.sp.wrapping_sub(1);
        }
        // 5: Idle
        6 => {
            cpu.state.t0 = bus.read(BRK_VECTOR);
        }
        7 => {
            cpu.final_cycle();

            let pc_msb = bus.read(BRK_VECTOR + 1);
            cpu.registers.pc = u16::from_le_bytes([cpu.state.t0, pc_msb]);
            cpu.registers.psw.break_flag = true;
            cpu.registers.psw.interrupt_enabled = false;
        }
        _ => invalid_cycle!(cpu),
    }
}

pub(crate) fn stop<B: BusInterface>(cpu: &mut Spc700, bus: &mut B) {
    cpu.final_cycle();
    bus.idle();
    cpu.state.stopped = true;
}
