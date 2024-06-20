//! SH-2 internal I/O registers (accessed using A29-31 = 111)

use crate::dma::DmaController;
use crate::sci::SerialInterface;
use crate::wdt::WatchdogTimer;
use crate::{Sh2, RESET_INTERRUPT_MASK};
use bincode::{Decode, Encode};
use jgenesis_common::num::{GetBit, U16Ext};

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct StatusRegister {
    // Interrupt levels <= this value are masked
    pub interrupt_mask: u8,
    // Used as a carry/test flag by many instructions
    pub t: bool,
    // Saturation flag used by multiply-accumulate instructions
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
    pub dma0_vector: u8,
    pub dma1_vector: u8,
    pub wdt_priority: u8,
    pub wdt_vector: u8,
    pub bsc_vector: u8,
    pub sci_priority: u8,
    pub sci_rx_error_vector: u8,
    pub sci_rx_ok_vector: u8,
    pub sci_tx_empty_vector: u8,
    pub sci_transfer_end_vector: u8,
    pub frt_priority: u8,
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

    fn write_ipra_high(&mut self, value: u8) {
        self.divu_priority = value >> 4;
        self.dmac_priority = value & 0xF;

        log::debug!("IPRA high write: {value:02X}");
        log::debug!("  DIVU interrupt priority: {}", self.divu_priority);
        log::debug!("  DMAC interrupt priority: {}", self.dmac_priority);
    }

    fn write_ipra_low(&mut self, value: u8) {
        self.wdt_priority = value >> 4;

        log::debug!("IPRA low write: {value:02X}");
        log::debug!("  WDT interrupt priority: {}", self.wdt_priority);
    }

    // $FFFFFE60: IPRB (Interrupt priority B)
    fn read_iprb(&self) -> u16 {
        (u16::from(self.sci_priority) << 12) | (u16::from(self.frt_priority) << 8)
    }

    // $FFFFFE60: IPRB (Interrupt priority B)
    fn write_iprb(&mut self, value: u16) {
        self.sci_priority = (value >> 12) as u8;
        self.frt_priority = ((value >> 8) & 0xF) as u8;

        log::debug!("IPRB write: {value:04X}");
        log::debug!("  SCI interrupt priority: {}", self.sci_priority);
        log::debug!("  FRT interrupt priority: {}", self.frt_priority);
    }

    fn write_iprb_high(&mut self, value: u8) {
        // IPRB low byte is not used; just shift the value
        self.write_iprb(u16::from(value) << 8);
    }

    // $FFFFFE62: VCRA (Vector number register A)
    fn read_vcra(&self) -> u16 {
        (u16::from(self.sci_rx_error_vector) << 8) | u16::from(self.sci_rx_ok_vector)
    }

    // $FFFFFE62: VCRA (Vector number register A)
    fn write_vcra(&mut self, value: u16) {
        self.sci_rx_error_vector = ((value >> 8) & 0x7F) as u8;
        self.sci_rx_ok_vector = (value & 0x7F) as u8;

        log::debug!("VCRA write: {value:04X}");
        log::debug!("  SCI RX error vector number: {}", self.sci_rx_error_vector);
        log::debug!("  SCI RX ok vector number: {}", self.sci_rx_ok_vector);
    }

    fn write_vcra_high(&mut self, value: u8) {
        self.sci_rx_error_vector = value & 0x7F;

        log::debug!("VCRA high write: {value:02X}");
        log::debug!("  SCI RX error vector number: {}", self.sci_rx_error_vector);
    }

    fn write_vcra_low(&mut self, value: u8) {
        self.sci_rx_ok_vector = value & 0x7F;

        log::debug!("VCRA low write: {value:02X}");
        log::debug!("  SCI RX ok vector number: {}", self.sci_rx_ok_vector);
    }

    // $FFFFFE64: VCRB (Vector number register B)
    fn read_vcrb(&self) -> u16 {
        (u16::from(self.sci_tx_empty_vector) << 8) | u16::from(self.sci_transfer_end_vector)
    }

    // $FFFFFE64: VCRB (Vector number register B)
    fn write_vcrb(&mut self, value: u16) {
        self.sci_tx_empty_vector = ((value >> 8) & 0x7F) as u8;
        self.sci_transfer_end_vector = (value & 0x7F) as u8;

        log::debug!("VCRB write: {value:04X}");
        log::debug!("  SCI TX empty vector number: {}", self.sci_tx_empty_vector);
        log::debug!("  SCI transfer end vector number: {}", self.sci_transfer_end_vector);
    }

    fn write_vcrb_high(&mut self, value: u8) {
        self.sci_tx_empty_vector = value & 0x7F;

        log::debug!("VCRB high write: {value:02X}");
        log::debug!("  SCI TX empty vector number: {}", self.sci_tx_empty_vector);
    }

    fn write_vcrb_low(&mut self, value: u8) {
        self.sci_transfer_end_vector = value & 0x7F;

        log::debug!("VCRB low write: {value:02X}");
        log::debug!("  SCI transfer end vector number: {}", self.sci_transfer_end_vector);
    }

    // $FFFFFEE4: VCRWDT (WDT interrupt vector number)
    fn read_vcrwdt(&self) -> u16 {
        (u16::from(self.wdt_vector) << 8) | u16::from(self.bsc_vector)
    }

    // $FFFFFEE4: VCRWDT (WDT interrupt vector number)
    fn write_vcrwdt(&mut self, value: u16) {
        self.wdt_vector = ((value >> 8) & 0x7F) as u8;
        self.bsc_vector = (value & 0x7F) as u8;

        log::debug!("VCRWDT write: {value:04X}");
        log::debug!("  WDT interrupt vector number: {}", self.wdt_vector);
        log::debug!("  BSC interrupt vector number: {}", self.bsc_vector);
    }

    fn write_vcrwdt_high(&mut self, value: u8) {
        self.wdt_vector = value & 0x7F;

        log::debug!("VCRWDT high write: {value:02X}");
        log::debug!("  WDT interrupt vector number: {}", self.wdt_vector);
    }

    fn write_vcrwdt_low(&mut self, value: u8) {
        self.bsc_vector = value & 0x7F;

        log::debug!("VCRWDT low write: {value:02X}");
        log::debug!("  BSC interrupt vector number: {}", self.bsc_vector);
    }

    // $FFFFFFA0: VCRDMA0 (Interrupt vector number for DMA0)
    fn write_vcrdma0(&mut self, value: u32) {
        self.dma0_vector = value as u8;

        log::debug!("VCRDMA0 write: {value:08X}");
        log::debug!("  DMA0 vector number: {}", self.dma0_vector);
    }

    // $FFFFFFA8: VCRDMA1 (Interrupt vector number for DMA1)
    fn write_vcrdma1(&mut self, value: u32) {
        self.dma1_vector = value as u8;

        log::debug!("VCRDMA1 write: {value:08X}");
        log::debug!("  DMA1 vector number: {}", self.dma1_vector);
    }
}

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
pub struct InternalInterrupt {
    pub priority: u8,
    pub vector_number: u8,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Sh7604Registers {
    pub break_registers: BreakRegisters,
    pub interrupts: InterruptRegisters,
    pub internal_interrupt: InternalInterrupt,
}

impl Sh7604Registers {
    pub fn new() -> Self {
        Self {
            break_registers: BreakRegisters::default(),
            interrupts: InterruptRegisters::default(),
            internal_interrupt: InternalInterrupt::default(),
        }
    }

