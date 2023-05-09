mod mappers;

use crate::bus::cartridge::mappers::{
    Axrom, ChrType, Cnrom, ColorDreams, Mmc1, Mmc2, Mmc3, Mmc5, NametableMirroring, Nrom, Sunsoft,
    Uxrom, Vrc4, Vrc6, Vrc7,
};
use bincode::{Decode, Encode};
use jgnes_proc_macros::MatchEachVariantMacro;
use std::{io, mem};
use thiserror::Error;

#[cfg(test)]
pub(crate) use mappers::new_mmc1;

#[derive(Debug, Clone, Encode, Decode)]
struct Cartridge {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    has_ram_battery: bool,
    prg_ram_dirty_bit: bool,
    chr_rom: Vec<u8>,
    chr_ram: Vec<u8>,
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

    fn clone_without_rom(&self) -> Self {
        Self {
            prg_rom: vec![],
            prg_ram: self.prg_ram.clone(),
            has_ram_battery: self.has_ram_battery,
            prg_ram_dirty_bit: self.prg_ram_dirty_bit,
            chr_rom: vec![],
            chr_ram: self.chr_ram.clone(),
        }
    }

    fn move_unserialized_fields_from(&mut self, other: &mut Self) {
        self.prg_rom = mem::take(&mut other.prg_rom);
        self.chr_rom = mem::take(&mut other.chr_rom);
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct MapperImpl<MapperData> {
    cartridge: Cartridge,
    data: MapperData,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Encode, Decode, MatchEachVariantMacro)]
pub(crate) enum Mapper {
    Axrom(MapperImpl<Axrom>),
    Cnrom(MapperImpl<Cnrom>),
    ColorDreams(MapperImpl<ColorDreams>),
    Mmc1(MapperImpl<Mmc1>),
    // Used for both MMC2 and MMC4 because they're almost exactly the same
    Mmc2(MapperImpl<Mmc2>),
    Mmc3(MapperImpl<Mmc3>),
    Mmc5(MapperImpl<Mmc5>),
    Nrom(MapperImpl<Nrom>),
    Sunsoft(MapperImpl<Sunsoft>),
    Uxrom(MapperImpl<Uxrom>),
    Vrc4(MapperImpl<Vrc4>),
    Vrc6(MapperImpl<Vrc6>),
    Vrc7(MapperImpl<Vrc7>),
}

impl Mapper {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Axrom(..) => "AxROM",
            Self::Cnrom(..) => "CNROM",
            Self::ColorDreams(..) => "Color Dreams",
            Self::Mmc1(..) => "MMC1",
            Self::Mmc2(mmc2) => mmc2.name(),
            Self::Mmc3(mmc3) => mmc3.name(),
            Self::Mmc5(..) => "MMC5",
            Self::Nrom(..) => "NROM",
            Self::Sunsoft(..) => "Sunsoft",
            Self::Uxrom(uxrom) => uxrom.name(),
            Self::Vrc4(vrc4) => vrc4.name(),
            Self::Vrc6(..) => "VRC6",
            Self::Vrc7(..) => "VRC7",
        }
    }

    pub(crate) fn read_cpu_address(&mut self, address: u16) -> u8 {
        match_each_variant!(self, mapper => mapper.read_cpu_address(address))
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        match_each_variant!(self, mapper => mapper.write_cpu_address(address, value));
    }

    pub(crate) fn read_ppu_address(&mut self, address: u16, vram: &[u8; 2048]) -> u8 {
        match_each_variant!(self, mapper => mapper.read_ppu_address(address, vram))
    }

    pub(crate) fn write_ppu_address(&mut self, address: u16, value: u8, vram: &mut [u8; 2048]) {
        match_each_variant!(self, mapper => mapper.write_ppu_address(address, value, vram));
    }

    pub(crate) fn tick(&mut self, ppu_bus_address: u16) {
        if let Self::Mmc3(mmc3) = self {
            mmc3.tick(ppu_bus_address);
        }
    }

    pub(crate) fn tick_cpu(&mut self) {
        match self {
            Self::Mmc1(mmc1) => {
                mmc1.tick_cpu();
            }
            Self::Mmc5(mmc5) => {
                mmc5.tick_cpu();
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

    pub(crate) fn interrupt_flag(&self) -> bool {
        match self {
            Self::Mmc3(mmc3) => mmc3.interrupt_flag(),
            Self::Mmc5(mmc5) => mmc5.interrupt_flag(),
            Self::Sunsoft(sunsoft) => sunsoft.interrupt_flag(),
            Self::Vrc4(vrc4) => vrc4.interrupt_flag(),
            Self::Vrc6(vrc6) => vrc6.interrupt_flag(),
            Self::Vrc7(vrc7) => vrc7.interrupt_flag(),
            _ => false,
        }
    }

    pub(crate) fn process_ppu_ctrl_update(&mut self, value: u8) {
        if let Self::Mmc5(mmc5) = self {
            mmc5.process_ppu_ctrl_update(value);
        }
    }

    // This should be called *before* the actual memory access; MMC5 depends on this for correctly
    // mapping PPUDATA accesses to the correct CHR bank
    pub(crate) fn about_to_access_ppu_data(&mut self) {
        if let Self::Mmc5(mmc5) = self {
            mmc5.about_to_access_ppu_data();
        }
    }

    pub(crate) fn get_and_clear_ram_dirty_bit(&mut self) -> bool {
        match_each_variant!(self, mapper => {
            let dirty_bit = mapper.cartridge.prg_ram_dirty_bit;
            mapper.cartridge.prg_ram_dirty_bit = false;
            dirty_bit
        })
    }

    pub(crate) fn get_prg_ram(&self) -> &[u8] {
        match_each_variant!(self, mapper => &mapper.cartridge.prg_ram)
    }

    pub(crate) fn sample_audio(&self, mixed_apu_sample: f64) -> f64 {
        match self {
            Self::Mmc5(mmc5) => mmc5.sample_audio(mixed_apu_sample),
            Self::Vrc6(vrc6) => vrc6.sample_audio(mixed_apu_sample),
            _ => mixed_apu_sample,
        }
    }

    pub(crate) fn clone_without_rom(&self) -> Self {
        match_each_variant!(
            self,
            mapper => :variant(MapperImpl {
                cartridge: mapper.cartridge.clone_without_rom(),
                data: mapper.data.clone(),
            })
        )
    }

    pub(crate) fn move_unserialized_fields_from(&mut self, other: &mut Self) {
        let other_cartridge = match_each_variant!(other, mapper => &mut mapper.cartridge);
        match_each_variant!(self, mapper => mapper.cartridge.move_unserialized_fields_from(other_cartridge));
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

        let has_trainer = header[6] & 0x04 != 0;

        let mapper_number = u16::from((header[7] & 0xF0) | ((header[6] & 0xF0) >> 4));

        let chr_type = if chr_rom_size == 0 {
            ChrType::RAM
        } else {
            ChrType::ROM
        };

        let nametable_mirroring = if header[6] & 0x01 != 0 {
            NametableMirroring::Vertical
        } else {
            NametableMirroring::Horizontal
        };

        let has_four_screen_vram = header[6] & 0x08 != 0;

        // TODO make persistent save files work
        let has_battery = header[6] & 0x02 != 0;

        let format = if header[7] & 0x0C == 0x08 {
            FileFormat::Nes2Point0
        } else {
            FileFormat::INes
        };

        let sub_mapper_number = match format {
            FileFormat::Nes2Point0 => header[8] >> 4,
            FileFormat::INes => 0,
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

pub(crate) fn from_ines_file(
    file_bytes: &[u8],
    sav_bytes: Option<Vec<u8>>,
) -> Result<Mapper, CartridgeFileError> {
    let header = INesHeader::parse_from_file(file_bytes)?;

    // Header is 16 bytes, trainer is 512 bytes if present
    let prg_rom_start_address = if header.has_trainer { 16 + 512 } else { 16 } as usize;
    let prg_rom_end_address = prg_rom_start_address + header.prg_rom_size as usize;
    let chr_rom_end_address = prg_rom_end_address + header.chr_rom_size as usize;

    let prg_rom = Vec::from(&file_bytes[prg_rom_start_address..prg_rom_end_address]);
    let chr_rom = Vec::from(&file_bytes[prg_rom_end_address..chr_rom_end_address]);

    let prg_ram = if let Some(sav_bytes) = sav_bytes {
        if sav_bytes.len() == header.prg_ram_size as usize {
            sav_bytes
        } else {
            vec![0; header.prg_ram_size as usize]
        }
    } else {
        vec![0; header.prg_ram_size as usize]
    };

    let cartridge = Cartridge {
        prg_rom,
        prg_ram,
        has_ram_battery: header.has_battery,
        prg_ram_dirty_bit: false,
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
        4 => Mapper::Mmc3(MapperImpl {
            cartridge,
            data: Mmc3::new(
                header.chr_type,
                header.prg_rom_size,
                chr_size,
                header.sub_mapper_number,
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
        11 => Mapper::ColorDreams(MapperImpl {
            cartridge,
            data: ColorDreams::new(header.nametable_mirroring),
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
        69 => Mapper::Sunsoft(MapperImpl {
            cartridge,
            data: Sunsoft::new(header.chr_type),
        }),
        85 => Mapper::Vrc7(MapperImpl {
            cartridge,
            data: Vrc7::new(header.sub_mapper_number, header.chr_type),
        }),
        _ => {
            return Err(CartridgeFileError::UnsupportedMapper {
                mapper_number: header.mapper_number,
            });
        }
    };

    log::info!(
        "Mapper number: {} ({})",
        header.mapper_number,
        mapper.name()
    );
    log::info!("PRG ROM size: {}", header.prg_rom_size);
    log::info!("PRG RAM size: {}", header.prg_ram_size);
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
