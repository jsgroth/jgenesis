mod mappers;

use crate::bus::cartridge::mappers::{
    Axrom, BandaiFcg, Bnrom, ChrType, Cnrom, Gxrom, Mmc1, Mmc2, Mmc3, Mmc5, Namco163, Namco175,
    NametableMirroring, Nrom, PpuMapResult, Sunsoft, Uxrom, Vrc4, Vrc6, Vrc7,
};
use bincode::de::{BorrowDecoder, Decoder};
use bincode::enc::Encoder;
use bincode::error::{DecodeError, EncodeError};
use bincode::{BorrowDecode, Decode, Encode};
use jgnes_proc_macros::MatchEachVariantMacro;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::{io, mem};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub enum TimingMode {
    Ntsc,
    Pal,
}

impl TimingMode {
    #[must_use]
    pub const fn all() -> &'static [TimingMode] {
        &[TimingMode::Ntsc, TimingMode::Pal]
    }
}

impl Display for TimingMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ntsc => write!(f, "NTSC"),
            Self::Pal => write!(f, "PAL"),
        }
    }
}

use crate::num::GetBit;
#[cfg(test)]
pub(crate) use mappers::new_mmc1;

#[derive(Debug, Clone)]
struct Cartridge {
    timing_mode: TimingMode,
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    has_ram_battery: bool,
    prg_ram_dirty_bit: bool,
    chr_rom: Vec<u8>,
    chr_ram: Vec<u8>,
}

// Encode and Decode are implemented explicitly instead of using derive in order to avoid
// serializing ROM bytes as part of save states
impl Encode for Cartridge {
    fn encode<E: Encoder>(&self, encoder: &mut E) -> Result<(), EncodeError> {
        self.timing_mode.encode(encoder)?;
        self.prg_ram.encode(encoder)?;
        self.has_ram_battery.encode(encoder)?;
        self.prg_ram_dirty_bit.encode(encoder)?;
        self.chr_ram.encode(encoder)?;

        Ok(())
    }
}

impl Decode for Cartridge {
    fn decode<D: Decoder>(decoder: &mut D) -> Result<Self, DecodeError> {
        let timing_mode = Decode::decode(decoder)?;
        let prg_ram = Decode::decode(decoder)?;
        let has_ram_battery = Decode::decode(decoder)?;
        let prg_ram_dirty_bit = Decode::decode(decoder)?;
        let chr_ram = Decode::decode(decoder)?;

        Ok(Self {
            timing_mode,
            prg_rom: vec![],
            prg_ram,
            has_ram_battery,
            prg_ram_dirty_bit,
            chr_rom: vec![],
            chr_ram,
        })
    }
}

impl<'de> BorrowDecode<'de> for Cartridge {
    fn borrow_decode<D: BorrowDecoder<'de>>(decoder: &mut D) -> Result<Self, DecodeError> {
        let timing_mode = BorrowDecode::borrow_decode(decoder)?;
        let prg_ram = BorrowDecode::borrow_decode(decoder)?;
        let has_ram_battery = BorrowDecode::borrow_decode(decoder)?;
        let prg_ram_dirty_bit = BorrowDecode::borrow_decode(decoder)?;
        let chr_ram = BorrowDecode::borrow_decode(decoder)?;

        Ok(Self {
            timing_mode,
            prg_rom: vec![],
            prg_ram,
            has_ram_battery,
            prg_ram_dirty_bit,
            chr_rom: vec![],
            chr_ram,
        })
    }
}

impl Cartridge {
    fn get_prg_rom(&self, address: u32) -> u8 {
        self.prg_rom[(address as usize) & (self.prg_rom.len() - 1)]
    }

    fn get_prg_ram(&self, address: u32) -> u8 {
        if !self.prg_ram.is_empty() {
            self.prg_ram[(address as usize) & (self.prg_ram.len() - 1)]
        } else {
            0xFF
        }
    }

