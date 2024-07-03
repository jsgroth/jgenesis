//! Genesis memory map and 68000 + Z80 bus interfaces

pub mod eeprom;
mod external;

use crate::api::GenesisRegion;
use crate::input::InputState;
use crate::memory::external::ExternalMemory;
use crate::svp::Svp;
use crate::vdp::Vdp;
use crate::ym2612::Ym2612;
use bincode::{Decode, Encode};
use crc::Crc;
use jgenesis_common::frontend::TimingMode;
use jgenesis_common::num::{GetBit, U16Ext};
use jgenesis_proc_macros::{FakeDecode, FakeEncode, PartialClone};
use regex::Regex;
use smsgg_core::psg::Psg;
use std::ops::Index;
use std::sync::OnceLock;
use std::{array, iter, mem};
use z80_emu::traits::InterruptLine;

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

        let mut rom_bytes = ensure_big_endian(rom_bytes);

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
            // Triple Play expects the third MB of the ROM to be mapped to $300000-$3FFFFF instead
            // of $200000-$2FFFFF; accomplish this by duplicating the data
            if rom_bytes.len() < 0x400000 {
                rom_bytes.extend(iter::repeat(0xFF).take(0x400000 - rom_bytes.len()));
            }

            let (first, second) = rom_bytes.split_at_mut(0x300000);
            second[..0x100000].copy_from_slice(&first[0x200000..0x300000]);
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

    fn take_rom(&mut self) -> Vec<u8> {
        mem::take(&mut self.rom).0
    }

    fn take_rom_from(&mut self, other: &mut Self) {
        self.rom = mem::take(&mut other.rom);
    }

    fn external_ram(&self) -> &[u8] {
        self.external_memory.get_memory()
    }

    fn is_ram_persistent(&self) -> bool {
        self.external_memory.is_persistent()
    }

    fn get_and_clear_ram_dirty(&mut self) -> bool {
        self.external_memory.get_and_clear_dirty_bit()
    }

    fn program_title(&self) -> String {
        parse_title_from_header(&self.rom.0, self.region)
    }
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

#[must_use]
#[allow(clippy::missing_panics_doc)]
pub fn parse_title_from_header(rom: &[u8], region: GenesisRegion) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();

    let addr = match region {
        GenesisRegion::Americas | GenesisRegion::Europe => 0x0150,
        GenesisRegion::Japan => 0x0120,
    };
    let bytes = &rom[addr..addr + 48];
    let title = bytes.iter().copied().map(|b| b as char).collect::<String>();

    let re = RE.get_or_init(|| Regex::new(r" +").unwrap());
    re.replace_all(title.trim(), " ").into()
}

fn is_virtua_racing(serial_number: &[u8]) -> bool {
    serial_number == b"MK-1229 " || serial_number == b"G-7001  "
}

pub trait PhysicalMedium {
    fn read_byte(&mut self, address: u32) -> u8;

    fn read_word(&mut self, address: u32) -> u16;

    // This exists as a separate method because of a Sega CD "feature" where DMA reads from word RAM
    // are delayed by a cycle, effectively meaning this should read (address - 2) instead of address
    fn read_word_for_dma(&mut self, address: u32) -> u16;

    fn write_byte(&mut self, address: u32, value: u8);

    fn write_word(&mut self, address: u32, value: u16);

