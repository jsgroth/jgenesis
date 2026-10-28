mod arcadecard;

use crate::cartridge::arcadecard::ArcadeCard;
use bincode::{Decode, Encode};
use crc::Crc;
use jgenesis_common::boxedarray::BoxedByteArray;
use jgenesis_common::debug::{DebugBytesView, DebugMemoryView};
use jgenesis_proc_macros::{FakeDecode, FakeEncode, PartialClone};
use pce_config::PceSystemCardModel;
use std::ops::Deref;
use std::{iter, mem};

const SUPER_SYSTEM_CARD_RAM_LEN: usize = 192 * 1024;

const POPULOUS_SRAM_LEN: usize = 32 * 1024;

#[derive(Debug, Clone, FakeEncode, FakeDecode)]
pub struct Rom(pub Box<[u8]>);

impl Default for Rom {
    fn default() -> Self {
        Self(vec![].into_boxed_slice())
    }
}

impl Deref for Rom {
    type Target = Box<[u8]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, Encode, Decode)]
enum Mapper {
    // Standard linear ROM mapping in all banks
    None,
    // ROM in banks $00-$3F, 192KB of RAM in banks $68-$7F
    SuperSystemCard { ram: BoxedByteArray<SUPER_SYSTEM_CARD_RAM_LEN> },
    // Super System Card plus 2MB additional RAM, accessible through ports in banks $40-$43
    // Also has a number of registers mapped to bank $FF offsets $1A00-$1AFF
    ArcadeCard(ArcadeCard),
    // Standard linear ROM mapping in banks $00-$3F, 32KB of SRAM mapped to $40-$43
    Populous { sram: BoxedByteArray<POPULOUS_SRAM_LEN>, sram_dirty: bool },
    // First 512KB of ROM in banks $00-$3F, mappable 512KB ROM bank in banks $40-$7F
    StreetFighter2 { rom_bank: u32 },
}

impl Mapper {
    fn guess_from_rom(
        rom: &[u8],
        initial_sram: Option<Vec<u8>>,
        disc_present: bool,
        system_card_model: PceSystemCardModel,
    ) -> Self {
        const CRC: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);

        let checksum = CRC.checksum(rom);

