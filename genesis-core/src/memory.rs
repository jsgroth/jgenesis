// TODO remove
#![allow(clippy::match_same_arms)]

use crate::input::InputState;
use crate::vdp::Vdp;
use crate::ym2612::Ym2612;
use bincode::de::{BorrowDecoder, Decoder};
use bincode::enc::Encoder;
use bincode::error::{DecodeError, EncodeError};
use bincode::{BorrowDecode, Decode, Encode};
use smsgg_core::num::GetBit;
use smsgg_core::psg::Psg;
use std::ops::Index;
use std::path::Path;
use std::{fs, io};
use thiserror::Error;
use z80_emu::traits::InterruptLine;

#[derive(Debug, Clone)]
struct Rom(Vec<u8>);

impl Index<usize> for Rom {
    type Output = u8;

    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

impl Index<u32> for Rom {
    type Output = u8;

    fn index(&self, index: u32) -> &Self::Output {
        &self.0[index as usize]
    }
}

impl Encode for Rom {
    fn encode<E: Encoder>(&self, _encoder: &mut E) -> Result<(), EncodeError> {
        Ok(())
    }
}

impl Decode for Rom {
    fn decode<D: Decoder>(_decoder: &mut D) -> Result<Self, DecodeError> {
        Ok(Self(vec![]))
    }
}

impl<'de> BorrowDecode<'de> for Rom {
    fn borrow_decode<D: BorrowDecoder<'de>>(_decoder: &mut D) -> Result<Self, DecodeError> {
        Ok(Self(vec![]))
    }
}

#[derive(Debug, Error)]
pub enum CartridgeLoadError {
    #[error("I/O error loading cartridge file: {source}")]
    Io {
        #[from]
        source: io::Error,
    },
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Cartridge {
    rom: Rom,
    address_mask: u32,
}

impl Cartridge {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, CartridgeLoadError> {
        let bytes = fs::read(path)?;
        Ok(Self::from_rom(bytes))
    }

    pub fn from_rom(rom_bytes: Vec<u8>) -> Self {
        // TODO parse stuff out of header
        let address_mask = (rom_bytes.len() - 1) as u32;
        Self {
            rom: Rom(rom_bytes),
            address_mask,
        }
    }

    fn read_byte(&self, address: u32) -> u8 {
        self.rom[address & self.address_mask]
    }

    fn read_word(&self, address: u32) -> u16 {
        u16::from_be_bytes([
            self.read_byte(address),
            self.read_byte(address.wrapping_add(1)),
        ])
    }
}

const MAIN_RAM_LEN: usize = 64 * 1024;
const AUDIO_RAM_LEN: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Z80BankRegister {
    bank_number: u32,
    current_bit: u8,
}

impl Z80BankRegister {
    const BITS: u8 = 9;

    fn map_to_68k_address(self, z80_address: u16) -> u32 {
        (self.bank_number << 15) | u32::from(z80_address & 0x7FFF)
    }

    fn write_bit(&mut self, bit: bool) {
        let mask = 1 << self.current_bit;
        self.bank_number = (self.bank_number & !mask) | (u32::from(bit) << self.current_bit);
        self.current_bit = (self.current_bit + 1) % Self::BITS;
    }
}

#[derive(Debug, Clone, Default)]
struct Signals {
    z80_busreq: bool,
    z80_reset: bool,
}

#[derive(Debug, Clone)]
pub struct Memory {
    cartridge: Cartridge,
    main_ram: Vec<u8>,
    audio_ram: Vec<u8>,
    z80_bank_register: Z80BankRegister,
    signals: Signals,
}

impl Memory {
    pub fn new(cartridge: Cartridge) -> Self {
        Self {
            cartridge,
            main_ram: vec![0; MAIN_RAM_LEN],
            audio_ram: vec![0; AUDIO_RAM_LEN],
            z80_bank_register: Z80BankRegister::default(),
            signals: Signals::default(),
        }
    }

    pub fn read_word_for_dma(&self, address: u32) -> u16 {
        // TODO assuming that DMA can only read from ROM and 68k RAM
        match address {
            0x000000..=0x3FFFFF => self.cartridge.read_word(address),
            0xE00000..=0xFFFFFF => {
                let addr = (address & 0xFFFF) as usize;
                u16::from_be_bytes([
                    self.main_ram[addr],
                    self.main_ram[addr.wrapping_add(1) & 0xFFFF],
                ])
            }
            _ => 0xFF,
        }
    }

