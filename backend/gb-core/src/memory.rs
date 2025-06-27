//! Game Boy internal memory

use crate::HardwareMode;
use crate::api::GameBoyLoadError;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::iter;

pub const DMG_BOOT_ROM_LEN: usize = 256;
pub const CGB_BOOT_ROM_LEN: usize = 256 + 2048;

const MAIN_RAM_LEN: usize = 32 * 1024;
const HRAM_LEN: usize = 127;

type MainRam = [u8; MAIN_RAM_LEN];
type Hram = [u8; HRAM_LEN];

// Initial contents of $FF80-$FFC2; the rest of HRAM is presumably randomized
// These values are normally written by the CGB boot ROM
#[rustfmt::skip]
const CGB_INITIAL_HRAM: &[u8; 67] = &[
    0x00, 0xFF, 0xFF, 0xFF, 0x7F, 0xFF, 0x7F, 0xFF, 0x7F, 0xFF, 0xFF, 0xFF, 0x7F, 0xFF, 0x7F, 0xFF,
    0x7F, 0xFF, 0xFF, 0xFF, 0x7F, 0xFF, 0x7F, 0xFF, 0x7F, 0xFF, 0xFF, 0xFF, 0x7F, 0xFF, 0x7F, 0xFF,
    0x7F, 0xFF, 0xFF, 0xFF, 0x7F, 0xFF, 0x7F, 0xFF, 0x7F, 0xFF, 0xFF, 0xFF, 0x7F, 0xFF, 0x7F, 0xFF,
    0x7F, 0xFF, 0xFF, 0xFF, 0x7F, 0xFF, 0x7F, 0xFF, 0x7F, 0xFF, 0xFF, 0xFF, 0x7F, 0xFF, 0x7F, 0xFF,
    0x7F, 0x00, 0x00,
];

#[derive(Debug, Clone, Encode, Decode)]
pub struct Memory {
    main_ram: Box<MainRam>,
    main_ram_bank: u8,
    hram: Box<Hram>,
    boot_rom: Box<[u8]>,
    boot_rom_present: bool,
    boot_rom_mapped: bool,
    hardware_mode: HardwareMode,
}

impl Memory {
    pub fn new(
        mut boot_rom: Option<Vec<u8>>,
        hardware_mode: HardwareMode,
    ) -> Result<Self, GameBoyLoadError> {
        let boot_rom_len = match hardware_mode {
            HardwareMode::Dmg => DMG_BOOT_ROM_LEN,
            HardwareMode::Cgb => CGB_BOOT_ROM_LEN,
        };

        // If boot ROM is present, validate that it's the expected length
        // Allow CGB boot ROMs to be either 2048 bytes or 2048+256 bytes (with a filler $01 page)
        if let Some(boot_rom) = &mut boot_rom {
            if boot_rom.len() != boot_rom_len {
                if hardware_mode == HardwareMode::Cgb && boot_rom.len() == boot_rom_len - 256 {
                    *boot_rom = pad_cgb_boot_rom(boot_rom);
                } else {
                    return Err(GameBoyLoadError::InvalidBootRomSize {
                        actual: boot_rom.len(),
                        expected: boot_rom_len,
                    });
                }
            }
        }

        let boot_rom_present = boot_rom.is_some();
        let boot_rom = boot_rom.unwrap_or_else(|| vec![0; boot_rom_len]).into_boxed_slice();

        let mut main_ram: Vec<u8> = iter::repeat_with(rand::random).take(MAIN_RAM_LEN).collect();
        let mut hram: Vec<u8> = iter::repeat_with(rand::random).take(HRAM_LEN).collect();

        if !boot_rom_present {
            // Randomize RAM contents at power-on except for bank 2, which the CGB boot ROM normally
            // zerofills. Worms Armageddon depends on bank 2 being zeroed out.
            // Hardware mode check is not necessary because banks 2-7 are not accessible on DMG
            main_ram[0x2000..0x3000].fill(0);

            hram.fill(0);
            if hardware_mode == HardwareMode::Cgb {
                hram[..CGB_INITIAL_HRAM.len()].copy_from_slice(CGB_INITIAL_HRAM);
            }
        }

        Ok(Self {
            main_ram: main_ram.into_boxed_slice().try_into().unwrap(),
            main_ram_bank: 0,
            hram: hram.into_boxed_slice().try_into().unwrap(),
            boot_rom,
            boot_rom_present,
            boot_rom_mapped: boot_rom_present,
            hardware_mode,
        })
    }

    pub fn read_main_ram(&self, address: u16) -> u8 {
        let ram_addr = map_ram_address(address, self.main_ram_bank);
        self.main_ram[ram_addr as usize]
    }

    pub fn write_main_ram(&mut self, address: u16, value: u8) {
        let ram_addr = map_ram_address(address, self.main_ram_bank);
        self.main_ram[ram_addr as usize] = value;
    }

    pub fn read_hram(&self, address: u16) -> u8 {
        self.hram[(address & 0x7F) as usize]
    }

    pub fn write_hram(&mut self, address: u16, value: u8) {
        self.hram[(address & 0x7F) as usize] = value;
    }

    pub fn try_read_boot_rom(&self, address: u16) -> Option<u8> {
        if !self.boot_rom_mapped {
            return None;
        }

        // DMG boot ROM is 256 bytes, mapped to $0000-$00FF
        // CGB boot ROM is 2048 bytes, first 256 mapped to $0000-$00FF and remaining 1792 mapped
        // to $0200-$08FF
        match (address, self.hardware_mode) {
            (0x0000..=0x00FF, _) | (0x0200..=0x08FF, HardwareMode::Cgb) => {
                Some(self.boot_rom[address as usize])
            }
            _ => None,
        }
    }

    pub fn read_svbk(&self) -> u8 {
        0xF8 | self.main_ram_bank
    }

    pub fn write_svbk(&mut self, value: u8) {
        self.main_ram_bank = value & 0x07;

        log::trace!("SVBK write: WRAM bank set to {}", self.main_ram_bank);
    }

    // $FF50: BANK (used only by boot ROM to unmap itself)
    pub fn write_bank(&mut self, value: u8) {
        self.boot_rom_mapped &= !value.bit(0);

        log::debug!("BANK write: {value:02X} (boot ROM mapped: {})", self.boot_rom_mapped);
    }

    pub fn clone_boot_rom(&mut self) -> Option<Vec<u8>> {
        self.boot_rom_present.then(|| self.boot_rom.to_vec())
    }

    pub fn boot_rom_mapped(&self) -> bool {
        self.boot_rom_mapped
    }
}

fn map_ram_address(address: u16, ram_bank: u8) -> u16 {
    match address & 0x1FFF {
        address @ 0x0000..=0x0FFF => address,
        0x1000..=0x1FFF => {
            // RAM bank of 0 functions as 1
            let ram_bank = if ram_bank == 0 { 1 } else { u16::from(ram_bank) };
            (ram_bank << 12) | (address & 0x0FFF)
        }
        _ => unreachable!("address & 0x1FFF is always <= 0x1FFF"),
    }
}

// Pad a 2048-byte CGB boot ROM to 2304 bytes by adding a filler $01 page
fn pad_cgb_boot_rom(boot_rom: &[u8]) -> Vec<u8> {
    let mut new_boot_rom = vec![0; CGB_BOOT_ROM_LEN];
    new_boot_rom[..0x0100].copy_from_slice(&boot_rom[..0x0100]);
    new_boot_rom.extend(iter::repeat_n(0, 256));
    new_boot_rom[0x0200..].copy_from_slice(&boot_rom[0x0100..0x0800]);

    new_boot_rom
}