        // TODO is there a better way to do this than checksum matching? PCE games don't seem to
        // have anything resembling a cartridge header
        match checksum {
            // Populous (Japan) (En)
            0xDB5F97B3 => {
                log::info!("Enabling Populous SRAM mapper (ROM checksum {checksum:08X})");

                let mut sram = BoxedByteArray::new();
                if let Some(initial_sram) = initial_sram
                    && initial_sram.len() >= POPULOUS_SRAM_LEN
                {
                    sram.copy_from_slice(&initial_sram[..POPULOUS_SRAM_LEN]);
                }

                Self::Populous { sram, sram_dirty: false }
            }
            // Street Fighter II' - Champion Edition (Japan)
            0x33DEB700 => {
                log::info!(
                    "Enabling Street Fighter II bank-switching mapper (ROM checksum {checksum:08X})"
                );
                Self::StreetFighter2 { rom_bank: 1 }
            }
            _ => {
                // CD-ROM² System Cards only map ROM to banks $00-$3F, so any ROM larger than 512KB
                // can't be a valid System Card BIOS ROM; use regular cartridge mapping instead
                let should_use_system_card = disc_present && rom.len() <= 512 * 1024;

                match system_card_model {
                    PceSystemCardModel::Super if should_use_system_card => {
                        log::info!("Using Super System Card");
                        Self::SuperSystemCard { ram: BoxedByteArray::new_random() }
                    }
                    PceSystemCardModel::Arcade if should_use_system_card => {
                        log::info!("Using Arcade Card");
                        Self::ArcadeCard(ArcadeCard::new())
                    }
                    _ => {
                        log::info!("Using standard mapper");
                        Self::None
                    }
                }
            }
        }
    }

    fn read(&mut self, address: u32, rom: &[u8]) -> u8 {
        debug_assert!(address <= 0x0FFFFF);

        match self {
            Self::None => read_rom_safely(rom, address),
            Self::Populous { sram, .. } => match address >> 13 {
                0x40..=0x43 => sram[(address & 0x7FFF) as usize],
                _ => read_rom_safely(rom, address),
            },
            Self::StreetFighter2 { rom_bank } => match address {
                0x000000..=0x07FFFF => read_rom_safely(rom, address),
                0x080000..=0x0FFFFF => {
                    let banked_addr = (*rom_bank << 19) | (address & 0x7FFFF);
                    read_rom_safely(rom, banked_addr)
                }
                _ => panic!("Invalid ROM address {address:06X}"),
            },
            Self::SuperSystemCard { ram } => match address {
                0x000000..=0x07FFFF => read_rom_safely(rom, address),
                0x080000..=0x0CFFFF => 0xFF, // Unused?
                0x0D0000..=0x0FFFFF => ram[(address - 0x0D0000) as usize],
                _ => panic!("Invalid ROM address {address:06X}"),
            },
            Self::ArcadeCard(card) => card.read(address, rom),
        }
    }

    fn write(&mut self, address: u32, value: u8) {
        match self {
            Self::None => {}
            Self::Populous { sram, sram_dirty } => {
                let bank = address >> 13;
                if (0x40..=0x43).contains(&bank) {
                    sram[(address & 0x7FFF) as usize] = value;
                    *sram_dirty = true;
                }
            }
            Self::StreetFighter2 { rom_bank } => {
                // Writing to $1FF0-$1FF3 changes the ROM bank based on the address written to
                // Value does not matter
                if (0x001FF0..=0x001FF3).contains(&address) {
                    *rom_bank = (address & 3) + 1;
                }
            }
            Self::SuperSystemCard { ram } => {
                if (0x0D0000..=0x0FFFFF).contains(&address) {
                    ram[(address - 0x0D0000) as usize] = value;
                }
            }
            Self::ArcadeCard(card) => card.write(address, value),
        }
    }

    fn read_cd_register(&mut self, address: u32) -> Option<u8> {
        let address = address & 0x1FFF;

        if (0x18C0..=0x18C3).contains(&address)
            && matches!(self, Self::SuperSystemCard { .. } | Self::ArcadeCard(_))
        {
            // Some sort of version ID, Super System Card BIOS relies on this for hardware detection
            return Some([0x00, 0xAA, 0x55, 0x03][(address & 3) as usize]);
        }

        match self {
            Self::ArcadeCard(card) => card.read_cd_register(address),
            _ => None,
        }
    }

    fn write_cd_register(&mut self, address: u32, value: u8) {
        if let Self::ArcadeCard(card) = self {
            card.write_cd_register(address, value);
        }
    }
}

#[inline(always)]
fn read_rom_safely(rom: &[u8], address: u32) -> u8 {
    rom[(address as usize) & (rom.len() - 1)]
}

#[derive(Debug, Clone, PartialClone, Encode, Decode)]
pub struct HuCard {
    #[partial_clone(default)]
    rom: Rom,
    mapper: Mapper,
}

impl HuCard {
    pub fn new(
        mut rom: Vec<u8>,
        initial_sram: Option<Vec<u8>>,
        disc_present: bool,
        system_card_model: PceSystemCardModel,
    ) -> Self {
        rom = mirror_hucard_rom(rom);

        let mapper = Mapper::guess_from_rom(&rom, initial_sram, disc_present, system_card_model);

        Self { rom: Rom(rom.into_boxed_slice()), mapper }
    }

    // Pages $00-$7F
    pub fn read(&mut self, address: u32) -> u8 {
        self.mapper.read(address, &self.rom)
    }

    // Pages $00-$7F
    pub fn write(&mut self, address: u32, value: u8) {
        self.mapper.write(address, value);
    }

    // $1800-$1BFF in page $FF (used by CD-ROM² System Cards)
    pub fn read_cd_register(&mut self, address: u32) -> Option<u8> {
        self.mapper.read_cd_register(address)
    }

