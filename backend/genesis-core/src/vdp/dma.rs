use crate::memory::{Memory, PhysicalMedium};
use crate::vdp::registers::{DmaMode, HorizontalDisplaySize, VramSizeKb};
use crate::vdp::{
    ActiveDma, DataPortLocation, PendingWrite, Vdp, MCLK_CYCLES_PER_SCANLINE, VSRAM_LEN,
};
use bincode::{Decode, Encode};
use jgenesis_common::num::U16Ext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineType {
    Active,
    Blanked,
}

impl LineType {
    pub fn from_vdp(vdp: &Vdp) -> Self {
        if !vdp.registers.display_enabled || vdp.in_vblank() { Self::Blanked } else { Self::Active }
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct DmaTracker {
    // TODO avoid floating point arithmetic?
    in_progress: bool,
    mode: DmaMode,
    bytes_remaining: f64,
    data_port_read: bool,
}

impl DmaTracker {
    pub fn new() -> Self {
        Self {
            in_progress: false,
            mode: DmaMode::MemoryToVram,
            bytes_remaining: 0.0,
            data_port_read: false,
        }
    }

    pub fn init(
        &mut self,
        mode: DmaMode,
        vram_size: VramSizeKb,
        dma_length: u32,
        data_port_location: DataPortLocation,
    ) {
        self.mode = mode;
        self.bytes_remaining = f64::from(match (data_port_location, vram_size) {
            (DataPortLocation::Vram, VramSizeKb::SixtyFour) => 2 * dma_length,
            (DataPortLocation::Vram, VramSizeKb::OneTwentyEight)
            | (DataPortLocation::Cram | DataPortLocation::Vsram, _) => dma_length,
        });
        self.in_progress = true;
        self.data_port_read = false;
    }

    pub fn is_in_progress(&self) -> bool {
        self.in_progress
    }

    pub fn record_data_port_read(&mut self) {
        self.data_port_read = true;
    }

    #[inline]
    pub fn tick(
        &mut self,
        master_clock_cycles: u64,
        h_display_size: HorizontalDisplaySize,
        line_type: LineType,
    ) {
        if !self.in_progress {
            return;
        }

        let bytes_per_line: u32 = match (self.mode, h_display_size, line_type) {
            (DmaMode::MemoryToVram, HorizontalDisplaySize::ThirtyTwoCell, LineType::Active) => 16,
            (DmaMode::MemoryToVram, HorizontalDisplaySize::FortyCell, LineType::Active) => 18,
            (DmaMode::MemoryToVram, HorizontalDisplaySize::ThirtyTwoCell, LineType::Blanked) => 167,
            (DmaMode::MemoryToVram, HorizontalDisplaySize::FortyCell, LineType::Blanked) => 205,
            (DmaMode::VramFill, HorizontalDisplaySize::ThirtyTwoCell, LineType::Active) => 15,
            (DmaMode::VramFill, HorizontalDisplaySize::FortyCell, LineType::Active) => 17,
            (DmaMode::VramFill, HorizontalDisplaySize::ThirtyTwoCell, LineType::Blanked) => 166,
            (DmaMode::VramFill, HorizontalDisplaySize::FortyCell, LineType::Blanked) => 204,
            (DmaMode::VramCopy, HorizontalDisplaySize::ThirtyTwoCell, LineType::Active) => 8,
            (DmaMode::VramCopy, HorizontalDisplaySize::FortyCell, LineType::Active) => 9,
            (DmaMode::VramCopy, HorizontalDisplaySize::ThirtyTwoCell, LineType::Blanked) => 83,
            (DmaMode::VramCopy, HorizontalDisplaySize::FortyCell, LineType::Blanked) => 102,
        };
        let bytes_per_line: f64 = bytes_per_line.into();
        self.bytes_remaining -=
            bytes_per_line * master_clock_cycles as f64 / MCLK_CYCLES_PER_SCANLINE as f64;
        if self.bytes_remaining <= 0.0 {
            log::trace!("VDP DMA in mode {:?} complete", self.mode);
            self.in_progress = false;
        }
    }

    pub fn should_halt_cpu(&self, pending_writes: &[PendingWrite]) -> bool {
        // Memory-to-VRAM DMA always halts the CPU; VRAM fill & VRAM copy only halt the CPU if it
        // accesses the VDP data port during the DMA
        self.in_progress
            && (self.mode == DmaMode::MemoryToVram
                || self.data_port_read
                || pending_writes.iter().any(|write| matches!(write, PendingWrite::Data(..))))
    }
}

impl Vdp {
    // TODO maybe do this piecemeal instead of all at once
    pub(super) fn run_dma<Medium: PhysicalMedium>(
        &mut self,
        memory: &mut Memory<Medium>,
        active_dma: ActiveDma,
    ) {
        match active_dma {
            ActiveDma::MemoryToVram => {
                let dma_length = self.registers.dma_length();
                self.dma_tracker.init(
                    DmaMode::MemoryToVram,
                    self.registers.vram_size,
                    dma_length,
                    self.state.data_port_location,
                );

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
                            self.vsram[addr % VSRAM_LEN] = word.msb();
                            self.vsram[(addr + 1) % VSRAM_LEN] = word.lsb();
                        }
                    }

                    source_addr = source_addr.wrapping_add(2);
                    self.increment_data_address();
                }

                self.registers.dma_source_address = source_addr;
            }
            ActiveDma::VramFill(fill_data) => {
                self.dma_tracker.init(
                    DmaMode::VramFill,
                    self.registers.vram_size,
                    self.registers.dma_length(),
                    DataPortLocation::Vram,
                );

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
                self.dma_tracker.init(
                    DmaMode::VramCopy,
                    self.registers.vram_size,
                    self.registers.dma_length(),
                    DataPortLocation::Vram,
                );

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
}
