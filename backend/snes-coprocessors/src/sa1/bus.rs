use crate::sa1::mmc::{BwramBitmapBits, BwramMapSource, Sa1Mmc};
use crate::sa1::registers::{InterruptVectorSource, Sa1Registers};
use crate::sa1::timer::Sa1Timer;
use crate::sa1::{Iram, Sa1};
use jgenesis_common::num::U16Ext;
use wdc65816_emu::traits::BusInterface;

impl Sa1 {
    pub fn snes_read(&mut self, address: u32) -> Option<u8> {
        let bank = (address >> 16) & 0xFF;
        let offset = address & 0xFFFF;
        match (bank, offset) {
            (0x00..=0x3F | 0x80..=0xBF, 0x8000..=0xFFFF) | (0xC0..=0xFF, _) => {
                // ROM

                // Check for NMI/IRQ vector reads first
                let nmi_vector_source = self.registers.snes_nmi_vector_source;
                let irq_vector_source = self.registers.snes_irq_vector_source;
                match (bank, offset) {
                    (0x00, 0xFFEA) if nmi_vector_source == InterruptVectorSource::IoPorts => {
                        Some(self.registers.snes_nmi_vector.lsb())
                    }
                    (0x00, 0xFFEB) if nmi_vector_source == InterruptVectorSource::IoPorts => {
                        Some(self.registers.snes_nmi_vector.msb())
                    }
                    (0x00, 0xFFEE) if irq_vector_source == InterruptVectorSource::IoPorts => {
                        Some(self.registers.snes_irq_vector.lsb())
                    }
                    (0x00, 0xFFEF) if irq_vector_source == InterruptVectorSource::IoPorts => {
                        Some(self.registers.snes_irq_vector.msb())
                    }
                    _ => self
                        .mmc
                        .map_rom_address(address)
                        .and_then(|rom_addr| self.rom.get(rom_addr as usize).copied()),
                }
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x2300..=0x230F) => {
                // SA-1 internal registers
                self.registers.snes_read(address)
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x3000..=0x37FF) => {
                // I-RAM
                Some(self.iram[(address & 0x7FF) as usize])
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x6000..=0x7FFF) => {
                // BW-RAM 8KB block
                let bwram_addr = (self.mmc.snes_bwram_base_addr | (address & 0x1FFF))
                    & (self.bwram.len() as u32 - 1);
                Some(self.bwram[bwram_addr as usize])
            }
            (0x40..=0x4F, _) => {
                // BW-RAM in full
                // If character conversion DMA type 1 is in progress, ignore the address and return
                // the next CCDMA byte
                if self.registers.ccdma_transfer_in_progress {
                    Some(self.registers.next_ccdma_byte(&mut self.iram, &self.bwram))
                } else {
                    let bwram_addr = (address as usize) & (self.bwram.len() - 1);
                    Some(self.bwram[bwram_addr])
                }
            }
            _ => None,
        }
    }

    pub fn snes_write(&mut self, address: u32, value: u8) {
        let bank = (address >> 16) & 0xFF;
        let offset = address & 0xFFFF;
        match (bank, offset) {
            (0x00..=0x3F | 0x80..=0xBF, 0x2200..=0x22FF) => {
                // SA-1 internal registers
                self.registers.snes_write(address, value, &mut self.mmc);
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x3000..=0x37FF) => {
                // I-RAM
                let iram_addr = address & 0x7FF;
                let write_protect_idx = iram_addr >> 8;
                if self.registers.snes_iram_writes_enabled[write_protect_idx as usize] {
                    self.iram[iram_addr as usize] = value;
                }
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x6000..=0x7FFF) => {
                // BW-RAM 8KB block
                let bwram_addr = (self.mmc.snes_bwram_base_addr | (address & 0x1FFF))
                    & (self.bwram.len() as u32 - 1);
                if self.registers.can_write_bwram(bwram_addr) {
                    self.bwram[bwram_addr as usize] = value;
                }
            }
            (0x40..=0x4F, _) => {
                // BW-RAM in full
                let bwram_addr = (address as usize) & (self.bwram.len() - 1);
                if self.registers.can_write_bwram(bwram_addr as u32) {
                    self.bwram[bwram_addr] = value;
                }
            }
            _ => {}
        }
    }
}

