//! Sega Master System / Game Gear memory map

mod mappers;
mod metadata;

use crate::SmsGgHardware;
use crate::memory::mappers::Mapper;
use bincode::{Decode, Encode};
use crc::Crc;
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::{FakeDecode, FakeEncode, PartialClone};
use smsgg_config::SmsGgRegion;
use std::ops::Index;
use std::{array, mem};

const SYSTEM_RAM_SIZE: usize = 8 * 1024;

// Most cartridges with RAM only had 8KB, but up to 32KB was supported, and the header contains
// no information on RAM size (or even whether RAM is present)
const CARTRIDGE_RAM_SIZE: usize = 32 * 1024;

const CRC: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);

#[derive(Debug, Clone, Default, FakeEncode, FakeDecode)]
struct Rom(Vec<u8>);

impl Index<usize> for Rom {
    type Output = <Vec<u8> as Index<usize>>::Output;

    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
struct Cartridge {
    #[partial_clone(default)]
    rom: Rom,
    ram: Vec<u8>,
    mapper: Mapper,
    has_battery: bool,
    ram_dirty: bool,
}

impl Cartridge {
    fn new(mut rom: Vec<u8>, initial_ram: Option<Vec<u8>>) -> Self {
        let mapper = Mapper::detect_from_rom(&rom);
        log::info!("Detected mapper {} from ROM header", mapper.name());

        let checksum = CRC.checksum(&rom);
        log::info!("ROM CRC32: {checksum:08X}");

        let has_battery = metadata::has_battery_backup(checksum);
        log::info!("Cartridge has battery-backed RAM: {has_battery}");

        let ram = match initial_ram {
            Some(ram) if ram.len() == CARTRIDGE_RAM_SIZE => {
                log::info!("Successfully loaded cartridge SRAM");
                ram
            }
            _ => vec![0; CARTRIDGE_RAM_SIZE],
        };

        jgenesis_common::rom::mirror_to_next_power_of_two(&mut rom);

        Self { rom: Rom(rom), ram, mapper, has_battery, ram_dirty: false }
    }

    fn read(&self, address: u16) -> u8 {
        self.mapper.read(address, &self.rom.0, &self.ram)
    }

