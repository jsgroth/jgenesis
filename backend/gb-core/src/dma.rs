use crate::cartridge::Cartridge;
use crate::memory::Memory;
use crate::ppu::Ppu;
use bincode::{Decode, Encode};
use jgenesis_common::num::U16Ext;

const OAM_DMA_M_CYCLES: u8 = 160;

#[derive(Debug, Clone, Encode, Decode)]
pub struct DmaUnit {
    oam_dma_source_address: u16,
    oam_dma_m_cycles_remaining: u8,
    oam_dma_pending: bool,
    oam_dma_running: bool,
}

impl DmaUnit {
    pub fn new() -> Self {
        Self {
            oam_dma_source_address: 0xFF00,
            oam_dma_m_cycles_remaining: 0,
            oam_dma_pending: false,
            oam_dma_running: false,
        }
    }

    pub fn read_dma_register(&self) -> u8 {
        self.oam_dma_source_address.msb()
    }

    pub fn write_dma_register(&mut self, value: u8) {
        self.oam_dma_source_address = u16::from_le_bytes([0x00, value]);

        // Writing to DMA register initiates OAM DMA, with a 1 M-cycle delay
        self.oam_dma_pending = true;

        log::trace!("DMA written: {value:02X}");
        log::trace!("  OAM DMA source address: {:04X}", self.oam_dma_source_address);
    }

    pub fn tick_m_cycle(&mut self, cartridge: &Cartridge, memory: &Memory, ppu: &mut Ppu) {
        if self.oam_dma_pending {
            self.oam_dma_m_cycles_remaining = OAM_DMA_M_CYCLES;
            self.oam_dma_pending = false;
            return;
        }

        if self.oam_dma_m_cycles_remaining == 0 {
            self.oam_dma_running = false;
            return;
        }

        self.oam_dma_running = true;

        let source_addr = self.oam_dma_source_address;
        let byte = match source_addr {
            0x0000..=0x7FFF => cartridge.read_rom(source_addr),
            0x8000..=0x9FFF => ppu.read_vram(source_addr),
            0xA000..=0xBFFF => cartridge.read_ram(source_addr),
            0xC000..=0xDFFF => memory.read_main_ram(source_addr),
            // OAM, I/O registers, and HRAM are not readable from OAM DMA
            0xE000..=0xFFFF => 0xFF,
        };
        ppu.write_oam(source_addr, byte);

        log::trace!(
            "Copied {byte:02X} to OAM from {source_addr:04X} to $FE{:02X}",
            source_addr.lsb()
        );

        self.oam_dma_source_address += 1;
        self.oam_dma_m_cycles_remaining -= 1;
    }

    pub fn oam_dma_in_progress(&self) -> bool {
        self.oam_dma_running
    }
}
