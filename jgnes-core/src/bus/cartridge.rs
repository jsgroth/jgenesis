use crate::bus;
use std::fs::File;
use std::io;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Clone)]
pub(crate) struct Cartridge {
    pub(crate) prg_rom: Vec<u8>,
    pub(crate) prg_ram: Vec<u8>,
    pub(crate) chr_rom: Vec<u8>,
    pub(crate) chr_ram: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum CpuMapResult {
    PrgROM(u32),
    PrgRAM(u32),
    None,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PpuMapResult {
    ChrROM(u32),
    ChrRAM(u32),
    Vram(u16),
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NromMirroring {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mmc1Mirroring {
    OneScreenLowerBank,
    OneScreenUpperBank,
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mmc1PrgBankingMode {
    Switch32Kb,
    Switch16KbFirstBankFixed,
    Switch16KbLastBankFixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mmc1ChrBankingMode {
    Single8KbBank,
    Two4KbBanks,
}

#[derive(Debug, Clone)]
pub(crate) enum Mapper {
    Nrom {
        prg_rom_size: u16,
        nametable_mirroring: NromMirroring,
    },
    Mmc1 {
        prg_rom_size: u32,
        prg_ram_size: u16,
        chr_rom_size: u32,
        shift_register: u8,
        shift_register_len: u8,
        written_this_cycle: bool,
        written_last_cycle: bool,
        nametable_mirroring: Mmc1Mirroring,
        prg_banking_mode: Mmc1PrgBankingMode,
        chr_banking_mode: Mmc1ChrBankingMode,
        chr_bank_0: u8,
        chr_bank_1: u8,
        prg_bank: u8,
    },
}

impl Mapper {
    pub(crate) fn map_cpu_address(&self, address: u16) -> CpuMapResult {
        match *self {
            Self::Nrom { prg_rom_size, .. } => {
                if address < 0x8000 {
                    CpuMapResult::None
                } else {
                    let relative_addr = (address - 0x8000) & (prg_rom_size - 1);
                    CpuMapResult::PrgROM(relative_addr.into())
                }
            }
            Self::Mmc1 {
                prg_rom_size,
                prg_ram_size,
                prg_banking_mode,
                prg_bank,
                ..
            } => match address {
                0x0000..=0x401F => panic!("invalid CPU map address: 0x{address:04X}"),
                0x4020..=0x5FFF => CpuMapResult::None,
                0x6000..=0x7FFF => {
                    if prg_ram_size > 0 {
                        CpuMapResult::PrgRAM(u32::from(address) - 0x6000)
                    } else {
                        CpuMapResult::None
                    }
                }
                0x8000..=0xFFFF => match prg_banking_mode {
                    Mmc1PrgBankingMode::Switch32Kb => {
                        let bank_address =
                            (u32::from(prg_bank & 0x0E) * 32 * 1024) & (prg_rom_size - 1);
                        CpuMapResult::PrgROM(bank_address + (u32::from(address) - 0x8000))
                    }
                    Mmc1PrgBankingMode::Switch16KbFirstBankFixed => match address {
                        0x0000..=0x7FFF => panic!("match arm should be unreachable"),
                        0x8000..=0xBFFF => CpuMapResult::PrgROM(u32::from(address) - 0x8000),
                        0xC000..=0xFFFF => {
                            let bank_address =
                                (u32::from(prg_bank) * 16 * 1024) & (prg_rom_size - 1);
                            CpuMapResult::PrgROM(bank_address + (u32::from(address) - 0xC000))
                        }
                    },
                    Mmc1PrgBankingMode::Switch16KbLastBankFixed => match address {
                        0x0000..=0x7FFF => panic!("match arm should be unreachable"),
                        0x8000..=0xBFFF => {
                            let bank_address =
                                (u32::from(prg_bank) * 16 * 1024) & (prg_rom_size - 1);
                            CpuMapResult::PrgROM(bank_address + (u32::from(address) - 0x8000))
                        }
                        0xC000..=0xFFFF => {
                            let bank_address = prg_rom_size - 0x4000;
                            CpuMapResult::PrgROM(bank_address + (u32::from(address) - 0xC000))
                        }
                    },
                },
            },
        }
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        match self {
            Self::Nrom { .. } => {}
            Self::Mmc1 {
                shift_register,
                shift_register_len,
                written_last_cycle,
                written_this_cycle,
                nametable_mirroring,
                prg_banking_mode,
                chr_banking_mode,
                chr_bank_0,
                chr_bank_1,
                prg_bank,
                ..
            } => match address {
                0x0000..=0x401F => panic!("invalid CPU map address: 0x{address:04X}"),
                0x4020..=0x7FFF => {}
                0x8000..=0xFFFF => {
                    if value & 0x80 != 0 {
                        *shift_register = 0;
                        *shift_register_len = 0;
                        *prg_banking_mode = Mmc1PrgBankingMode::Switch16KbLastBankFixed;
                        return;
                    }

                    if *written_last_cycle {
                        return;
                    }

                    *written_this_cycle = true;

                    *shift_register = (*shift_register << 1) | (value & 0x01);
                    *shift_register_len += 1;

                    if *shift_register_len == 5 {
                        match address {
                            0x0000..=0x7FFF => panic!("match arm should be unreachable"),
                            0x8000..=0x9FFF => {
                                *nametable_mirroring = match *shift_register & 0x03 {
                                    0x00 => Mmc1Mirroring::OneScreenLowerBank,
                                    0x01 => Mmc1Mirroring::OneScreenUpperBank,
                                    0x02 => Mmc1Mirroring::Vertical,
                                    0x03 => Mmc1Mirroring::Horizontal,
                                    _ => panic!(
                                        "{shift_register} & 0x03 was not 0x00/0x01/0x02/0x03"
                                    ),
                                };

                                *prg_banking_mode = match *shift_register & 0x0C {
                                    0x00 | 0x04 => Mmc1PrgBankingMode::Switch32Kb,
                                    0x08 => Mmc1PrgBankingMode::Switch16KbFirstBankFixed,
                                    0x0C => Mmc1PrgBankingMode::Switch16KbLastBankFixed,
                                    _ => panic!(
                                        "{shift_register} & 0x0C was not 0x00/0x04/0x08/0x0C"
                                    ),
                                };

                                *chr_banking_mode = if *shift_register & 0x01 != 0 {
                                    Mmc1ChrBankingMode::Two4KbBanks
                                } else {
                                    Mmc1ChrBankingMode::Single8KbBank
                                };
                            }
                            0xA000..=0xBFFF => {
                                *chr_bank_0 = *shift_register;
                            }
                            0xC000..=0xDFFF => {
                                *chr_bank_1 = *shift_register;
                            }
                            0xE000..=0xFFFF => {
                                *prg_bank = *shift_register;
                            }
                        }

                        *shift_register = 0;
                        *shift_register_len = 0;
                    }
                }
            },
        }
    }

    pub(crate) fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        match *self {
            Self::Nrom {
                nametable_mirroring,
                ..
            } => match address {
                address @ bus::PPU_PATTERN_TABLES_START..=bus::PPU_PATTERN_TABLES_END => {
                    PpuMapResult::ChrROM(address.into())
                }
                address @ bus::PPU_NAMETABLES_START..=bus::PPU_NAMETABLES_END => {
                    let relative_addr = (address - bus::PPU_NAMETABLES_START) & 0x0FFF;
                    let vram_addr = match nametable_mirroring {
                        NromMirroring::Horizontal => {
                            // Swap bits 10 and 11, and then discard the new bit 11
                            (relative_addr & 0x0800 >> 1) | (relative_addr & 0x03FF)
                        }
                        NromMirroring::Vertical => relative_addr & 0x07FF,
                    };
                    PpuMapResult::Vram(vram_addr)
                }
                _ => panic!("invalid PPU map address: 0x{address:04X}"),
            },
            Self::Mmc1 {
                chr_rom_size,
                nametable_mirroring,
                chr_banking_mode,
                chr_bank_0,
                chr_bank_1,
                ..
            } => {
                if chr_rom_size == 0 {
                    // ???
                    return PpuMapResult::None;
                }

                match address {
                    0x0000..=0x1FFF => match chr_banking_mode {
                        Mmc1ChrBankingMode::Two4KbBanks => {
                            let (bank_number, relative_addr) = if address < 0x1000 {
                                (chr_bank_0, address)
                            } else {
                                (chr_bank_1, address - 0x1000)
                            };
                            let bank_address = u32::from(bank_number) * 4 * 1024;
                            PpuMapResult::ChrROM(bank_address + u32::from(relative_addr))
                        }
                        Mmc1ChrBankingMode::Single8KbBank => {
                            let chr_bank = chr_bank_0 & 0x1E;
                            let bank_address = u32::from(chr_bank) * 8 * 1024;
                            PpuMapResult::ChrROM(bank_address + u32::from(address))
                        }
                    },
                    0x2000..=0x3EFF => {
                        let nametable_relative_addr = (address & 0x2FFF) - 0x2000;

                        match nametable_mirroring {
                            Mmc1Mirroring::OneScreenLowerBank => todo!(),
                            Mmc1Mirroring::OneScreenUpperBank => todo!(),
                            Mmc1Mirroring::Vertical => {
                                PpuMapResult::ChrROM(u32::from(nametable_relative_addr) & 0x07FF)
                            }
                            Mmc1Mirroring::Horizontal => {
                                // Swap bits 10 and 11, and then discard the new bit 11
                                let rom_address = (nametable_relative_addr & 0x0800 >> 1)
                                    | (nametable_relative_addr & 0x03FF);
                                PpuMapResult::ChrROM(u32::from(rom_address))
                            }
                        }
                    }
                    0x3F00..=0xFFFF => panic!("invalid PPU map address: 0x{address:04X}"),
                }
            }
        }
    }

    pub(crate) fn write_ppu_address(&mut self, address: u16, value: u8) {
        match self {
            Self::Nrom { .. } | Self::Mmc1 { .. } => {}
        }
    }

    pub(crate) fn tick(&mut self) {
        match self {
            Self::Nrom { .. } => {}
            Self::Mmc1 {
                written_last_cycle,
                written_this_cycle,
                ..
            } => {
                *written_last_cycle = *written_this_cycle;
                *written_this_cycle = false;
            }
        }
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
    UnsupportedMapper { mapper_number: u8 },
}

pub(crate) fn from_file<P>(path: P) -> Result<(Cartridge, Mapper), CartridgeFileError>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();

    log::info!("Loading cartridge from {}", path.display());

    let mut file = File::open(path)?;

    let mut buf = [0; 8];
    file.read_exact(&mut buf)?;

    // First 4 bytes should be equal to "NES<EOF>"
    if buf[..4] != [0x4E, 0x45, 0x53, 0x1A] {
        log::error!("First 4 bytes of file do not match the iNES header");
        return Err(CartridgeFileError::Format);
    }

    from_ines_file(file)
}

fn from_ines_file(mut file: File) -> Result<(Cartridge, Mapper), CartridgeFileError> {
    file.seek(SeekFrom::Start(0))?;

    let mut header = [0; 16];
    file.read_exact(&mut header)?;

    let prg_rom_size = 16 * 1024 * ((u32::from(header[9] & 0x0F) << 8) | u32::from(header[4]));
    let chr_rom_size = 8 * 1024 * ((u32::from(header[9] & 0xF0) << 4) | u32::from(header[5]));

    let has_trainer = header[6] & 0x04 != 0;

    let mapper_number = (header[7] & 0xF0) | ((header[6] & 0xF0) >> 4);

    let prg_rom_start_address = if has_trainer { 16 + 512 } else { 16 };

    let mut prg_rom = vec![0; prg_rom_size as usize];
    file.seek(SeekFrom::Start(prg_rom_start_address))?;
    file.read_exact(&mut prg_rom)?;

    let mut chr_rom = vec![0; chr_rom_size as usize];
    file.read_exact(&mut chr_rom)?;

    log::info!("PRG ROM size: {prg_rom_size}");
    log::info!("CHR ROM size: {chr_rom_size}");
    log::info!("Mapper number: {mapper_number}");

    let cartridge = Cartridge {
        prg_rom,
        prg_ram: Vec::new(),
        chr_rom,
        chr_ram: Vec::new(),
    };

    let mapper = match mapper_number {
        0 => {
            let nametable_mirroring = if header[6] & 0x01 != 0 {
                NromMirroring::Vertical
            } else {
                NromMirroring::Horizontal
            };
            Mapper::Nrom {
                prg_rom_size: prg_rom_size as u16,
                nametable_mirroring,
            }
        }
        1 => Mapper::Mmc1 {
            prg_rom_size,
            prg_ram_size: 8192,
            chr_rom_size,
            shift_register: 0,
            shift_register_len: 0,
            written_this_cycle: false,
            written_last_cycle: false,
            nametable_mirroring: Mmc1Mirroring::Vertical,
            prg_banking_mode: Mmc1PrgBankingMode::Switch16KbLastBankFixed,
            chr_banking_mode: Mmc1ChrBankingMode::Two4KbBanks,
            chr_bank_0: 0,
            chr_bank_1: 0,
            prg_bank: 0,
        },
        _ => {
            return Err(CartridgeFileError::UnsupportedMapper { mapper_number });
        }
    };

    Ok((cartridge, mapper))
}
