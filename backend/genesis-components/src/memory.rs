//! Genesis memory map and 68000 + Z80 bus interfaces

use bincode::{Decode, Encode};
use genesis_config::GenesisEmulatorConfig;
use jgenesis_common::cheats::CheatWordOverrides;
use jgenesis_common::num::{GetBit, U16Ext};

const MAIN_RAM_LEN_WORDS: usize = 64 * 1024 / 2;
const AUDIO_RAM_LEN: usize = 8 * 1024;

type RamCheatOverrides = CheatWordOverrides<0xFF0000, 0xFFFFFF>;

#[derive(Debug, Clone, Encode, Decode)]
pub struct Memory {
    main_ram: Box<[u16; MAIN_RAM_LEN_WORDS]>,
    audio_ram: Box<[u8; AUDIO_RAM_LEN]>,
    ram_cheat_overrides: RamCheatOverrides,
}

impl Memory {
    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn new(config: &GenesisEmulatorConfig) -> Self {
        Self {
            main_ram: vec![0; MAIN_RAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
            audio_ram: vec![0; AUDIO_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            ram_cheat_overrides: RamCheatOverrides::new(&config.cheat_codes),
        }
    }

    #[must_use]
    pub fn read_main_ram_byte(&self, address: u32) -> u8 {
        if let Some(cheat) = self.ram_cheat_overrides.get(address) {
            return cheat.to_be_bytes()[(address & 1) as usize];
        }

        let word = self.main_ram[((address & 0xFFFF) >> 1) as usize];
        word.to_be_bytes()[(address & 1) as usize]
    }

    #[must_use]
    pub fn read_main_ram_word(&self, address: u32) -> u16 {
        if let Some(cheat) = self.ram_cheat_overrides.get(address) {
            return cheat;
        }

        self.main_ram[((address & 0xFFFF) >> 1) as usize]
    }

    pub fn write_main_ram_byte(&mut self, address: u32, value: u8) {
        let word = &mut self.main_ram[((address & 0xFFFF) >> 1) as usize];
        if !address.bit(0) {
            word.set_msb(value);
        } else {
            word.set_lsb(value);
        }
    }

    pub fn write_main_ram_word(&mut self, address: u32, value: u16) {
        self.main_ram[((address & 0xFFFF) >> 1) as usize] = value;
    }

    #[must_use]
    pub fn read_audio_ram(&self, address: u16) -> u8 {
        self.audio_ram[(address & 0x1FFF) as usize]
    }

    pub fn write_audio_ram(&mut self, address: u16, value: u8) {
        self.audio_ram[(address & 0x1FFF) as usize] = value;
    }

    pub fn reload_config(&mut self, config: &GenesisEmulatorConfig) {
        self.ram_cheat_overrides.update_cheat_codes(&config.cheat_codes);
    }

    #[must_use]
    pub fn debug_ram_view(&mut self) -> (&mut [u16], &mut [u8]) {
        (self.main_ram.as_mut_slice(), self.audio_ram.as_mut_slice())
    }
}
