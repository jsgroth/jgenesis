use std::fs::File;
use std::io;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Clone)]
struct Cartridge {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_ram: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(crate) struct MapperImpl<MapperData> {
    cartridge: Cartridge,
    data: MapperData,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum CpuMapResult {
    PrgROM(u32),
    PrgRAM(u32),
    None,
}

impl CpuMapResult {
    fn read(self, cartridge: &Cartridge) -> u8 {
        match self {
            Self::PrgROM(address) => cartridge.prg_rom[address as usize],
            Self::PrgRAM(address) => cartridge.prg_ram[address as usize],
            Self::None => 0xFF,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PpuMapResult {
    ChrROM(u32),
    ChrRAM(u32),
    Vram(u16),
    None,
}

impl PpuMapResult {
    fn read(self, cartridge: &Cartridge, vram: &[u8; 2048]) -> u8 {
        match self {
            Self::ChrROM(address) => cartridge.chr_rom[address as usize],
            Self::ChrRAM(address) => cartridge.chr_ram[address as usize],
            Self::Vram(address) => vram[address as usize],
            Self::None => 0xFF,
        }
    }

    fn write(self, value: u8, cartridge: &mut Cartridge, vram: &mut [u8; 2048]) {
        match self {
            Self::ChrROM(_) | Self::None => {}
            Self::ChrRAM(address) => {
                cartridge.chr_ram[address as usize] = value;
            }
            Self::Vram(address) => {
                vram[address as usize] = value;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NametableMirroring {
    Horizontal,
    Vertical,
}

impl NametableMirroring {
    fn map_to_vram(self, address: u16) -> u16 {
        assert!((0x2000..=0x3EFF).contains(&address));

        let relative_addr = address & 0x0FFF;

        match self {
            Self::Horizontal => ((relative_addr & 0x0800) >> 1) | (relative_addr & 0x03FF),
            Self::Vertical => relative_addr & 0x07FF,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Nrom {
    nametable_mirroring: NametableMirroring,
}

impl MapperImpl<Nrom> {
    fn read_cpu_address(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: 0x{address:04X}"),
            0x4020..=0x7FFF => 0xFF,
            0x8000..=0xFFFF => {
                let prg_rom_addr =
                    usize::from(address & 0x7FFF) & (self.cartridge.prg_rom.len() - 1);
                self.cartridge.prg_rom[prg_rom_addr]
            }
        }
    }

    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        match address {
            0x0000..=0x1FFF => PpuMapResult::ChrROM(address.into()),
            0x2000..=0x3EFF => {
                PpuMapResult::Vram(self.data.nametable_mirroring.map_to_vram(address))
            }
            _ => panic!("invalid PPU map address: 0x{address:04X}"),
        }
    }

    fn read_ppu_address(&self, address: u16, vram: &[u8; 2048]) -> u8 {
        self.map_ppu_address(address).read(&self.cartridge, vram)
    }

    fn write_ppu_address(&mut self, address: u16, value: u8, vram: &mut [u8; 2048]) {
        self.map_ppu_address(address)
            .write(value, &mut self.cartridge, vram);
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChrType {
    ROM,
    RAM,
}

impl ChrType {
    fn to_map_result(self, address: u32) -> PpuMapResult {
        match self {
            Self::ROM => PpuMapResult::ChrROM(address),
            Self::RAM => PpuMapResult::ChrRAM(address),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Uxrom {
    prg_bank: u8,
    chr_type: ChrType,
    nametable_mirroring: NametableMirroring,
}

impl MapperImpl<Uxrom> {
    fn read_cpu_address(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: 0x{address:04X}"),
            0x4020..=0x7FFF => 0xFF,
            0x8000..=0xBFFF => {
                let bank_address = (u32::from(self.data.prg_bank) << 14)
                    & (self.cartridge.prg_rom.len() as u32 - 1);
                let prg_rom_addr = bank_address + u32::from(address & 0x3FFF);
                self.cartridge.prg_rom[prg_rom_addr as usize]
            }
            0xC000..=0xFFFF => {
                let last_bank_address = self.cartridge.prg_rom.len() - (1 << 14);
                let prg_rom_addr = last_bank_address + usize::from(address & 0x3FFF);
                self.cartridge.prg_rom[prg_rom_addr]
            }
        }
    }

    fn write_cpu_address(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: 0x{address:04X}"),
            0x4020..=0x7FFF => {}
            0x8000..=0xFFFF => {
                self.data.prg_bank = value;
            }
        }
    }

    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        match address {
            0x0000..=0x1FFF => self.data.chr_type.to_map_result(address.into()),
            0x2000..=0x3EFF => {
                PpuMapResult::Vram(self.data.nametable_mirroring.map_to_vram(address))
            }
            _ => panic!("invalid PPU map address: 0x{address:04X}"),
        }
    }

    fn read_ppu_address(&self, address: u16, vram: &[u8; 2048]) -> u8 {
        self.map_ppu_address(address).read(&self.cartridge, vram)
    }

    fn write_ppu_address(&mut self, address: u16, value: u8, vram: &mut [u8; 2048]) {
        self.map_ppu_address(address)
            .write(value, &mut self.cartridge, vram);
    }
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
pub(crate) struct Mmc1 {
    chr_type: ChrType,
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
}

impl MapperImpl<Mmc1> {
    fn map_cpu_address(&self, address: u16) -> CpuMapResult {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: 0x{address:04X}"),
            0x4020..=0x5FFF => CpuMapResult::None,
            0x6000..=0x7FFF => {
                if !self.cartridge.prg_ram.is_empty() {
                    CpuMapResult::PrgRAM(u32::from(address & 0x1FFF))
                } else {
                    CpuMapResult::None
                }
            }
            0x8000..=0xFFFF => match self.data.prg_banking_mode {
                Mmc1PrgBankingMode::Switch32Kb => {
                    let bank_address = (u32::from(self.data.prg_bank & 0x0E) << 15)
                        & (self.cartridge.prg_rom.len() as u32 - 1);
                    CpuMapResult::PrgROM(bank_address + u32::from(address & 0x7FFF))
                }
                Mmc1PrgBankingMode::Switch16KbFirstBankFixed => match address {
                    0x8000..=0xBFFF => CpuMapResult::PrgROM(u32::from(address) & 0x3FFF),
                    0xC000..=0xFFFF => {
                        let bank_address = (u32::from(self.data.prg_bank) << 14)
                            & (self.cartridge.prg_rom.len() as u32 - 1);
                        CpuMapResult::PrgROM(bank_address + (u32::from(address) & 0x3FFF))
                    }
                    _ => panic!("match arm should be unreachable"),
                },
                Mmc1PrgBankingMode::Switch16KbLastBankFixed => match address {
                    0x8000..=0xBFFF => {
                        let bank_address = (u32::from(self.data.prg_bank) << 14)
                            & (self.cartridge.prg_rom.len() as u32 - 1);
                        CpuMapResult::PrgROM(bank_address + (u32::from(address) & 0x3FFF))
                    }
                    0xC000..=0xFFFF => {
                        let last_bank_address = self.cartridge.prg_rom.len() as u32 - 0x4000;
                        CpuMapResult::PrgROM(last_bank_address + (u32::from(address) & 0x3FFF))
                    }
                    _ => panic!("match arm should be unreachable"),
                },
            },
        }
    }

    fn read_cpu_address(&self, address: u16) -> u8 {
        self.map_cpu_address(address).read(&self.cartridge)
    }

    fn write_cpu_address(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: 0x{address:04X}"),
            0x4020..=0x5FFF => {}
            0x6000..=0x7FFF => {
                if !self.cartridge.prg_ram.is_empty() {
                    let prg_ram_len = self.cartridge.prg_ram.len();
                    self.cartridge.prg_ram[(address as usize) & (prg_ram_len - 1)] = value;
                }
            }
            0x8000..=0xFFFF => {
                if value & 0x80 != 0 {
                    self.data.shift_register = 0;
                    self.data.shift_register_len = 0;
                    self.data.prg_banking_mode = Mmc1PrgBankingMode::Switch16KbLastBankFixed;
                    return;
                }

                if self.data.written_last_cycle {
                    return;
                }

                self.data.written_this_cycle = true;

                self.data.shift_register = (self.data.shift_register >> 1) | ((value & 0x01) << 4);
                self.data.shift_register_len += 1;

                if self.data.shift_register_len == 5 {
                    let shift_register = self.data.shift_register;

                    self.data.shift_register = 0;
                    self.data.shift_register_len = 0;

                    match address {
                        0x8000..=0x9FFF => {
                            self.data.nametable_mirroring = match shift_register & 0x03 {
                                0x00 => Mmc1Mirroring::OneScreenLowerBank,
                                0x01 => Mmc1Mirroring::OneScreenUpperBank,
                                0x02 => Mmc1Mirroring::Vertical,
                                0x03 => Mmc1Mirroring::Horizontal,
                                _ => panic!("{shift_register} & 0x03 was not 0x00/0x01/0x02/0x03",),
                            };

                            self.data.prg_banking_mode = match shift_register & 0x0C {
                                0x00 | 0x04 => Mmc1PrgBankingMode::Switch32Kb,
                                0x08 => Mmc1PrgBankingMode::Switch16KbFirstBankFixed,
                                0x0C => Mmc1PrgBankingMode::Switch16KbLastBankFixed,
                                _ => panic!("{shift_register} & 0x0C was not 0x00/0x04/0x08/0x0C"),
                            };

                            self.data.chr_banking_mode = if shift_register & 0x10 != 0 {
                                Mmc1ChrBankingMode::Two4KbBanks
                            } else {
                                Mmc1ChrBankingMode::Single8KbBank
                            };
                        }
                        0xA000..=0xBFFF => {
                            self.data.chr_bank_0 = shift_register;
                        }
                        0xC000..=0xDFFF => {
                            self.data.chr_bank_1 = shift_register;
                        }
                        0xE000..=0xFFFF => {
                            self.data.prg_bank = shift_register;
                        }
                        _ => panic!("match arm should be unreachable"),
                    }
                }
            }
        }
    }

    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        match address {
            0x0000..=0x1FFF => match self.data.chr_banking_mode {
                Mmc1ChrBankingMode::Two4KbBanks => {
                    let (bank_number, relative_addr) = if address < 0x1000 {
                        (self.data.chr_bank_0, address)
                    } else {
                        (self.data.chr_bank_1, address - 0x1000)
                    };
                    let bank_address = u32::from(bank_number) * 4 * 1024;
                    let chr_address = bank_address + u32::from(relative_addr);
                    self.data.chr_type.to_map_result(chr_address)
                }
                Mmc1ChrBankingMode::Single8KbBank => {
                    let chr_bank = self.data.chr_bank_0 & 0x1E;
                    let bank_address = u32::from(chr_bank) * 4 * 1024;
                    let chr_address = bank_address + u32::from(address);
                    self.data.chr_type.to_map_result(chr_address)
                }
            },
            0x2000..=0x3EFF => match self.data.nametable_mirroring {
                Mmc1Mirroring::OneScreenLowerBank => PpuMapResult::Vram(address & 0x03FF),
                Mmc1Mirroring::OneScreenUpperBank => {
                    PpuMapResult::Vram(0x0400 + (address & 0x03FF))
                }
                Mmc1Mirroring::Vertical => {
                    PpuMapResult::Vram(NametableMirroring::Vertical.map_to_vram(address))
                }
                Mmc1Mirroring::Horizontal => {
                    PpuMapResult::Vram(NametableMirroring::Horizontal.map_to_vram(address))
                }
            },
            _ => panic!("invalid PPU map address: 0x{address:04X}"),
        }
    }

    fn read_ppu_address(&self, address: u16, vram: &[u8; 2048]) -> u8 {
        self.map_ppu_address(address).read(&self.cartridge, vram)
    }

    fn write_ppu_address(&mut self, address: u16, value: u8, vram: &mut [u8; 2048]) {
        self.map_ppu_address(address)
            .write(value, &mut self.cartridge, vram);
    }

    fn tick(&mut self) {
        self.data.written_last_cycle = self.data.written_this_cycle;
        self.data.written_this_cycle = false;
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Mapper {
    Nrom(MapperImpl<Nrom>),
    Uxrom(MapperImpl<Uxrom>),
    Mmc1(MapperImpl<Mmc1>),
}

impl Mapper {
    pub(crate) fn read_cpu_address(&self, address: u16) -> u8 {
        match self {
            Self::Nrom(nrom) => nrom.read_cpu_address(address),
            Self::Uxrom(uxrom) => uxrom.read_cpu_address(address),
            Self::Mmc1(mmc1) => mmc1.read_cpu_address(address),
        }
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        match self {
            Self::Nrom(nrom) => {}
            Self::Uxrom(uxrom) => {
                uxrom.write_cpu_address(address, value);
            }
            Self::Mmc1(mmc1) => {
                mmc1.write_cpu_address(address, value);
            }
        }
    }

    pub(crate) fn read_ppu_address(&self, address: u16, vram: &[u8; 2048]) -> u8 {
        match self {
            Self::Nrom(nrom) => nrom.read_ppu_address(address, vram),
            Self::Uxrom(uxrom) => uxrom.read_ppu_address(address, vram),
            Self::Mmc1(mmc1) => mmc1.read_ppu_address(address, vram),
        }
    }

    pub(crate) fn write_ppu_address(&mut self, address: u16, value: u8, vram: &mut [u8; 2048]) {
        match self {
            Self::Nrom(nrom) => {
                nrom.write_ppu_address(address, value, vram);
            }
            Self::Uxrom(uxrom) => {
                uxrom.write_ppu_address(address, value, vram);
            }
            Self::Mmc1(mmc1) => {
                mmc1.write_ppu_address(address, value, vram);
            }
        }
    }

    pub(crate) fn tick(&mut self) {
        match self {
            Self::Nrom(..) | Self::Uxrom(..) => {}
            Self::Mmc1(mmc1) => {
                mmc1.tick();
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn new_mmc1(prg_rom: Vec<u8>) -> Self {
        Self::Mmc1(MapperImpl {
            cartridge: Cartridge {
                prg_rom,
                prg_ram: vec![0; 8192],
                chr_rom: vec![0; 8192],
                chr_ram: Vec::new(),
            },
            data: Mmc1 {
                chr_type: ChrType::ROM,
                shift_register: 0,
                shift_register_len: 0,
                written_this_cycle: false,
                written_last_cycle: false,
                nametable_mirroring: Mmc1Mirroring::Vertical,
                prg_banking_mode: Mmc1PrgBankingMode::Switch16KbLastBankFixed,
                chr_banking_mode: Mmc1ChrBankingMode::Single8KbBank,
                chr_bank_0: 0,
                chr_bank_1: 0,
                prg_bank: 0,
            },
        })
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

pub(crate) fn from_file<P>(path: P) -> Result<Mapper, CartridgeFileError>
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

fn from_ines_file(mut file: File) -> Result<Mapper, CartridgeFileError> {
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

    let chr_type = if chr_rom_size == 0 {
        ChrType::RAM
    } else {
        ChrType::ROM
    };

    let chr_ram_size = match chr_type {
        ChrType::RAM => 8192,
        ChrType::ROM => 0,
    };

    log::info!("PRG ROM size: {prg_rom_size}");
    log::info!("CHR ROM size: {chr_rom_size}");
    log::info!("Mapper number: {mapper_number}");

    let cartridge = Cartridge {
        prg_rom,
        // TODO actually figure out size
        prg_ram: vec![0; 8192],
        chr_rom,
        // TODO actually figure out size
        chr_ram: vec![0; chr_ram_size],
    };

    let nametable_mirroring = if header[6] & 0x01 != 0 {
        NametableMirroring::Vertical
    } else {
        NametableMirroring::Horizontal
    };

    let mapper = match mapper_number {
        0 => {
            log::info!("NROM mapper using mirroring {nametable_mirroring:?}");
            Mapper::Nrom(MapperImpl {
                cartridge,
                data: Nrom {
                    nametable_mirroring,
                },
            })
        }
        1 => Mapper::Mmc1(MapperImpl {
            cartridge,
            data: Mmc1 {
                chr_type,
                shift_register: 0,
                shift_register_len: 0,
                written_this_cycle: false,
                written_last_cycle: false,
                nametable_mirroring: Mmc1Mirroring::Horizontal,
                prg_banking_mode: Mmc1PrgBankingMode::Switch16KbLastBankFixed,
                chr_banking_mode: Mmc1ChrBankingMode::Single8KbBank,
                chr_bank_0: 0,
                chr_bank_1: 0,
                prg_bank: 0,
            },
        }),
        2 => Mapper::Uxrom(MapperImpl {
            cartridge,
            data: Uxrom {
                prg_bank: 0,
                chr_type,
                nametable_mirroring,
            },
        }),
        _ => {
            return Err(CartridgeFileError::UnsupportedMapper { mapper_number });
        }
    };

    Ok(mapper)
}
