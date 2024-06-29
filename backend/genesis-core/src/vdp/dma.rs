use crate::memory::{Memory, PhysicalMedium};
use crate::vdp;
use crate::vdp::registers::DmaMode;
use crate::vdp::timing::{DmaInitArgs, LineType};
use crate::vdp::{ActiveDma, DataPortLocation, Vdp};
use jgenesis_common::num::U16Ext;

impl Vdp {
    // TODO maybe do this piecemeal instead of all at once
    pub(super) fn run_dma<Medium: PhysicalMedium>(
        &mut self,
        memory: &mut Memory<Medium>,
        active_dma: ActiveDma,
    ) {
        match active_dma {
            ActiveDma::MemoryToVram => {
                self.init_dma_timing(DmaMode::MemoryToVram, self.state.data_port_location);

                let dma_length = self.registers.dma_length();
                let mut source_addr = self.registers.dma_source_address;

                log::trace!(
                    "Copying {} words from {source_addr:06X} to {:04X}, write location={:?}; data_addr_increment={:04X}",
                    dma_length,
                    self.state.data_address,
                    self.state.data_port_location,
                    self.registers.data_port_auto_increment
                );

                for _ in 0..dma_length {
                    let word = memory.read_word_for_dma(source_addr);
                    match self.state.data_port_location {
                        DataPortLocation::Vram => {
                            self.write_vram_word(self.state.data_address, word);
                        }
                        DataPortLocation::Cram => {
                            self.write_cram_word(self.state.data_address, word);
                        }
                        DataPortLocation::Vsram => {
                            let addr = self.state.data_address as usize;
                            // TODO fix VSRAM wrapping
                            self.vsram[addr % vdp::VSRAM_LEN] = word.msb();
                            self.vsram[(addr + 1) % vdp::VSRAM_LEN] = word.lsb();
                        }
                    }

                    source_addr = source_addr.wrapping_add(2);
                    self.increment_data_address();
                }

                self.registers.dma_source_address = source_addr;
            }
            ActiveDma::VramFill(fill_data) => {
                self.init_dma_timing(DmaMode::VramFill, DataPortLocation::Vram);

                log::trace!(
                    "Running VRAM fill with addr {:04X} and length {}",
                    self.state.data_address,
                    self.registers.dma_length()
                );

                // VRAM fill is weird; it first performs a normal VRAM write with the given fill
                // data, then it repeatedly writes the MSB only to (address ^ 1)

                self.write_vram_word(self.state.data_address, fill_data);
                self.increment_data_address();

                let [msb, _] = fill_data.to_be_bytes();
                for _ in 0..self.registers.dma_length() {
                    let vram_addr = (self.state.data_address ^ 0x1) & 0xFFFF;
                    self.vram[vram_addr as usize] = msb;
                    self.maybe_update_sprite_cache(vram_addr as u16, msb);

                    self.increment_data_address();
                }
            }
            ActiveDma::VramCopy => {
                self.init_dma_timing(DmaMode::VramCopy, DataPortLocation::Vram);

                log::trace!(
                    "Running VRAM copy with source addr {:04X}, dest addr {:04X}, and length {}",
                    self.registers.dma_source_address,
                    self.state.data_address,
                    self.registers.dma_length()
                );

                // VRAM copy DMA treats the source address as A15-A0 instead of A23-A1
                let mut source_addr = (self.registers.dma_source_address >> 1) as u16;
                for _ in 0..self.registers.dma_length() {
                    let dest_addr = self.state.data_address & 0xFFFF;
                    let byte = self.vram[source_addr as usize];
                    self.vram[dest_addr as usize] = byte;
                    self.maybe_update_sprite_cache(dest_addr as u16, byte);

                    source_addr = source_addr.wrapping_add(1);
                    self.increment_data_address();
                }

                self.registers.dma_source_address = u32::from(source_addr) << 1;
            }
        }

        self.state.pending_dma = None;
        self.registers.dma_length = 0;
    }

    fn init_dma_timing(&mut self, mode: DmaMode, data_port_location: DataPortLocation) {
        let line_type = LineType::from_vdp(self);
        let h_display_size = self.registers.horizontal_display_size;
        let pixel = vdp::scanline_mclk_to_pixel(self.state.scanline_mclk_cycles, h_display_size);
        let dma_length = self.registers.dma_length();
        self.dma_tracker.init(DmaInitArgs {
            mode,
            vram_size: self.registers.vram_size,
            dma_length,
            data_port_location,
            scanline: self.state.scanline,
            pixel,
            line_type,
            h_display_size,
        });
    }
}
