use crate::cartridge::external::ExternalMemory;
use crate::memory::PhysicalMedium;
use crate::svp::Svp;
use bincode::{Decode, Encode};
use crc::Crc;
use genesis_config::GenesisRegion;
use jgenesis_common::num::{GetBit, U16Ext};
use jgenesis_proc_macros::{FakeDecode, FakeEncode, PartialClone};
use regex::Regex;
use std::ops::Index;
use std::sync::LazyLock;
use std::{array, iter, mem};

pub mod eeprom;
pub mod external;

const CRC: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);

#[derive(Debug, Clone, Default, FakeEncode, FakeDecode)]
struct Rom(Vec<u8>);

impl Rom {
    fn get(&self, i: usize) -> Option<u8> {
        self.0.get(i).copied()
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

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct SegaMapper {
    bank_numbers: [u8; 8],
}

impl SegaMapper {
    #[must_use]
    pub fn new() -> Self {
        Self { bank_numbers: array::from_fn(|i| i as u8) }
    }

    pub fn write(&mut self, address: u32, value: u8) {
        let idx = (address >> 1) & 0x07;
        if idx == 0 {
            // First bank can't be changed, always points to first 512KB of ROM
            return;
        }

        self.bank_numbers[idx as usize] = value;
    }

    #[must_use]
    pub fn map_address(self, address: u32) -> u32 {
        let idx = (address >> 19) & 0x07;
        let bank_number: u32 = self.bank_numbers[idx as usize].into();
        (bank_number << 19) | (address & 0x07FFFF)
    }

    #[must_use]
    pub fn should_use(rom: &[u8]) -> bool {
        // Only one game uses the bank switching Sega mapper, Super Street Fighter 2
        // Additionally enable the bank switching mapper for any cartridge that declares its system type as "SEGA SSF"
        let serial_number = &rom[0x183..0x18B];
        let is_ssf2 = is_super_street_fighter_2(serial_number);
        let is_ssf_system = &rom[0x100..0x110] == b"SEGA SSF        ";

        // Demons of Asteborg specifies its system as "SEGA DOA" but expects the SSF mapper
        let is_doa = &rom[0x100..0x108] == b"SEGA DOA";

        is_ssf2 | is_ssf_system | is_doa
    }
}

impl Default for SegaMapper {
    fn default() -> Self {
        Self::new()
    }
}

fn is_super_street_fighter_2(serial_number: &[u8]) -> bool {
    serial_number == b"T-12056 " || serial_number == b"MK-12056" || serial_number == b"T-12043 "
}

pub trait GenesisRegionExt: Sized + Copy {
    #[must_use]
    fn from_rom(rom: &[u8]) -> Option<Self>;

    #[must_use]
    fn version_bit(self) -> bool;
}

impl GenesisRegionExt for GenesisRegion {
    fn from_rom(rom: &[u8]) -> Option<Self> {
        // European games with incorrect region headers that indicate US or JP support
        const DEFAULT_EUROPE_CHECKSUMS: &[u32] = &[
            0x28165BD1, // Alisia Dragoon (Europe)
            0x224256C7, // Andre Agassi Tennis (Europe)
            0x90F5C2B7, // Brian Lara Cricket (Europe)
            0xEB8F4374, // Indiana Jones and the Last Crusade (Europe)
            0xFA537A45, // Winter Olympics (Europe)
            0xDACA01C3, // World Class Leader Board (Europe)
            0xC0DCE0E5, // Midway Presents Arcade's Greatest Hits (Europe)
            0x4C926BF6, // Nuance Xmas-Intro 2024
            0x0F51DD6A, // Chaekopon by Limp Ninja
        ];

        if DEFAULT_EUROPE_CHECKSUMS.contains(&CRC.checksum(rom)) {
            return Some(GenesisRegion::Europe);
        }

        if &rom[0x1F0..0x1F6] == b"EUROPE" {
            // Another World (E) has the string "EUROPE" in the region section; special case this
            // so that it's not detected as U (this game does not work with NTSC timings)
            return Some(GenesisRegion::Europe);
        }

        let region_bytes = &rom[0x1F0..0x1F3];

        // Prefer Americas if region code contains a 'U'
        if region_bytes.contains(&b'U') {
            return Some(GenesisRegion::Americas);
        }

        // Otherwise, prefer Japan if it contains a 'J'
        if region_bytes.contains(&b'J') {
            return Some(GenesisRegion::Japan);
        }

        // Finally, prefer Europe if it contains an 'E'
        if region_bytes.contains(&b'E') {
            return Some(GenesisRegion::Europe);
        }

        // If region code contains neither a 'U' nor a 'J', treat it as a hex char
        let c = region_bytes[0] as char;
        let value = u8::from_str_radix(&c.to_string(), 16).ok()?;
        if value.bit(2) {
            // Bit 2 = Americas
            Some(GenesisRegion::Americas)
        } else if value.bit(0) {
            // Bit 0 = Asia
            Some(GenesisRegion::Japan)
        } else if value.bit(3) {
            // Bit 3 = Europe
            Some(GenesisRegion::Europe)
        } else {
            // Invalid
            None
        }
    }

    #[inline]
    fn version_bit(self) -> bool {
        self != Self::Japan
    }
}

#[derive(Debug, Clone)]
pub struct CartridgeHeader {
    pub region: Option<GenesisRegion>,
    pub ssf_mapper: bool,
}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct Cartridge {
    #[partial_clone(default)]
    rom: Rom,
    external_memory: ExternalMemory,
    ram_mapped: bool,
    mapper: Option<SegaMapper>,
    svp: Option<Svp>,
    region: GenesisRegion,
    is_unlicensed_rockman_x3: bool,
}

const TRIPLE_PLAY_GOLD_SERIAL: &[u8] = b"T-172116";
const TRIPLE_PLAY_96_SERIAL: &[u8] = b"T-172026";

const QUACKSHOT_REV_A_SERIAL: &[u8] = b"GM 00004054-01";

const ROCKMAN_X3_CHECKSUM: u32 = 0x3EE639F0;

impl Cartridge {
    pub fn from_rom(
        rom_bytes: Vec<u8>,
        initial_ram_bytes: Option<Vec<u8>>,
        forced_region: Option<GenesisRegion>,
    ) -> Self {
        // Take checksum before potentially byteswapping the ROM
        let checksum = CRC.checksum(&rom_bytes);
        log::info!("ROM CRC32: {checksum:08X}");

        let mut rom_bytes = ensure_rom_in_expected_format(rom_bytes);

        let region = forced_region.unwrap_or_else(|| {
            GenesisRegion::from_rom(&rom_bytes).unwrap_or_else(|| {
                log::warn!("Unable to determine cartridge region from ROM header; using Americas");
                GenesisRegion::Americas
            })
        });
        log::info!("Genesis hardware region: {region:?}");

        let external_memory = ExternalMemory::from_rom(&rom_bytes, checksum, initial_ram_bytes);

        // Initialize ram_mapped to true if external memory is present
        // Only one game ever unmaps RAM (Phantasy Star 4)
        let ram_mapped = !matches!(external_memory, ExternalMemory::None);

        let mapper = SegaMapper::should_use(&rom_bytes).then(SegaMapper::new);
        log::info!("Using Sega banked mapper: {}", mapper.is_some());

        let serial_number = &rom_bytes[0x183..0x18B];

        // Only one game uses the SVP, Virtua Racing
        let svp = is_virtua_racing(serial_number).then(Svp::new);

        if rom_bytes.len() >= 0x300000
            && (serial_number == TRIPLE_PLAY_GOLD_SERIAL || serial_number == TRIPLE_PLAY_96_SERIAL)
        {
            fix_triple_play_rom(&mut rom_bytes);
        }

        if rom_bytes.len() == 0x80000 && &rom_bytes[0x180..0x18E] == QUACKSHOT_REV_A_SERIAL {
            rom_bytes = fix_quackshot_rev_a_rom(rom_bytes);
        }

        let is_unlicensed_rockman_x3 = checksum == ROCKMAN_X3_CHECKSUM;

        Self {
            rom: Rom(rom_bytes),
            external_memory,
            ram_mapped,
            mapper,
            svp,
            region,
            is_unlicensed_rockman_x3,
        }
    }

    #[inline]
    pub fn tick(&mut self, m68k_cycles: u32) {
        if let Some(svp) = &mut self.svp {
            svp.tick(&self.rom.0, m68k_cycles);
        }
    }

    fn write_cartridge_register(&mut self, address: u32, value: u8) {
        match address {
            0xA130F1 => {
                self.ram_mapped = value.bit(0);
            }
            0xA130F3..=0xA130FF => {
                if let Some(mapper) = &mut self.mapper {
                    mapper.write(address, value);
                }
            }
            _ => log::error!(
                "unexpected cartridge register write; address={address:06X}, value={value:02X}"
            ),
        }
    }

    #[must_use]
    pub fn take_rom(&mut self) -> Vec<u8> {
        mem::take(&mut self.rom).0
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        self.rom = mem::take(&mut other.rom);
    }

    #[must_use]
    pub fn external_ram(&self) -> &[u8] {
        self.external_memory.get_memory()
    }

    #[must_use]
    pub fn is_ram_persistent(&self) -> bool {
        self.external_memory.is_persistent()
    }

    #[must_use]
    pub fn get_and_clear_ram_dirty(&mut self) -> bool {
        self.external_memory.get_and_clear_dirty_bit()
    }

    #[must_use]
    pub fn program_title(&self) -> String {
        parse_title_from_header(&self.rom.0, self.region)
    }
}

fn ensure_rom_in_expected_format(mut rom: Vec<u8>) -> Vec<u8> {
    // For very tiny ROMs, pad to 1KB before doing anything else
    // e.g. "Mona in 344 bytes" demo
    const MIN_ROM_LEN: usize = 1024;

    if rom.len() < MIN_ROM_LEN {
        jgenesis_common::rom::mirror_to_next_power_of_two(&mut rom);

        while rom.len() < MIN_ROM_LEN {
            for i in 0..rom.len() {
                rom.push(rom[i]);
            }
        }
    }

    rom = remove_copier_header(rom);
    rom = deinterleave_rom(rom);
    ensure_big_endian(rom)
}

fn remove_copier_header(rom: Vec<u8>) -> Vec<u8> {
    // Some older ROMs contain a useless 512-byte copier header; remove it if present
    if rom.len() & 0x3FF != 0x200 {
        // ROM length is not off by 512 from a reasonable number
        return rom;
    }

    // TMSS header is normally at $100-$103, would be at $303-$304 with the 512-byte header
    let tmss_header = &rom[0x300..0x304];

    // Interleaved header bytes are normally at $80-$81 for even and $2080-$2081 for odd
    let interleaved_tmss_even = &rom[0x0280..0x0282];
    let interleaved_tmss_odd = &rom[0x2280..0x2282];

    if tmss_header != b"SEGA"
        && tmss_header != b"ESAG"
        && !(interleaved_tmss_even == b"EA" && interleaved_tmss_odd == b"SG")
    {
        // Removing the copier header would not produce a valid TMSS header
        return rom;
    }

    log::info!("ROM image appears to have a 512-byte copier header; removing it");

    rom.into_iter().skip(512).collect()
}

fn ensure_big_endian(mut rom: Vec<u8>) -> Vec<u8> {
    // Every licensed game contains the ASCII string "SEGA" at $100-$104 in ROM
    // If the string "ESAG" is detected there, byteswap the ROM
    if &rom[0x100..0x104] == "ESAG".as_bytes() {
        log::info!("Byteswapping ROM because it appears to be little-endian");

        for chunk in rom.chunks_exact_mut(2) {
            chunk.swap(0, 1);
        }
    }

    rom
}

fn deinterleave_rom(rom: Vec<u8>) -> Vec<u8> {
    // Some older ROM images, usually with the .smd file extension, are interleaved.
    // This format consists of 16KB blocks where each block contains 8KB of even bytes followed by
    // 8KB of odd bytes.
    if rom.len() % (16 * 1024) != 0 {
        // Interleaved ROM sizes should always be a multiple of 16KB
        return rom;
    }

    if &rom[0x100..0x104] == b"SEGA" || &rom[0x100..0x104] == b"ESAG" {
        // ROM image already contains valid TMSS text; don't try to deinterleave
        return rom;
    }

    if &rom[0x0080..0x0082] != b"EA" || &rom[0x2080..0x2082] != b"SG" {
        // Deinterleaving would not produce valid TMSS text; don't try to deinterleave
        return rom;
    }

    log::info!("ROM image appears to be interleaved; deinterleaving it");

    let mut deinterleaved = vec![0; rom.len()];
    for block_addr in (0..rom.len()).step_by(0x4000) {
        for i in 0..0x2000 {
            deinterleaved[block_addr + 2 * i] = rom[block_addr + 0x2000 + i];
            deinterleaved[block_addr + 2 * i + 1] = rom[block_addr + i];
        }
    }

    deinterleaved
}

fn fix_triple_play_rom(rom: &mut Vec<u8>) {
    // Triple Play expects the third MB of the ROM to be mapped to $300000-$3FFFFF instead
    // of $200000-$2FFFFF; accomplish this by duplicating the data
    if rom.len() < 0x400000 {
        rom.extend(iter::repeat_n(0xFF, 0x400000 - rom.len()));
    }

    let (first, second) = rom.split_at_mut(0x300000);
    second[..0x100000].copy_from_slice(&first[0x200000..0x300000]);
}

fn fix_quackshot_rev_a_rom(rom: Vec<u8>) -> Vec<u8> {
    // QuackShot (Rev A) is a 512KB ROM with an unusual ROM address mapping:
    //   $000000-$0FFFFF: First 256KB of ROM mirrored 4x
    //   $100000-$1FFFFF: Second 256KB of ROM mirrored 4x
    // Rather than implement custom mapping logic, just remap the ROM while loading it
    let mut remapped_rom = vec![0; 0x200000];
    for i in (0x000000..0x100000).step_by(0x40000) {
        remapped_rom[i..i + 0x40000].copy_from_slice(&rom[..0x40000]);
        remapped_rom[i + 0x100000..i + 0x140000].copy_from_slice(&rom[0x40000..]);
    }

    remapped_rom
}

#[must_use]
#[allow(clippy::missing_panics_doc, clippy::items_after_statements)]
pub fn parse_title_from_header(rom: &[u8], region: GenesisRegion) -> String {
    let addr = match region {
        GenesisRegion::Americas | GenesisRegion::Europe => 0x0150,
        GenesisRegion::Japan => 0x0120,
    };
    let bytes = &rom[addr..addr + 48];
    let title = bytes.iter().copied().map(|b| b as char).collect::<String>();

    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" +").unwrap());
    RE.replace_all(title.trim(), " ").into()
}

