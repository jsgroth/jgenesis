use crate::bus::WhichCpu;
use crate::vdp::Vdp;
use bincode::{Decode, Encode};
use jgenesis_common::num::{GetBit, U16Ext, U24Ext};
use std::array;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum Access {
    M68k = 0,
    Sh2 = 1,
}

impl Display for Access {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::M68k => write!(f, "68000"),
            Self::Sh2 => write!(f, "SH-2"),
        }
    }
}

impl Access {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Sh2 } else { Self::M68k }
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct Sh2Interrupts {
    pub reset_pending: bool,
    pub v_pending: bool,
    pub v_enabled: bool,
    pub h_pending: bool,
    pub h_enabled: bool,
    pub command_pending: bool,
    pub command_enabled: bool,
    pub pwm_pending: bool,
    pub pwm_enabled: bool,
    pub current_interrupt_level: u8,
}

impl Sh2Interrupts {
    fn mask_bits(&self) -> u16 {
        (u16::from(self.v_enabled) << 3)
            | (u16::from(self.h_enabled) << 2)
            | (u16::from(self.command_enabled) << 1)
            | u16::from(self.pwm_enabled)
    }

    fn write_mask_bits(&mut self, value: u16) {
        self.v_enabled = value.bit(3);
        self.h_enabled = value.bit(2);
        self.command_enabled = value.bit(1);
        self.pwm_enabled = value.bit(0);

        self.update_interrupt_level();
    }

    fn clear_reset(&mut self) {
        self.reset_pending = false;
        self.update_interrupt_level();
    }

    fn clear_v(&mut self) {
        self.v_pending = false;
        self.update_interrupt_level();
    }

    fn clear_h(&mut self) {
        self.h_pending = false;
        self.update_interrupt_level();
    }

    fn clear_command(&mut self) {
        self.command_pending = false;
        self.update_interrupt_level();
    }

    fn clear_pwm(&mut self) {
        self.pwm_pending = false;
        self.update_interrupt_level();
    }

    fn update_interrupt_level(&mut self) {
        self.current_interrupt_level = if self.reset_pending {
            14
        } else if self.v_pending {
            12
        } else if self.h_pending {
            10
        } else if self.command_pending && self.command_enabled {
            8
        } else if self.pwm_pending {
            6
        } else {
            0
        };
    }
}

const DMA_FIFO_LEN: usize = 4;

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct DmaFifo {
    blocks: [[u16; 4]; 2],
    ready: [bool; 2],
    m68k_block: usize,
    m68k_idx: usize,
    sh2_block: usize,
    sh2_idx: usize,
}

impl DmaFifo {
    pub fn push(&mut self, value: u16) {
        log::trace!("DMA FIFO push: {value:04X}");

        self.blocks[self.m68k_block][self.m68k_idx] = value;
        self.m68k_idx += 1;

        if self.m68k_idx == DMA_FIFO_LEN {
            self.ready[self.m68k_block] = true;
            self.m68k_block ^= 1;
            self.m68k_idx = 0;
        }
    }

    pub fn pop(&mut self) -> u16 {
        let value = self.blocks[self.sh2_block][self.sh2_idx];
        self.sh2_idx += 1;

        if self.sh2_idx == DMA_FIFO_LEN {
            self.ready[self.sh2_block] = false;
            self.sh2_block ^= 1;
            self.sh2_idx = 0;
        }

        log::trace!("DMA FIFO pop: {value:04X}");

        value
    }

    pub fn sh2_is_empty(&self) -> bool {
        !self.ready[self.sh2_block]
    }