    pub fn read_rom_u32(&self, address: u32) -> u32 {
        let b3 = self.cartridge.rom[address];
        let b2 = self.cartridge.rom[address + 1];
        let b1 = self.cartridge.rom[address + 2];
        let b0 = self.cartridge.rom[address + 3];
        u32::from_be_bytes([b3, b2, b1, b0])
    }
}

pub struct MainBus<'a> {
    memory: &'a mut Memory,
    vdp: &'a mut Vdp,
    psg: &'a mut Psg,
    ym2612: &'a mut Ym2612,
    input: &'a mut InputState,
}

impl<'a> MainBus<'a> {
    pub fn new(
        memory: &'a mut Memory,
        vdp: &'a mut Vdp,
        psg: &'a mut Psg,
        ym2612: &'a mut Ym2612,
        input: &'a mut InputState,
    ) -> Self {
        Self {
            memory,
            vdp,
            psg,
            ym2612,
            input,
        }
    }

    fn read_io_register(&self, address: u32) -> u8 {
        match address {
            0xA10000 | 0xA10001 => 0xA0, // Version register
            0xA10002 | 0xA10003 => self.input.read_data(),
            0xA10008 | 0xA10009 => self.input.read_ctrl(),
            // TxData registers return 0xFF by default
            0xA1000E | 0xA1000F | 0xA10014 | 0xA10015 | 0xA1001A | 0xA1001B => 0xFF,
            // Other I/O registers return 0x00 by default
            _ => 0x00,
        }
    }

    fn write_io_register(&mut self, address: u32, value: u8) {
        match address {
            0xA10002 | 0xA10003 => {
                self.input.write_data(value);
            }
            0xA10008 | 0xA10009 => {
                self.input.write_ctrl(value);
            }
            _ => {}
        }
    }

    fn read_vdp_byte(&mut self, address: u32) -> u8 {
        match address & 0x1F {
            0x00 | 0x02 => (self.vdp.read_data() >> 8) as u8,
            0x01 | 0x03 => self.vdp.read_data() as u8,
            0x04 | 0x06 => (self.vdp.read_status() >> 8) as u8,
            0x05 | 0x07 => self.vdp.read_status() as u8,
            0x08..=0x0F => {
                todo!("HV counter")
            }
            0x10..=0x1F => {
                // PSG / unused space; PSG is not readable
                0xFF
            }
            _ => unreachable!("address & 0x1F is always <= 0x1F"),
        }
    }

    fn write_vdp_byte(&mut self, address: u32, value: u8) {
        match address & 0x1F {
            0x00..=0x03 => {
                self.vdp.write_data(value.into());
            }
            0x04..=0x07 => {
                self.vdp.write_control(value.into());
            }
            0x08..=0x0F => {
                todo!("HV counter")
            }
            0x11 | 0x13 | 0x15 | 0x17 => {
                self.psg.write(value);
            }
            0x10 | 0x12 | 0x14 | 0x16 | 0x18..=0x1F => {}
            _ => unreachable!("address & 0x1F is always <= 0x1F"),
        }
    }
}

// The Genesis has a 24-bit bus, not 32-bit
const ADDRESS_MASK: u32 = 0xFFFFFF;

impl<'a> m68000_emu::BusInterface for MainBus<'a> {
    fn read_byte(&mut self, address: u32) -> u8 {
        let address = address & ADDRESS_MASK;
        log::trace!("Main bus byte read, address={address:06X}");
        match address {
            0x000000..=0x3FFFFF => self.memory.cartridge.read_byte(address),
            0xA00000..=0xA0FFFF => {
                // Z80 memory map
                // For 68k access, $8000-$FFFF mirrors $0000-$7FFF
                <Self as z80_emu::BusInterface>::read_memory(self, (address & 0x7FFF) as u16)
            }
            0xA10000..=0xA1001F => self.read_io_register(address),
            0xA11100..=0xA11101 => {
                // TODO wait until Z80 has stalled?
                (!self.memory.signals.z80_busreq).into()
            }
            0xA13000..=0xA130FF => {
                todo!("timer register")
            }
            0xC00000..=0xC0001F => self.read_vdp_byte(address),
            0xE00000..=0xFFFFFF => self.memory.main_ram[(address & 0xFFFF) as usize],
            _ => 0xFF,
        }
    }