    fn region(&self) -> GenesisRegion;
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

const MAIN_RAM_LEN: usize = 64 * 1024;
const AUDIO_RAM_LEN: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
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
        self.bank_number = (self.bank_number >> 1) | (u32::from(bit) << (Self::BITS - 1));
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct Signals {
    z80_busreq: bool,
    z80_reset: bool,
}

impl Default for Signals {
    fn default() -> Self {
        Self { z80_busreq: false, z80_reset: true }
    }
}

impl Signals {
    fn z80_busack(self) -> bool {
        self.z80_busreq && !self.z80_reset
    }
}

#[derive(Debug, Encode, Decode, PartialClone)]
pub struct Memory<Medium> {
    #[partial_clone(partial)]
    physical_medium: Medium,
    main_ram: Box<[u8; MAIN_RAM_LEN]>,
    audio_ram: Box<[u8; AUDIO_RAM_LEN]>,
    z80_bank_register: Z80BankRegister,
    signals: Signals,
}

impl<Medium: PhysicalMedium> Memory<Medium> {
    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn new(physical_medium: Medium) -> Self {
        Self {
            physical_medium,
            main_ram: vec![0; MAIN_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            audio_ram: vec![0; AUDIO_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            z80_bank_register: Z80BankRegister::default(),
            signals: Signals::default(),
        }
    }

    #[must_use]
    pub(crate) fn read_word_for_dma(&mut self, address: u32) -> u16 {
        match address {
            0x000000..=0x3FFFFF => self.physical_medium.read_word_for_dma(address),
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

    #[inline]
    #[must_use]
    pub fn hardware_region(&self) -> GenesisRegion {
        self.physical_medium.region()
    }

    #[inline]
    #[must_use]
    pub fn medium(&self) -> &Medium {
        &self.physical_medium
    }

    #[inline]
    #[must_use]
    pub fn medium_mut(&mut self) -> &mut Medium {
        &mut self.physical_medium
    }

    #[inline]
    pub fn reset_z80_signals(&mut self) {
        self.signals = Signals::default();
    }
}

impl Memory<Cartridge> {
    #[must_use]
    pub fn take_rom(&mut self) -> Vec<u8> {
        self.physical_medium.take_rom()
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        self.physical_medium.take_rom_from(&mut other.physical_medium);
    }

    #[must_use]
    pub fn game_title(&self) -> String {
        self.physical_medium.program_title()
    }

    #[inline]
    #[must_use]
    pub fn external_ram(&self) -> &[u8] {
        self.physical_medium.external_ram()
    }

    #[inline]
    #[must_use]
    pub fn is_external_ram_persistent(&self) -> bool {
        self.physical_medium.is_ram_persistent()
    }

    #[inline]
    #[must_use]
    pub fn get_and_clear_external_ram_dirty(&mut self) -> bool {
        self.physical_medium.get_and_clear_ram_dirty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MainBusSignals {
    pub m68k_reset: bool,
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct MainBusWrites {
    byte: Vec<(u32, u8)>,
    word: Vec<(u32, u16)>,
}

impl MainBusWrites {
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self { byte: Vec::with_capacity(20), word: Vec::with_capacity(20) }
    }

    fn clear(&mut self) {
        self.byte.clear();
        self.word.clear();
    }
}

pub struct MainBus<'a, Medium> {
    memory: &'a mut Memory<Medium>,
    vdp: &'a mut Vdp,
    psg: &'a mut Psg,
    ym2612: &'a mut Ym2612,
    input: &'a mut InputState,
    timing_mode: TimingMode,
    signals: MainBusSignals,
    pending_writes: MainBusWrites,
    z80_accessed_68k_bus: bool,
    // Last word-size read; used to pseudo-emulate open bus bits in the Z80 BUSACK register
    last_word_read: u16,
}

impl<'a, Medium: PhysicalMedium> MainBus<'a, Medium> {
    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub fn new(
        memory: &'a mut Memory<Medium>,
        vdp: &'a mut Vdp,
        psg: &'a mut Psg,
        ym2612: &'a mut Ym2612,
        input: &'a mut InputState,
        timing_mode: TimingMode,
        signals: MainBusSignals,
        pending_writes: MainBusWrites,
    ) -> Self {
        Self {
            memory,
            vdp,
            psg,
            ym2612,
            input,
            timing_mode,
            signals,
            pending_writes,
            z80_accessed_68k_bus: false,
            last_word_read: 0,
        }
    }

    fn read_io_register(&self, address: u32) -> u8 {
        match address {
            // Version register
            0xA10000 | 0xA10001 => {
                0x20 | (u8::from(self.memory.hardware_region().version_bit()) << 7)
                    | (u8::from(self.timing_mode == TimingMode::Pal) << 6)
            }
            0xA10002 | 0xA10003 => self.input.read_p1_data(),
            0xA10004 | 0xA10005 => self.input.read_p2_data(),
            0xA10008 | 0xA10009 => self.input.read_p1_ctrl(),
            0xA1000A | 0xA1000B => self.input.read_p2_ctrl(),
            // TxData registers return 0xFF by default
            0xA1000E | 0xA1000F | 0xA10014 | 0xA10015 | 0xA1001A | 0xA1001B => 0xFF,
            // Other I/O registers return 0x00 by default
            _ => 0x00,
        }
    }

    fn write_io_register(&mut self, address: u32, value: u8) {
        match address {
            0xA10002 | 0xA10003 => {
                self.input.write_p1_data(value);
            }
            0xA10004 | 0xA10005 => {
                self.input.write_p2_data(value);
            }
            0xA10008 | 0xA10009 => {
                self.input.write_p1_ctrl(value);
            }
            0xA1000A | 0xA1000B => {
                self.input.write_p2_ctrl(value);
            }
            _ => {}
        }
    }

    fn read_vdp_byte(&mut self, address: u32) -> u8 {
        match address & 0x1F {
            0x00 | 0x02 => self.vdp.read_data().msb(),
            0x01 | 0x03 => self.vdp.read_data().lsb(),
            0x04 | 0x06 => self.vdp.read_status().msb(),
            0x05 | 0x07 => self.vdp.read_status().lsb(),
            0x08 | 0x0A => self.vdp.hv_counter().msb(),
            0x09 | 0x0B => self.vdp.hv_counter().lsb(),
            0x10..=0x1F => {
                // PSG / unused space; PSG is not readable
                0xFF
            }
            _ => unreachable!("address & 0x1F is always <= 0x1F"),
        }
    }

    fn write_vdp_byte(&mut self, address: u32, value: u8) {
        // Byte-size VDP writes duplicate the byte into a word
        let vdp_word = u16::from_le_bytes([value, value]);
        match address & 0x1F {
            0x00..=0x03 => {
                self.vdp.write_data(vdp_word);
            }
            0x04..=0x07 => {
                self.vdp.write_control(vdp_word);
            }
            0x11 | 0x13 | 0x15 | 0x17 => {
                self.psg.write(value);
            }
            0x10 | 0x12 | 0x14 | 0x16 | 0x18..=0x1F => {}
            _ => unreachable!("address & 0x1F is always <= 0x1F"),
        }
    }

    /// Take the pending writes Vecs without applying them
    #[inline]
    #[must_use]
    pub fn take_writes(self) -> MainBusWrites {
        self.pending_writes
    }

    /// Apply all pending writes, then clear and return the pending writes Vecs
    #[inline]
    #[must_use]
    pub fn apply_writes(mut self) -> MainBusWrites {
        let mut pending_writes = mem::take(&mut self.pending_writes);

        for &(address, value) in &pending_writes.byte {
            self.apply_byte_write(address, value);
        }

        for &(address, value) in &pending_writes.word {
            self.apply_word_write(address, value);
        }

        pending_writes.clear();
        pending_writes
    }

    fn apply_byte_write(&mut self, address: u32, value: u8) {
        let address = address & ADDRESS_MASK;
        log::trace!("Main bus byte write: address={address:06X}, value={value:02X}");
        match address {
            0x000000..=0x9FFFFF | 0xA12000..=0xA153FF => {
                self.memory.physical_medium.write_byte(address, value);
            }
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
                log::trace!("Set Z80 BUSREQ to {}", self.memory.signals.z80_busreq);
            }
            0xA11200..=0xA11201 => {
                self.memory.signals.z80_reset = !value.bit(0);
                log::trace!("Set Z80 RESET to {}", self.memory.signals.z80_reset);
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

    fn apply_word_write(&mut self, address: u32, value: u16) {
        let address = address & ADDRESS_MASK;
        log::trace!("Main bus word write: address={address:06X}, value={value:02X}");
        match address {
            0x000000..=0x9FFFFF | 0xA12000..=0xA153FF => {
                self.memory.physical_medium.write_word(address, value);
            }
            0xA00000..=0xA0FFFF => {
                // Z80 memory map; word-size writes write the MSB as a byte-size write
                self.apply_byte_write(address, value.msb());
            }
            0xA10000..=0xA1001F => {
                self.write_io_register(address, value.lsb());
            }
            0xA11100..=0xA11101 => {
                self.memory.signals.z80_busreq = value.bit(8);
                log::trace!("Set Z80 BUSREQ to {}", self.memory.signals.z80_busreq);
            }
            0xA11200..=0xA11201 => {
                self.memory.signals.z80_reset = !value.bit(8);
                log::trace!("Set Z80 RESET to {}", self.memory.signals.z80_reset);
            }
            0xC00000..=0xC00003 => {
                self.vdp.write_data(value);
            }
            0xC00004..=0xC00007 => {
                self.vdp.write_control(value);
            }
            0xC0001C => {
                self.vdp.write_debug_register(value);
            }
            0xE00000..=0xFFFFFF => {
                let ram_addr = (address & 0xFFFF) as usize;
                self.memory.main_ram[ram_addr] = value.msb();
                self.memory.main_ram[(ram_addr + 1) & 0xFFFF] = value.lsb();
            }
            _ => {}
        }
    }

    #[inline]
    #[must_use]
    pub fn z80_accessed_68k_bus(&self) -> bool {
        self.z80_accessed_68k_bus
    }

    // $A11100
    fn read_busack_register(&self) -> u16 {
        // Word reads of Z80 BUSREQ signal mirror the byte in both MSB and LSB (TODO is this right or should only bit 8 be set?)
        let busack_byte: u8 = (!self.memory.signals.z80_busack()).into();
        let busack_word = u16::from_be_bytes([busack_byte, busack_byte]);

        // Unused bits should read open bus; Danny Sullivan's Indy Heat (Proto) depends on this or
        // it will fail to boot
        busack_word | (self.last_word_read & !0x0101)
    }
}

// The Genesis has a 24-bit bus, not 32-bit
const ADDRESS_MASK: u32 = 0xFFFFFF;

impl<'a, Medium: PhysicalMedium> m68000_emu::BusInterface for MainBus<'a, Medium> {
    #[inline]
    fn read_byte(&mut self, address: u32) -> u8 {
        let address = address & ADDRESS_MASK;
        log::trace!("Main bus byte read, address={address:06X}");
        match address {
            0x000000..=0x9FFFFF | 0xA12000..=0xA153FF => {
                self.memory.physical_medium.read_byte(address)
            }
            0xA00000..=0xA0FFFF => {
                // Z80 memory map
                // For 68k access, $8000-$FFFF mirrors $0000-$7FFF
                <Self as z80_emu::BusInterface>::read_memory(self, (address & 0x7FFF) as u16)
            }
            0xA10000..=0xA1001F => self.read_io_register(address),
            0xA11100..=0xA11101 => (self.read_busack_register() >> 8) as u8,
            0xC00000..=0xC0001F => self.read_vdp_byte(address),
            0xE00000..=0xFFFFFF => self.memory.main_ram[(address & 0xFFFF) as usize],
            _ => 0xFF,
        }
    }

    #[inline]
    fn read_word(&mut self, address: u32) -> u16 {
        let address = address & ADDRESS_MASK;
        log::trace!("Main bus word read, address={address:06X}");

        self.last_word_read = match address {
            0x000000..=0x9FFFFF | 0xA12000..=0xA153FF => {
                self.memory.physical_medium.read_word(address)
            }
            0xA00000..=0xA0FFFF => {
                // All Z80 access is byte-size; word reads mirror the byte in both MSB and LSB
                let byte = self.read_byte(address);
                u16::from_le_bytes([byte, byte])
            }
            0xA10000..=0xA1001F => self.read_io_register(address).into(),
            0xA11100..=0xA11101 => self.read_busack_register(),
            0xC00000..=0xC00003 => self.vdp.read_data(),
            0xC00004..=0xC00007 => self.vdp.read_status(),
            0xC00008..=0xC0000F => self.vdp.hv_counter(),
            0xE00000..=0xFFFFFF => {
                let ram_addr = (address & 0xFFFF) as usize;
                u16::from_be_bytes([
                    self.memory.main_ram[ram_addr],
                    self.memory.main_ram[(ram_addr + 1) & 0xFFFF],
                ])
            }
            _ => 0xFFFF,
        };

        self.last_word_read
    }

    #[inline]
    fn write_byte(&mut self, address: u32, value: u8) {
        self.pending_writes.byte.push((address, value));
    }

    #[inline]
    fn write_word(&mut self, address: u32, value: u16) {
        self.pending_writes.word.push((address, value));
    }

    #[inline]
    fn interrupt_level(&self) -> u8 {
        self.vdp.m68k_interrupt_level()
    }

    #[inline]
    fn acknowledge_interrupt(&mut self, _interrupt_level: u8) {
        // When the 68000 acknowledges a VDP interrupt, the VDP acknowledges whatever level it is
        // currently raising rather than paying attention to the 68000's IACK lines. This is noted
        // in official documentation which describes this hardware bug: If both HINT are VINT are
        // enabled and the 68000 executes a long instruction right before HINT would trigger on line
        // 224, it's possible for the following sequence of events to happen:
        //   1. The VDP sets IPL2 to indicate a level 4 interrupt (HINT)
        //   2. After the 68000 finishes its long instruction, it begins to handle the level 4 interrupt
        //   3. Before the acknowledge, VBlank begins and the VDP sets IPL2+IPL1 for level 6 interrupt (VINT)
        //   4. The 68000 sets its VPA signal to acknowledge the interrupt, and the VDP acknowledges VINT instead of HINT
        //   5. After the 68000 returns from its HINT handler, it immediately handles the HINT a second time (having missed VINT)
        self.vdp.acknowledge_m68k_interrupt();
    }

    #[inline]
    fn halt(&self) -> bool {
        self.vdp.should_halt_cpu()
    }

    #[inline]
    fn reset(&self) -> bool {
        self.signals.m68k_reset
    }
}

impl<'a, Medium: PhysicalMedium> z80_emu::BusInterface for MainBus<'a, Medium> {
    #[inline]
    // TODO remove
    #[allow(clippy::match_same_arms)]
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
                self.z80_accessed_68k_bus = true;

                let m68k_addr = self.memory.z80_bank_register.map_to_68k_address(address);
                if !(0xA00000..=0xA0FFFF).contains(&m68k_addr) {
                    <Self as m68000_emu::BusInterface>::read_byte(self, m68k_addr)
                } else {
                    // TODO this should lock up the system
                    panic!(
                        "Z80 attempted to read its own memory from the 68k bus; z80_addr={address:04X}, m68k_addr={m68k_addr:08X}"
                    );
                }
            }
        }
    }

    #[inline]
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
                    0x00 => self.ym2612.write_address_1(value),
                    0x02 => self.ym2612.write_address_2(value),
                    0x01 | 0x03 => self.ym2612.write_data(value),
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
                self.z80_accessed_68k_bus = true;

                let m68k_addr = self.memory.z80_bank_register.map_to_68k_address(address);
                if !(0xA00000..=0xA0FFFF).contains(&m68k_addr) {
                    self.apply_byte_write(m68k_addr, value);
                } else {
                    // TODO this should lock up the system
                    panic!(
                        "Z80 attempted to read its own memory from the 68k bus; z80_addr={address:04X}, m68k_addr={m68k_addr:08X}"
                    );
                }
            }
        }
    }

    #[inline]
    fn read_io(&mut self, _address: u16) -> u8 {
        // I/O ports are not wired up to the Z80
        0xFF
    }

    #[inline]
    fn write_io(&mut self, _address: u16, _value: u8) {
        // I/O ports are not wired up to the Z80
    }

    #[inline]
    fn nmi(&self) -> InterruptLine {
        // The NMI line is not connected to anything
        InterruptLine::High
    }

    #[inline]
    fn int(&self) -> InterruptLine {
        self.vdp.z80_interrupt_line()
    }

    #[inline]
    fn busreq(&self) -> bool {
        self.memory.signals.z80_busreq
    }

    #[inline]
    fn reset(&self) -> bool {
        self.memory.signals.z80_reset
    }
}