    pub fn is_full(&self) -> bool {
        self.ready[self.m68k_block]
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct DmaRegisters {
    pub rom_to_vram_dma: bool,
    // TODO not sure what this does - seems like maybe something to do with Sega CD?
    pub bit_1: bool,
    pub active: bool,
    pub source_address: u32,
    pub destination_address: u32,
    pub length: u16,
    pub fifo: DmaFifo,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SystemRegisters {
    pub adapter_enabled: bool,
    pub reset_sh2: bool,
    pub vdp_access: Access,
    pub m68k_rom_bank: u8,
    pub communication_ports: [u16; 8],
    pub master_interrupts: Sh2Interrupts,
    pub slave_interrupts: Sh2Interrupts,
    pub dma: DmaRegisters,
    // Functionality not emulated, only this bit being R/W
    pub sega_tv_bit: bool,
}

impl SystemRegisters {
    pub fn new() -> Self {
        Self {
            adapter_enabled: false,
            reset_sh2: true,
            vdp_access: Access::M68k,
            m68k_rom_bank: 0,
            communication_ports: array::from_fn(|_| 0),
            master_interrupts: Sh2Interrupts::default(),
            slave_interrupts: Sh2Interrupts::default(),
            dma: DmaRegisters::default(),
            sega_tv_bit: false,
        }
    }

    pub fn notify_vblank(&mut self) {
        self.master_interrupts.v_pending |= self.master_interrupts.v_enabled;
        self.slave_interrupts.v_pending |= self.slave_interrupts.v_enabled;

        self.master_interrupts.update_interrupt_level();
        self.slave_interrupts.update_interrupt_level();
    }

    pub fn notify_h_interrupt(&mut self) {
        self.master_interrupts.h_pending |= self.master_interrupts.h_enabled;
        self.slave_interrupts.h_pending |= self.slave_interrupts.h_enabled;

        self.master_interrupts.update_interrupt_level();
        self.slave_interrupts.update_interrupt_level();
    }

    pub fn notify_pwm_timer(&mut self) {
        self.master_interrupts.pwm_pending |= self.master_interrupts.pwm_enabled;
        self.slave_interrupts.pwm_pending |= self.slave_interrupts.pwm_enabled;

        self.master_interrupts.update_interrupt_level();
        self.slave_interrupts.update_interrupt_level();
    }

    pub fn reset(&mut self) {
        self.master_interrupts.reset_pending = true;
        self.slave_interrupts.reset_pending = true;

        self.master_interrupts.update_interrupt_level();
        self.slave_interrupts.update_interrupt_level();
    }

    pub fn m68k_read(&mut self, address: u32) -> u16 {
        match address {
            0xA15100 => self.read_adapter_control(),
            0xA15102 => self.read_interrupt_control(),
            0xA15104 => self.read_68k_rom_bank(),
            0xA15106 => self.m68k_read_dreq_control(),
            0xA15108 => self.read_dreq_source_high(),
            0xA1510A => self.read_dreq_source_low(),
            0xA1510C => self.read_dreq_destination_high(),
            0xA1510E => self.read_dreq_destination_low(),
            0xA15110 => self.dma.length,
            0xA1511A => self.sega_tv_bit.into(),
            0xA15120..=0xA1512F => self.read_communication_port(address),
            _ => {
                log::warn!("M68K invalid register read: {address:06X}");
                0
            }
        }
    }

    pub fn m68k_write_byte(&mut self, address: u32, value: u8) {
        let mut word = self.m68k_read(address & !1);
        if !address.bit(0) {
            word.set_msb(value);
        } else {
            word.set_lsb(value);
        }
        self.m68k_write(address & !1, word);
    }

    pub fn m68k_write(&mut self, address: u32, value: u16) {
        match address {
            0xA15100 => self.write_adapter_control(value),
            0xA15102 => self.write_interrupt_control(value),
            0xA15104 => self.write_68k_rom_bank(value),
            0xA15106 => self.write_dreq_control(value),
            0xA15108 => self.write_dreq_source_high(value),
            0xA1510A => self.write_dreq_source_low(value),
            0xA1510C => self.write_dreq_destination_high(value),
            0xA1510E => self.write_dreq_destination_low(value),
            0xA15110 => self.write_dreq_length(value),
            0xA15112 => self.write_dreq_fifo(value),
            0xA1511A => {
                self.sega_tv_bit = value.bit(0);
            }
            0xA15120..=0xA1512F => self.write_communication_port(address, value),
            0xA15130..=0xA15138 => {
                log::warn!("Ignoring PWM register write: {address:06X} {value:04X}");
            }
            _ => log::warn!("M68K invalid register write: {address:06X} {value:04X}"),
        }
    }

    pub fn sh2_read(&mut self, address: u32, which: WhichCpu, vdp: &Vdp) -> u16 {
        match address {
            0x4000 => self.read_interrupt_mask(which, vdp),
            0x4004 => vdp.h_interrupt_interval(),
            0x4006 => self.sh2_read_dreq_control(),
            0x4008 => self.read_dreq_source_high(),
            0x400A => self.read_dreq_source_low(),
            0x400C => self.read_dreq_destination_high(),
            0x400E => self.read_dreq_destination_low(),
            0x4010 => self.dma.length,
            0x4012 => self.read_dreq_fifo(),
            // TODO these registers shouldn't be readable? (interrupt clear)
            0x4014 | 0x4016 | 0x4018 | 0x401A | 0x401C => 0,
            0x4020..=0x402F => self.read_communication_port(address),
            _ => {
                log::warn!("SH-2 invalid register read: {address:08X} {which:?}");
                0
            }
        }
    }

    pub fn sh2_write_byte(&mut self, address: u32, value: u8, which: WhichCpu, vdp: &mut Vdp) {
        let mut word = self.sh2_read(address & !1, which, vdp);
        if !address.bit(0) {
            word.set_msb(value);
        } else {
            word.set_lsb(value);
        }
        self.sh2_write(address & !1, word, which, vdp);
    }

    pub fn sh2_write(&mut self, address: u32, value: u16, which: WhichCpu, vdp: &mut Vdp) {
        match address {
            0x4000 => self.write_interrupt_mask(value, which, vdp),
            0x4004 => vdp.write_h_interrupt_interval(value),
            0x4014 => self.clear_reset_interrupt(which),
            0x4016 => self.clear_v_interrupt(which),
            0x4018 => self.clear_h_interrupt(which),
            0x401A => self.clear_command_interrupt(which),
            0x401C => self.clear_pwm_interrupt(which),
            0x4020..=0x402F => self.write_communication_port(address, value),
            _ => log::warn!("SH-2 invalid register write: {address:08X} {value:04X} {which:?}"),
        }
    }

    // 68000: $A15100
    fn read_adapter_control(&self) -> u16 {
        // TODO bit 7? (REN / reset enabled)
        ((self.vdp_access as u16) << 15)
            | (1 << 7)
            | (u16::from(!self.reset_sh2) << 1)
            | u16::from(self.adapter_enabled)
    }

    // 68000: $A15100
    fn write_adapter_control(&mut self, value: u16) {
        self.adapter_enabled = value.bit(0);
        self.reset_sh2 = !value.bit(1);
        self.vdp_access = Access::from_bit(value.bit(15));

        log::trace!("Adapter control write: {value:04X}");
        log::trace!("  32X adapter enabled: {}", self.adapter_enabled);
        log::trace!("  Reset SH-2: {}", self.reset_sh2);
        log::trace!("  32X VDP access: {}", self.vdp_access);
    }

    // 68000: $A15102
    fn read_interrupt_control(&self) -> u16 {
        (u16::from(self.slave_interrupts.command_pending) << 1)
            | u16::from(self.master_interrupts.command_pending)
    }

    // 68000: $A15102
    fn write_interrupt_control(&mut self, value: u16) {
        self.master_interrupts.command_pending = value.bit(0);
        self.slave_interrupts.command_pending = value.bit(1);

        self.master_interrupts.update_interrupt_level();
        self.slave_interrupts.update_interrupt_level();

        log::trace!("Interrupt control write: {value:04X}");
        log::trace!("  Master command interrupt: {}", self.master_interrupts.command_pending);
        log::trace!("  Slave command interrupt: {}", self.slave_interrupts.command_pending);
    }

    // 68000: $A15104
    fn read_68k_rom_bank(&self) -> u16 {
        self.m68k_rom_bank.into()
    }

    // 68000: $A15104
    fn write_68k_rom_bank(&mut self, value: u16) {
        self.m68k_rom_bank = (value & 0x03) as u8;
        log::trace!("68000 ROM bank: {}", self.m68k_rom_bank);
    }

    // 68000: $A15106
    fn m68k_read_dreq_control(&self) -> u16 {
        (u16::from(self.dma.fifo.is_full()) << 7)
            | (u16::from(self.dma.active) << 2)
            | (u16::from(self.dma.bit_1) << 1)
            | u16::from(self.dma.rom_to_vram_dma)
    }

    // SH-2: $4006
    fn sh2_read_dreq_control(&self) -> u16 {
        (u16::from(self.dma.fifo.is_full()) << 15)
            | (u16::from(self.dma.fifo.sh2_is_empty()) << 14)
            | (u16::from(self.dma.active) << 2)
            | (u16::from(self.dma.bit_1) << 1)
            | u16::from(self.dma.rom_to_vram_dma)
    }

    // 68000: $A15106
    fn write_dreq_control(&mut self, value: u16) {
        self.dma.rom_to_vram_dma = value.bit(0);
        self.dma.bit_1 = value.bit(1);
        self.dma.active = value.bit(2);

        if !self.dma.active {
            self.dma.fifo.clear();
        }

        log::trace!("DREQ control write: {value:04X}");
        log::trace!("  ROM-to-VRAM DMA active: {}", self.dma.rom_to_vram_dma);
        log::trace!("  DMA active: {}", self.dma.active);
    }

    // 68000: $A15108
    // SH-2: $4008
    fn read_dreq_source_high(&self) -> u16 {
        (self.dma.source_address >> 16) as u16
    }

    // 68000: $A1510A
    // SH-2: $400A
    fn read_dreq_source_low(&self) -> u16 {
        self.dma.source_address as u16
    }

    // 68000: $A15108
    fn write_dreq_source_high(&mut self, value: u16) {
        self.dma.source_address.set_high_byte(value as u8);

        log::trace!("DREQ source address high write: {value:04X}");
        log::trace!("  New address: {:06X}", self.dma.source_address);
    }

    // 68000: $A1510A
    fn write_dreq_source_low(&mut self, value: u16) {
        self.dma.source_address = (self.dma.source_address & 0xFFFF0000) | u32::from(value);

        log::trace!("DREQ source address low write: {value:04X}");
        log::trace!("  New address: {:06X}", self.dma.source_address);
    }

    // 68000: $A1510C
    // SH-2: $410C
    fn read_dreq_destination_high(&self) -> u16 {
        (self.dma.destination_address >> 16) as u16
    }

    // 68000: $A1510E
    // SH-2: $410E
    fn read_dreq_destination_low(&self) -> u16 {
        self.dma.destination_address as u16
    }

    // 68000: $A1510C
    fn write_dreq_destination_high(&mut self, value: u16) {
        self.dma.destination_address.set_high_byte(value as u8);

        log::trace!("DREQ destination address high write: {value:04X}");
        log::trace!("  New address: {:06X}", self.dma.destination_address);
    }

    // 68000: $A1510E
    fn write_dreq_destination_low(&mut self, value: u16) {
        self.dma.destination_address =
            (self.dma.destination_address & 0xFFFF0000) | u32::from(value);

        log::trace!("DREQ destination address low write: {value:04X}");
        log::trace!("  New address: {:06X}", self.dma.destination_address);
    }

    // 68000: $A15110
    fn write_dreq_length(&mut self, value: u16) {
        // Lowest 2 bits are forced to 0
        self.dma.length = value & !3;
        log::trace!("DREQ length: {:04X}", self.dma.length);
    }

    // SH-2: $4012
    fn read_dreq_fifo(&mut self) -> u16 {
        self.dma.length = self.dma.length.wrapping_sub(1);
        if self.dma.length == 0 {
            self.dma.active = false;
        }

        self.dma.fifo.pop()
    }

    // 68000: $A15112
    fn write_dreq_fifo(&mut self, value: u16) {
        // Only push to the DMA FIFO if 68000-to-32X DMA is currently active. Virtua Racing Deluxe
        // depends on this or else it will crash after the title screen.
        // It does 68000-to-32X DMAs of length 64 while consistently pushing 65 words into the FIFO
        // for each DMA, and it depends on the 65th word never getting transferred.
        if self.dma.active {
            self.dma.fifo.push(value);
        }
    }

    // SH-2: $4000
    fn read_interrupt_mask(&self, which: WhichCpu, vdp: &Vdp) -> u16 {
        let mask_bits: u16 = match which {
            WhichCpu::Master => self.master_interrupts.mask_bits(),
            WhichCpu::Slave => self.slave_interrupts.mask_bits(),
        };

        // Bit 8 (Cartridge not inserted) hardcoded to 0
        ((self.vdp_access as u16) << 15)
            | (u16::from(self.adapter_enabled) << 9)
            | (u16::from(vdp.hen_bit()) << 7)
            | mask_bits
    }

    // SH-2: $4000
    pub fn write_interrupt_mask(&mut self, value: u16, which: WhichCpu, vdp: &mut Vdp) {
        self.vdp_access = Access::from_bit(value.bit(15));

        vdp.write_hen_bit(value.bit(7));

        match which {
            WhichCpu::Master => self.master_interrupts.write_mask_bits(value),
            WhichCpu::Slave => self.slave_interrupts.write_mask_bits(value),
        }

        log::trace!("Interrupt mask write ({which:?}): {value:04X}");
        log::trace!("  VDP access: {:?}", self.vdp_access);
        log::trace!("  HINT during VBlank: {}", vdp.hen_bit());

        if log::log_enabled!(log::Level::Trace) {
            let interrupts = match which {
                WhichCpu::Master => &self.master_interrupts,
                WhichCpu::Slave => &self.slave_interrupts,
            };

            log::trace!("  V interrupt enabled: {}", interrupts.v_enabled);
            log::trace!("  H interrupt enabled: {}", interrupts.h_enabled);
            log::trace!("  Command interrupt enabled: {}", interrupts.command_enabled);
            log::trace!("  PWM interrupt enabled: {}", interrupts.pwm_enabled);
        }
    }

    // SH-2: $4014
    fn clear_reset_interrupt(&mut self, which: WhichCpu) {
        match which {
            WhichCpu::Master => self.master_interrupts.clear_reset(),
            WhichCpu::Slave => self.slave_interrupts.clear_reset(),
        }
        log::trace!("VRESINT cleared");
    }

    // SH-2: $4016
    fn clear_v_interrupt(&mut self, which: WhichCpu) {
        match which {
            WhichCpu::Master => self.master_interrupts.clear_v(),
            WhichCpu::Slave => self.slave_interrupts.clear_v(),
        }
        log::trace!("VINT cleared");
    }

    // SH-2: $4018
    fn clear_h_interrupt(&mut self, which: WhichCpu) {
        match which {
            WhichCpu::Master => self.master_interrupts.clear_h(),
            WhichCpu::Slave => self.slave_interrupts.clear_h(),
        }
    }

    // SH-2: $401A
    fn clear_command_interrupt(&mut self, which: WhichCpu) {
        match which {
            WhichCpu::Master => self.master_interrupts.clear_command(),
            WhichCpu::Slave => self.slave_interrupts.clear_command(),
        }
        log::trace!("CMDINT cleared");
    }

    // SH-2: $401C
    fn clear_pwm_interrupt(&mut self, which: WhichCpu) {
        match which {
            WhichCpu::Master => self.master_interrupts.clear_pwm(),
            WhichCpu::Slave => self.slave_interrupts.clear_pwm(),
        }
        log::trace!("PWMINT cleared");
    }

    // 68000: $A15120-$A1512F
    // SH-2: $4020-$402F
    fn read_communication_port(&self, address: u32) -> u16 {
        let idx = (address >> 1) & 0x7;
        self.communication_ports[idx as usize]
    }

    // 68000: $A15120-$A1512F
    // SH-2: $4020-$402F
    fn write_communication_port(&mut self, address: u32, value: u16) {
        let idx = (address >> 1) & 0x7;
        self.communication_ports[idx as usize] = value;
    }
}
