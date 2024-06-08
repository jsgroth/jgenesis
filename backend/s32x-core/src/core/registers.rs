use crate::core::bus::WhichCpu;
use bincode::{Decode, Encode};
use jgenesis_common::num::{GetBit, U16Ext};
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

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
pub struct Sh2InterruptMasks {
    pub vertical: bool,
    pub horizontal: bool,
    pub command: bool,
    pub pwm_timer: bool,
}

impl From<Sh2InterruptMasks> for u16 {
    fn from(value: Sh2InterruptMasks) -> Self {
        (u16::from(value.vertical) << 3)
            | (u16::from(value.horizontal) << 2)
            | (u16::from(value.command) << 1)
            | u16::from(value.pwm_timer)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SystemRegisters {
    pub adapter_enabled: bool,
    pub reset_sh2: bool,
    pub vdp_access: Access,
    pub communication_ports: [u16; 8],
    pub horizontal_interrupt_in_vblank: bool,
    pub master_interrupt_masks: Sh2InterruptMasks,
    pub slave_interrupt_masks: Sh2InterruptMasks,
}

impl SystemRegisters {
    pub fn new() -> Self {
        Self {
            adapter_enabled: false,
            reset_sh2: true,
            vdp_access: Access::M68k,
            communication_ports: [0; 8],
            horizontal_interrupt_in_vblank: false,
            master_interrupt_masks: Sh2InterruptMasks::default(),
            slave_interrupt_masks: Sh2InterruptMasks::default(),
        }
    }

    fn read_adapter_control(&self) -> u16 {
        // TODO bit 7? (REN / reset enabled)
        ((self.vdp_access as u16) << 15)
            | (1 << 7)
            | (u16::from(!self.reset_sh2) << 1)
            | u16::from(self.adapter_enabled)
    }

    fn write_adapter_control(&mut self, value: u16) {
        self.adapter_enabled = value.bit(0);
        self.reset_sh2 = !value.bit(1);
        self.vdp_access = Access::from_bit(value.bit(15));

        log::trace!("Adapter control write: {value:04X}");
        log::trace!("  32X adapter enabled: {}", self.adapter_enabled);
        log::trace!("  Reset SH-2: {}", self.reset_sh2);
        log::trace!("  32X VDP access: {}", self.vdp_access);
    }

    fn read_interrupt_mask(&self, which: WhichCpu) -> u16 {
        let mask_bits: u16 = match which {
            WhichCpu::Master => self.master_interrupt_masks.into(),
            WhichCpu::Slave => self.slave_interrupt_masks.into(),
        };

        // Bit 8 (Cartridge inserted, read-only) hardcoded to 1
        ((self.vdp_access as u16) << 15)
            | (u16::from(self.adapter_enabled) << 9)
            | (1 << 8)
            | (u16::from(self.horizontal_interrupt_in_vblank) << 7)
            | mask_bits
    }

    fn read_communication_port(&self, address: u32) -> u16 {
        let idx = (address >> 1) & 0x7;
        self.communication_ports[idx as usize]
    }

    fn write_communication_port_u16(&mut self, address: u32, value: u16) {
        let idx = (address >> 1) & 0x7;
        self.communication_ports[idx as usize] = value;

        log::trace!("Communication port {idx} write: {value:04X}");
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Sega32XRegisters {
    pub system: SystemRegisters,
}

impl Sega32XRegisters {
    pub fn new() -> Self {
        Self { system: SystemRegisters::new() }
    }

    pub fn m68k_read_byte(&mut self, address: u32) -> u8 {
        log::trace!("M68K byte read: {address:06X}");

        match address {
            0xA15100 | 0xA15101 => {
                let value = self.system.read_adapter_control();
                (value >> (8 * ((address & 1) ^ 1))) as u8
            }
            _ => todo!("M68K read byte {address:06X}"),
        }
    }

    pub fn m68k_write_byte(&mut self, address: u32, value: u8) {
        log::trace!("M68K byte write: {address:06X} {value:02X}");

        match address {
            0xA15100 => {
                let value_u16 =
                    u16::from_be_bytes([value, self.system.read_adapter_control().lsb()]);
                self.system.write_adapter_control(value_u16);
            }
            0xA15101 => {
                let value_u16 =
                    u16::from_be_bytes([self.system.read_adapter_control().msb(), value]);
                self.system.write_adapter_control(value_u16);
            }
            _ => todo!("M68K write byte {address:06X} {value:02X}"),
        }
    }

    pub fn m68k_write_word(&mut self, address: u32, value: u16) {
        log::trace!("M68K word write: {address:06X} {value:04X}");

        match address {
            0xA15120..=0xA1512F => self.system.write_communication_port_u16(address, value),
            _ => todo!("M68K write word {address:06X} {value:04X}"),
        }
    }

    pub fn sh2_read(&mut self, address: u32, which: WhichCpu) -> u16 {
        log::trace!("SH-2 read: {address:08X} {which:?}");

        match address {
            0x4000 => self.system.read_interrupt_mask(which),
            0x4020..=0x402F => self.system.read_communication_port(address),
            _ => todo!("SH-2 register read: {address:08X} {which:?}"),
        }
    }
}
