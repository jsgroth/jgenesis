use crate::RESET_INTERRUPT_MASK;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct StatusRegister {
    // Interrupt levels <= this value are masked
    pub interrupt_mask: u8,
    // Used as a carry/test flag by many instructions
    pub t: bool,
    // Flag used by multiply-accumulate instructions
    pub s: bool,
    // Flags used by division instructions
    pub q: bool,
    pub m: bool,
}

impl Default for StatusRegister {
    fn default() -> Self {
        Self { t: false, s: false, interrupt_mask: RESET_INTERRUPT_MASK, q: false, m: false }
    }
}

impl From<u32> for StatusRegister {
    fn from(value: u32) -> Self {
        Self {
            interrupt_mask: ((value >> 4) & 0xF) as u8,
            t: value.bit(0),
            s: value.bit(1),
            q: value.bit(8),
            m: value.bit(9),
        }
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct Sh2Registers {
    // General-purpose registers
    pub gpr: [u32; 16],
    // Status register
    pub sr: StatusRegister,
    // Global base register (used with GBR addressing modes)
    pub gbr: u32,
    // Vector base register (base of exception vector area)
    pub vbr: u32,
    // Multiply-accumulator
    pub macl: u32,
    pub mach: u32,
    // Procedure register (return address)
    pub pr: u32,
    // Program counter
    pub pc: u32,
    pub next_pc: u32,
    // Set when next_pc is changed by an instruction with a branch delay slot
    pub next_op_in_delay_slot: bool,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct BusControllerRegisters {}

impl BusControllerRegisters {
    pub fn new() -> Self {
        Self {}
    }

    pub fn write_register(&mut self, address: u32, value: u32) {
        match address {
            0xFFFFFFE0 => self.write_control_1(value),
            0xFFFFFFE4 => self.write_control_2(value),
            0xFFFFFFE8 => self.write_wait_control(value),
            0xFFFFFFEC => self.write_individual_memory_control(value),
            0xFFFFFFF0 => self.write_refresh_timer_control(value),
            0xFFFFFFF4 => self.write_refresh_timer_counter(value),
            0xFFFFFFF8 => self.write_refresh_timer_constant(value),
            _ => todo!("Bus control register write {address:08X} {value:08X}"),
        }
    }

    fn write_control_1(&mut self, value: u32) {
        log::trace!("BCR1 write: {value:08X}");
        log::trace!("  Master mode: {}", !value.bit(15));
        log::trace!("  Big endian mode: {}", !value.bit(12));
        log::trace!("  Area 0 burst ROM enabled: {}", value.bit(11));
        log::trace!("  Partial-share master mode: {}", value.bit(10));
        log::trace!("  Long wait specification for areas 2/3: {} waits", ((value >> 8) & 3) + 3);
        log::trace!("  Long wait specification for area 1: {} waits", ((value >> 6) & 3) + 3);
        log::trace!("  Long wait specification for area 0: {} waits", ((value >> 4) & 3) + 3);
        log::trace!("  DRAM specification bits: {}", value & 7);
    }

    fn write_control_2(&mut self, value: u32) {
        log::trace!("BCR2 write: {value:08X}");
        log::trace!("  Size specification for area 3: {}", bus_area_size(value >> 6));
        log::trace!("  Size specification for area 2: {}", bus_area_size(value >> 4));
        log::trace!("  Size specification for area 1: {}", bus_area_size(value >> 2));
    }

    fn write_wait_control(&mut self, value: u32) {
        log::trace!("WCR write: {value:08X}");
        log::trace!("  Idles between cycles for area 3: {}", idle_cycles(value >> 14));
        log::trace!("  Idles between cycles for area 2: {}", idle_cycles(value >> 12));
        log::trace!("  Idles between cycles for area 1: {}", idle_cycles(value >> 10));
        log::trace!("  Idles between cycles for area 0: {}", idle_cycles(value >> 8));
        log::trace!("  Wait control for area 3: {}", (value >> 6) & 3);
        log::trace!("  Wait control for area 2: {}", (value >> 4) & 3);
        log::trace!("  Wait control for area 1: {}", (value >> 2) & 3);
        log::trace!("  Wait control for area 0: {}", value & 3);
    }

    fn write_individual_memory_control(&mut self, value: u32) {
        log::trace!("MCR write: {value:08X}");
        log::trace!("  RAS precharge time: {}", if value.bit(15) { 2 } else { 1 });
        log::trace!("  RAS-CAS delay: {}", if value.bit(14) { 2 } else { 1 });
        log::trace!("  Write precharge delay: {}", if value.bit(13) { 2 } else { 1 });
        log::trace!(
            "  CAS-before-RAS refresh RAS assert time: {}",
            match (value >> 11) & 3 {
                0 => "2 cycles",
                1 => "3 cycles",
                2 => "4 cycles",
                3 => "(Reserved)",
                _ => unreachable!(),
            }
        );
        log::trace!("  Burst enabled: {}", value.bit(10));
        log::trace!("  RAS down mode enabled: {}", value.bit(9));
        log::trace!("  Address multiplexing bits: {}", ((value >> 5) & 0x4) | ((value >> 4) & 0x3));
        log::trace!("  DRAM memory data size: {}", if value.bit(6) { "Longword" } else { "Word" });
        log::trace!("  DRAM refresh enabled: {}", value.bit(3));
        log::trace!("  Self-refresh enabled: {}", value.bit(2));
    }

    fn write_refresh_timer_control(&mut self, value: u32) {
        log::trace!("RTCSR write: {value:08X}");
        log::trace!("  Compare match flag: {}", value.bit(7));
        log::trace!("  Compare match interrupt enabled: {}", value.bit(6));
        log::trace!("  Clock select bits: {}", (value >> 3) & 7);

        if value.bit(6) {
            panic!("Compare match interrupt was enabled");
        }
    }

    fn write_refresh_timer_counter(&mut self, value: u32) {
        log::trace!("RTCNT write: {value:08X}");
        log::trace!("  Refresh timer counter: 0x{:02X}", value & 0xFF);
    }

    fn write_refresh_timer_constant(&mut self, value: u32) {
        log::trace!("RTCOR write: {value:08X}");
        log::trace!("  Refresh time constant for compare: 0x{:02X}", value & 0xFF);
    }
}

fn bus_area_size(value: u32) -> &'static str {
    match value & 3 {
        0 => "(Reserved)",
        1 => "Byte",
        2 => "Word",
        3 => "Longword",
        _ => unreachable!("value & 3 is always <= 3"),
    }
}

fn idle_cycles(value: u32) -> &'static str {
    match value & 3 {
        0 => "0 cycles",
        1 => "1 cycle",
        2 => "2 cycles",
        3 => "(Reserved)",
        _ => unreachable!("value & 3 is always <= 3"),
    }
}
