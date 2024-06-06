//! UNROM 512 (iNES mapper 30)
//!
//! This is a homebrew mapper that extends UNROM to support up to 512KB of PRG ROM and up to 32KB of
//! bankable CHR RAM.
//!
//! There is a variant of this board that supports "four-screen VRAM" by mapping the nametable
//! addresses to CHR RAM, and there is another variant with flashable battery-backed PRG ROM.
//!
//! <https://www.nesdev.org/wiki/UNROM_512>

use crate::bus;
use crate::bus::cartridge::mappers::{BankSizeKb, NametableMirroring};
use crate::bus::cartridge::{INesHeader, MapperImpl};
use bincode::{Decode, Encode};
use crc::Crc;
use jgenesis_common::num::GetBit;

const CRC: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
const BLACK_BOX_CHALLENGE_CHECKSUM: u32 = 0xBFB003C9;

pub const MAPPER_NUMBER: u16 = 30;

// iNES headers don't always specify the CHR RAM size, so default to 32KB if header is not NES 2.0
pub const INES_CHR_RAM_LEN: u32 = 32 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum HardwiredNametableMirroring {
    Horizontal,
    Vertical,
    SingleScreen,
    FourScreen,
}

impl HardwiredNametableMirroring {
    fn from_header(header: &INesHeader) -> Self {
        // Bits 3 (4-screen VRAM) and 0 (horizontal / vertical mirroring) determine mirroring:
        //   00 = Horizontal
        //   01 = Vertical
        //   10 = Switchable single-screen
        //   11 = Four-screen
        match (header.has_four_screen_vram, header.nametable_mirroring) {
            (false, NametableMirroring::Horizontal) => HardwiredNametableMirroring::Horizontal,
            (false, NametableMirroring::Vertical) => HardwiredNametableMirroring::Vertical,
            (true, NametableMirroring::Horizontal) => HardwiredNametableMirroring::SingleScreen,
            (true, NametableMirroring::Vertical) => HardwiredNametableMirroring::FourScreen,
            _ => panic!(
                "iNES header nametable mirroring should always be vertical or horizontal, was {:?}",
                header.nametable_mirroring
            ),
        }
    }

    fn to_effective(self, single_screen_select: bool) -> Option<NametableMirroring> {
        match (self, single_screen_select) {
            (Self::Horizontal, _) => Some(NametableMirroring::Horizontal),
            (Self::Vertical, _) => Some(NametableMirroring::Vertical),
            (Self::SingleScreen, false) => Some(NametableMirroring::SingleScreenBank0),
            (Self::SingleScreen, true) => Some(NametableMirroring::SingleScreenBank1),
            (Self::FourScreen, _) => None,
        }
    }
}

// Prelude for both flash write commands
const FLASH_PRELUDE: &[(u16, u8)] =
    &[(0xC000, 0x01), (0x9555, 0xAA), (0xC000, 0x00), (0xAAAA, 0x55), (0xC000, 0x01)];

// Clear command bytes that occur after the prelude
const FLASH_CLEAR: &[(u16, u8)] =
    &[(0x9555, 0x80), (0xC000, 0x01), (0x9555, 0xAA), (0xC000, 0x00), (0xAAAA, 0x55)];

