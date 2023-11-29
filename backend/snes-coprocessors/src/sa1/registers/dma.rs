use crate::sa1::mmc::Sa1Mmc;
use crate::sa1::registers::{
    CharacterConversionColorBits, DmaDestinationDevice, DmaSourceDevice, DmaState, Sa1Registers,
};
use crate::sa1::Iram;
use jgenesis_common::num::GetBit;

impl Sa1Registers {
    pub fn next_ccdma_byte(&mut self, iram: &mut Iram, bwram: &[u8]) -> u8 {
        let DmaState::CharacterConversion1Active {
            buffer_idx,
            dma_bytes_remaining,
            next_tile_number,
        } = self.dma_state
        else {
            log::error!(
                "next_ccdma_byte() called while CCDMA type 1 is not active; current DMA state is {:?}",
                self.dma_state
            );
            return 0;
        };

        let tile_size = self.ccdma_color_depth.tile_size();
        let base_iram_addr = (self.dma_destination_address & self.ccdma_dest_addr_mask())
            + u32::from(buffer_idx) * tile_size;
        let iram_addr = base_iram_addr + tile_size - u32::from(dma_bytes_remaining);
        let next_byte = iram[iram_addr as usize];

        if dma_bytes_remaining == 1 {
            self.progress_ccdma_type_1(buffer_idx, next_tile_number, iram, bwram);
        } else {
            self.dma_state = DmaState::CharacterConversion1Active {
                buffer_idx,
                dma_bytes_remaining: dma_bytes_remaining - 1,
                next_tile_number,
            };
        }

        next_byte
    }

    pub fn progress_normal_dma(
        &mut self,
        mmc: &Sa1Mmc,
        rom: &[u8],
        iram: &mut Iram,
        bwram: &mut [u8],
    ) {
        let source_byte = match self.dma_source {
            DmaSourceDevice::Rom => {
                let Some(rom_addr) = mmc.map_rom_address(self.dma_source_address) else {
                    self.dma_state = DmaState::Idle;
                    return;
                };
                rom.get(rom_addr as usize).copied().unwrap_or(0)
            }
            DmaSourceDevice::Iram => {
                let iram_addr = self.dma_source_address & 0x7FF;
                iram[iram_addr as usize]
            }
            DmaSourceDevice::Bwram => {
                let bwram_addr = (self.dma_source_address as usize) & (bwram.len() - 1);
                bwram[bwram_addr]
            }
        };

        match self.dma_destination {
            DmaDestinationDevice::Iram => {
                let iram_addr = self.dma_destination_address & 0x7FF;
                iram[iram_addr as usize] = source_byte;
            }
            DmaDestinationDevice::Bwram => {
                let bwram_addr = (self.dma_destination_address as usize) & (bwram.len() - 1);
                bwram[bwram_addr] = source_byte;
            }
        }

        self.dma_source_address = (self.dma_source_address + 1) & 0xFFFFFF;
        self.dma_destination_address = (self.dma_destination_address + 1) & 0xFFFFFF;
        self.dma_terminal_counter = self.dma_terminal_counter.wrapping_sub(1);

        self.dma_state = match (self.dma_terminal_counter, self.dma_source, self.dma_destination) {
            (0, _, _) => DmaState::Idle,
            (_, DmaSourceDevice::Rom, DmaDestinationDevice::Iram) => DmaState::NormalCopying,
            _ => DmaState::NormalWaitCycle,
        };

        if self.dma_state == DmaState::Idle {
            log::trace!("SA-1 DMA complete");
            self.sa1_dma_irq = true;
        }
    }

    pub fn character_conversion_2(
        &mut self,
        base_idx: usize,
        buffer_idx: u8,
        rows_copied: u8,
        iram: &mut Iram,
    ) {
        let buffer_idx: u32 = buffer_idx.into();
        let rows_copied: u32 = rows_copied.into();

        let color_depth = self.ccdma_color_depth;
        let tile_size = color_depth.tile_size();

        let base_iram_addr =
            (self.dma_destination_address & 0x7FF) + buffer_idx * tile_size + 2 * rows_copied;

        // Convert 8 packed pixels to 1 row in an SNES tile
        for pixel_idx in 0..8 {
            let pixel = self.bitmap_pixels[base_idx + pixel_idx];
            let shift = 7 - pixel_idx;

            for plane in (0..color_depth.bitplanes()).step_by(2) {
                let iram_addr = base_iram_addr + 8 * plane;

                iram[iram_addr as usize] = (iram[iram_addr as usize] & !(1 << shift))
                    | (u8::from(pixel.bit(plane as u8)) << shift);
                iram[(iram_addr + 1) as usize] = (iram[(iram_addr + 1) as usize] & !(1 << shift))
                    | (u8::from(pixel.bit((plane + 1) as u8)) << shift);
            }
        }

        let rows_copied = ((rows_copied + 1) & 0x07) as u8;
        let buffer_idx = (if rows_copied == 0 { 1 - buffer_idx } else { buffer_idx }) as u8;
        self.dma_state = DmaState::CharacterConversion2 { buffer_idx, rows_copied }
    }