    fn set_prg_ram(&mut self, address: u32, value: u8) {
        if !self.prg_ram.is_empty() {
            let prg_ram_len = self.prg_ram.len();
            self.prg_ram[(address as usize) & (prg_ram_len - 1)] = value;
            if self.has_ram_battery {
                self.prg_ram_dirty_bit = true;
            }
        }
    }

    fn get_chr_rom(&self, address: u32) -> u8 {
        self.chr_rom[(address as usize) & (self.chr_rom.len() - 1)]
    }

    fn get_chr_ram(&self, address: u32) -> u8 {
        self.chr_ram[(address as usize) & (self.chr_ram.len() - 1)]
    }

    fn set_chr_ram(&mut self, address: u32, value: u8) {
        let chr_ram_len = self.chr_ram.len();
        self.chr_ram[(address as usize) & (chr_ram_len - 1)] = value;
    }

    fn move_rom_from(&mut self, other: &mut Self) {
        self.prg_rom = mem::take(&mut other.prg_rom);
        self.chr_rom = mem::take(&mut other.chr_rom);
    }

    fn clone_without_rom(&self) -> Self {
        Self {
            timing_mode: self.timing_mode,
            prg_rom: vec![],
            prg_ram: self.prg_ram.clone(),
            has_ram_battery: self.has_ram_battery,
            prg_ram_dirty_bit: self.prg_ram_dirty_bit,
            chr_rom: vec![],
            chr_ram: self.chr_ram.clone(),
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct MapperImpl<MapperData> {
    cartridge: Cartridge,
    data: MapperData,
}

pub(crate) trait HasBasicPpuMapping {
    fn map_ppu_address(&self, address: u16) -> PpuMapResult;
}

impl<MapperData> MapperImpl<MapperData>
where
    MapperImpl<MapperData>: HasBasicPpuMapping,
{
    fn read_ppu_address(&self, address: u16, vram: &[u8; 2048]) -> u8 {
        self.map_ppu_address(address).read(&self.cartridge, vram)
    }

    fn write_ppu_address(&mut self, address: u16, value: u8, vram: &mut [u8; 2048]) {
        self.map_ppu_address(address)
            .write(value, &mut self.cartridge, vram);
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Encode, Decode, MatchEachVariantMacro)]
pub(crate) enum Mapper {
    Axrom(MapperImpl<Axrom>),
    BandaiFcg(MapperImpl<BandaiFcg>),
    Bnrom(MapperImpl<Bnrom>),
    Cnrom(MapperImpl<Cnrom>),
    Gxrom(MapperImpl<Gxrom>),
    Mmc1(MapperImpl<Mmc1>),
    Mmc2(MapperImpl<Mmc2>),
    Mmc3(MapperImpl<Mmc3>),
    Mmc5(MapperImpl<Mmc5>),
    Namco163(MapperImpl<Namco163>),
    Namco175(MapperImpl<Namco175>),
    Nrom(MapperImpl<Nrom>),
    Sunsoft(MapperImpl<Sunsoft>),
    Uxrom(MapperImpl<Uxrom>),
    Vrc4(MapperImpl<Vrc4>),
    Vrc6(MapperImpl<Vrc6>),
    Vrc7(MapperImpl<Vrc7>),
}

impl Mapper {
    /// Retrieve the mapper's user-readable name. Only used for logging output.
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Axrom(..) => "AxROM",
            Self::BandaiFcg(bandai_fcg) => bandai_fcg.name(),
            Self::Bnrom(..) => "BNROM / NINA-001",
            Self::Cnrom(..) => "CNROM",
            Self::Gxrom(gxrom) => gxrom.name(),
            Self::Mmc1(..) => "MMC1",
            Self::Mmc2(mmc2) => mmc2.name(),
            Self::Mmc3(mmc3) => mmc3.name(),
            Self::Mmc5(..) => "MMC5",
            Self::Namco163(..) => "Namco 163",
            Self::Namco175(..) => "Namco 175",
            Self::Nrom(..) => "NROM",
            Self::Sunsoft(..) => "Sunsoft",
            Self::Uxrom(uxrom) => uxrom.name(),
            Self::Vrc4(vrc4) => vrc4.name(),
            Self::Vrc6(..) => "VRC6",
            Self::Vrc7(..) => "VRC7",
        }
    }

    /// Read a value from the given address in the CPU address space.
    pub(crate) fn read_cpu_address(&mut self, address: u16) -> u8 {
        match_each_variant!(self, mapper => mapper.read_cpu_address(address))
    }

    /// Write a value to the given address in the CPU address space.
    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        match_each_variant!(self, mapper => mapper.write_cpu_address(address, value));
    }