    // $1800-$1BFF in page $FF (used by CD-ROM² System Cards)
    pub fn write_cd_register(&mut self, address: u32, value: u8) {
        self.mapper.write_cd_register(address, value);
    }

    pub fn clone_rom(&self) -> Vec<u8> {
        self.rom.0.to_vec()
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        self.rom.0 = mem::take(&mut other.rom.0);
    }

    pub fn sram(&self) -> Option<&[u8]> {
        match &self.mapper {
            Mapper::Populous { sram, .. } => Some(sram.as_slice()),
            _ => None,
        }
    }

    pub fn is_sram_dirty(&self) -> bool {
        match self.mapper {
            Mapper::Populous { sram_dirty, .. } => sram_dirty,
            _ => false,
        }
    }

    pub fn clear_sram_dirty(&mut self) {
        if let Mapper::Populous { sram_dirty, .. } = &mut self.mapper {
            *sram_dirty = false;
        }
    }

    pub fn debug_rom_view(&mut self) -> impl DebugMemoryView {
        DebugBytesView(&mut self.rom.0)
    }

    pub fn debug_super_syscard_ram_view(&mut self) -> Option<Box<dyn DebugMemoryView + '_>> {
        match &mut self.mapper {
            Mapper::SuperSystemCard { ram } => Some(Box::new(DebugBytesView(ram.as_mut_slice()))),
            Mapper::ArcadeCard(card) => Some(Box::new(card.debug_super_ram_view())),
            _ => None,
        }
    }

    pub fn debug_arcade_ram_view(&mut self) -> Option<Box<dyn DebugMemoryView + '_>> {
        match &mut self.mapper {
            Mapper::ArcadeCard(card) => Some(Box::new(card.debug_arcade_ram_view())),
            _ => None,
        }
    }
}

fn mirror_hucard_rom(mut rom: Vec<u8>) -> Vec<u8> {
    if rom.is_empty() {
        // Nothing really reasonable to do here; just make the entire cartridge read 0xFF
        rom.extend(iter::repeat_n(0xFF, 256 * 1024));
    }

    let mut new_rom = if rom.len() == 384 * 1024 {
        // 384KB HuCards contain two ROM chips, a 256KB chip and a 128KB chip, mapped like so:
        //   $000000-$07FFFF (banks $00-$3F): First 256KB of ROM, mirrored 2x
        //   $080000-$0FFFFF (banks $40-$7F): Last 128KB of ROM, mirrored 4x
        let mut new_rom = Vec::with_capacity(1024 * 1024);

        for _ in 0..2 {
            new_rom.extend(&rom[..256 * 1024]);
        }
        new_rom.extend(&rom[256 * 1024..]);

        new_rom
    } else if rom.len() == 512 * 1024 {
        // 512KB HuCards can apparently be one of two mappings.
        // Mapping A (2x 256KB chips):
        //   $000000-$07FFFF (banks $00-$3F): First 256KB of ROM, mirrored 2x
        //   $080000-$0FFFFF (banks $40-$7F): Last 256KB of ROM, mirrored 2x
        // Mapping B (1x 512KB chip):
        //   $000000-$0FFFFF (banks $00-$7F): Full 512KB of ROM, mirrored 2x
        // It's virtually impossible to detect which mapping a game expects, so for highest
        // compatibility, mirror the last 256KB of ROM 3x (inspired by what Mednafen does).
        // Explicitly:
        //   $00-$1F: First 256KB
        //   $20-$3F: Second 256KB (important for games with 1x 512KB chip)
        //   $40-$5F: Second 256KB (important for games with 2x 256KB chips)
        //   $60-$7F: Second 256KB (probably never used?)
        if rom.capacity() < 1024 * 1024 {
            rom.reserve(1024 * 1024 - rom.capacity());
        }

        for i in 256 * 1024..512 * 1024 {
            rom.push(rom[i]);
        }

        rom
    } else {
        // For other sizes (e.g. 768KB or 1MB), normal mirroring up to the next power of two works
        rom
    };

    jgenesis_common::rom::mirror_to_next_power_of_two(&mut new_rom);

    new_rom
}
