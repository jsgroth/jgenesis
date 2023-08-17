// TODO remove
#![allow(clippy::match_same_arms)]

use crate::vdp::Vdp;
use bincode::de::{BorrowDecoder, Decoder};
use bincode::enc::Encoder;
use bincode::error::{DecodeError, EncodeError};
use bincode::{BorrowDecode, Decode, Encode};
use smsgg_core::psg::Psg;
use std::ops::Index;
use std::path::Path;
use std::{fs, io};
use thiserror::Error;

#[derive(Debug, Clone)]
struct Rom(Vec<u8>);

impl Rom {
    fn len(&self) -> usize {
        self.0.len()
    }
}

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

pub struct Memory {
    cartridge: Cartridge,
    main_ram: Vec<u8>,
    audio_ram: Vec<u8>,
}

impl Memory {
    pub fn new(cartridge: Cartridge) -> Self {
        Self {
            cartridge,
            main_ram: vec![0; MAIN_RAM_LEN],
            audio_ram: vec![0; AUDIO_RAM_LEN],
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
}

impl<'a> MainBus<'a> {
    pub fn new(memory: &'a mut Memory, vdp: &'a mut Vdp, psg: &'a mut Psg) -> Self {
        Self { memory, vdp, psg }
    }
}

// The Genesis has a 24-bit bus, not 32-bit
const ADDRESS_MASK: u32 = 0xFFFFFF;

impl<'a> m68000_emu::BusInterface for MainBus<'a> {
    fn read_byte(&mut self, address: u32) -> u8 {
        let address = address & ADDRESS_MASK;
        match address {
            0x000000..=0x3FFFFF => self.memory.cartridge.read_byte(address),
            0xA00000..=0xA0FFFF => {
                // TODO access to Z80 memory map
                0xFF
            }
            0xA10000..=0xA1001F => {
                // TODO I/O ports
                0xFF
            }
            0xA11000..=0xA11001 => {
                // TODO Z80 BUSREQ
                0xFF
            }
            0xA11100..=0xA11101 => {
                // TODO Z80 RESET
                0xFF
            }
            0xA13000..=0xA130FF => {
                // TODO timer registers
                0xFF
            }
            0xC00000..=0xC00003 => self.vdp.read_data() as u8,
            0xC00004..=0xC00007 => self.vdp.read_status() as u8,
            0xC00008..=0xC0000F => {
                // TODO HV counter
                0xFF
            }
            0xFF0000..=0xFFFFFF => self.memory.main_ram[(address & 0xFFFF) as usize],
            _ => 0xFF,
        }
    }

    fn read_word(&mut self, address: u32) -> u16 {
        let address = address & ADDRESS_MASK;
        match address {
            0x000000..=0x3FFFFF => self.memory.cartridge.read_word(address),
            0xA00000..=0xA0FFFF => {
                // TODO access to Z80 memory map
                0xFF
            }
            0xA10000..=0xA1001F => {
                // TODO I/O ports
                0xFF
            }
            0xA11000 => {
                // TODO Z80 BUSREQ
                0xFF
            }
            0xA11100 => {
                // TODO Z80 RESET
                0xFF
            }
            0xA13000..=0xA130FF => {
                // TODO timer registers
                0xFF
            }
            0xC00000..=0xC00003 => self.vdp.read_data(),
            0xC00004..=0xC00007 => self.vdp.read_status(),
            0xC00008..=0xC0000F => {
                // TODO HV counter
                0xFF
            }
            0xFF0000..=0xFFFFFF => {
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
        match address {
            0xA00000..=0xA0FFFF => {
                // TODO access to Z80 memory map
            }
            0xA10000..=0xA1001F => {
                // TODO I/O ports
            }
            0xA11000..=0xA11001 => {
                // TODO Z80 BUSREQ
            }
            0xA11100..=0xA11101 => {
                // TODO Z80 RESET
            }
            0xA13000..=0xA130FF => {
                // TODO timer registers
            }
            0xC00000..=0xC00003 => {
                self.vdp.write_data(value.into());
            }
            0xC00004..=0xC00007 => {
                self.vdp.write_control(value.into());
            }
            0xC00008..=0xC0000F => {
                // TODO HV counter
            }
            0xC00011 | 0xC00013 | 0xC00015 | 0xC00017 => {
                self.psg.write(value);
            }
            0xFF0000..=0xFFFFFF => {
                self.memory.main_ram[(address & 0xFFFF) as usize] = value;
            }
            _ => {}
        }
    }

    fn write_word(&mut self, address: u32, value: u16) {
        let address = address & ADDRESS_MASK;
        match address {
            0xA00000..=0xA0FFFF => {
                // TODO access to Z80 memory map
            }
            0xA10000..=0xA1001F => {
                // TODO I/O ports
            }
            0xA11000..=0xA11001 => {
                // TODO Z80 BUSREQ
            }
            0xA11100..=0xA11101 => {
                // TODO Z80 RESET
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
            0xFF0000..=0xFFFFFF => {
                let ram_addr = (address & 0xFFFF) as usize;
                self.memory.main_ram[ram_addr] = (value >> 8) as u8;
                self.memory.main_ram[(ram_addr + 1) & 0xFFFF] = value as u8;
            }
            _ => {}
        }
    }
}