pub struct Sa1Bus<'a> {
    pub rom: &'a [u8],
    pub iram: &'a mut Iram,
    pub bwram: &'a mut [u8],
    pub mmc: &'a mut Sa1Mmc,
    pub registers: &'a mut Sa1Registers,
    pub timer: &'a mut Sa1Timer,
}

impl<'a> BusInterface for Sa1Bus<'a> {
    #[inline]
    fn read(&mut self, address: u32) -> u8 {
        let bank = (address >> 16) & 0xFF;
        let offset = address & 0xFFFF;
        match (bank, offset) {
            (0x00..=0x3F | 0x80..=0xBF, 0x8000..=0xFFFF) | (0xC0..=0xFF, _) => {
                // ROM

                // Check for NMI/IRQ/RESET vector reads first
                match (bank, offset) {
                    (0x00, 0xFFEA) => self.registers.sa1_nmi_vector.lsb(),
                    (0x00, 0xFFEB) => self.registers.sa1_nmi_vector.msb(),
                    (0x00, 0xFFEE) => self.registers.sa1_irq_vector.lsb(),
                    (0x00, 0xFFEF) => self.registers.sa1_irq_vector.msb(),
                    (0x00, 0xFFFC) => self.registers.sa1_reset_vector.lsb(),
                    (0x00, 0xFFFD) => self.registers.sa1_reset_vector.msb(),
                    _ => self
                        .mmc
                        .map_rom_address(address)
                        .and_then(|rom_addr| self.rom.get(rom_addr as usize).copied())
                        .unwrap_or(0),
                }
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x2300..=0x230F) => {
                // SA-1 internal registers
                self.registers.sa1_read(address, self.timer)
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x0000..=0x07FF | 0x3000..=0x37FF) => {
                // I-RAM (mapped to both $0000-$07FF and $3000-$37FF for SA-1 CPU)
                self.iram[(address & 0x7FF) as usize]
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x6000..=0x7FFF) => {
                // BW-RAM 8KB block
                match self.mmc.sa1_bwram_source {
                    BwramMapSource::Normal => {
                        let bwram_addr = ((self.mmc.sa1_bwram_base_addr & 0x3E000)
                            | (address & 0x01FFF))
                            & (self.bwram.len() as u32 - 1);
                        self.bwram[bwram_addr as usize]
                    }
                    BwramMapSource::Bitmap => {
                        let bitmap_addr = self.mmc.sa1_bwram_base_addr | (address & 0x1FFF);
                        read_bwram_bitmap(bitmap_addr, self.mmc, self.bwram)
                    }
                }
            }
            (0x40..=0x4F, _) => {
                // BW-RAM in full
                let bwram_addr = (address as usize) & (self.bwram.len() - 1);
                self.bwram[bwram_addr]
            }
            (0x60..=0x6F, _) => {
                // BW-RAM bitmap view
                read_bwram_bitmap(address, self.mmc, self.bwram)
            }
            _ => 0,
        }
    }