    /// Read a value from the given address in the PPU address space.
    pub(crate) fn read_ppu_address(&mut self, address: u16, vram: &[u8; 2048]) -> u8 {
        match_each_variant!(self, mapper => mapper.read_ppu_address(address, vram))
    }

    /// Write a value to the given address in the PPU address space.
    pub(crate) fn write_ppu_address(&mut self, address: u16, value: u8, vram: &mut [u8; 2048]) {
        match_each_variant!(self, mapper => mapper.write_ppu_address(address, value, vram));
    }

    /// Perform any processing that should be performed after every PPU cycle.
    pub(crate) fn tick(&mut self, ppu_bus_address: u16) {
        if let Self::Mmc3(mmc3) = self {
            mmc3.tick(ppu_bus_address);
        }
    }

    /// Perform any processing that should be performed after every CPU cycle. Commonly used for
    /// interrupt counters and expansion audio.
    pub(crate) fn tick_cpu(&mut self) {
        match self {
            Self::BandaiFcg(bandai_fcg) => {
                bandai_fcg.tick_cpu();
            }
            Self::Mmc1(mmc1) => {
                mmc1.tick_cpu();
            }
            Self::Mmc5(mmc5) => {
                mmc5.tick_cpu();
            }
            Self::Namco163(namco163) => {
                namco163.tick_cpu();
            }
            Self::Sunsoft(sunsoft) => {
                sunsoft.tick_cpu();
            }
            Self::Vrc4(vrc4) => {
                vrc4.tick_cpu();
            }
            Self::Vrc6(vrc6) => {
                vrc6.tick_cpu();
            }
            Self::Vrc7(vrc7) => {
                vrc7.tick_cpu();
            }
            _ => {}
        }
    }

    /// Return whether this board is currently generating an IRQ.
    pub(crate) fn interrupt_flag(&self) -> bool {
        match self {
            Self::BandaiFcg(bandai_fcg) => bandai_fcg.interrupt_flag(),
            Self::Mmc3(mmc3) => mmc3.interrupt_flag(),
            Self::Mmc5(mmc5) => mmc5.interrupt_flag(),
            Self::Namco163(namco163) => namco163.interrupt_flag(),
            Self::Sunsoft(sunsoft) => sunsoft.interrupt_flag(),
            Self::Vrc4(vrc4) => vrc4.interrupt_flag(),
            Self::Vrc6(vrc6) => vrc6.interrupt_flag(),
            Self::Vrc7(vrc7) => vrc7.interrupt_flag(),
            _ => false,
        }
    }

    /// Process a PPUCTRL write. Only needed by the MMC5 mapper in order to know whether double
    /// height sprites are enabled.
    pub(crate) fn process_ppu_ctrl_update(&mut self, value: u8) {
        if let Self::Mmc5(mmc5) = self {
            mmc5.process_ppu_ctrl_update(value);
        }
    }

    /// Notify the mapper that the CPU will imminently access the PPUDATA register. This is required
    /// by MMC5 to map PPUDATA reads/writes to the correct CHR banks.
    ///
    /// This should be called *before* the actual memory access.
    pub(crate) fn about_to_access_ppu_data(&mut self) {
        if let Self::Mmc5(mmc5) = self {
            mmc5.about_to_access_ppu_data();
        }
    }

