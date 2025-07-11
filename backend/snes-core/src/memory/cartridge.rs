//! SNES cartridge loading and mapping code

use crate::api::{CoprocessorRoms, SnesLoadError, SnesLoadResult};
use bincode::{Decode, Encode};
use crc::Crc;
use jgenesis_common::frontend::{PartialClone, SaveWriter, TimingMode};
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use snes_coprocessors::cx4::Cx4;
use snes_coprocessors::obc1::Obc1;
use snes_coprocessors::sa1::Sa1;
use snes_coprocessors::sdd1::Sdd1;
use snes_coprocessors::spc7110::Spc7110;
use snes_coprocessors::srtc::SRtc;
use snes_coprocessors::st018::St018;
use snes_coprocessors::superfx::SuperFx;
use snes_coprocessors::upd77c25::{Upd77c25, Upd77c25Variant};
use snes_coprocessors::{superfx, upd77c25};
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::mem;
use std::num::NonZeroU64;
use std::ops::Deref;

const CRC: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);

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
    Obc1,
    Sa1,
    Sdd1,
    Spc7110,
    SuperFx,
}

impl Display for CartridgeType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LoRom => write!(f, "LoROM"),
            Self::HiRom => write!(f, "HiROM"),
            Self::ExHiRom => write!(f, "ExHiROM"),
            Self::Cx4 => write!(f, "CX4"),
            Self::Obc1 => write!(f, "OBC1"),
            Self::Sa1 => write!(f, "SA-1"),
            Self::Sdd1 => write!(f, "S-DD1"),
            Self::Spc7110 => write!(f, "SPC7110"),
            Self::SuperFx => write!(f, "Super FX"),
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

const HEADER_MAP_OFFSET: usize = 0x15;

