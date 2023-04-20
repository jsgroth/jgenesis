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
    PrgROM(u16),
    PrgRAM(u16),
    None,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PpuMapResult {
    ChrROM(u16),
    ChrRAM(u16),
    Vram(u16),
    None,
}

#[derive(Debug, Clone)]
pub(crate) enum Mapper {
    Nrom,
}

impl Mapper {
    pub(crate) fn map_cpu_address(&self, address: u16) -> CpuMapResult {
        todo!()
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        todo!()
    }

    pub(crate) fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        todo!()
    }

    pub(crate) fn write_ppu_address(&mut self, address: u16, value: u8) {
        todo!()
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

    if mapper_number != 0 {
        return Err(CartridgeFileError::UnsupportedMapper { mapper_number });
    }

    let prg_rom_start_address = if has_trainer { 16 + 512 } else { 16 };

    let mut prg_rom = vec![0; prg_rom_size as usize];
    file.seek(SeekFrom::Start(prg_rom_start_address))?;
    file.read_exact(&mut prg_rom)?;

    let mut chr_rom = vec![0; chr_rom_size as usize];
    file.seek(SeekFrom::Start(
        prg_rom_start_address + u64::from(chr_rom_size),
    ))?;
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

    let mapper = Mapper::Nrom;

    Ok((cartridge, mapper))
}