    /// Return whether the board's writable memory (if any) has been written to since the last time
    /// this method was called.
    pub(crate) fn get_and_clear_ram_dirty_bit(&mut self) -> bool {
        match self {
            Mapper::BandaiFcg(mapper) => {
                if mapper.get_and_clear_eeprom_dirty_bit() {
                    return true;
                }
            }
            Mapper::Namco163(mapper) => {
                if mapper.has_battery_backed_internal_ram()
                    && mapper.get_and_clear_internal_ram_dirty_bit()
                {
                    return true;
                }
            }
            _ => {}
        }

        match_each_variant!(self, mapper => {
            let dirty_bit = mapper.cartridge.prg_ram_dirty_bit;
            mapper.cartridge.prg_ram_dirty_bit = false;
            dirty_bit
        })
    }

    /// Return the board's writable memory as a slice. This will be an empty slice if the board
    /// has no PRG RAM or EEPROM.
    pub(crate) fn get_prg_ram(&self) -> &[u8] {
        match self {
            Mapper::BandaiFcg(mapper) => {
                if let Some(eeprom) = mapper.eeprom() {
                    return eeprom;
                }
            }
            Mapper::Namco163(mapper) => {
                if mapper.has_battery_backed_internal_ram() {
                    return mapper.get_internal_ram();
                }
            }
            _ => {}
        }

        match_each_variant!(self, mapper => &mapper.cartridge.prg_ram)
    }

    pub(crate) fn has_persistent_ram(&self) -> bool {
        match self {
            Mapper::BandaiFcg(mapper) => {
                if mapper.eeprom().is_some() {
                    return true;
                }
            }
            Mapper::Namco163(mapper) => {
                if mapper.has_battery_backed_internal_ram() {
                    return true;
                }
            }
            _ => {}
        }

        !self.get_prg_ram().is_empty()
            && match_each_variant!(self, mapper => mapper.cartridge.has_ram_battery)
    }

    /// Retrieve the timing mode of the cartridge (NTSC/PAL).
    pub(crate) fn timing_mode(&self) -> TimingMode {
        match_each_variant!(self, mapper => mapper.cartridge.timing_mode)
    }

    /// If the board has expansion audio, generate an audio sample and mix it with the mixed APU
    /// sample.
    ///
    /// If the board does not have expansion audio or it is not enabled then this method will simply
    /// return the mixed APU sample as-is.
    pub(crate) fn sample_audio(&self, mixed_apu_sample: f64) -> f64 {
        match self {
            Self::Mmc5(mmc5) => mmc5.sample_audio(mixed_apu_sample),
            Self::Namco163(namco163) => namco163.sample_audio(mixed_apu_sample),
            Self::Sunsoft(sunsoft) => sunsoft.sample_audio(mixed_apu_sample),
            Self::Vrc6(vrc6) => vrc6.sample_audio(mixed_apu_sample),
            Self::Vrc7(vrc7) => vrc7.sample_audio(mixed_apu_sample),
            _ => mixed_apu_sample,
        }
    }

    /// Move cartridge ROM fields from another `Mapper` instance. Used when loading save states.
    pub(crate) fn move_rom_from(&mut self, other: &mut Self) {
        let other_cartridge = match_each_variant!(other, mapper => &mut mapper.cartridge);
        match_each_variant!(self, mapper => mapper.cartridge.move_rom_from(other_cartridge));
    }

    /// Clone all internal state except for the cartridge ROM fields, which will be set to empty
    /// Vecs in the clone.
    pub(crate) fn clone_without_rom(&self) -> Self {
        let cartridge = match_each_variant!(self, mapper => &mapper.cartridge);
        match_each_variant!(self, mapper => :variant(MapperImpl {
            cartridge: cartridge.clone_without_rom(),
            data: mapper.data.clone(),
        }))
    }
}