    fn read_word(&mut self, address: u32) -> u16 {
        let address = address & ADDRESS_MASK;
        log::trace!("Main bus word read, address={address:06X}");
        match address {
            0x000000..=0x3FFFFF => self.memory.cartridge.read_word(address),
            0xA00000..=0xA0FFFF => {
                // All Z80 access is byte-size; word reads mirror the byte in both MSB and LSB
                let byte = self.read_byte(address);
                u16::from_le_bytes([byte, byte])
            }
            0xA10000..=0xA1001F => self.read_io_register(address).into(),
            0xA11100..=0xA11101 => {
                // TODO wait until Z80 has stalled?
                (!self.memory.signals.z80_busreq).into()
            }
            0xA13000..=0xA130FF => {
                todo!("timer register")
            }
            0xC00000..=0xC00003 => self.vdp.read_data(),
            0xC00004..=0xC00007 => self.vdp.read_status(),
            0xC00008..=0xC0000F => {
                todo!("HV counter")
            }
            0xE00000..=0xFFFFFF => {
                let ram_addr = (address & 0xFFFF) as usize;
                u16::from_be_bytes([
                    self.memory.main_ram[ram_addr],
                    self.memory.main_ram[(ram_addr + 1) & 0xFFFF],
                ])
            }
            _ => 0xFFFF,
        }
    }

    fn write_byte(&mut self, address: u32, value: u8) {
        let address = address & ADDRESS_MASK;
        log::trace!("Main bus byte write: address={address:06X}, value={value:02X}");
        match address {
            0xA00000..=0xA0FFFF => {
                // Z80 memory map
                // For 68k access, $8000-$FFFF mirrors $0000-$7FFF
                <Self as z80_emu::BusInterface>::write_memory(
                    self,
                    (address & 0x7FFF) as u16,
                    value,
                );
            }
            0xA10000..=0xA1001F => {
                self.write_io_register(address, value);
            }
            0xA11100..=0xA11101 => {
                self.memory.signals.z80_busreq = value.bit(0);
            }
            0xA11200..=0xA11201 => {
                self.memory.signals.z80_reset = value.bit(0);
            }
            0xA13000..=0xA130FF => {
                // TODO timer registers
            }
            0xC00000..=0xC0001F => {
                self.write_vdp_byte(address, value);
            }
            0xE00000..=0xFFFFFF => {
                self.memory.main_ram[(address & 0xFFFF) as usize] = value;
            }
            _ => {}
        }
    }

    fn write_word(&mut self, address: u32, value: u16) {
        let address = address & ADDRESS_MASK;
        log::trace!("Main bus word write: address={address:06X}, value={value:02X}");
        match address {
            0xA00000..=0xA0FFFF => {
                // Z80 memory map; word-size writes write the MSB as a byte-size write
                self.write_byte(address, (value >> 8) as u8);
            }
            0xA10000..=0xA1001F => {
                self.write_io_register(address, value as u8);
            }
            0xA11100..=0xA11101 => {
                self.memory.signals.z80_busreq = value.bit(8);
                log::trace!("Set Z80 BUSREQ to {}", self.memory.signals.z80_busreq);
            }
            0xA11200..=0xA11201 => {
                self.memory.signals.z80_reset = value.bit(8);
                log::trace!("Set Z80 RESET to {}", self.memory.signals.z80_reset);
            }
            0xA13000..=0xA130FF => {
                // TODO timer registers
            }
            0xC00000..=0xC00003 => {
                self.vdp.write_data(value);
            }
            0xC00004..=0xC00007 => {
                self.vdp.write_control(value);
            }
            0xC00008..=0xC0000F => {
                // TODO HV counter
            }
            0xE00000..=0xFFFFFF => {
                let ram_addr = (address & 0xFFFF) as usize;
                self.memory.main_ram[ram_addr] = (value >> 8) as u8;
                self.memory.main_ram[(ram_addr + 1) & 0xFFFF] = value as u8;
            }
            _ => {}
        }
    }