    pub fn update_interrupt_level(
        &mut self,
        dma_controller: &DmaController,
        watchdog_timer: &WatchdogTimer,
        serial: &SerialInterface,
    ) {
        self.internal_interrupt = InternalInterrupt::default();

        if self.interrupts.dmac_priority != 0 {
            if dma_controller.channels[0].control.interrupt_pending() {
                self.internal_interrupt = InternalInterrupt {
                    priority: self.interrupts.dmac_priority,
                    vector_number: self.interrupts.dma0_vector,
                };
            } else if dma_controller.channels[1].control.interrupt_pending() {
                self.internal_interrupt = InternalInterrupt {
                    priority: self.interrupts.dmac_priority,
                    vector_number: self.interrupts.dma1_vector,
                };
            }
        }

        if serial.rx_interrupt_pending()
            && self.interrupts.sci_priority > self.internal_interrupt.priority
        {
            self.internal_interrupt = InternalInterrupt {
                priority: self.interrupts.sci_priority,
                vector_number: self.interrupts.sci_rx_ok_vector,
            };
        }

        if watchdog_timer.overflow_flag()
            && self.interrupts.wdt_priority > self.internal_interrupt.priority
        {
            self.internal_interrupt = InternalInterrupt {
                priority: self.interrupts.wdt_priority,
                vector_number: self.interrupts.wdt_vector,
            };
        }
    }
}

impl Sh2 {
    pub(super) fn read_internal_register_byte(&self, address: u32) -> u8 {
        log::trace!("[{}] Internal register byte read: {address:08X}", self.name);

        match address {
            0xFFFFFC17 => {
                // Cosmic Carnage constantly accesses this address - not sure what it's supposed to be
                0
            }
            0xFFFFFE00..=0xFFFFFE05 => self.serial.read_register(address),
            0xFFFFFE10..=0xFFFFFE19 => self.free_run_timer.read_register(address),
            0xFFFFFE60 => self.sh7604.interrupts.read_iprb().msb(),
            0xFFFFFE61 => self.sh7604.interrupts.read_iprb().lsb(),
            0xFFFFFE62 => self.sh7604.interrupts.read_vcra().msb(),
            0xFFFFFE63 => self.sh7604.interrupts.read_vcra().lsb(),
            0xFFFFFE64 => self.sh7604.interrupts.read_vcrb().msb(),
            0xFFFFFE65 => self.sh7604.interrupts.read_vcrb().lsb(),
            0xFFFFFE80 => self.watchdog_timer.read_control(),
            0xFFFFFE81 => self.watchdog_timer.read_counter(),
            0xFFFFFE92 => self.cache.read_control(),
            0xFFFFFEE2 => self.sh7604.interrupts.read_ipra().msb(),
            0xFFFFFEE3 => self.sh7604.interrupts.read_ipra().lsb(),
            0xFFFFFEE4 => self.sh7604.interrupts.read_vcrwdt().msb(),
            0xFFFFFEE5 => self.sh7604.interrupts.read_vcrwdt().lsb(),
            _ => {
                log::warn!("[{}] Unexpected internal register byte read: {address:08X}", self.name);
                0
            }
        }
    }

