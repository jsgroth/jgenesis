//! Code for the unlicensed board used in Action 52 and Cheetahmen II (iNES mapper 228).

use crate::bus::cartridge::mappers::{BankSizeKb, NametableMirroring, PpuMapResult};
use crate::bus::cartridge::{HasBasicPpuMapping, MapperImpl};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum PrgMode {
    // Maps prg_bank to both $8000-$BFFF and $C000-$FFFF
    Zero,
    // Maps (prg_bank & !1) to $8000-$BFFF and (prg_bank | 1) to $C000-$FFFF
    One,
}

impl PrgMode {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::One } else { Self::Zero }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Action52 {
    selected_chip: u8,
    prg_bank: u8,
    prg_bank_mask: u8,
    prg_mode: PrgMode,
    chr_bank: u8,
    nametable_mirroring: NametableMirroring,
}

impl Action52 {
    pub fn new(prg_rom_len: u32) -> Self {
        let prg_bank_mask =
            if prg_rom_len < 512 * 1024 { ((prg_rom_len - 1) >> 14) as u8 } else { 0xFF };

        Self::new_with_mask(prg_bank_mask)
    }

    fn new_with_mask(prg_bank_mask: u8) -> Self {
        Self {
            selected_chip: 0,
            prg_bank: 0,
            prg_bank_mask,
            prg_mode: PrgMode::Zero,
            chr_bank: 0,
            nametable_mirroring: NametableMirroring::Vertical,
        }
    }
}

impl MapperImpl<Action52> {
    pub fn reset(&mut self) {
        self.data = Action52::new_with_mask(self.data.prg_bank_mask);
    }

    pub fn read_cpu_address(&self, address: u16, cpu_open_bus: u8) -> u8 {
        let prg_bank = match address {
            0x0000..=0x7FFF => return cpu_open_bus,
            0x8000..=0xBFFF => match self.data.prg_mode {
                PrgMode::Zero => self.data.prg_bank & !1,
                PrgMode::One => self.data.prg_bank,
            },
            0xC000..=0xFFFF => match self.data.prg_mode {
                PrgMode::Zero => self.data.prg_bank | 1,
                PrgMode::One => self.data.prg_bank,
            },
        };

        let full_prg_bank = (prg_bank & self.data.prg_bank_mask) | (self.data.selected_chip << 5);
        let rom_addr = BankSizeKb::Sixteen.to_absolute_address(full_prg_bank, address);
        self.cartridge.prg_rom.get(rom_addr as usize).copied().unwrap_or(cpu_open_bus)
    }

    pub fn write_cpu_address(&mut self, address: u16, value: u8) {
        if address < 0x8000 {
            // Action 52 crashes after the title screen if writes to $4020-$7FFF are allowed to do
            // anything
            return;
        }

        self.data.chr_bank = (value & 3) | ((address & 0xF) << 2) as u8;
        self.data.prg_mode = PrgMode::from_bit(address.bit(5));

        self.data.prg_bank = ((address >> 6) & 0x1F) as u8;

        // In Action 52 (1.5MB PRG ROM), chip 3 should map to the last 512KB of ROM and chip 2
        // should be open bus. Emulate this by swapping chips 2 and 3 on write.
        // Only apply these writes if there is more than 512KB of PRG ROM or else Cheetahmen II will
        // fail to boot.
        if self.cartridge.prg_rom.len() > 512 * 1024 {
            let selected_chip = ((address >> 11) & 3) as u8;
            self.data.selected_chip = selected_chip ^ (selected_chip >> 1);
        }

        self.data.nametable_mirroring = if address.bit(14) {
            NametableMirroring::Horizontal
        } else {
            NametableMirroring::Vertical
        };

        log::trace!("Mapper write: {address:04X} {value:02X}");
        log::trace!("  Mapper state: {:?}", self.data);
    }
}

impl HasBasicPpuMapping for MapperImpl<Action52> {
    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        match address {
            0x0000..=0x1FFF => PpuMapResult::ChrROM(
                BankSizeKb::Eight.to_absolute_address(self.data.chr_bank, address),
            ),
            0x2000..=0x3EFF => {
                PpuMapResult::Vram(self.data.nametable_mirroring.map_to_vram(address))
            }
            _ => panic!("Invalid PPU mapper address: {address:04X}"),
        }
    }
}