const LOROM_RESET_VECTOR: usize = 0x7FFC;
const HIROM_RESET_VECTOR: usize = 0xFFFC;

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub enum Cartridge {
    LoRom {
        #[partial_clone(default)]
        rom: Rom,
        sram: Box<[u8]>,
    },
    HiRom {
        #[partial_clone(default)]
        rom: Rom,
        sram: Box<[u8]>,
    },
    ExHiRom {
        #[partial_clone(default)]
        rom: Rom,
        sram: Box<[u8]>,
        srtc: Option<SRtc>,
    },
    Cx4(#[partial_clone(partial)] Cx4),
    DspLoRom {
        #[partial_clone(default)]
        rom: Rom,
        sram: Box<[u8]>,
        upd77c25: Upd77c25,
    },
    DspHiRom {
        #[partial_clone(default)]
        rom: Rom,
        sram: Box<[u8]>,
        upd77c25: Upd77c25,
    },
    Obc1(#[partial_clone(partial)] Obc1),
    Sa1(#[partial_clone(partial)] Sa1),
    Sdd1(#[partial_clone(partial)] Sdd1),
    Spc7110(#[partial_clone(partial)] Spc7110),
    SuperFx(#[partial_clone(partial)] SuperFx),
    St01x {
        #[partial_clone(default)]
        rom: Rom,
        upd77c25: Upd77c25,
    },
    St018 {
        #[partial_clone(default)]
        rom: Rom,
        sram: Box<[u8]>,
        st018: St018,
    },
}

impl Cartridge {
    pub fn create<S: SaveWriter>(
        mut rom: Vec<u8>,
        initial_sram: Option<Vec<u8>>,
        coprocessor_roms: &CoprocessorRoms,
        forced_timing_mode: Option<TimingMode>,
        gsu_overclock_factor: NonZeroU64,
        save_writer: &mut S,
    ) -> SnesLoadResult<Self> {
        // Older SNES ROM images have an extra 512-byte header; check for that and strip it off
        if rom.len() & 0x7FFF == 0x0200 {
            rom = rom[0x200..].to_vec();
        }

        let cartridge_type = guess_cartridge_type(&rom).unwrap_or_else(|| {
            log::error!("Unable to confidently determine ROM type; defaulting to LoROM");
            CartridgeType::LoRom
        });

        if cartridge_type != CartridgeType::Spc7110 {
            jgenesis_common::rom::mirror_to_next_power_of_two(&mut rom);
            log::debug!("ROM length after mirroring is {} bytes", rom.len());

            debug_assert_eq!(rom.len().count_ones(), 1);
        }

        let rom = rom.into_boxed_slice();

        let rom_header_addr = match cartridge_type {
            CartridgeType::LoRom
            | CartridgeType::Cx4
            | CartridgeType::Obc1
            | CartridgeType::Sa1
            | CartridgeType::Sdd1
            | CartridgeType::SuperFx => LOROM_HEADER_ADDR,
            CartridgeType::HiRom | CartridgeType::Spc7110 => HIROM_HEADER_ADDR,
            CartridgeType::ExHiRom => EXHIROM_HEADER_ADDR,
        };

        // Determine NTSC/PAL
        let region_byte = rom[rom_header_addr + 0x19];
        let timing_mode = forced_timing_mode.unwrap_or_else(|| region_to_timing_mode(region_byte));

        // $FFD8 contains SRAM size as a kilobytes power of 2
        let sram_header_byte = rom[rom_header_addr | 0x0018];

        // Check if cartridge is ST010/ST011; these don't report RAM size in the header (always 4KB)
        let chipset_byte = rom[rom_header_addr + 0x16];
        let is_st01x = chipset_byte == 0xF6
            && rom[rom_header_addr + 0x14] == 0x00
            && rom[rom_header_addr - 1] == 0x01;

        let sram_len = if is_st01x {
            upd77c25::ST01X_RAM_LEN_BYTES
        } else if cartridge_type == CartridgeType::SuperFx {
            superfx::guess_ram_len(&rom)
        } else if sram_header_byte == 0 || sram_header_byte > 21 {
            0
        } else {
            1 << (10 + sram_header_byte)
        };

        let sram = match initial_sram {
            Some(sram) if sram.len() == sram_len => sram.into_boxed_slice(),
            _ => vec![0xFF; sram_len].into_boxed_slice(),
        };

        log::info!("Using mapper {cartridge_type} with SRAM size {sram_len}");

        if is_st01x {
            let st01x_variant = guess_st01x_variant(&rom);

            log::info!("Detected {st01x_variant} coprocessor");

            let st01x_rom_fn = match st01x_variant {
                St01xVariant::St010 => {
                    coprocessor_roms.st010.as_ref().ok_or(SnesLoadError::MissingSt010Rom)?
                }
                St01xVariant::St011 => {
                    coprocessor_roms.st011.as_ref().ok_or(SnesLoadError::MissingSt011Rom)?
                }
            };

            let st01x_rom = st01x_rom_fn()
                .map_err(|(source, path)| SnesLoadError::CoprocessorRomLoad { source, path })?;
            let upd77c25 = Upd77c25::new(&st01x_rom, st01x_variant.into(), &sram, timing_mode);

            return Ok(Self::St01x { rom: Rom(rom), upd77c25 });
        }

        let is_st018 = chipset_byte == 0xF5 && rom[rom_header_addr - 1] == 0x02;
        if is_st018 {
            log::info!("Detected ST018 coprocessor");

            let st018_rom_fn =
                coprocessor_roms.st018.as_ref().ok_or(SnesLoadError::MissingSt018Rom)?;
            let st018_rom = st018_rom_fn()
                .map_err(|(source, path)| SnesLoadError::CoprocessorRomLoad { source, path })?;
            let st018 = St018::new(&st018_rom).map_err(SnesLoadError::St018RomLoad)?;

            return Ok(Self::St018 { rom: Rom(rom), sram, st018 });
        }

        let rom_checksum = CRC.checksum(&rom);
        log::info!("ROM CRC32: {rom_checksum:08X}");

        // Check for DSP-1/2/3/4 coprocessor (identified by chipset $03-$05, can be LoROM or HiROM)
        let force_dsp1 = should_force_dsp1(rom_checksum);
        if force_dsp1
            || ((0x03..0x06).contains(&chipset_byte)
                && matches!(cartridge_type, CartridgeType::LoRom | CartridgeType::HiRom))
        {
            let dsp_variant = guess_dsp_variant(rom_checksum);

            log::info!("Detected DSP coprocessor of type {dsp_variant}");

            let dsp_rom_fn = match dsp_variant {
                DspVariant::Dsp1 => {
                    coprocessor_roms.dsp1.as_ref().ok_or(SnesLoadError::MissingDsp1Rom)?
                }
                DspVariant::Dsp2 => {
                    coprocessor_roms.dsp2.as_ref().ok_or(SnesLoadError::MissingDsp2Rom)?
                }
                DspVariant::Dsp3 => {
                    coprocessor_roms.dsp3.as_ref().ok_or(SnesLoadError::MissingDsp3Rom)?
                }
                DspVariant::Dsp4 => {
                    coprocessor_roms.dsp4.as_ref().ok_or(SnesLoadError::MissingDsp4Rom)?
                }
            };

            let dsp_rom = dsp_rom_fn()
                .map_err(|(source, path)| SnesLoadError::CoprocessorRomLoad { source, path })?;
            let upd77c25 = Upd77c25::new(&dsp_rom, Upd77c25Variant::Dsp, &sram, timing_mode);

            return match cartridge_type {
                CartridgeType::LoRom => Ok(Self::DspLoRom { rom: Rom(rom), sram, upd77c25 }),
                CartridgeType::HiRom => Ok(Self::DspHiRom { rom: Rom(rom), sram, upd77c25 }),
                _ => unreachable!("nested match expressions"),
            };
        }

        Ok(match cartridge_type {
            CartridgeType::LoRom => Self::LoRom { rom: Rom(rom), sram },
            CartridgeType::HiRom => Self::HiRom { rom: Rom(rom), sram },
            CartridgeType::ExHiRom => new_exhirom_cartridge(rom, sram, save_writer),
            CartridgeType::Cx4 => Self::Cx4(Cx4::new(rom)),
            CartridgeType::Obc1 => Self::Obc1(Obc1::new(rom, sram)),
            CartridgeType::Sa1 => Self::Sa1(Sa1::new(rom, sram, timing_mode)),
            CartridgeType::Sdd1 => Self::Sdd1(Sdd1::new(rom, sram)),
            CartridgeType::Spc7110 => Self::Spc7110(Spc7110::new(rom, sram, save_writer)),
            CartridgeType::SuperFx => Self::SuperFx(SuperFx::new(rom, sram, gsu_overclock_factor)),
        })
    }

    pub fn read(&mut self, address: u32) -> Option<u8> {
        let bank = (address >> 16) & 0xFF;
        let offset = address & 0xFFFF;
        let (mapped_address, rom, sram) = match self {
            Self::LoRom { rom, sram } => {
                (lorom_map_address(address, rom.len() as u32, sram.len() as u32), rom, sram)
            }
            Self::DspLoRom { rom, sram, upd77c25 } => match (bank, offset) {
                (0x30..=0x3F | 0xB0..=0xBF, 0x8000..=0xBFFF) => return Some(upd77c25.read_data()),
                (0x30..=0x3F | 0xB0..=0xBF, 0xC000..=0xFFFF) => {
                    return Some(upd77c25.read_status());
                }
                _ => (lorom_map_address(address, rom.len() as u32, sram.len() as u32), rom, sram),
            },
            Self::HiRom { rom, sram } => {
                (hirom_map_address(address, rom.len() as u32, sram.len() as u32), rom, sram)
            }
            Self::DspHiRom { rom, sram, upd77c25 } => match (bank, offset) {
                (0x00..=0x0F | 0x80..=0x8F, 0x6000..=0x6FFF) => return Some(upd77c25.read_data()),
                (0x00..=0x0F | 0x80..=0x8F, 0x7000..=0x7FFF) => {
                    return Some(upd77c25.read_status());
                }
                _ => (hirom_map_address(address, rom.len() as u32, sram.len() as u32), rom, sram),
            },
            Self::ExHiRom { rom, sram, srtc, .. } => match (bank, offset, srtc) {
                (0x00..=0x3F | 0x80..=0xBF, 0x2800, Some(srtc)) => return Some(srtc.read()),
                _ => (exhirom_map_address(address, rom.len() as u32, sram.len() as u32), rom, sram),
            },
            Self::Cx4(cx4) => return cx4.read(address),
            Self::Obc1(obc1) => return obc1.read(address),
            Self::Sa1(sa1) => return sa1.snes_read(address),
            Self::Sdd1(sdd1) => return sdd1.read(address),
            Self::Spc7110(spc7110) => return spc7110.read(address),
            Self::SuperFx(sfx) => return sfx.read(address),
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
            Self::St018 { rom, sram, st018 } => {
                return match (bank, offset) {
                    (0x00..=0x3F | 0x80..=0xBF, 0x3800..=0x3804) => st018.snes_read(address),
                    (0x68..=0x6F, 0x0000..=0x7FFF) => {
                        let sram_addr = (offset as usize) & (sram.len() - 1);
                        Some(sram[sram_addr])
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
            Self::LoRom { rom, sram } => {
                if let CartridgeAddress::Sram(sram_addr) =
                    lorom_map_address(address, rom.len() as u32, sram.len() as u32)
                {
                    sram[sram_addr as usize] = value;
                }
            }
            Self::DspLoRom { rom, sram, upd77c25 } => match (bank, offset) {
                (0x30..=0x3F | 0xB0..=0xBF, 0x8000..=0xBFFF) => {
                    upd77c25.write_data(value);
                }
                _ => {
                    if let CartridgeAddress::Sram(sram_addr) =
                        lorom_map_address(address, rom.len() as u32, sram.len() as u32)
                    {
                        sram[sram_addr as usize] = value;
                    }
                }
            },
            Self::HiRom { rom, sram, .. } => {
                if let CartridgeAddress::Sram(sram_addr) =
                    hirom_map_address(address, rom.len() as u32, sram.len() as u32)
                {
                    sram[sram_addr as usize] = value;
                }
            }
            Self::DspHiRom { rom, sram, upd77c25 } => match (bank, offset) {
                (0x00..=0x0F | 0x80..=0x8F, 0x6000..=0x6FFF) => {
                    upd77c25.write_data(value);
                }
                _ => {
                    if let CartridgeAddress::Sram(sram_addr) =
                        hirom_map_address(address, rom.len() as u32, sram.len() as u32)
                    {
                        sram[sram_addr as usize] = value;
                    }
                }
            },
            Self::ExHiRom { rom, sram, srtc, .. } => match (bank, offset, srtc) {
                (0x00..=0x3F | 0x80..=0xBF, 0x2801, Some(srtc)) => srtc.write(value),
                _ => {
                    if let CartridgeAddress::Sram(sram_addr) =
                        exhirom_map_address(address, rom.len() as u32, sram.len() as u32)
                    {
                        sram[sram_addr as usize] = value;
                    }
                }
            },
            Self::Cx4(cx4) => {
                cx4.write(address, value);
            }
            Self::Obc1(obc1) => {
                obc1.write(address, value);
            }
            Self::Sa1(sa1) => {
                sa1.snes_write(address, value);
            }
            Self::Sdd1(sdd1) => {
                sdd1.write(address, value);
            }
            Self::Spc7110(spc7110) => {
                spc7110.write(address, value);
            }
            Self::SuperFx(sfx) => {
                sfx.write(address, value);
            }
            Self::St01x { upd77c25, .. } => match (bank, offset) {
                (0x60..=0x67, 0x0000) => upd77c25.write_data(value),
                (0x68..=0x6F, 0x0000..=0x0FFF) => {
                    let sram_addr = ((bank & 0x7) << 12) | (offset & 0xFFF);
                    upd77c25.write_ram(sram_addr, value);
                }
                _ => {}
            },
            Self::St018 { sram, st018, .. } => match (bank, offset) {
                (0x00..=0x3F | 0x80..=0xBF, 0x3800..=0x3804) => st018.snes_write(address, value),
                (0x68..=0x6F, 0x0000..=0x7FFF) => {
                    let sram_addr = (address as usize) & (sram.len() - 1);
                    sram[sram_addr] = value;
                }
                _ => {}
            },
        }
    }

    pub fn irq(&self) -> bool {
        match self {
            Self::Sa1(sa1) => sa1.snes_irq(),
            Self::SuperFx(sfx) => sfx.irq(),
            _ => false,
        }
    }

    pub fn take_rom(&mut self) -> Vec<u8> {
        match self {
            Self::LoRom { rom, .. }
            | Self::HiRom { rom, .. }
            | Self::ExHiRom { rom, .. }
            | Self::DspLoRom { rom, .. }
            | Self::DspHiRom { rom, .. }
            | Self::St01x { rom, .. }
            | Self::St018 { rom, .. } => mem::take(&mut rom.0).into_vec(),
            Self::Cx4(cx4) => cx4.take_rom(),
            Self::Obc1(obc1) => obc1.take_rom(),
            Self::Sa1(sa1) => sa1.take_rom(),
            Self::Sdd1(sdd1) => sdd1.take_rom(),
            Self::Spc7110(spc7110) => spc7110.take_rom(),
            Self::SuperFx(sfx) => sfx.take_rom(),
        }
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        let other_rom = other.take_rom();

        match self {
            Self::LoRom { rom, .. }
            | Self::HiRom { rom, .. }
            | Self::ExHiRom { rom, .. }
            | Self::DspLoRom { rom, .. }
            | Self::DspHiRom { rom, .. }
            | Self::St01x { rom, .. }
            | Self::St018 { rom, .. } => {
                *rom = Rom(other_rom.into_boxed_slice());
            }
            Self::Cx4(cx4) => {
                cx4.set_rom(other_rom);
            }
            Self::Obc1(obc1) => {
                obc1.set_rom(other_rom);
            }
            Self::Sa1(sa1) => {
                sa1.set_rom(other_rom);
            }
            Self::Sdd1(sdd1) => {
                sdd1.set_rom(other_rom);
            }
            Self::Spc7110(spc7110) => {
                spc7110.set_rom(other_rom);
            }
            Self::SuperFx(sfx) => {
                sfx.set_rom(other_rom);
            }
        }
    }

    pub fn has_battery(&self) -> bool {
        match self {
            Self::Cx4(..) => false,
            Self::ExHiRom { .. }
            | Self::Obc1(..)
            | Self::Spc7110(..)
            | Self::St01x { .. }
            | Self::St018 { .. } => true,
            Self::LoRom { sram, .. }
            | Self::HiRom { sram, .. }
            | Self::DspLoRom { sram, .. }
            | Self::DspHiRom { sram, .. } => !sram.is_empty(),
            Self::Sa1(sa1) => sa1.has_battery(),
            Self::Sdd1(sdd1) => sdd1.has_battery(),
            Self::SuperFx(sfx) => sfx.has_battery(),
        }
    }

    pub fn sram(&self) -> Option<&[u8]> {
        match self {
            Self::LoRom { sram, .. }
            | Self::HiRom { sram, .. }
            | Self::ExHiRom { sram, .. }
            | Self::DspLoRom { sram, .. }
            | Self::DspHiRom { sram, .. }
                if !sram.is_empty() =>
            {
                Some(sram)
            }
            Self::LoRom { .. }
            | Self::HiRom { .. }
            | Self::ExHiRom { .. }
            | Self::DspLoRom { .. }
            | Self::DspHiRom { .. }
            | Self::Cx4 { .. } => None,
            Self::Obc1(obc1) => Some(obc1.sram()),
            Self::Sa1(sa1) => sa1.sram(),
            Self::Sdd1(sdd1) => sdd1.sram(),
            Self::Spc7110(spc7110) => Some(spc7110.sram()),
            Self::SuperFx(sfx) => Some(sfx.sram()),
            Self::St01x { upd77c25, .. } => Some(upd77c25.sram()),
            Self::St018 { sram, .. } => Some(sram),
        }
    }

    pub fn write_auxiliary_save_files<S: SaveWriter>(
        &self,
        save_writer: &mut S,
    ) -> Result<(), S::Err> {
        match self {
            Self::ExHiRom { srtc: Some(srtc), .. } => {
                save_writer.persist_serialized("rtc", srtc)?;
            }
            Self::Spc7110(spc7110) => {
                if let Some(rtc) = spc7110.rtc() {
                    save_writer.persist_serialized("rtc", rtc)?;
                }
            }
            _ => {}
        }

        Ok(())
    }

    pub fn tick(&mut self, master_cycles_elapsed: u64) {
        match self {
            Self::DspLoRom { upd77c25, .. }
            | Self::DspHiRom { upd77c25, .. }
            | Self::St01x { upd77c25, .. } => {
                upd77c25.tick(master_cycles_elapsed);
            }
            Self::Sa1(sa1) => {
                sa1.tick(master_cycles_elapsed);
            }
            Self::SuperFx(sfx) => {
                sfx.tick(master_cycles_elapsed);
            }
            Self::St018 { st018, .. } => {
                st018.tick(master_cycles_elapsed);
            }
            _ => {}
        }
    }

    pub fn reset(&mut self) {
        match self {
            Self::ExHiRom { srtc: Some(srtc), .. } => {
                srtc.reset_state();
            }
            Self::DspLoRom { upd77c25, .. }
            | Self::DspHiRom { upd77c25, .. }
            | Self::St01x { upd77c25, .. } => {
                upd77c25.reset();
            }
            Self::Sa1(sa1) => {
                sa1.reset();
            }
            Self::Sdd1(sdd1) => {
                sdd1.notify_dma_end();
            }
            Self::SuperFx(sfx) => {
                sfx.reset();
            }
            _ => {}
        }
    }

    pub fn notify_dma_start(&mut self, channel: u8, source_address: u32) {
        match self {
            Self::Sa1(sa1) => sa1.notify_dma_start(source_address),
            Self::Sdd1(sdd1) => sdd1.notify_dma_start(channel, source_address),
            _ => {}
        }
    }

    pub fn notify_dma_end(&mut self) {
        match self {
            Self::Sa1(sa1) => sa1.notify_dma_end(),
            Self::Sdd1(sdd1) => sdd1.notify_dma_end(),
            _ => {}
        }
    }

    pub fn update_gsu_overclock_factor(&mut self, overclock_factor: NonZeroU64) {
        if let Self::SuperFx(sfx) = self {
            sfx.update_gsu_overclock_factor(overclock_factor);
        }
    }
}

fn new_exhirom_cartridge<S: SaveWriter>(
    rom: Box<[u8]>,
    initial_sram: Box<[u8]>,
    save_writer: &mut S,
) -> Cartridge {
    // Chipset byte of $55 indicates S-RTC present (only used by Daikaijuu Monogatari II)
    let has_srtc = rom[EXHIROM_HEADER_ADDR + 0x16] == 0x55;
    let srtc = has_srtc.then(|| {
        let mut srtc = save_writer.load_serialized("rtc").ok().unwrap_or_else(SRtc::new);
        srtc.reset_state();
        srtc
    });

    log::info!("ExHiROM cartridge has S-RTC: {}", srtc.is_some());

    Cartridge::ExHiRom { rom: Rom(rom), sram: initial_sram, srtc }
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

    if rom.len() < 0x10000 {
        // Any ROM less than 64KB must be LoROM; HiROM <64KB wouldn't have anywhere to store
        // the 65816 interrupt vectors
        if let Some(coprocessor) = check_for_lorom_coprocessor(rom) {
            return Some(coprocessor);
        }

        return Some(CartridgeType::LoRom);
    }

    if rom.len() >= 0x410000 {
        // $25 = ExHiROM, $35 = ExHiROM + FastROM
        // A ROM >4MB with $25/$35 in the header is almost certainly ExHiROM; only 2 (?) non-ExHiROM
        // games are larger than 4MB
        let exhirom_map_byte = rom[EXHIROM_HEADER_ADDR + HEADER_MAP_OFFSET];
        if exhirom_map_byte == 0x25 || exhirom_map_byte == 0x35 {
            return Some(CartridgeType::ExHiRom);
        }
    }

    let mut lorom_points = 0;
    let mut hirom_points = 0;

    let lorom_map_byte = rom[LOROM_HEADER_ADDR + HEADER_MAP_OFFSET];
    if lorom_map_byte == 0x20 || lorom_map_byte == 0x30 {
        // $20 == LoROM, $30 == LoROM + FastROM
        lorom_points += 1;
    }

    let hirom_map_byte = rom[HIROM_HEADER_ADDR + HEADER_MAP_OFFSET];
    if hirom_map_byte == 0x21 || hirom_map_byte == 0x31 {
        // $21 == HiROM, $31 == HiROM + FastROM
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

    // Check for any coprocessors that put the header in the LoROM header location
    if lorom_points >= hirom_points {
        if let Some(coprocessor) = check_for_lorom_coprocessor(rom) {
            return Some(coprocessor);
        }
    }

    // Check for SPC7110
    // Identified by chipset == $F5/$F9 and sub-chipset == $00 in HiROM header area
    let hirom_chipset_byte = rom[HIROM_HEADER_ADDR + 0x16];
    if hirom_points >= lorom_points
        && rom[HIROM_HEADER_ADDR + 0x1A] == 0x33
        && (hirom_chipset_byte == 0xF5 || hirom_chipset_byte == 0xF9)
        && rom[HIROM_HEADER_ADDR - 1] == 0x00
    {
        return Some(CartridgeType::Spc7110);
    }

    match lorom_points.cmp(&hirom_points) {
        Ordering::Less => Some(CartridgeType::HiRom),
        Ordering::Greater => Some(CartridgeType::LoRom),
        Ordering::Equal => None,
    }
}

fn check_for_lorom_coprocessor(rom: &[u8]) -> Option<CartridgeType> {
    let lorom_map_byte = rom[LOROM_HEADER_ADDR + HEADER_MAP_OFFSET];

    // Check for CX4
    // Identified by chipset == $Fx and subtype == $10
    if rom[LOROM_HEADER_ADDR + 0x1A] == 0x33
        && rom[LOROM_HEADER_ADDR + 0x16] & 0xF0 == 0xF0
        && rom[LOROM_HEADER_ADDR - 1] == 0x10
    {
        return Some(CartridgeType::Cx4);
    }

    // Check for S-DD1
    // Identified by map == $22/$32 and chipset == $4x in the LoROM header area
    if (lorom_map_byte == 0x22 || lorom_map_byte == 0x32)
        && (0x43..0x46).contains(&rom[LOROM_HEADER_ADDR + 0x16])
    {
        return Some(CartridgeType::Sdd1);
    }

    // Check for SA-1
    // Identified by map == $23 and chipset == $3x in the LoROM header area
    if lorom_map_byte == 0x23 && (0x33..0x36).contains(&rom[LOROM_HEADER_ADDR + 0x16]) {
        return Some(CartridgeType::Sa1);
    }

    // Check for Super FX
    // Identified by map == $20 and chipset $13-$1A in the LoROM header area
    if lorom_map_byte == 0x20 && (0x13..0x1B).contains(&rom[LOROM_HEADER_ADDR + 0x16]) {
        return Some(CartridgeType::SuperFx);
    }

    // Check for OBC1
    // Identified by chipset $25 in the LoROM header area
    if rom[LOROM_HEADER_ADDR + 0x16] == 0x25 {
        return Some(CartridgeType::Obc1);
    }

    None
}

fn should_force_dsp1(checksum: u32) -> bool {
    // DSP1 Tech Demo (World) (Tech Demo)
    checksum == 0xD18A00CD
}

fn guess_dsp_variant(checksum: u32) -> DspVariant {
    match checksum {
        // Dungeon Master (U/J/J Rev 1/E)
        0x0DFD9CEB | 0x7A1BA194 | 0xAA79FA33 | 0x89A67ADF => DspVariant::Dsp2,
        // SD Gundam GX (J)
        0x4DC3D903 => DspVariant::Dsp3,
        // Top Gear 3000 (U/E) / The Planet's Champ TG 3000 (J)
        0xA20BE998 | 0x493FDB13 | 0xB9B9DF06 => DspVariant::Dsp4,
        _ => DspVariant::Dsp1,
    }
}

fn guess_st01x_variant(rom: &[u8]) -> St01xVariant {
    let checksum = CRC.checksum(rom);

    // Hayazashi Nidan Morita Shougi (J)
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
        (0x00..=0x3F | 0x80..=0xBF | 0x70..=0x7D | 0xF0..=0xFF, 0x8000..=0xFFFF)
        | (0x40..=0x6F | 0xC0..=0xEF, _) => {
            // ROM; typically at $8000-$FFFF and sometimes mirrored into $0000-$7FFF
            let rom_addr = lorom_map_rom_address(address, rom_len);
            CartridgeAddress::Rom(rom_addr)
        }
        (0x70..=0x7D | 0xF0..=0xFF, 0x0000..=0x7FFF) => {
            // SRAM, if mapped
            if sram_len != 0 {
                // Ignore A15 and shift A16-19 right by one
                // Games with more than 32KB of SRAM depend on this, e.g. Dezaemon (128KB SRAM)
                let sram_addr = (address & 0x7FFF) | ((address & 0xF0000) >> 1);

                // SRAM size is always a power of 2; use that to mask address
                let sram_addr = sram_addr & (sram_len - 1);
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
    rom_addr & (rom_len - 1)
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
    let rom_addr = address & 0x3FFFFF;
    rom_addr & (rom_len - 1)
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
    rom_addr & (rom_len - 1)
}