    pub fn start_ccdma_type_1(&mut self, iram: &mut Iram, bwram: &[u8]) {
        let source_addr =
            self.dma_source_address & (bwram.len() as u32 - 1) & self.ccdma_source_addr_mask();
        let dest_addr = self.dma_destination_address & self.ccdma_dest_addr_mask();

        character_conversion_1_copy_tile(
            source_addr,
            dest_addr,
            0,
            self.ccdma_color_depth,
            self.virtual_vram_width_tiles.into(),
            iram,
            bwram,
        );

        self.dma_state = DmaState::CharacterConversion1Active {
            buffer_idx: 0,
            dma_bytes_remaining: self.ccdma_color_depth.tile_size() as u8,
            next_tile_number: 1,
        };

        log::trace!("CCDMA type 1 initial copy completed; generating IRQ");
        self.character_conversion_irq = true;
    }

    fn progress_ccdma_type_1(
        &mut self,
        buffer_idx: u8,
        next_tile_number: u16,
        iram: &mut Iram,
        bwram: &[u8],
    ) {
        // Invert buffer index
        let buffer_idx: u32 = (1 - buffer_idx).into();

        let source_addr =
            self.dma_source_address & (bwram.len() as u32 - 1) & self.ccdma_source_addr_mask();
        let dest_addr = (self.dma_destination_address & self.ccdma_dest_addr_mask())
            + buffer_idx * self.ccdma_color_depth.tile_size();
        character_conversion_1_copy_tile(
            source_addr,
            dest_addr,
            next_tile_number.into(),
            self.ccdma_color_depth,
            self.virtual_vram_width_tiles.into(),
            iram,
            bwram,
        );

        self.dma_state = DmaState::CharacterConversion1Active {
            buffer_idx: buffer_idx as u8,
            dma_bytes_remaining: self.ccdma_color_depth.tile_size() as u8,
            next_tile_number: next_tile_number + 1,
        };
    }

    fn ccdma_source_addr_mask(&self) -> u32 {
        let shift = self.ccdma_color_depth.bitplanes().trailing_zeros()
            + self.virtual_vram_width_tiles.trailing_zeros()
            + 3;
        !((1 << shift) - 1)
    }

    fn ccdma_dest_addr_mask(&self) -> u32 {
        match self.ccdma_color_depth {
            CharacterConversionColorBits::Two => !((1 << 5) - 1),
            CharacterConversionColorBits::Four => !((1 << 6) - 1),
            CharacterConversionColorBits::Eight => !((1 << 7) - 1),
        }
    }
}

fn character_conversion_1_copy_tile(
    source_addr: u32,
    dest_addr: u32,
    tile_number: u32,
    color_depth: CharacterConversionColorBits,
    vram_width: u32,
    iram: &mut Iram,
    bwram: &[u8],
) {
    let bitplanes = color_depth.bitplanes();
    let pixels_per_byte = 8 / bitplanes;
    let pixel_mask = if bitplanes == 8 { 0xFF } else { (1 << bitplanes) - 1 };

    let source_tile_addr = source_addr
        + (tile_number & (vram_width - 1)) * bitplanes
        + tile_number / vram_width * color_depth.tile_size() * vram_width;

    for line in 0..8 {
        let source_line_addr = source_tile_addr + line * bitplanes * vram_width;
        let dest_line_addr = dest_addr + 2 * line;

        for pixel in 0..8 {
            let pixel_addr = source_line_addr + pixel / pixels_per_byte;
            let pixel_shift = match color_depth {
                CharacterConversionColorBits::Two => 2 * (3 - (pixel % 4)),
                CharacterConversionColorBits::Four => 4 * (1 - (pixel % 2)),
                CharacterConversionColorBits::Eight => 0,
            };

            let bm_pixel = (bwram[pixel_addr as usize] >> pixel_shift) & pixel_mask;
            let bm_shift = 7 - pixel;

            for plane in (0..bitplanes).step_by(2) {
                let iram_addr = dest_line_addr + 8 * plane;

                iram[iram_addr as usize] = (iram[iram_addr as usize] & !(1 << bm_shift))
                    | (u8::from(bm_pixel.bit(plane as u8)) << bm_shift);
                iram[(iram_addr + 1) as usize] = (iram[(iram_addr + 1) as usize]
                    & !(1 << bm_shift))
                    | (u8::from(bm_pixel.bit((plane + 1) as u8)) << bm_shift);
            }
        }
    }
}