#[derive(Debug, Error)]
pub enum CartridgeFileError {
    #[error("I/O error: {source}")]
    Io {
        #[from]
        source: io::Error,
    },
    #[error("invalid or unsupported file format")]
    Format,
    #[error("unsupported mapper: {mapper_number}")]
    UnsupportedMapper { mapper_number: u16 },
    #[error("cartridge header specifies both volatile and non-volatile PRG RAM")]
    MultiplePrgRamTypes,
    #[error("unsupported timing mode byte: {byte}")]
    UnsupportedTimingMode { byte: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileFormat {
    INes,
    Nes2Point0,
}

#[derive(Debug, Clone)]
struct INesHeader {
    mapper_number: u16,
    sub_mapper_number: u8,
    timing_mode: TimingMode,
    prg_rom_size: u32,
    prg_ram_size: u32,
    chr_rom_size: u32,
    chr_ram_size: u32,
    chr_type: ChrType,
    nametable_mirroring: NametableMirroring,
    has_trainer: bool,
    has_battery: bool,
    has_four_screen_vram: bool,
}

impl INesHeader {
    fn parse_from_file(file_bytes: &[u8]) -> Result<INesHeader, CartridgeFileError> {
        let header = &file_bytes[..16];

        // All iNES headers should begin with this 4-byte sequence, which is "NES" followed by the
        // character that MS-DOS used for EOF
        if header[..4] != [0x4E, 0x45, 0x53, 0x1A] {
            return Err(CartridgeFileError::Format);
        }

        let prg_rom_size = 16 * 1024 * ((u32::from(header[9] & 0x0F) << 8) | u32::from(header[4]));
        let chr_rom_size = 8 * 1024 * ((u32::from(header[9] & 0xF0) << 4) | u32::from(header[5]));

        let has_trainer = header[6].bit(2);

        let mapper_number = u16::from((header[7] & 0xF0) | ((header[6] & 0xF0) >> 4));

        let chr_type = if chr_rom_size == 0 {
            ChrType::RAM
        } else {
            ChrType::ROM
        };

        let nametable_mirroring = if header[6].bit(0) {
            NametableMirroring::Vertical
        } else {
            NametableMirroring::Horizontal
        };

        let has_four_screen_vram = header[6].bit(3);

        let has_battery = header[6].bit(1);

        let format = if header[7] & 0x0C == 0x08 {
            FileFormat::Nes2Point0
        } else {
            FileFormat::INes
        };

        let sub_mapper_number = match format {
            FileFormat::Nes2Point0 => header[8] >> 4,
            FileFormat::INes => 0,
        };

        let timing_mode = match format {
            FileFormat::Nes2Point0 => {
                let timing_mode_byte = header[12] & 0x03;
                match timing_mode_byte {
                    0x00 | 0x02 => TimingMode::Ntsc,
                    0x01 => TimingMode::Pal,
                    0x03 => {
                        return Err(CartridgeFileError::UnsupportedTimingMode {
                            byte: timing_mode_byte,
                        });
                    }
                    _ => unreachable!("value & 0x03 should always be 0x00/0x01/0x02/0x03"),
                }
            }
            FileFormat::INes => {
                if header[0].bit(0) {
                    TimingMode::Pal
                } else {
                    TimingMode::Ntsc
                }
            }
        };

        let prg_ram_size = match format {
            FileFormat::Nes2Point0 => {
                let volatile_shift = header[10] & 0x0F;
                let non_volatile_shift = header[10] >> 4;
                // TODO separate these? very very few games have both volatile and non-volatile RAM
                let volatile_ram = if volatile_shift > 0 {
                    64 << volatile_shift
                } else {
                    0
                };
                let non_volatile_ram = if non_volatile_shift > 0 {
                    64 << non_volatile_shift
                } else {
                    0
                };
                volatile_ram + non_volatile_ram
            }
            FileFormat::INes => 8192,
        };

        let chr_ram_size = match (chr_type, format) {
            (ChrType::RAM, FileFormat::Nes2Point0) => {
                let chr_ram_shift = header[11] & 0x0F;
                if chr_ram_shift > 0 {
                    64 << chr_ram_shift
                } else {
                    0
                }
            }
            (ChrType::RAM, FileFormat::INes) => 8192,
            (ChrType::ROM, _) => 0,
        };

        Ok(Self {
            mapper_number,
            sub_mapper_number,
            timing_mode,
            prg_rom_size,
            prg_ram_size,
            chr_rom_size,
            chr_ram_size,
            chr_type,
            nametable_mirroring,
            has_trainer,
            has_battery,
            has_four_screen_vram,
        })
    }
}

/// Parse cartridge data out of an iNES file.
///
/// # Errors
///
/// This function will return an error if the given bytes do not appear to represent an iNES file.
pub(crate) fn from_ines_file(
    file_bytes: &[u8],
    sav_bytes: Option<Vec<u8>>,
    forced_timing_mode: Option<TimingMode>,
) -> Result<Mapper, CartridgeFileError> {
    let header = INesHeader::parse_from_file(file_bytes)?;

    // Header is 16 bytes, trainer is 512 bytes if present
    let prg_rom_start_address = if header.has_trainer { 16 + 512 } else { 16 } as usize;
    let prg_rom_end_address = prg_rom_start_address + header.prg_rom_size as usize;
    let chr_rom_end_address = prg_rom_end_address + header.chr_rom_size as usize;

    let prg_rom = Vec::from(&file_bytes[prg_rom_start_address..prg_rom_end_address]);
    let chr_rom = Vec::from(&file_bytes[prg_rom_end_address..chr_rom_end_address]);

    // Hack to handle MMC5 headers that don't specify PRG RAM size; use 64KB of RAM in this case,
    // which is the maximum possible PRG RAM size for MMC5
    let prg_ram_size = if header.mapper_number == 5 && header.prg_ram_size == 0 {
        64 * 1024
    } else {
        header.prg_ram_size
    };
    if prg_ram_size != header.prg_ram_size {
        log::info!(
            "Ignoring PRG RAM size of {} in header; using {prg_ram_size} instead",
            header.prg_ram_size
        );
    }

    let prg_ram = if let Some(sav_bytes) = &sav_bytes {
        if sav_bytes.len() == prg_ram_size as usize {
            sav_bytes.clone()
        } else {
            vec![0; prg_ram_size as usize]
        }
    } else {
        vec![0; prg_ram_size as usize]
    };

    let timing_mode = forced_timing_mode.unwrap_or(header.timing_mode);
    if timing_mode != header.timing_mode {
        log::info!(
            "Forcing timing mode to {timing_mode}; ignoring timing mode {} from cartridge header",
            header.timing_mode
        );
    }

    let cartridge = Cartridge {
        timing_mode,
        prg_rom,
        prg_ram,
        has_ram_battery: header.has_battery,
        prg_ram_dirty_bit: header.has_battery,
        chr_rom,
        chr_ram: vec![0; header.chr_ram_size as usize],
    };

    let chr_size = match header.chr_type {
        ChrType::ROM => header.chr_rom_size,
        ChrType::RAM => header.chr_ram_size,
    };

    let mapper = match header.mapper_number {
        0 => Mapper::Nrom(MapperImpl {
            cartridge,
            data: Nrom::new(header.chr_type, header.nametable_mirroring),
        }),
        1 => Mapper::Mmc1(MapperImpl {
            cartridge,
            data: Mmc1::new(header.chr_type),
        }),
        2 | 71 => Mapper::Uxrom(MapperImpl {
            cartridge,
            data: Uxrom::new(
                header.mapper_number,
                header.sub_mapper_number,
                header.chr_type,
                header.nametable_mirroring,
            ),
        }),
        3 => Mapper::Cnrom(MapperImpl {
            cartridge,
            data: Cnrom::new(header.chr_type, header.nametable_mirroring),
        }),
        4 | 76 | 88 | 95 | 154 | 206 => Mapper::Mmc3(MapperImpl {
            cartridge,
            data: Mmc3::new(
                header.chr_type,
                header.prg_rom_size,
                chr_size,
                header.mapper_number,
                header.sub_mapper_number,
                header.nametable_mirroring,
                header.has_four_screen_vram,
            ),
        }),
        5 => Mapper::Mmc5(MapperImpl {
            cartridge,
            data: Mmc5::new(),
        }),
        7 => Mapper::Axrom(MapperImpl {
            cartridge,
            data: Axrom::new(header.chr_type),
        }),
        9 => Mapper::Mmc2(MapperImpl {
            cartridge,
            data: Mmc2::new_mmc2(),
        }),
        10 => Mapper::Mmc2(MapperImpl {
            cartridge,
            data: Mmc2::new_mmc4(),
        }),
        11 | 66 | 140 => Mapper::Gxrom(MapperImpl {
            cartridge,
            data: Gxrom::new(header.mapper_number, header.nametable_mirroring),
        }),
        16 | 153 | 159 => Mapper::BandaiFcg(MapperImpl {
            cartridge,
            data: BandaiFcg::new(
                header.mapper_number,
                header.sub_mapper_number,
                header.chr_type,
                header.prg_ram_size,
                sav_bytes.as_ref(),
            ),
        }),
        19 => Mapper::Namco163(MapperImpl {
            cartridge,
            data: Namco163::new(
                header.sub_mapper_number,
                header.chr_type,
                header.has_battery,
                header.prg_ram_size,
                sav_bytes,
            ),
        }),
        21 | 22 | 23 | 25 => Mapper::Vrc4(MapperImpl {
            cartridge,
            data: Vrc4::new(
                header.mapper_number,
                header.sub_mapper_number,
                header.chr_type,
            ),
        }),
        24 | 26 => Mapper::Vrc6(MapperImpl {
            cartridge,
            data: Vrc6::new(header.mapper_number, header.chr_type),
        }),
        34 => Mapper::Bnrom(MapperImpl {
            cartridge,
            data: Bnrom::new(header.chr_type, header.nametable_mirroring),
        }),
        69 => Mapper::Sunsoft(MapperImpl {
            cartridge,
            data: Sunsoft::new(header.chr_type),
        }),
        85 => Mapper::Vrc7(MapperImpl {
            cartridge,
            data: Vrc7::new(header.sub_mapper_number, header.chr_type),
        }),
        210 => Mapper::Namco175(MapperImpl {
            cartridge,
            data: Namco175::new(
                header.sub_mapper_number,
                header.chr_type,
                header.nametable_mirroring,
            ),
        }),
        _ => {
            return Err(CartridgeFileError::UnsupportedMapper {
                mapper_number: header.mapper_number,
            });
        }
    };

    log::info!("Timing mode: {timing_mode}");
    log::info!(
        "Mapper number: {} ({})",
        header.mapper_number,
        mapper.name()
    );
    log::info!("PRG ROM size: {}", header.prg_rom_size);
    log::info!("PRG RAM size: {}", prg_ram_size);
    log::info!(
        "Cartridge has battery-backed PRG RAM: {}",
        header.has_battery
    );
    log::info!("CHR ROM size: {}", header.chr_rom_size);
    log::info!("CHR RAM size: {}", header.chr_ram_size);
    log::info!("CHR memory type: {:?}", header.chr_type);
    log::info!(
        "Hardwired nametable mirroring: {:?} (not applicable to all mappers)",
        header.nametable_mirroring
    );
    log::info!(
        "Has 4-screen nametable VRAM: {}",
        header.has_four_screen_vram
    );

    Ok(mapper)
}
