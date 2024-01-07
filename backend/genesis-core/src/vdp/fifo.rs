use crate::vdp::dma::LineType;
use crate::vdp::registers::{HorizontalDisplaySize, VramSizeKb};
use crate::vdp::DataPortLocation;
use bincode::{Decode, Encode};
use std::collections::VecDeque;

const FIFO_CAPACITY: usize = 4;

// Adapted from https://gendev.spritesmind.net/forum/viewtopic.php?t=851
const H32_ACCESS_SLOT_PIXELS: &[u16] =
    &[2, 18, 34, 66, 82, 98, 130, 146, 162, 194, 210, 226, 256, 258, 286, 314];

// Adapted from https://gendev.spritesmind.net/forum/viewtopic.php?t=851
const H40_ACCESS_SLOT_PIXELS: &[u16] =
    &[2, 18, 34, 66, 82, 98, 130, 146, 162, 194, 210, 226, 258, 274, 290, 320, 322, 370];

#[derive(Debug, Clone, Encode, Decode)]
pub struct FifoTracker {
    slots_required_fifo: VecDeque<u8>,
    last_scanline: u16,
    last_slot_index: u8,
}

impl FifoTracker {
    pub fn new() -> Self {
        Self {
            slots_required_fifo: VecDeque::with_capacity(FIFO_CAPACITY + 1),
            last_scanline: 0,
            last_slot_index: u8::MAX,
        }
    }

    pub fn record_access(
        &mut self,
        line_type: LineType,
        data_port_location: DataPortLocation,
        vram_size: VramSizeKb,
    ) {
        // VRAM/CRAM/VSRAM accesses can only delay the CPU during active display
        if line_type == LineType::Blanked {
            return;
        }

        let slots_required = match (data_port_location, vram_size) {
            (DataPortLocation::Vram, VramSizeKb::SixtyFour) => 2,
            (DataPortLocation::Vram, VramSizeKb::OneTwentyEight)
            | (DataPortLocation::Cram | DataPortLocation::Vsram, _) => 1,
        };
        self.slots_required_fifo.push_back(slots_required);

        log::trace!("FIFO access recorded; current state {self:?}");
    }

    pub fn advance_to_pixel(
        &mut self,
        scanline: u16,
        pixel: u16,
        h_display_size: HorizontalDisplaySize,
        line_type: LineType,
    ) {
        let slot_pixels = match h_display_size {
            HorizontalDisplaySize::ThirtyTwoCell => H32_ACCESS_SLOT_PIXELS,
            HorizontalDisplaySize::FortyCell => H40_ACCESS_SLOT_PIXELS,
        };

        if self.slots_required_fifo.is_empty() {
            self.skip_to_pixel(scanline, pixel, slot_pixels);
            return;
        }

        if line_type == LineType::Blanked {
            // CPU never gets delayed during VBlank or when the display is off
            self.slots_required_fifo.clear();
            self.skip_to_pixel(scanline, pixel, slot_pixels);
            return;
        }

        if scanline != self.last_scanline {
            for _ in 0..(slot_pixels.len() as u8).saturating_sub(self.last_slot_index) {
                self.pop_slot();
            }

            self.last_scanline = scanline;
            self.last_slot_index = 0;
        }

        while self.last_slot_index < slot_pixels.len() as u8
            && pixel >= slot_pixels[self.last_slot_index as usize]
        {
            self.pop_slot();
            self.last_slot_index += 1;
        }
    }

    fn skip_to_pixel(&mut self, scanline: u16, pixel: u16, slot_pixels: &[u16]) {
        if scanline != self.last_scanline {
            self.last_scanline = scanline;
            self.last_slot_index = 0;
        }

        while self.last_slot_index < slot_pixels.len() as u8
            && pixel >= slot_pixels[self.last_slot_index as usize]
        {
            self.last_slot_index += 1;
        }
    }

    fn pop_slot(&mut self) {
        let Some(front) = self.slots_required_fifo.front_mut() else { return };
        if *front == 1 {
            self.slots_required_fifo.pop_front();
        } else {
            *front -= 1;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.slots_required_fifo.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.slots_required_fifo.len() >= FIFO_CAPACITY
    }

    pub fn should_halt_cpu(&self) -> bool {
        self.slots_required_fifo.len() > FIFO_CAPACITY
    }
}