    pub(super) fn read_internal_register_word(&self, address: u32) -> u16 {
        log::trace!("[{}] Internal register word read: {address:08X}", self.name);

        match address {
            0xFFFFFE60 => self.sh7604.interrupts.read_iprb(),
            0xFFFFFE62 => self.sh7604.interrupts.read_vcra(),
            0xFFFFFE64 => self.sh7604.interrupts.read_vcrb(),
            0xFFFFFEE2 => self.sh7604.interrupts.read_ipra(),
            0xFFFFFEE4 => self.sh7604.interrupts.read_vcrwdt(),
            0xFFFFFF08 => (self.divu.read_control() >> 16) as u16,
            0xFFFFFF0A => self.divu.read_control() as u16,
            0xFFFFFF40 => self.sh7604.break_registers.read_break_address_a_high(),
            0xFFFFFF42 => self.sh7604.break_registers.read_break_address_a_low(),
            0xFFFFFF60 => self.sh7604.break_registers.read_break_address_b_high(),
            0xFFFFFF62 => self.sh7604.break_registers.read_break_address_b_low(),
            _ => {
                log::warn!("[{}] Unexpected internal register word read: {address:08X}", self.name);
                0
            }
        }
    }

    pub(super) fn read_internal_register_longword(&mut self, address: u32) -> u32 {
        log::trace!("[{}] Internal register longword read: {address:08X}", self.name);

        match address {
            0xFFFFFF00..=0xFFFFFF1F => self.divu.read_register(address),
            // Break registers; break functionality is not implemented but some games use the registers as R/W storage
            0xFFFFFF40 => self.sh7604.break_registers.break_address_a,
            0xFFFFFF60 => self.sh7604.break_registers.break_address_b,
            0xFFFFFF80..=0xFFFFFF9F | 0xFFFFFFB0 => self.dmac.read_register(address),
            0xFFFFFFA0 => self.sh7604.interrupts.dma0_vector.into(),
            0xFFFFFFA8 => self.sh7604.interrupts.dma1_vector.into(),
            // Bus control register; not emulated, 32X games only ever write this value to it before reading
            0xFFFFFFE0 => 0xA55A0001,
            _ => {
                log::warn!(
                    "[{}] Unexpected internal register longword read: {address:08X}",
                    self.name
                );
                0
            }
        }
    }

