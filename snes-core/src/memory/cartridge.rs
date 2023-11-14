use crate::api::{CoprocessorRoms, LoadError, LoadResult};
use crate::coprocessors::cx4::Cx4;
use crate::coprocessors::upd77c25;
use crate::coprocessors::upd77c25::{Upd77c25, Upd77c25Variant};
use bincode::{Decode, Encode};
use crc::Crc;
use jgenesis_common::frontend::{PartialClone, TimingMode};
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::mem;
use std::ops::Deref;

#[derive(Debug, Clone, FakeEncode, FakeDecode)]
pub struct Rom(pub Box<[u8]>);

impl Default for Rom {
    fn default() -> Self {
        Rom(vec![].into_boxed_slice())
    }
}

impl Deref for Rom {
    type Target = Box<[u8]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CartridgeType {
    LoRom,
    HiRom,
    ExHiRom,
    Cx4,
}

impl Display for CartridgeType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LoRom => write!(f, "LoROM"),
            Self::HiRom => write!(f, "HiROM"),
            Self::ExHiRom => write!(f, "ExHiROM"),
            Self::Cx4 => write!(f, "CX4"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DspVariant {
    Dsp1,
    Dsp2,
    Dsp3,
    Dsp4,
}

impl Display for DspVariant {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dsp1 => write!(f, "DSP-1"),
            Self::Dsp2 => write!(f, "DSP-2"),
            Self::Dsp3 => write!(f, "DSP-3"),
            Self::Dsp4 => write!(f, "DSP-4"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum St01xVariant {
    St010,
    St011,
}

impl From<St01xVariant> for Upd77c25Variant {
    fn from(value: St01xVariant) -> Self {
        match value {
            St01xVariant::St010 => Self::St010,
            St01xVariant::St011 => Self::St011,
        }
    }
}

impl Display for St01xVariant {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::St010 => write!(f, "ST010"),
            Self::St011 => write!(f, "ST011"),
        }
    }
}

const LOROM_HEADER_ADDR: usize = 0x007FC0;
const HIROM_HEADER_ADDR: usize = 0x00FFC0;
const EXHIROM_HEADER_ADDR: usize = 0x40FFC0;

const HEADER_TYPE_OFFSET: usize = 0x15;

const LOROM_RESET_VECTOR: usize = 0x7FFC;
const HIROM_RESET_VECTOR: usize = 0xFFFC;

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub enum Cartridge {
    LoRom {
        #[partial_clone(default)]
        rom: Rom,
        sram: Box<[u8]>,
        upd77c25: Option<Upd77c25>,
    },
    HiRom {
        #[partial_clone(default)]
        rom: Rom,
        sram: Box<[u8]>,
        upd77c25: Option<Upd77c25>,
    },
    ExHiRom {
        #[partial_clone(default)]
        rom: Rom,
        sram: Box<[u8]>,
    },
    Cx4(#[partial_clone(partial)] Cx4),
    St01x {
        #[partial_clone(default)]
        rom: Rom,
        upd77c25: Upd77c25,
    },
}

impl Cartridge {
    pub fn create(
        rom: Box<[u8]>,
        initial_sram: Option<Vec<u8>>,
        coprocessor_roms: &CoprocessorRoms,
        forced_timing_mode: Option<TimingMode>,
    ) -> LoadResult<Self> {
        let cartridge_type = guess_cartridge_type(&rom).unwrap_or_else(|| {
            log::error!("Unable to confidently determine ROM type; defaulting to LoROM");
            CartridgeType::LoRom
        });

        let rom_header_addr = match cartridge_type {
            CartridgeType::LoRom | CartridgeType::Cx4 => LOROM_HEADER_ADDR,
            CartridgeType::HiRom => HIROM_HEADER_ADDR,
            CartridgeType::ExHiRom => EXHIROM_HEADER_ADDR,
        };

        // Determine NTSC/PAL
        let region_byte = rom[rom_header_addr + 0x19];
        let timing_mode = forced_timing_mode.unwrap_or_else(|| region_to_timing_mode(region_byte));

        // Check if cartridge is ST010/ST011; these don't report RAM size in the header (always 4KB)
        let chipset_byte = rom[rom_header_addr + 0x16];
        let is_st01x = chipset_byte == 0xF6
            && rom[rom_header_addr + 0x14] == 0x00
            && rom[rom_header_addr - 1] == 0x01;

        // $FFD8 contains SRAM size as a kilobytes power of 2
        let sram_header_byte = rom[rom_header_addr | 0x0018];
        let sram_len = if is_st01x {
            upd77c25::ST01X_RAM_LEN_BYTES
        } else if sram_header_byte == 0 {
            0
        } else {
            1 << (10 + sram_header_byte)
        };

        let sram = match initial_sram {
            Some(sram) if sram.len() == sram_len => sram.into_boxed_slice(),
            _ => vec![0; sram_len].into_boxed_slice(),
        };

        log::info!("Using mapper {cartridge_type} with SRAM size {sram_len}");

        if is_st01x {
            let st01x_variant = guess_st01x_variant(&rom);

            log::info!("Detected {st01x_variant} coprocessor");

            let st01x_rom_fn = match st01x_variant {
                St01xVariant::St010 => {
                    coprocessor_roms.st010.as_ref().ok_or(LoadError::MissingSt010Rom)?
                }
                St01xVariant::St011 => {
                    coprocessor_roms.st011.as_ref().ok_or(LoadError::MissingSt011Rom)?
                }
            };

            let st01x_rom = st01x_rom_fn()
                .map_err(|(source, path)| LoadError::CoprocessorRomLoad { source, path })?;
            let upd77c25 = Upd77c25::new(&st01x_rom, st01x_variant.into(), &sram, timing_mode);

            return Ok(Self::St01x { rom: Rom(rom), upd77c25 });
        }

        // Check for DSP-1/2/3/4 coprocessor
        let chipset_byte = rom[rom_header_addr + 0x16];
        let upd77c25 = if (0x03..0x06).contains(&chipset_byte) {
            let dsp_variant = guess_dsp_variant(&rom);

            log::info!("Detected DSP coprocessor of type {dsp_variant}");

            let dsp_rom_fn = match dsp_variant {
                DspVariant::Dsp1 => {
                    coprocessor_roms.dsp1.as_ref().ok_or(LoadError::MissingDsp1Rom)?
                }
                DspVariant::Dsp2 => {
                    coprocessor_roms.dsp2.as_ref().ok_or(LoadError::MissingDsp2Rom)?
                }
                DspVariant::Dsp3 => {
                    coprocessor_roms.dsp3.as_ref().ok_or(LoadError::MissingDsp3Rom)?
                }
                DspVariant::Dsp4 => {
                    coprocessor_roms.dsp4.as_ref().ok_or(LoadError::MissingDsp4Rom)?
                }
            };

            let dsp_rom = dsp_rom_fn()
                .map_err(|(source, path)| LoadError::CoprocessorRomLoad { source, path })?;

            Some(Upd77c25::new(&dsp_rom, Upd77c25Variant::Dsp, &sram, timing_mode))
        } else {
            None
        };

        Ok(match cartridge_type {
            CartridgeType::LoRom => Self::LoRom { rom: Rom(rom), sram, upd77c25 },
            CartridgeType::HiRom => Self::HiRom { rom: Rom(rom), sram, upd77c25 },
            CartridgeType::ExHiRom => Self::ExHiRom { rom: Rom(rom), sram },
            CartridgeType::Cx4 => Self::Cx4(Cx4::new(Rom(rom))),
        })
    }

    pub fn read(&mut self, address: u32) -> Option<u8> {
        let bank = (address >> 16) & 0xFF;
        let offset = address & 0xFFFF;
        let (mapped_address, rom, sram) = match self {
            Self::LoRom { rom, sram, upd77c25: Some(upd77c25) } => match (bank, offset) {
                (0x30..=0x3F | 0xC0..=0xCF, 0x8000..=0xBFFF) => return Some(upd77c25.read_data()),
                (0x30..=0x3F | 0xC0..=0xCF, 0xC000..=0xFFFF) => {
                    return Some(upd77c25.read_status());
                }
                _ => (lorom_map_address(address, rom.len() as u32, sram.len() as u32), rom, sram),
            },
            Self::LoRom { rom, sram, .. } => {
                (lorom_map_address(address, rom.len() as u32, sram.len() as u32), rom, sram)
            }
            Self::HiRom { rom, sram, upd77c25: Some(upd77c25) } => match (bank, offset) {
                (0x00..=0x0F | 0x80..=0x8F, 0x6000..=0x6FFF) => return Some(upd77c25.read_data()),
                (0x00..=0x0F | 0x80..=0x8F, 0x7000..=0x7FFF) => {
                    return Some(upd77c25.read_status());
                }
                _ => (hirom_map_address(address, rom.len() as u32, sram.len() as u32), rom, sram),
            },
            Self::HiRom { rom, sram, .. } => {
                (hirom_map_address(address, rom.len() as u32, sram.len() as u32), rom, sram)
            }
            Self::ExHiRom { rom, sram } => {
                (exhirom_map_address(address, rom.len() as u32, sram.len() as u32), rom, sram)
            }
            Self::Cx4(cx4) => return cx4.read(address),
            Self::St01x { rom, upd77c25 } => {
                return match (bank, offset) {
                    (0x60..=0x67, 0x0000) => Some(upd77c25.read_data()),
                    (0x60..=0x67, 0x0001) => Some(upd77c25.read_status()),
                    (0x68..=0x6F, 0x0000..=0x0FFF) => {
                        let sram_addr = ((bank & 0x7) << 12) | (offset & 0xFFF);
                        Some(upd77c25.read_ram(sram_addr))
                    }
                    _ => match lorom_map_address(address, rom.len() as u32, 0) {
                        CartridgeAddress::Rom(rom_addr) => Some(rom[rom_addr as usize]),
                        _ => None,
                    },
                };
            }
        };

        match mapped_address {
            CartridgeAddress::None => None,
            CartridgeAddress::Rom(rom_addr) => Some(rom[rom_addr as usize]),
            CartridgeAddress::Sram(sram_addr) => Some(sram[sram_addr as usize]),
        }
    }

    pub fn write(&mut self, address: u32, value: u8) {
        let bank = (address >> 16) & 0xFF;
        let offset = address & 0xFFFF;
        match self {
            Self::LoRom { rom, sram, upd77c25 } => match (upd77c25, bank, offset) {
                (Some(upd77c25), 0x30..=0x3F | 0xC0..=0xCF, 0x8000..=0xBFFF) => {
                    upd77c25.write_data(value);
                }
                _ => match lorom_map_address(address, rom.len() as u32, sram.len() as u32) {
                    CartridgeAddress::Rom(_) | CartridgeAddress::None => {}
                    CartridgeAddress::Sram(sram_addr) => {
                        sram[sram_addr as usize] = value;
                    }
                },
            },
            Self::HiRom { rom, sram, upd77c25 } => match (upd77c25, bank, offset) {
                (Some(upd77c25), 0x00..=0x0F | 0x80..=0x8F, 0x6000..=0x6FFF) => {
                    upd77c25.write_data(value);
                }
                _ => match hirom_map_address(address, rom.len() as u32, sram.len() as u32) {
                    CartridgeAddress::Rom(_) | CartridgeAddress::None => {}
                    CartridgeAddress::Sram(sram_addr) => {
                        sram[sram_addr as usize] = value;
                    }
                },
            },
            Self::ExHiRom { rom, sram } => {
                match exhirom_map_address(address, rom.len() as u32, sram.len() as u32) {
                    CartridgeAddress::Sram(sram_addr) => {
                        sram[sram_addr as usize] = value;
                    }
                    CartridgeAddress::Rom(_) | CartridgeAddress::None => {}
                }
            }
            Self::Cx4(cx4) => {
                cx4.write(address, value);
            }
            Self::St01x { upd77c25, .. } => match (bank, offset) {
                (0x60..=0x67, 0x0000) => upd77c25.write_data(value),
                (0x68..=0x6F, 0x0000..=0x0FFF) => {
                    let sram_addr = ((bank & 0x7) << 12) | (offset & 0xFFF);
                    upd77c25.write_ram(sram_addr, value);
                }
                _ => {}
            },
        }
    }

    pub fn take_rom(&mut self) -> Vec<u8> {
        match self {
            Self::LoRom { rom, .. }
            | Self::HiRom { rom, .. }
            | Self::ExHiRom { rom, .. }
            | Self::St01x { rom, .. } => mem::take(&mut rom.0).into_vec(),
            Self::Cx4(cx4) => cx4.take_rom(),
        }
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        let other_rom = other.take_rom();

        match self {
            Self::LoRom { rom, .. }
            | Self::HiRom { rom, .. }
            | Self::ExHiRom { rom, .. }
            | Self::St01x { rom, .. } => {
                *rom = Rom(other_rom.into_boxed_slice());
            }
            Self::Cx4(cx4) => {
                cx4.set_rom(other_rom);
            }
        }
    }

    pub fn sram(&self) -> Option<&[u8]> {
        match self {
            Self::LoRom { sram, .. } | Self::HiRom { sram, .. } | Self::ExHiRom { sram, .. }
                if !sram.is_empty() =>
            {
                Some(sram)
            }
            Self::LoRom { .. } | Self::HiRom { .. } | Self::ExHiRom { .. } | Self::Cx4 { .. } => {
                None
            }
            Self::St01x { upd77c25, .. } => Some(upd77c25.sram()),
        }
    }

    pub fn tick(&mut self, master_cycles_elapsed: u64) {
        match self {
            Self::LoRom { upd77c25: Some(upd77c25), .. }
            | Self::HiRom { upd77c25: Some(upd77c25), .. }
            | Self::St01x { upd77c25, .. } => {
                upd77c25.tick(master_cycles_elapsed);
            }
            _ => {}
        }
    }

    pub fn reset(&mut self) {
        match self {
            Self::LoRom { upd77c25: Some(upd77c25), .. }
            | Self::HiRom { upd77c25: Some(upd77c25), .. } => {
                upd77c25.reset();
            }
            _ => {}
        }
    }
}

pub fn region_to_timing_mode(region_byte: u8) -> TimingMode {
    match region_byte {
        // Japan / USA / South Korea / Canada / Brazil
        0x00 | 0x01 | 0x0D | 0x0F | 0x10 => TimingMode::Ntsc,
        // various European and Asian countries (other than Japan/Korea) + Australia
        0x02..=0x0C | 0x11 => TimingMode::Pal,
        _ => {
            log::warn!(
                "Unrecognized region byte in ROM header, defaulting to NTSC: {region_byte:02X}"
            );
            TimingMode::Ntsc
        }
    }
}

fn guess_cartridge_type(rom: &[u8]) -> Option<CartridgeType> {
    if rom.len() < 0x8000 {
        log::error!("ROM is too small; all ROMs should be at least 32KB, was {} bytes", rom.len());
        return None;
    }

    // Check for CX4 (always LoROM); identified by type == $Fx and subtype == $10
    if rom[LOROM_HEADER_ADDR + 0x1A] == 0x33
        && rom[LOROM_HEADER_ADDR + 0x16] & 0xF0 == 0xF0
        && rom[LOROM_HEADER_ADDR - 1] == 0x10
    {
        return Some(CartridgeType::Cx4);
    }

    if rom.len() < 0x10000 {
        // Any ROM less than 64KB must be LoROM; HiROM <64KB wouldn't have anywhere to store
        // the 65816 interrupt vectors
        return Some(CartridgeType::LoRom);
    }

    if rom.len() >= 0x410000 {
        // $25 = ExHiROM, $35 = ExHiROM + FastROM
        // A ROM >4MB with $25/$35 in the header is almost certainly ExHiROM
        let exhirom_type_byte = rom[EXHIROM_HEADER_ADDR + HEADER_TYPE_OFFSET];
        if exhirom_type_byte == 0x25 || exhirom_type_byte == 0x35 {
            return Some(CartridgeType::ExHiRom);
        }
    }

    let mut lorom_points = 0;
    let mut hirom_points = 0;

    let lorom_type_byte = rom[LOROM_HEADER_ADDR + HEADER_TYPE_OFFSET];
    if lorom_type_byte == 0x20 || lorom_type_byte == 0x30 {
        // $20 = LoROM, $30 = LoROM + FastROM
        lorom_points += 1;
    }

    let hirom_type_byte = rom[HIROM_HEADER_ADDR + HEADER_TYPE_OFFSET];
    if hirom_type_byte == 0x21 || hirom_type_byte == 0x31 {
        // $21 = HiROM, $31 = HiROM + FastROM
        hirom_points += 1;
    }

    // All LoROM vectors should be in the range $8000-$FFFF, and A15 is ignored for mapping to ROM
    let lorom_vector = u16::from_le_bytes([rom[LOROM_RESET_VECTOR], rom[LOROM_RESET_VECTOR + 1]]);
    if lorom_vector >= 0x8000 && seems_like_valid_reset_vector(rom, lorom_vector & 0x7FFF) {
        lorom_points += 1;
    }

    let hirom_vector = u16::from_le_bytes([rom[HIROM_RESET_VECTOR], rom[HIROM_RESET_VECTOR + 1]]);
    if seems_like_valid_reset_vector(rom, hirom_vector) {
        hirom_points += 1;
    }

    match lorom_points.cmp(&hirom_points) {
        Ordering::Less => Some(CartridgeType::HiRom),
        Ordering::Greater => Some(CartridgeType::LoRom),
        Ordering::Equal => None,
    }
}

const CRC: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);

fn guess_dsp_variant(rom: &[u8]) -> DspVariant {
    let mut digest = CRC.digest();
    digest.update(rom);
    let checksum = digest.finalize();

    match checksum {
        // Dungeon Master (U/J/E)
        0x0DFD9CEB | 0xAA79FA33 | 0x89A67ADF => DspVariant::Dsp2,
        // SD Gundam GX (J)
        0x4DC3D903 => DspVariant::Dsp3,
        // Top Gear 3000 (U/E) / The Planet's Champ TG 3000 (J)
        0xA20BE998 | 0x493FDB13 | 0xB9B9DF06 => DspVariant::Dsp4,
        _ => DspVariant::Dsp1,
    }
}

fn guess_st01x_variant(rom: &[u8]) -> St01xVariant {
    let mut digest = CRC.digest();
    digest.update(rom);
    let checksum = digest.finalize();

    // Hayazashi Nidan Morita Shogi (J)
    if checksum == 0x81E822AD { St01xVariant::St011 } else { St01xVariant::St010 }
}

const CLC_OPCODE: u8 = 0x18;
const SEI_OPCODE: u8 = 0x78;

fn seems_like_valid_reset_vector(rom: &[u8], vector: u16) -> bool {
    // Nearly all games execute either SEI or CLC as the first instruction at the RESET vector
    let vector = vector as usize;
    vector < rom.len() && (rom[vector] == CLC_OPCODE || rom[vector] == SEI_OPCODE)
}

pub(crate) enum CartridgeAddress {
    None,
    Rom(u32),
    Sram(u32),
}

pub(crate) fn lorom_map_address(address: u32, rom_len: u32, sram_len: u32) -> CartridgeAddress {
    let bank = address >> 16;
    let offset = address & 0xFFFF;
    match (bank, offset) {
        (0x00..=0x6F | 0x80..=0xEF, _) | (0x70..=0x7D | 0xF0..=0xFF, 0x8000..=0xFFFF) => {
            // ROM; typically at $8000-$FFFF and sometimes mirrored into $0000-$7FFF
            let rom_addr = lorom_map_rom_address(address, rom_len);
            CartridgeAddress::Rom(rom_addr)
        }
        (0x70..=0x7D | 0xF0..=0xFF, 0x0000..=0x7FFF) => {
            // SRAM, if mapped
            if sram_len != 0 {
                // SRAM size is always a power of 2; use that to mask address
                // TODO apparently some games have >32KB of SRAM?
                let sram_addr = address & (sram_len - 1);
                CartridgeAddress::Sram(sram_addr)
            } else {
                // Treat as ROM mirror
                let rom_addr = lorom_map_rom_address(address, rom_len);
                CartridgeAddress::Rom(rom_addr)
            }
        }
        _ => CartridgeAddress::None,
    }
}

pub(crate) fn lorom_map_rom_address(address: u32, rom_len: u32) -> u32 {
    // LoROM mapping ignores A23 and A15, and A16-22 are shifted right 1
    let rom_addr = ((address & 0x7F0000) >> 1) | (address & 0x007FFF);
    // TODO better handle unusual ROM sizes
    rom_addr % rom_len
}

fn hirom_map_address(address: u32, rom_len: u32, sram_len: u32) -> CartridgeAddress {
    let bank = address >> 16;
    let offset = address & 0xFFFF;
    match (bank, offset) {
        (0x40..=0x7D | 0xC0..=0xFF, _) | (0x00..=0x3F | 0x80..=0xBF, 0x8000..=0xFFFF) => {
            // ROM
            let rom_addr = hirom_map_rom_address(address, rom_len);
            CartridgeAddress::Rom(rom_addr)
        }
        (0x20..=0x3F | 0xA0..=0xBF, 0x6000..=0x7FFF) if sram_len != 0 => {
            // SRAM, if mapped
            let sram_bank = bank & 0x1F;
            let sram_addr = (sram_bank << 13) | (offset & 0x1FFF);
            CartridgeAddress::Sram(sram_addr & (sram_len - 1))
        }
        _ => CartridgeAddress::None,
    }
}

fn hirom_map_rom_address(address: u32, rom_len: u32) -> u32 {
    // HiROM mapping simply ignores A23 and A22
    // TODO better handle unusual ROM size
    (address & 0x3FFFFF) % rom_len
}

fn exhirom_map_address(address: u32, rom_len: u32, sram_len: u32) -> CartridgeAddress {
    let bank = address >> 16;
    let offset = address & 0xFFFF;
    match (bank, offset) {
        (0x40..=0x7D | 0xC0..=0xFF, _) | (0x00..=0x3F | 0x80..=0xBF, 0x8000..=0xFFFF) => {
            // ROM
            let rom_addr = exhirom_map_rom_address(address, rom_len);
            CartridgeAddress::Rom(rom_addr)
        }
        (0x80..=0xBF, 0x6000..=0x7FFF) if sram_len != 0 => {
            // SRAM, if mapped (note bank range is different from regular HiROM)
            let sram_bank = bank & 0x1F;
            let sram_addr = ((sram_bank << 13) | (offset & 0x1FFF)) & (sram_len - 1);
            CartridgeAddress::Sram(sram_addr)
        }
        _ => CartridgeAddress::None,
    }
}

fn exhirom_map_rom_address(address: u32, rom_len: u32) -> u32 {
    // ExHiROM mapping ignores A22, and A23 is inverted and shifted right 1
    let rom_addr = (address & 0x3FFFFF) | (((address >> 1) & 0x400000) ^ 0x400000);
    // TODO better handle unusual ROM size
    rom_addr % rom_len
}
