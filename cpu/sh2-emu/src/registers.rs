use crate::{Sh2, RESET_INTERRUPT_MASK};
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

impl From<StatusRegister> for u32 {
    fn from(value: StatusRegister) -> Self {
        (u32::from(value.m) << 9)
            | (u32::from(value.q) << 8)
            | (u32::from(value.interrupt_mask) << 4)
            | (u32::from(value.s) << 1)
            | u32::from(value.t)
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

impl Sh2Registers {
    pub fn mac(&self) -> i64 {
        (i64::from(self.mach) << 32) | i64::from(self.macl)
    }

    pub fn set_mac(&mut self, mac: i64) {
        self.macl = mac as u32;
        self.mach = ((mac as u64) >> 32) as u32;
    }
}

// User break functionality is not emulated, but After Burner Complete uses these R/W registers to
// store state in its audio processing code
#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct BreakRegisters {
    pub break_address_a: u32,
    pub break_address_b: u32,
}

impl BreakRegisters {
    fn read_break_address_a_high(&self) -> u16 {
        (self.break_address_a >> 16) as u16
    }

    fn read_break_address_a_low(&self) -> u16 {
        self.break_address_a as u16
    }

    fn read_break_address_b_high(&self) -> u16 {
        (self.break_address_b >> 16) as u16
    }

    fn read_break_address_b_low(&self) -> u16 {
        self.break_address_b as u16
    }

    fn write_break_address_a(&mut self, value: u32) {
        self.break_address_a = value;
        log::trace!("Break address A write: {value:08X}");
    }

    fn write_break_address_a_high(&mut self, value: u16) {
        self.break_address_a = (self.break_address_a & 0xFFFF) | (u32::from(value) << 16);
        log::trace!("Break address A high write: {value:04X}");
    }

    fn write_break_address_a_low(&mut self, value: u16) {
        self.break_address_a = (self.break_address_a & !0xFFFF) | u32::from(value);
        log::trace!("Break address A low write: {value:04X}");
    }

    fn write_break_address_b(&mut self, value: u32) {
        self.break_address_b = value;
        log::trace!("Break address B write: {value:08X}");
    }

    fn write_break_address_b_high(&mut self, value: u16) {
        self.break_address_b = (self.break_address_b & 0xFFFF) | (u32::from(value) << 16);
        log::trace!("Break address B high write: {value:04X}");
    }