// Write command byte that occurs after the prelude
const FLASH_WRITE: (u16, u8) = (0x9555, 0xA0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum FlashWriteState {
    Prelude { idx: u8 },
    ClearOrWrite,
    Clear { idx: u8 },
    ClearBank,
    ClearLastByte { bank: u8 },
    WriteBank,
    WriteLastByte { bank: u8 },
}

impl Default for FlashWriteState {
    fn default() -> Self {
        Self::Prelude { idx: 0 }
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct FlashState {
    state: FlashWriteState,
    dirty: bool,
}

impl FlashState {
    fn new() -> Self {
        Self { state: FlashWriteState::default(), dirty: false }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Unrom512 {
    prg_bank: u8,
    chr_bank: u8,
    nametable_mirroring: HardwiredNametableMirroring,
    effective_nametable_mirroring: Option<NametableMirroring>,
    single_screen_select: bool,
    flashable: bool,
    flash_state: FlashState,
}

impl Unrom512 {
    pub fn new(original_prg_rom: &[u8], header: &INesHeader) -> Self {
        let mut nametable_mirroring = HardwiredNametableMirroring::from_header(header);

        if nametable_mirroring == HardwiredNametableMirroring::SingleScreen
            && CRC.checksum(original_prg_rom) == BLACK_BOX_CHALLENGE_CHECKSUM
        {
            // Black Box Challenge expects 4-screen mirroring, but the version of the ROM for download
            // on the developer's website specifies 1-screen in the iNES header; override that so
            // the game renders correctly
            log::info!("Overriding UNROM 512 nametable mirroring from 1-screen to 4-screen");
            nametable_mirroring = HardwiredNametableMirroring::FourScreen;
        }

        log::info!("UNROM 512 nametable mirroring: {nametable_mirroring:?}");

        Self {
            prg_bank: 0,
            chr_bank: 0,
            nametable_mirroring,
            effective_nametable_mirroring: nametable_mirroring.to_effective(false),
            single_screen_select: false,
            flashable: header.has_battery,
            flash_state: FlashState::new(),
        }
    }
}

enum PpuMapResult {
    PpuVram(u16),
    ChrRam(u32),
}

impl MapperImpl<Unrom512> {
    pub fn is_flashable(&self) -> bool {
        self.data.flashable
    }

    pub fn get_and_clear_dirty_bit(&mut self) -> bool {
        let dirty = self.data.flash_state.dirty;
        self.data.flash_state.dirty = false;
        dirty
    }

    pub fn read_cpu_address(&mut self, address: u16) -> u8 {
        match address {
            0x8000..=0xBFFF => {
                // Mappable 16KB PRG ROM bank
                let rom_addr = BankSizeKb::Sixteen.to_absolute_address(self.data.prg_bank, address);
                self.cartridge.get_prg_rom(rom_addr)
            }
            0xC000..=0xFFFF => {
                // Fixed to last 16KB of PRG ROM
                let rom_addr = BankSizeKb::Sixteen
                    .to_absolute_address_last_bank(self.cartridge.prg_rom.len() as u32, address);
                self.cartridge.get_prg_rom(rom_addr)
            }
            0x0000..=0x401F => panic!("Invalid CPU map address: {address:04X}"),
            0x4020..=0x7FFF => bus::cpu_open_bus(address),
        }
    }

    pub fn write_cpu_address(&mut self, address: u16, value: u8) {
        // This mapper only has one register located at $8000-$FFFF
        if !address.bit(15) {
            return;
        }

        let single_screen_select = value.bit(7);
        self.data.single_screen_select = single_screen_select;
        self.data.effective_nametable_mirroring =
            self.data.nametable_mirroring.to_effective(single_screen_select);

        self.data.chr_bank = (value >> 5) & 0x03;
        self.data.prg_bank = value & 0x1F;

        if self.data.flashable {
            self.try_flash_write(address, value);
        }
    }

    fn try_flash_write(&mut self, address: u16, value: u8) {
        self.data.flash_state.state = match self.data.flash_state.state {
            FlashWriteState::Prelude { idx } => {
                if (address, value) == FLASH_PRELUDE[idx as usize] {
                    let new_idx = idx + 1;
                    if new_idx == FLASH_PRELUDE.len() as u8 {
                        FlashWriteState::ClearOrWrite
                    } else {
                        FlashWriteState::Prelude { idx: new_idx }
                    }
                } else {
                    FlashWriteState::default()
                }
            }
            FlashWriteState::ClearOrWrite => {
                if (address, value) == FLASH_CLEAR[0] {
                    FlashWriteState::Clear { idx: 1 }
                } else if (address, value) == FLASH_WRITE {
                    FlashWriteState::WriteBank
                } else {
                    FlashWriteState::default()
                }
            }
            FlashWriteState::Clear { idx } => {
                if (address, value) == FLASH_CLEAR[idx as usize] {
                    let new_idx = idx + 1;
                    if new_idx == FLASH_CLEAR.len() as u8 {
                        FlashWriteState::ClearBank
                    } else {
                        FlashWriteState::Clear { idx: new_idx }
                    }
                } else {
                    FlashWriteState::default()
                }
            }
            FlashWriteState::ClearBank => {
                // Bank is always written to $C000
                if address == 0xC000 {
                    FlashWriteState::ClearLastByte { bank: value }
                } else {
                    FlashWriteState::default()
                }
            }
            FlashWriteState::ClearLastByte { bank } => {
                // Last byte written should always be $30
                if value == 0x30 {
                    // Bits 12-13 of address specify which 4KB bank to clear within the 16KB bank
                    let base_addr = (u32::from(bank) << 14)
                        | u32::from(address & 0x3000) & (self.cartridge.prg_rom.len() as u32 - 1);
                    let end_addr = base_addr + 0x1000;
                    self.cartridge.prg_rom[base_addr as usize..end_addr as usize].fill(0);
                    self.data.flash_state.dirty = true;

                    log::trace!("Cleared PRG ROM [{base_addr:X} {end_addr:X})");
                }

                FlashWriteState::default()
            }
            FlashWriteState::WriteBank => {
                // Bank is always written to $C000
                if address == 0xC000 {
                    FlashWriteState::WriteLastByte { bank: value }
                } else {
                    FlashWriteState::default()
                }
            }
            FlashWriteState::WriteLastByte { bank } => {
                // Bits 0-13 of address specify the address within the 16KB bank to write
                let address = (u32::from(bank) << 14)
                    | u32::from(address & 0x3FFF) & (self.cartridge.prg_rom.len() as u32 - 1);
                self.cartridge.prg_rom[address as usize] = value;
                self.data.flash_state.dirty = true;

                log::trace!("PRG ROM flash write: {address:X} {value:02X}");

                FlashWriteState::default()
            }
        };
    }

    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        match (address, self.data.effective_nametable_mirroring) {
            (0x0000..=0x1FFF, _) => {
                // Mappable 8KB CHR RAM bank
                let ram_addr = BankSizeKb::Eight.to_absolute_address(self.data.chr_bank, address);
                PpuMapResult::ChrRam(ram_addr)
            }
            (0x2000..=0x3FFF, None) => {
                // Uses 4-screen VRAM; fixed to last 8KB of CHR RAM
                let ram_addr = BankSizeKb::Eight
                    .to_absolute_address_last_bank(self.cartridge.chr_ram.len() as u32, address);
                PpuMapResult::ChrRam(ram_addr)
            }
            (0x2000..=0x3FFF, Some(nametable_mirroring)) => {
                // Maps to PPU VRAM using specified nametable mirroring
                let ram_addr = nametable_mirroring.map_to_vram(address);
                PpuMapResult::PpuVram(ram_addr)
            }
            (0x4000..=0xFFFF, _) => panic!("Invalid PPU map address: {address:04X}"),
        }
    }

    pub fn read_ppu_address(&mut self, address: u16, vram: &[u8; 2048]) -> u8 {
        match self.map_ppu_address(address) {
            PpuMapResult::PpuVram(vram_addr) => vram[vram_addr as usize],
            PpuMapResult::ChrRam(chr_ram_addr) => self.cartridge.get_chr_ram(chr_ram_addr),
        }
    }

    pub fn write_ppu_address(&mut self, address: u16, value: u8, vram: &mut [u8; 2048]) {
        match self.map_ppu_address(address) {
            PpuMapResult::PpuVram(vram_addr) => {
                vram[vram_addr as usize] = value;
            }
            PpuMapResult::ChrRam(chr_ram_addr) => {
                self.cartridge.set_chr_ram(chr_ram_addr, value);
            }
        }
    }
}
