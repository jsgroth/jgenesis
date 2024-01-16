use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum BwramMapSource {
    #[default]
    Normal,
    Bitmap,
}

impl BwramMapSource {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Bitmap } else { Self::Normal }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum BwramBitmapBits {
    Two,
    #[default]
    Four,
}

impl BwramBitmapBits {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Two } else { Self::Four }
    }
}

const DEFAULT_BANK_C_ADDR: u32 = 0x000000;
const DEFAULT_BANK_D_ADDR: u32 = 0x100000;
const DEFAULT_BANK_E_ADDR: u32 = 0x200000;
const DEFAULT_BANK_F_ADDR: u32 = 0x300000;

#[derive(Debug, Clone, Encode, Decode)]
pub struct Sa1Mmc {
    pub bank_c_base_addr: u32,
    pub bank_c_lorom_mapped: bool,
    pub lorom_bank_c_addr: u32,
    pub bank_d_base_addr: u32,
    pub bank_d_lorom_mapped: bool,
    pub lorom_bank_d_addr: u32,
    pub bank_e_base_addr: u32,
    pub bank_e_lorom_mapped: bool,
    pub lorom_bank_e_addr: u32,
    pub bank_f_base_addr: u32,
    pub bank_f_lorom_mapped: bool,
    pub lorom_bank_f_addr: u32,
    pub snes_bwram_base_addr: u32,
    pub sa1_bwram_base_addr: u32,
    pub sa1_bwram_source: BwramMapSource,
    pub bwram_bitmap_format: BwramBitmapBits,
}

impl Sa1Mmc {
    pub fn new() -> Self {
        Self {
            bank_c_base_addr: DEFAULT_BANK_C_ADDR,
            bank_c_lorom_mapped: false,
            lorom_bank_c_addr: DEFAULT_BANK_C_ADDR,
            bank_d_base_addr: DEFAULT_BANK_D_ADDR,
            bank_d_lorom_mapped: false,
            lorom_bank_d_addr: DEFAULT_BANK_D_ADDR,
            bank_e_base_addr: DEFAULT_BANK_E_ADDR,
            bank_e_lorom_mapped: false,
            lorom_bank_e_addr: DEFAULT_BANK_E_ADDR,
            bank_f_base_addr: DEFAULT_BANK_F_ADDR,
            bank_f_lorom_mapped: false,
            lorom_bank_f_addr: DEFAULT_BANK_F_ADDR,
            snes_bwram_base_addr: 0,
            sa1_bwram_base_addr: 0,
            sa1_bwram_source: BwramMapSource::default(),
            bwram_bitmap_format: BwramBitmapBits::default(),
        }
    }

    pub fn map_rom_address(&self, address: u32) -> Option<u32> {
        let bank = (address >> 16) & 0xFF;
        let offset = address & 0xFFFF;
        match (bank, offset) {
            (0x00..=0x1F, 0x8000..=0xFFFF) => {
                // LoROM bank C
                Some(lorom_map_address(self.lorom_bank_c_addr, address))
            }
            (0x20..=0x3F, 0x8000..=0xFFFF) => {
                // LoROM bank D
                Some(lorom_map_address(self.lorom_bank_d_addr, address))
            }
            (0x80..=0x9F, 0x8000..=0xFFFF) => {
                // LoROM bank E
                Some(lorom_map_address(self.lorom_bank_e_addr, address))
            }
            (0xA0..=0xBF, 0x8000..=0xFFFF) => {
                // LoROM bank F
                Some(lorom_map_address(self.lorom_bank_f_addr, address))
            }
            (0xC0..=0xCF, _) => {
                // HiROM bank C
                Some(self.bank_c_base_addr | (address & 0xFFFFF))
            }
            (0xD0..=0xDF, _) => {
                // HiROM bank D
                Some(self.bank_d_base_addr | (address & 0xFFFFF))
            }
            (0xE0..=0xEF, _) => {
                // HiROM bank E
                Some(self.bank_e_base_addr | (address & 0xFFFFF))
            }
            (0xF0..=0xFF, _) => {
                // HiROM bank F
                Some(self.bank_f_base_addr | (address & 0xFFFFF))
            }
            _ => None,
        }
    }

    pub fn write_cxb(&mut self, value: u8) {
        self.bank_c_base_addr = u32::from(value & 0x07) << 20;
        self.bank_c_lorom_mapped = value.bit(7);

        self.lorom_bank_c_addr =
            if self.bank_c_lorom_mapped { self.bank_c_base_addr } else { DEFAULT_BANK_C_ADDR };

        log::trace!("  MMC bank C base address: {:06X}", self.bank_c_base_addr);
        log::trace!("  MMC bank C LoROM mapped: {}", self.bank_c_lorom_mapped);
    }

    pub fn write_dxb(&mut self, value: u8) {
        self.bank_d_base_addr = u32::from(value & 0x07) << 20;
        self.bank_d_lorom_mapped = value.bit(7);

        self.lorom_bank_d_addr =
            if self.bank_d_lorom_mapped { self.bank_d_base_addr } else { DEFAULT_BANK_D_ADDR };

        log::trace!("  MMC bank D base address: {:06X}", self.bank_d_base_addr);
        log::trace!("  MMC bank D LoROM mapped: {}", self.bank_d_lorom_mapped);
    }

    pub fn write_exb(&mut self, value: u8) {
        self.bank_e_base_addr = u32::from(value & 0x07) << 20;
        self.bank_e_lorom_mapped = value.bit(7);

        self.lorom_bank_e_addr =
            if self.bank_e_lorom_mapped { self.bank_e_base_addr } else { DEFAULT_BANK_E_ADDR };

        log::trace!("  MMC bank E base address: {:06X}", self.bank_e_base_addr);
        log::trace!("  MMC bank E LoROM mapped: {}", self.bank_e_lorom_mapped);
    }

    pub fn write_fxb(&mut self, value: u8) {
        self.bank_f_base_addr = u32::from(value & 0x07) << 20;
        self.bank_f_lorom_mapped = value.bit(7);

        self.lorom_bank_f_addr =
            if self.bank_f_lorom_mapped { self.bank_f_base_addr } else { DEFAULT_BANK_F_ADDR };

        log::trace!("  MMC bank F base address: {:06X}", self.bank_f_base_addr);
        log::trace!("  MMC bank F LoROM mapped: {}", self.bank_f_lorom_mapped);
    }

    pub fn write_bmaps(&mut self, value: u8) {
        self.snes_bwram_base_addr = u32::from(value & 0x1F) << 13;

        log::trace!("  SNES BW-RAM base address: {:X}", self.snes_bwram_base_addr);
    }

    pub fn write_bmap(&mut self, value: u8) {
        self.sa1_bwram_base_addr = u32::from(value & 0x7F) << 13;
        self.sa1_bwram_source = BwramMapSource::from_bit(value.bit(7));

        log::trace!("  SA-1 BW-RAM base address: {:X}", self.sa1_bwram_base_addr);
        log::trace!("  SA-1 BW-RAM block source: {:?}", self.sa1_bwram_source);
    }

    pub fn write_bbf(&mut self, value: u8) {
        self.bwram_bitmap_format = BwramBitmapBits::from_bit(value.bit(7));

        log::trace!("  SA-1 BW-RAM bitmap format: {:?}", self.bwram_bitmap_format);
    }
}

fn lorom_map_address(base_addr: u32, address: u32) -> u32 {
    // A21-A23 come from MMC base address, but other than that this is standard LoROM mapping
    // Ignore A15 and shift A16-A20 right by 1
    base_addr | (address & 0x7FFF) | ((address & 0x1F0000) >> 1)
}
