use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::ops::Deref;

// Main SNES master clock frequencies
pub const NTSC_MASTER_CLOCK_FREQUENCY: u64 = 21_477_270;
pub const PAL_MASTER_CLOCK_FREQUENCY: u64 = 21_281_370;

#[derive(Debug, Clone, Default, FakeEncode, FakeDecode)]
pub struct Rom(pub Box<[u8]>);

impl Deref for Rom {
    type Target = Box<[u8]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn lorom_map_rom_address(address: u32, rom_len: u32) -> u32 {
    // LoROM mapping ignores A23 and A15, and A16-22 are shifted right 1
    let rom_addr = ((address & 0x7F0000) >> 1) | (address & 0x007FFF);
    // TODO better handle unusual ROM sizes
    rom_addr % rom_len
}

macro_rules! impl_take_set_rom {
    ($rom:ident) => {
        #[must_use]
        pub fn take_rom(&mut self) -> ::std::vec::Vec<u8> {
            ::std::mem::take(&mut self.$rom.0).into_vec()
        }

        pub fn set_rom(&mut self, rom: ::std::vec::Vec<u8>) {
            self.$rom.0 = rom.into_boxed_slice();
        }
    };
}

pub(crate) use impl_take_set_rom;