    #[allow(clippy::match_same_arms)]
    pub(super) fn write_internal_register_byte(&mut self, address: u32, value: u8) {
        log::trace!("[{}] Internal register byte write: {address:08X} {value:02X}", self.name);

        match address {
            // Cosmic Carnage constantly accesses this address - not sure what it's supposed to be
            0xFFFFFC17 => {}
            0xFFFFFE00..=0xFFFFFE05 => self.serial.write_register(address, value),
            0xFFFFFE10..=0xFFFFFE19 => self.free_run_timer.write_register(address, value),
            0xFFFFFE60 => self.sh7604.interrupts.write_iprb_high(value),
            // IPRB low byte; does not do anything
            0xFFFFFE61 => {}
            0xFFFFFE62 => self.sh7604.interrupts.write_vcra_high(value),
            0xFFFFFE63 => self.sh7604.interrupts.write_vcra_low(value),
            0xFFFFFE64 => self.sh7604.interrupts.write_vcrb_high(value),
            0xFFFFFE65 => self.sh7604.interrupts.write_vcrb_low(value),
            // DMA request/response selection control registers; unusual only if a non-zero value is written
            0xFFFFFE71 | 0xFFFFFE72 => {
                if value != 0 {
                    log::warn!(
                        "[{}] Unexpected DRCR0/DRCR1 write: {address:08X} {value:02X}",
                        self.name
                    );
                }
            }
            0xFFFFFE91 => log_standby_control_write(value, &self.name),
            0xFFFFFE92 => self.cache.write_control(value),
            0xFFFFFEE2 => self.sh7604.interrupts.write_ipra_high(value),
            0xFFFFFEE3 => self.sh7604.interrupts.write_ipra_low(value),
            0xFFFFFEE4 => self.sh7604.interrupts.write_vcrwdt_high(value),
            0xFFFFFEE5 => self.sh7604.interrupts.write_vcrwdt_low(value),
            _ => log::warn!(
                "[{}] Unexpected internal register byte write: {address:08X} {value:02X}",
                self.name
            ),
        }
    }

    pub(super) fn write_internal_register_word(&mut self, address: u32, value: u16) {
        log::trace!("[{}] Internal register word write: {address:08X} {value:04X}", self.name);

        match address {
            // Writing to this address sets SDRAM 16-bit CAS latency; ignore
            0xFFFF8446 => {}
            0xFFFFFE60 => self.sh7604.interrupts.write_iprb(value),
            0xFFFFFE62 => self.sh7604.interrupts.write_vcra(value),
            0xFFFFFE64 => self.sh7604.interrupts.write_vcrb(value),
            0xFFFFFE80 => self.watchdog_timer.write_control(value),
            0xFFFFFE92 => self.cache.write_control(value as u8),
            0xFFFFFEE2 => self.sh7604.interrupts.write_ipra(value),
            0xFFFFFEE4 => self.sh7604.interrupts.write_vcrwdt(value),
            // DIVU control register is writable 16-bit
            0xFFFFFF08 => self.divu.write_register(address, value.into()),
            0xFFFFFF40 => self.sh7604.break_registers.write_break_address_a_high(value),
            0xFFFFFF42 => self.sh7604.break_registers.write_break_address_a_low(value),
            0xFFFFFF60 => self.sh7604.break_registers.write_break_address_b_high(value),
            0xFFFFFF62 => self.sh7604.break_registers.write_break_address_b_low(value),
            _ => log::warn!(
                "[{}] Unexpected internal register word write: {address:08X} {value:04X}",
                self.name
            ),
        }
    }

    #[allow(clippy::match_same_arms)]
    pub(super) fn write_internal_register_longword(&mut self, address: u32, value: u32) {
        log::trace!("[{}] Internal register longword write: {address:08X} {value:08X}", self.name);

        match address {
            0xFFFFFF00..=0xFFFFFF1F => self.divu.write_register(address, value),
            0xFFFFFF40 => self.sh7604.break_registers.write_break_address_a(value),
            // Break bus cycle register A; ignore
            0xFFFFFF48 => {}
            0xFFFFFF60 => self.sh7604.break_registers.write_break_address_b(value),
            // Break bus cycle register B; ignore
            0xFFFFFF68 => {}
            0xFFFFFF80..=0xFFFFFF9F | 0xFFFFFFB0 => self.dmac.write_register(address, value),
            0xFFFFFFA0 => self.sh7604.interrupts.write_vcrdma0(value),
            0xFFFFFFA8 => self.sh7604.interrupts.write_vcrdma1(value),
            0xFFFFFFE0..=0xFFFFFFFF => log_bus_control_write(address, value),
            _ => log::warn!(
                "[{}] Unexpected internal register longword write: {address:08X} {value:08X}",
                self.name
            ),
        }
    }
}

// $FFFFFE91: SBYCR (Standby control register); not emulated
fn log_standby_control_write(value: u8, name: &str) {
    log::trace!("[{name}] SBYCR write: {value:02X}");
    log::trace!("  Standby mode enabled: {}", value.bit(7));
    log::trace!("  Pins at Hi-Z in standby: {}", value.bit(6));
    log::trace!("  DMAC clock halted: {}", value.bit(4));
    log::trace!("  MULT clock halted: {}", value.bit(3));
    log::trace!("  DIVU clock halted: {}", value.bit(2));
    log::trace!("  FRT clock halted: {}", value.bit(1));
    log::trace!("  SCI clock halted: {}", value.bit(0));
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
                log::error!("SH-2 FRT compare match interrupt was enabled; not emulated");
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
        _ => log::warn!("Bus control register write {address:08X} {value:08X}"),
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