fn is_virtua_racing(serial_number: &[u8]) -> bool {
    serial_number == b"MK-1229 " || serial_number == b"G-7001  "
}

impl PhysicalMedium for Cartridge {
    #[inline]
    fn read_byte(&mut self, address: u32) -> u8 {
        if let Some(svp) = &mut self.svp {
            let word = svp.m68k_read(address & !1, &self.rom.0);
            return if address.bit(0) { word.lsb() } else { word.msb() };
        }

        if self.ram_mapped {
            if let Some(byte) = self.external_memory.read_byte(address) {
                return byte;
            }
        }

        let rom_addr = self.mapper.map_or(address, |mapper| mapper.map_address(address));
        self.rom.get(rom_addr as usize).unwrap_or_else(|| {
            log::debug!("Out-of-bounds cartridge byte read: {address:06X}");
            0xFF
        })
    }

    #[inline]
    fn read_word(&mut self, address: u32) -> u16 {
        if let Some(svp) = &mut self.svp {
            return svp.m68k_read(address, &self.rom.0);
        }

        // The unlicensed Rockman X3 port depends on $A13000 reads returning a value where the lower
        // 4 bits are $C or else it will immediately crash and display "decode error"
        if self.is_unlicensed_rockman_x3 && address == 0xA13000 {
            return 0x000C;
        }

        if self.ram_mapped {
            if let Some(word) = self.external_memory.read_word(address) {
                return word;
            }
        }

        let rom_addr = self.mapper.map_or(address, |mapper| mapper.map_address(address));
        let msb = self.rom.get(rom_addr as usize).unwrap_or_else(|| {
            log::debug!("Out-of-bounds cartridge word read: {address:06X}");
            0xFF
        });
        let lsb = self.rom.get((rom_addr + 1) as usize).unwrap_or(0xFF);
        u16::from_be_bytes([msb, lsb])
    }

