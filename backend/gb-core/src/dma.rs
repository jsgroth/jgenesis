use crate::cartridge::Cartridge;
use crate::memory::Memory;
use crate::ppu::{Ppu, PpuMode};
use bincode::{Decode, Encode};
use jgenesis_common::num::{GetBit, U16Ext};

const OAM_DMA_M_CYCLES: u8 = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum VramDmaState {
    Idle,
    GpDmaActive,
    HDmaActive { hblank_bytes_remaining: u8 },
    HDmaPending,
}

impl Default for VramDmaState {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct DmaUnit {
    oam_dma_source_address: u16,
    oam_dma_m_cycles_remaining: u8,
    oam_dma_pending: bool,
    oam_dma_running: bool,
    vram_dma_source_address: u16,
    vram_dma_destination_address: u16,
    vram_dma_length: u16,
    vram_dma_state: VramDmaState,
    last_ppu_mode: PpuMode,
}

impl DmaUnit {
    pub fn new() -> Self {
        Self {
            oam_dma_source_address: 0xFF00,
            oam_dma_m_cycles_remaining: 0,
            oam_dma_pending: false,
            oam_dma_running: false,
            vram_dma_source_address: 0,
            vram_dma_destination_address: 0,
            vram_dma_length: 0,
            vram_dma_state: VramDmaState::default(),
            last_ppu_mode: PpuMode::VBlank,
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

    pub fn oam_dma_tick_m_cycle(&mut self, cartridge: &Cartridge, memory: &Memory, ppu: &mut Ppu) {
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
        ppu.write_oam_for_dma(source_addr, byte);

        log::trace!(
            "Copied {byte:02X} to OAM from {source_addr:04X} to $FE{:02X}",
            source_addr.lsb()
        );

        self.oam_dma_source_address += 1;
        self.oam_dma_m_cycles_remaining -= 1;
    }

    pub fn vram_dma_copy_byte(&mut self, cartridge: &Cartridge, memory: &Memory, ppu: &mut Ppu) {
        let last_ppu_mode = self.last_ppu_mode;
        self.last_ppu_mode = ppu.mode();

        match self.vram_dma_state {
            VramDmaState::Idle => return,
            VramDmaState::HDmaPending => {
                if last_ppu_mode != PpuMode::HBlank && ppu.mode() == PpuMode::HBlank {
                    // Just reached HBlank; halt the CPU and copy 16 bytes
                    self.vram_dma_state = VramDmaState::HDmaActive { hblank_bytes_remaining: 16 };
                } else {
                    // HDMA is still running but the PPU is not in HBlank yet; wait
                    return;
                }
            }
            _ => {}
        }

        let source_addr = self.vram_dma_source_address;
        let byte = match self.vram_dma_source_address {
            0x0000..=0x7FFF => cartridge.read_rom(source_addr),
            0xA000..=0xBFFF => cartridge.read_ram(source_addr),
            0xC000..=0xDFFF => memory.read_main_ram(source_addr),
            // VRAM, OAM, I/O registers, and HRAM are not accessible from VRAM DMA
            0x8000..=0x9FFF | 0xE000..=0xFFFF => 0xFF,
        };

        ppu.write_vram(self.vram_dma_destination_address, byte);

        self.vram_dma_source_address = self.vram_dma_source_address.wrapping_add(1);
        self.vram_dma_destination_address = self.vram_dma_destination_address.wrapping_add(1);
        self.vram_dma_length -= 1;

        if let VramDmaState::HDmaActive { hblank_bytes_remaining } = &mut self.vram_dma_state {
            *hblank_bytes_remaining -= 1;
            if *hblank_bytes_remaining == 0 {
                // Finished copying the 16-byte chunk; wait until next HBlank period
                self.vram_dma_state = VramDmaState::HDmaPending;
            }
        }

        // End VRAM DMA when length reaches 0 or destination address overflows out of VRAM
        if self.vram_dma_length == 0 || self.vram_dma_destination_address > 0x9FFF {
            log::trace!("VRAM DMA complete");

            self.vram_dma_state = VramDmaState::Idle;
        }
    }

    pub fn oam_dma_in_progress(&self) -> bool {
        self.oam_dma_running
    }

    pub fn vram_dma_active(&self) -> bool {
        matches!(self.vram_dma_state, VramDmaState::GpDmaActive | VramDmaState::HDmaActive { .. })
    }

    pub fn write_hdma1(&mut self, value: u8) {
        // HDMA1: VRAM DMA source address, MSB
        self.vram_dma_source_address.set_msb(value);

        log::trace!("HDMA1 write, VRAM DMA source address MSB: {value:02X}");
    }

    pub fn write_hdma2(&mut self, value: u8) {
        // HDMA2: VRAM DMA source address, LSB
        // Ignore lowest 4 bits
        self.vram_dma_source_address.set_lsb(value & 0xF0);

        log::trace!("HDMA2 write, VRAM DMA source address LSB: {value:02X}");
    }

    pub fn write_hdma3(&mut self, value: u8) {
        // HDMA3: VRAM DMA destination address, MSB
        // Ignore highest 3 bits, hardcode to $8000 (start of VRAM)
        self.vram_dma_destination_address.set_msb(0x80 | (value & 0x1F));

        log::trace!("HDMA3 write, VRAM DMA destination address MSB: {value:02X}");
    }

    pub fn write_hdma4(&mut self, value: u8) {
        // HDMA4: VRAM DMA destination address, MSB
        // Ignore lowest 4 bits
        self.vram_dma_destination_address.set_lsb(value & 0xF0);

        log::trace!("HDMA4 write, VRAM DMA destination address LSB: {value:02X}");
    }

    pub fn read_hdma5(&self) -> u8 {
        let length_bits = ((self.vram_dma_length / 16) as u8).wrapping_sub(1) & 0x7F;
        let status_bit = u8::from(self.vram_dma_state == VramDmaState::Idle);
        length_bits | (status_bit << 7)
    }

    pub fn write_hdma5(&mut self, value: u8, ppu_mode: PpuMode) {
        // HDMA5: VRAM DMA length/mode + initiate VRAM DMA
        if self.vram_dma_state != VramDmaState::Idle {
            if !value.bit(7) {
                // Writing HDMA5 with bit 7 clear while an HDMA is in progress immediately cancels it
                self.vram_dma_state = VramDmaState::Idle;
            }

            return;
        }

        self.vram_dma_length = 16 * u16::from((value & 0x7F) + 1);

        self.vram_dma_state = if value.bit(7) {
            // HDMA
            if ppu_mode == PpuMode::HBlank {
                VramDmaState::HDmaActive { hblank_bytes_remaining: 16 }
            } else {
                VramDmaState::HDmaPending
            }
        } else {
            // GPDMA
            VramDmaState::GpDmaActive
        };

        log::trace!("HDMA5 write, VRAM DMA initiated: {value:02X}");
        log::trace!("  VRAM DMA length: {:04X}", self.vram_dma_length);
        log::trace!("  VRAM DMA state: {:?}", self.vram_dma_state);
    }
}