    fn write(&mut self, address: u16, value: u8) {
        self.mapper.write(address, value, &mut self.ram, &mut self.ram_dirty);
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct AudioControl {
    fm_enabled: bool,
    psg_enabled: bool,
}

impl Default for AudioControl {
    fn default() -> Self {
        Self { fm_enabled: false, psg_enabled: true }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct MemoryControl {
    pub cartridge_enabled: bool,
    pub bios_enabled: bool,
}

impl MemoryControl {
    pub fn new(bios_rom: Option<&Vec<u8>>) -> Self {
        match bios_rom {
            Some(_) => Self { cartridge_enabled: false, bios_enabled: true },
            None => Self { cartridge_enabled: true, bios_enabled: false },
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct GameGearRegisters {
    pub ext_port: u8,
    pub parallel_port: u8,
    // TODO emulate serial port registers
}

impl GameGearRegisters {
    fn new() -> Self {
        Self { ext_port: 0x7F, parallel_port: 0xFF }
    }
}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct Memory {
    #[partial_clone(partial)]
    cartridge: Cartridge,
    bios_rom: Option<Vec<u8>>,
    bios_rom_banks: [u32; 3],
    ram: Box<[u8; SYSTEM_RAM_SIZE]>,
    memory_control: MemoryControl,
    audio_control: AudioControl,
    gg_registers: GameGearRegisters,
    hardware: SmsGgHardware,
}

impl Memory {
    pub fn new(
        rom: Vec<u8>,
        bios_rom: Option<Vec<u8>>,
        initial_cartridge_ram: Option<Vec<u8>>,
        hardware: SmsGgHardware,
    ) -> Self {
        let memory_control = MemoryControl::new(bios_rom.as_ref());

        let mut ram = Box::new(array::from_fn(|_| 0));
        if bios_rom.is_none() {
            // Some BIOS versions write their last port $3E write to the beginning of RAM; mimic
            // this if no BIOS is present
            //   0xAB = cartridge port enabled, RAM enabled, everything else disabled
            ram[0] = 0xAB;
        }

        Self {
            cartridge: Cartridge::new(rom, initial_cartridge_ram),
            bios_rom,
            bios_rom_banks: [0, 1, 2],
            ram,
            memory_control,
            audio_control: AudioControl::default(),
            gg_registers: GameGearRegisters::new(),
            hardware,
        }
    }

    pub fn read(&self, address: u16) -> u8 {
        match address {
            0x0000..=0xBFFF => {
                match self.hardware {
                    SmsGgHardware::MasterSystem => {
                        if self.memory_control.cartridge_enabled {
                            let cartridge_byte = self.cartridge.read(address);
                            if self.memory_control.bios_enabled {
                                // Cartridge and BIOS are both enabled; return logical AND of their bytes
                                let bios_byte = self.read_bios_sms(address);
                                cartridge_byte & bios_byte
                            } else {
                                cartridge_byte
                            }
                        } else if self.memory_control.bios_enabled {
                            self.read_bios_sms(address)
                        } else {
                            log::debug!(
                                "Slot read ${address:04X} with neither cartridge nor BIOS enabled"
                            );
                            0xFF
                        }
                    }
                    SmsGgHardware::GameGear => {
                        // Cartridge is always enabled on Game Gear
                        // BIOS is mapped to $0000-$03FF if enabled
                        if self.memory_control.bios_enabled && address <= 0x03FF {
                            self.bios_rom
                                .as_ref()
                                .and_then(|bios| bios.get(address as usize))
                                .copied()
                                .unwrap_or(0xFF)
                        } else {
                            self.cartridge.read(address)
                        }
                    }
                }
            }
            0xC000..=0xFFFF => {
                let ram_addr = address & 0x1FFF;
                self.ram[ram_addr as usize]
            }
        }
    }

    fn read_bios_sms(&self, address: u16) -> u8 {
        let Some(bios_rom) = &self.bios_rom else {
            log::debug!("BIOS ROM read ${address:04X} with no BIOS");
            return 0xFF;
        };

        let bios_addr: u32 = if bios_rom.len() > 32 * 1024 {
            match address {
                0x0000..=0x03FF => address.into(),
                0x0400..=0xBFFF => {
                    let rom_bank_idx = address / 0x4000;
                    let rom_bank = self.bios_rom_banks[rom_bank_idx as usize];
                    (rom_bank << 14) | u32::from(address & 0x3FFF)
                }
                0xC000..=0xFFFF => panic!("Invalid BIOS address: {address:04X}"),
            }
        } else {
            address.into()
        };

        bios_rom[(bios_addr as usize) & (bios_rom.len() - 1)]
    }

    pub fn write(&mut self, address: u16, value: u8) {
        if address >= 0xC000 {
            let ram_addr = address & 0x1FFF;
            self.ram[ram_addr as usize] = value;
        }

        if self.memory_control.bios_enabled && (0xFFFD..=0xFFFF).contains(&address) {
            log::debug!("BIOS ROM bank {} set to {value:02X}", address - 0xFFFD);
            self.bios_rom_banks[(address - 0xFFFD) as usize] = value.into();
        }

        if self.memory_control.cartridge_enabled {
            self.cartridge.write(address, value);
        }

        // If a game ever writes to cartridge RAM, assume it is battery-backed (since this isn't in
        // the cartridge header)
        self.cartridge.has_battery |= self.cartridge.ram_dirty;
    }

    pub fn cartridge_ram(&self) -> &[u8] {
        &self.cartridge.ram
    }

    pub fn cartridge_has_battery(&self) -> bool {
        self.cartridge.has_battery
    }

    pub fn cartridge_ram_dirty(&self) -> bool {
        self.cartridge.ram_dirty
    }

    pub fn clear_cartridge_ram_dirty(&mut self) {
        self.cartridge.ram_dirty = false;
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        self.cartridge.rom = mem::take(&mut other.cartridge.rom);
    }

    pub fn reset(&mut self) {
        *self = Self::new(
            mem::take(&mut self.cartridge.rom.0),
            mem::take(&mut self.bios_rom),
            Some(mem::take(&mut self.cartridge.ram)),
            self.hardware,
        );
    }

    pub fn fm_enabled(&self) -> bool {
        self.audio_control.fm_enabled
    }

    pub fn psg_enabled(&self) -> bool {
        self.audio_control.psg_enabled
    }

    pub fn read_audio_control(&self) -> u8 {
        match (self.audio_control.fm_enabled, self.audio_control.psg_enabled) {
            (false, true) => 0x00,
            (true, false) => 0x01,
            (false, false) => 0x02,
            (true, true) => 0x03,
        }
    }

    pub fn write_audio_control(&mut self, value: u8) {
        let control_bits = value & 0x03;
        self.audio_control.fm_enabled = control_bits.bit(0);
        self.audio_control.psg_enabled = control_bits == 0 || control_bits == 3;
    }

    pub fn guess_cartridge_region(&self) -> SmsGgRegion {
        const POSSIBLE_HEADER_LOCATIONS: [usize; 3] = [0x7FF0, 0x3FF0, 0x1FF0];

        let rom = &self.cartridge.rom.0;
        for header_start in POSSIBLE_HEADER_LOCATIONS {
            if rom.len() < header_start + 16 {
                // ROM is too small for the header to be here
                continue;
            }

            // The first 8 bytes of a valid header should be the string "TMR SEGA"
            if &rom[header_start..header_start + 8] != b"TMR SEGA" {
                continue;
            }

            // Intentionally don't validate checksum; some games have invalid checksums in their headers

            let region_code = rom[header_start + 15] >> 4;
            match region_code {
                // SMS Domestic / GG Domestic
                3 | 5 => return SmsGgRegion::Domestic,
                // SMS Export / GG Export / GG International
                4 | 6 | 7 => return SmsGgRegion::International,
                _ => {
                    log::warn!("Unexpected region code in cartridge header: {region_code:X}");
                }
            }
        }

        // If no valid header was found, assume region Domestic/Japan
        // Every GG game and non-JP SMS game should have a header, but some JP SMS games do not
        SmsGgRegion::Domestic
    }

    pub fn gg_registers(&mut self) -> &mut GameGearRegisters {
        &mut self.gg_registers
    }

    pub fn memory_control(&mut self) -> &mut MemoryControl {
        &mut self.memory_control
    }
}