    #[inline]
    fn read_word_for_dma(&mut self, address: u32) -> u16 {
        if self.svp.is_some() {
            // SVP cartridge memory has the same delay issue as Sega CD word RAM; Virtua Racing sets
            // DMA source address 2 higher than the "correct" address
            self.read_word(address.wrapping_sub(2))
        } else {
            self.read_word(address)
        }
    }

    #[inline]
    fn write_byte(&mut self, address: u32, value: u8) {
        if let Some(svp) = &mut self.svp {
            svp.m68k_write_byte(address, value);
            return;
        }

        match address {
            0x000000..=0x3FFFFF => {
                if self.ram_mapped {
                    self.external_memory.write_byte(address, value);
                } else {
                    log::debug!("Cartridge write with no RAM mapped: {address:06X} {value:02X}");
                }
            }
            0xA13000..=0xA130FF => {
                self.write_cartridge_register(address, value);
            }
            _ => {
                log::debug!("Write to invalid cartridge address: {address:06X} {value:02X}");
            }
        }
    }

    #[inline]
    fn write_word(&mut self, address: u32, value: u16) {
        if let Some(svp) = &mut self.svp {
            svp.m68k_write_word(address, value);
            return;
        }

        match address {
            0x000000..=0x3FFFFF => {
                if self.ram_mapped {
                    self.external_memory.write_word(address, value);
                } else {
                    log::debug!("Cartridge write with no RAM mapped: {address:06X} {value:04X}");
                }
            }
            0xA13000..=0xA130FF => {
                self.write_cartridge_register(address + 1, value as u8);
            }
            _ => {
                log::debug!("Write to invalid cartridge address: {address:06X} {value:04X}");
            }
        }
    }

    #[inline]
    fn region(&self) -> GenesisRegion {
        self.region
    }
}