    fn write_break_address_b_low(&mut self, value: u16) {
        self.break_address_b = (self.break_address_b & !0xFFFF) | u32::from(value);
        log::trace!("Break address B low write: {value:04X}");
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct InterruptRegisters {
    pub divu_priority: u8,
    pub dmac_priority: u8,
    pub wdt_priority: u8,
    pub wdt_vector: u8,
    pub bsc_vector: u8,
}

impl InterruptRegisters {
    // $FFFFFEE2: IPRA (Interrupt priority A)
    fn read_ipra(&self) -> u16 {
        (u16::from(self.divu_priority) << 12)
            | (u16::from(self.dmac_priority) << 8)
            | (u16::from(self.wdt_priority) << 4)
    }

    // $FFFFFEE2: IPRA (Interrupt priority A)
    fn write_ipra(&mut self, value: u16) {
        self.divu_priority = (value >> 12) as u8;
        self.dmac_priority = ((value >> 8) & 0xF) as u8;
        self.wdt_priority = ((value >> 4) & 0xF) as u8;

        log::debug!("IPRA write: {value:04X}");
        log::debug!("  DIVU interrupt priority: {}", self.divu_priority);
        log::debug!("  DMAC interrupt priority: {}", self.dmac_priority);
        log::debug!("  WDT interrupt priority: {}", self.wdt_priority);
    }

    // $FFFFFEE4: VCRWDT (WDT interrupt vector number)
    fn write_vcrwdt(&mut self, value: u16) {
        self.wdt_vector = ((value >> 8) & 0x7F) as u8;
        self.bsc_vector = (value & 0x7F) as u8;

        log::debug!("VCRWDT write: {value:04X}");
        log::debug!("  WDT interrupt vector number: {}", self.wdt_vector);
        log::debug!("  BSC interrupt vector number: {}", self.bsc_vector);
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Sh7604Registers {
    pub break_registers: BreakRegisters,
    pub interrupts: InterruptRegisters,
    pub watchdog_interrupt_pending: bool,
}

impl Sh7604Registers {
    pub fn new() -> Self {
        Self {
            break_registers: BreakRegisters::default(),
            interrupts: InterruptRegisters::default(),
            watchdog_interrupt_pending: false,
        }
    }
}

impl Sh2 {
    pub(super) fn read_internal_register_byte(&self, address: u32) -> u8 {
        log::trace!("[{}] Internal register byte read: {address:08X}", self.name);

        match address {
            0xFFFFFE10..=0xFFFFFE19 => self.free_run_timer.read_register(address),
            0xFFFFFE92 => self.cache.read_control(),
            0xFFFFFE93..=0xFFFFFE9F => 0xFF,
            _ => todo!("[{}] Internal register byte read {address:08X}", self.name),
        }
    }

    pub(super) fn read_internal_register_word(&self, address: u32) -> u16 {
        log::trace!("[{}] Internal register word read: {address:08X}", self.name);

        match address {
            0xFFFFFF40 => self.sh7604.break_registers.read_break_address_a_high(),
            0xFFFFFF42 => self.sh7604.break_registers.read_break_address_a_low(),
            0xFFFFFF60 => self.sh7604.break_registers.read_break_address_b_high(),
            0xFFFFFF62 => self.sh7604.break_registers.read_break_address_b_low(),
            0xFFFFFEE2 => self.sh7604.interrupts.read_ipra(),
            _ => todo!("[{}] Internal register word read {address:08X}", self.name),
        }
    }

    pub(super) fn read_internal_register_longword(&mut self, address: u32) -> u32 {
        log::trace!("[{}] Internal register longword read: {address:08X}", self.name);

        match address {
            0xFFFFFF00..=0xFFFFFF1C => self.divu.read_register(address),
            // Break registers; break functionality is not implemented
            0xFFFFFF40 => self.sh7604.break_registers.break_address_a,
            0xFFFFFF60 => self.sh7604.break_registers.break_address_b,
            0xFFFFFF80..=0xFFFFFF9F | 0xFFFFFFB0 => self.dmac.read_register(address),
            0xFFFFFFE0..=0xFFFFFFFF => todo!("read bus control register {address:08X}"),
            _ => todo!("Unexpected internal register longword read: {address:08X}"),
        }
    }

    pub(super) fn write_internal_register_byte(&mut self, address: u32, value: u8) {
        log::trace!("[{}] Internal register byte write: {address:08X} {value:08X}", self.name);

        match address {
            0xFFFFFE10..=0xFFFFFE19 => self.free_run_timer.write_register(address, value),
            0xFFFFFE92 => self.cache.write_control(value),
            0xFFFFFE93..=0xFFFFFE9F => {}
            0xFFFFFE91 => {
                // SBYCR (Standby control register)
                log::trace!("[{}] SBYCR write: {value:02X}", self.name);
                log::trace!("  Standby mode enabled: {}", value.bit(7));
                log::trace!("  Pins at Hi-Z in standby: {}", value.bit(6));
                log::trace!("  DMAC clock halted: {}", value.bit(4));
                log::trace!("  MULT clock halted: {}", value.bit(3));
                log::trace!("  DIVU clock halted: {}", value.bit(2));
                log::trace!("  FRT clock halted: {}", value.bit(1));
                log::trace!("  SCI clock halted: {}", value.bit(0));
            }
            _ => todo!(
                "[{}] Unexpected internal register byte write: {address:08X} {value:02X}",
                self.name
            ),
        }
    }

    pub(super) fn write_internal_register_word(&mut self, address: u32, value: u16) {
        log::trace!("[{}] Internal register word write: {address:08X} {value:04X}", self.name);

        match address {
            0xFFFF8446 => {
                log::trace!(
                    "[{}] $FFFF8446 write ({value:04X}): SDRAM 16-bit CAS latency set to 2",
                    self.name
                );
            }
            0xFFFFFF40 => self.sh7604.break_registers.write_break_address_a_high(value),
            0xFFFFFF42 => self.sh7604.break_registers.write_break_address_a_low(value),
            0xFFFFFF60 => self.sh7604.break_registers.write_break_address_b_high(value),
            0xFFFFFF62 => self.sh7604.break_registers.write_break_address_b_low(value),
            0xFFFFFE80 => self.watchdog_timer.write_control(value),
            0xFFFFFEE2 => self.sh7604.interrupts.write_ipra(value),
            0xFFFFFEE4 => self.sh7604.interrupts.write_vcrwdt(value),
            _ => todo!(
                "[{}] Unexpected internal register word write: {address:08X} {value:04X}",
                self.name
            ),
        }
    }

    pub(super) fn write_internal_register_longword(&mut self, address: u32, value: u32) {
        log::trace!("[{}] Internal register longword write: {address:08X} {value:08X}", self.name);

        match address {
            0xFFFFFF00..=0xFFFFFF14 => self.divu.write_register(address, value),
            0xFFFFFF40 => self.sh7604.break_registers.write_break_address_a(value),
            0xFFFFFF48 => {
                log::warn!(
                    "[{}] Ignoring write to break bus cycle register A ($FFFFFF48): {value:08X}",
                    self.name
                );
            }
            0xFFFFFF60 => self.sh7604.break_registers.write_break_address_b(value),
            0xFFFFFF68 => {
                log::warn!(
                    "[{}] Ignoring write to break bus cycle register B ($FFFFFF68): {value:08X}",
                    self.name
                );
            }
            0xFFFFFF80..=0xFFFFFF9F | 0xFFFFFFB0 => self.dmac.write_register(address, value),
            0xFFFFFFE0..=0xFFFFFFFF => log_bus_control_write(address, value),
            _ => todo!(
                "[{}] Unexpected internal register longword write: {address:08X} {value:08X}",
                self.name
            ),
        }
    }
}

fn log_bus_control_write(address: u32, value: u32) {
    // TODO actually emulate these registers?
    match address {
        0xFFFFFFE0 => {
            log::trace!("BCR1 write: {value:08X}");
            log::trace!("  Master mode: {}", !value.bit(15));
            log::trace!("  Big endian mode: {}", !value.bit(12));
            log::trace!("  Area 0 burst ROM enabled: {}", value.bit(11));
            log::trace!("  Partial-share master mode: {}", value.bit(10));
            log::trace!(
                "  Long wait specification for areas 2/3: {} waits",
                ((value >> 8) & 3) + 3
            );
            log::trace!("  Long wait specification for area 1: {} waits", ((value >> 6) & 3) + 3);
            log::trace!("  Long wait specification for area 0: {} waits", ((value >> 4) & 3) + 3);
            log::trace!("  DRAM specification bits: {}", value & 7);
        }
        0xFFFFFFE4 => {
            log::trace!("BCR2 write: {value:08X}");
            log::trace!("  Size specification for area 3: {}", bus_area_size(value >> 6));
            log::trace!("  Size specification for area 2: {}", bus_area_size(value >> 4));
            log::trace!("  Size specification for area 1: {}", bus_area_size(value >> 2));
        }
        0xFFFFFFE8 => {
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
        0xFFFFFFEC => {
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
            log::trace!(
                "  Address multiplexing bits: {}",
                ((value >> 5) & 0x4) | ((value >> 4) & 0x3)
            );
            log::trace!(
                "  DRAM memory data size: {}",
                if value.bit(6) { "Longword" } else { "Word" }
            );
            log::trace!("  DRAM refresh enabled: {}", value.bit(3));
            log::trace!("  Self-refresh enabled: {}", value.bit(2));
        }
        0xFFFFFFF0 => {
            log::trace!("RTCSR write: {value:08X}");
            log::trace!("  Compare match flag: {}", value.bit(7));
            log::trace!("  Compare match interrupt enabled: {}", value.bit(6));
            log::trace!("  Clock select bits: {}", (value >> 3) & 7);

            if value.bit(6) {
                todo!("SH-2 FRT compare match interrupt was enabled");
            }
        }
        0xFFFFFFF4 => {
            log::trace!("RTCNT write: {value:08X}");
            log::trace!("  Refresh timer counter: 0x{:02X}", value & 0xFF);
        }
        0xFFFFFFF8 => {
            log::trace!("RTCOR write: {value:08X}");
            log::trace!("  Refresh time constant for compare: 0x{:02X}", value & 0xFF);
        }
        _ => todo!("bus control register write {address:08X} {value:08X}"),
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