    fn interrupt_level(&self) -> u8 {
        self.vdp.m68k_interrupt_level()
    }

    fn acknowledge_interrupt(&mut self) {
        self.vdp.acknowledge_m68k_interrupt();
    }
}

impl<'a> z80_emu::BusInterface for MainBus<'a> {
    fn read_memory(&mut self, address: u16) -> u8 {
        log::trace!("Z80 bus read from {address:04X}");

        match address {
            0x0000..=0x3FFF => {
                // Z80 RAM (mirrored at $2000-$3FFF)
                let address = address & 0x1FFF;
                self.memory.audio_ram[address as usize]
            }
            0x4000..=0x5FFF => {
                // YM2612 registers/ports (mirrored every 4 addresses)
                // All YM2612 reads function identically
                self.ym2612.read_register()
            }
            0x6000..=0x60FF => {
                // Bank number register
                // TODO what should this do on reads?
                0xFF
            }
            0x6100..=0x7EFF => {
                // Unused address space
                0xFF
            }
            0x7F00..=0x7F1F => {
                // VDP ports
                self.read_vdp_byte(address.into())
            }
            0x7F20..=0x7FFF => {
                // Invalid addresses
                0xFF
            }
            0x8000..=0xFFFF => {
                // TODO 68000 memory area
                let m68k_addr = self.memory.z80_bank_register.map_to_68k_address(address);
                if !(0xA00000..=0xA0FFFF).contains(&m68k_addr) {
                    <Self as m68000_emu::BusInterface>::read_byte(self, m68k_addr)
                } else {
                    // TODO this should lock up the system
                    panic!("Z80 attempted to read its own memory from the 68k bus; z80_addr={address:04X}, m68k_addr={m68k_addr:08X}");
                }
            }
        }
    }

    fn write_memory(&mut self, address: u16, value: u8) {
        log::trace!("Z80 bus write at {address:04X}");

        match address {
            0x0000..=0x3FFF => {
                // Z80 RAM (mirrored at $2000-$3FFF)
                let address = address & 0x1FFF;
                self.memory.audio_ram[address as usize] = value;
            }
            0x4000..=0x5FFF => {
                // YM2612 registers/ports (mirrored every 4 addresses)
                match address & 0x03 {
                    0x00 => {
                        self.ym2612.write_address_1(value);
                    }
                    0x01 => {
                        self.ym2612.write_data_1(value);
                    }
                    0x02 => {
                        self.ym2612.write_address_2(value);
                    }
                    0x03 => {
                        self.ym2612.write_data_2(value);
                    }
                    _ => unreachable!("value & 0x03 is always <= 0x03"),
                }
            }
            0x6000..=0x60FF => {
                self.memory.z80_bank_register.write_bit(value.bit(0));
            }
            0x6100..=0x7EFF | 0x7F20..=0x7FFF => {
                // Unused / invalid addresses
                // TODO writes to $7F20-$7FFF should halt the system
            }
            0x7F00..=0x7F1F => {
                // VDP addresses
                self.write_vdp_byte(address.into(), value);
            }
            0x8000..=0xFFFF => {
                let m68k_addr = self.memory.z80_bank_register.map_to_68k_address(address);
                if !(0xA00000..=0xA0FFFF).contains(&m68k_addr) {
                    <Self as m68000_emu::BusInterface>::write_byte(self, m68k_addr, value);
                } else {
                    // TODO this should lock up the system
                    panic!("Z80 attempted to read its own memory from the 68k bus; z80_addr={address:04X}, m68k_addr={m68k_addr:08X}");
                }
            }
        }
    }

    fn read_io(&mut self, _address: u16) -> u8 {
        // I/O ports are not wired up to the Z80
        0xFF
    }

    fn write_io(&mut self, _address: u16, _value: u8) {
        // I/O ports are not wired up to the Z80
    }

    fn nmi(&self) -> InterruptLine {
        // The NMI line is not connected to anything
        InterruptLine::High
    }

    fn int(&self) -> InterruptLine {
        self.vdp.z80_interrupt_line()
    }

    fn busreq(&self) -> bool {
        self.memory.signals.z80_busreq
    }

    fn reset(&self) -> bool {
        self.memory.signals.z80_reset
    }
}