    #[inline]
    fn write(&mut self, address: u32, value: u8) {
        let bank = (address >> 16) & 0xFF;
        let offset = address & 0xFFFF;
        match (bank, offset) {
            (0x00..=0x3F | 0x80..=0xBF, 0x2200..=0x22FF) => {
                // SA-1 internal registers
                self.registers.sa1_write(address, value, self.timer, self.mmc, self.rom, self.iram);
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x0000..=0x07FF | 0x3000..=0x37FF) => {
                // I-RAM (mapped to both $0000-$07FF and $3000-$37FF for SA-1 CPU)
                let iram_addr = address & 0x7FF;
                let write_protect_idx = iram_addr >> 8;
                if self.registers.sa1_iram_writes_enabled[write_protect_idx as usize] {
                    self.iram[iram_addr as usize] = value;
                }
            }
            (0x00..=0x3F | 0x80..=0xBF, 0x6000..=0x7FFF) => {
                // BW-RAM 8KB block
                match self.mmc.sa1_bwram_source {
                    BwramMapSource::Normal => {
                        let bwram_addr = ((self.mmc.sa1_bwram_base_addr & 0x3E000)
                            | (address & 0x01FFF))
                            & (self.bwram.len() as u32 - 1);
                        if self.registers.can_write_bwram(bwram_addr) {
                            self.bwram[bwram_addr as usize] = value;
                        }
                    }
                    BwramMapSource::Bitmap => {
                        let bitmap_addr = self.mmc.sa1_bwram_base_addr | (address & 0x1FFF);
                        write_bwram_bitmap(bitmap_addr, value, self.mmc, self.bwram);
                    }
                }
            }
            (0x40..=0x4F, _) => {
                // BW-RAM in full (256KB max, mirrored every 4 banks)
                let bwram_addr = (address as usize) & (self.bwram.len() - 1);
                if self.registers.can_write_bwram(bwram_addr as u32) {
                    self.bwram[bwram_addr] = value;
                }
            }
            (0x60..=0x6F, _) => {
                // BW-RAM bitmap view
                write_bwram_bitmap(address, value, self.mmc, self.bwram);
            }
            _ => {}
        }
    }

    #[inline]
    fn idle(&mut self) {}

    #[inline]
    fn nmi(&self) -> bool {
        self.registers.sa1_nmi_enabled && self.registers.sa1_nmi
    }

    #[inline]
    fn acknowledge_nmi(&mut self) {}

    #[inline]
    fn irq(&self) -> bool {
        (self.registers.sa1_irq_from_snes_enabled && self.registers.sa1_irq_from_snes)
            || (self.registers.timer_irq_enabled && self.timer.irq_pending)
            || (self.registers.dma_irq_enabled && self.registers.sa1_dma_irq)
    }

    #[inline]
    fn halt(&self) -> bool {
        self.registers.sa1_wait
    }

    #[inline]
    fn reset(&self) -> bool {
        self.registers.sa1_reset
    }
}

fn read_bwram_bitmap(address: u32, mmc: &Sa1Mmc, bwram: &[u8]) -> u8 {
    match mmc.bwram_bitmap_format {
        BwramBitmapBits::Two => {
            let address = address & 0xFFFFF;
            let bwram_addr = (address >> 2) & (bwram.len() as u32 - 1);
            let bwram_shift = 2 * (address & 0x03);

            (bwram[bwram_addr as usize] >> bwram_shift) & 0x03
        }
        BwramBitmapBits::Four => {
            let address = address & 0x7FFFF;
            let bwram_addr = (address >> 1) & (bwram.len() as u32 - 1);
            let bwram_shift = 4 * (address & 0x01);

            (bwram[bwram_addr as usize] >> bwram_shift) & 0x0F
        }
    }
}

fn write_bwram_bitmap(address: u32, value: u8, mmc: &Sa1Mmc, bwram: &mut [u8]) {
    match mmc.bwram_bitmap_format {
        BwramBitmapBits::Two => {
            let address = address & 0xFFFFF;
            let bwram_addr = (address >> 2) & (bwram.len() as u32 - 1);
            let shift = 2 * (address & 0x03);

            let existing_value = bwram[bwram_addr as usize];
            let new_value = (existing_value & !(0x03 << shift)) | ((value & 0x03) << shift);
            bwram[bwram_addr as usize] = new_value;
        }
        BwramBitmapBits::Four => {
            let address = address & 0x7FFFF;
            let bwram_addr = (address >> 1) & (bwram.len() as u32 - 1);
            let shift = 4 * (address & 0x01);

            let existing_value = bwram[bwram_addr as usize];
            let new_value = (existing_value & !(0x0F << shift)) | ((value & 0x0F) << shift);
            bwram[bwram_addr as usize] = new_value;
        }
    }
}
