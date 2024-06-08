use crate::bus;
use crate::bus::WhichCpu;
use bincode::{Decode, Encode};
use jgenesis_common::num::{GetBit, U16Ext, U24Ext};
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
    pub h_in_vblank: bool,
    pub command_pending: bool,
    pub command_enabled: bool,
    pub pwm_pending: bool,
    pub pwm_enabled: bool,
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
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct DmaRegisters {
    pub rom_to_vram_dma: bool,
    pub active: bool,
    pub source_address: u32,
    pub destination_address: u32,
    pub length: u16,
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
    pub m68k_h_int_vector: u32,
}

impl SystemRegisters {
    pub fn new() -> Self {
        Self {
            adapter_enabled: false,
            reset_sh2: true,
            vdp_access: Access::M68k,
            m68k_rom_bank: 0,
            communication_ports: [0; 8],
            master_interrupts: Sh2Interrupts::default(),
            slave_interrupts: Sh2Interrupts::default(),
            dma: DmaRegisters::default(),
            m68k_h_int_vector: u32::from_be_bytes(
                bus::M68K_VECTORS[0x70..0x74].try_into().unwrap(),
            ),
        }
    }

    pub fn m68k_read(&mut self, address: u32) -> u16 {
        match address {
            0xA15100 => self.read_adapter_control(),
            0xA15104 => self.read_68k_rom_bank(),
            0xA15106 => self.read_dreq_control(),
            0xA15120..=0xA1512F => self.read_communication_port(address),
            _ => todo!("M68K register read: {address:06X}"),
        }
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
            0xA15120..=0xA1512F => self.write_communication_port(address, value),
            0xA15130..=0xA15138 => {
                log::warn!("Ignoring PWM register write: {address:06X} {value:04X}")
            }
            _ => todo!("M68K register write: {address:06X} {value:04X}"),
        }
    }

    pub fn sh2_read(&mut self, address: u32, which: WhichCpu) -> u16 {
        match address {
            0x4000 => self.read_interrupt_mask(which),
            0x4020..=0x402F => self.read_communication_port(address),
            _ => todo!("SH-2 register read: {address:08X} {which:?}"),
        }
    }

    pub fn sh2_write(&mut self, address: u32, value: u16, which: WhichCpu) {
        match address {
            0x4000 => self.write_interrupt_mask(value, which),
            0x4014 => self.clear_reset_interrupt(which),
            0x401A => self.clear_command_interrupt(which),
            0x4020..=0x402F => self.write_communication_port(address, value),
            _ => todo!("SH-2 register write: {address:08X} {value:04X} {which:?}"),
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
    fn write_interrupt_control(&mut self, value: u16) {
        self.master_interrupts.command_pending = value.bit(0);
        self.slave_interrupts.command_pending = value.bit(1);

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
    fn read_dreq_control(&self) -> u16 {
        // TODO bit 7 (full) hardcoded to 0
        (u16::from(self.dma.active) << 2) | u16::from(self.dma.rom_to_vram_dma)
    }

    // 68000: $A15106
    fn write_dreq_control(&mut self, value: u16) {
        self.dma.rom_to_vram_dma = value.bit(0);
        self.dma.active = value.bit(2);

        log::trace!("DREQ control write: {value:04X}");
        log::trace!("  ROM-to-VRAM DMA active: {}", self.dma.rom_to_vram_dma);
        log::trace!("  DMA active: {}", self.dma.active);
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

    // SH-2: $4000
    fn read_interrupt_mask(&self, which: WhichCpu) -> u16 {
        let mask_bits: u16 = match which {
            WhichCpu::Master => self.master_interrupts.mask_bits(),
            WhichCpu::Slave => self.slave_interrupts.mask_bits(),
        };

        // Bit 8 (Cartridge not inserted) hardcoded to 0
        ((self.vdp_access as u16) << 15)
            | (u16::from(self.adapter_enabled) << 9)
            | (u16::from(self.master_interrupts.h_in_vblank) << 7)
            | mask_bits
    }

    // SH-2: $4000
    pub fn write_interrupt_mask(&mut self, value: u16, which: WhichCpu) {
        self.vdp_access = Access::from_bit(value.bit(15));

        let h_in_vblank = value.bit(7);
        self.master_interrupts.h_in_vblank = h_in_vblank;
        self.slave_interrupts.h_in_vblank = h_in_vblank;

        match which {
            WhichCpu::Master => self.master_interrupts.write_mask_bits(value),
            WhichCpu::Slave => self.slave_interrupts.write_mask_bits(value),
        }

        log::trace!("Interrupt mask write: {value:04X}");
        log::trace!("  VDP access: {:?}", self.vdp_access);
        log::trace!("  HINT during VBlank: {h_in_vblank}");
        log::trace!(
            "  Interrupt mask bits: {:04b}",
            match which {
                WhichCpu::Master => self.master_interrupts.mask_bits(),
                WhichCpu::Slave => self.slave_interrupts.mask_bits(),
            }
        );
    }

    // SH-2: $4014
    fn clear_reset_interrupt(&mut self, which: WhichCpu) {
        match which {
            WhichCpu::Master => self.master_interrupts.reset_pending = false,
            WhichCpu::Slave => self.slave_interrupts.reset_pending = false,
        }
        log::trace!("VRESINT cleared");
    }

    // SH-2: $401A
    fn clear_command_interrupt(&mut self, which: WhichCpu) {
        match which {
            WhichCpu::Master => self.master_interrupts.command_pending = false,
            WhichCpu::Slave => self.slave_interrupts.command_pending = false,
        }
        log::trace!("CMDINT cleared");
    }

    // 68000: $A15120-$A1512E
    // SH-2: $4020-$402E
    fn read_communication_port(&self, address: u32) -> u16 {
        let idx = (address >> 1) & 0x7;
        self.communication_ports[idx as usize]
    }

    fn write_communication_port(&mut self, address: u32, value: u16) {
        let idx = (address >> 1) & 0x7;
        self.communication_ports[idx as usize] = value;

        log::trace!("Communication port {idx} write: {value:04X}");
    }
}
