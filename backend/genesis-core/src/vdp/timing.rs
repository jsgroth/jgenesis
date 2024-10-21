use crate::vdp::registers::{DmaMode, HorizontalDisplaySize, VramSizeKb};
use crate::vdp::{DataPortLocation, PendingWrite, Vdp};
use bincode::{Decode, Encode};
use std::collections::VecDeque;

// Adapted from https://gendev.spritesmind.net/forum/viewtopic.php?t=851 and modified so that 0 is
// the first slot of active display
const H32_ACCESS_SLOT_PIXELS: &[u16] =
    &[2, 18, 34, 66, 82, 98, 130, 146, 162, 194, 210, 226, 256, 258, 286, 314];
const H40_ACCESS_SLOT_PIXELS: &[u16] =
    &[2, 18, 34, 66, 82, 98, 130, 146, 162, 194, 210, 226, 258, 274, 290, 320, 322, 370];

// Official documentation says that DMA during blanking can transfer 167 bytes/line in H32 mode and
// 205 bytes/line in H40 mode, but testing on actual hardware has shown that there's an extra
// refresh slot on every line that decreases the rate to 166 bytes/line in H32 and 204 bytes/line in H40:
//   <https://gendev.spritesmind.net/forum/viewtopic.php?p=20921#p20921>
const H32_REFRESH_SLOT_PIXELS: &[u16] = &[50, 114, 178, 242, 306];
const H40_REFRESH_SLOT_PIXELS: &[u16] = &[50, 114, 178, 242, 306, 372];

const H32_PIXELS_PER_LINE: u16 = 342;
const H40_PIXELS_PER_LINE: u16 = 420;

const H32_SLOTS_PER_BLANK_LINE: u32 = 166;
const H40_SLOTS_PER_BLANK_LINE: u32 = 204;

const FIFO_CAPACITY: usize = 4;

impl HorizontalDisplaySize {
    fn fifo_slot_pixels(self) -> &'static [u16] {
        match self {
            Self::ThirtyTwoCell => H32_ACCESS_SLOT_PIXELS,
            Self::FortyCell => H40_ACCESS_SLOT_PIXELS,
        }
    }

    fn dma_slot_pixels(self, line_type: LineType) -> &'static [u16] {
        match (self, line_type) {
            (Self::ThirtyTwoCell, LineType::Active) => H32_ACCESS_SLOT_PIXELS,
            (Self::ThirtyTwoCell, LineType::Blanked) => H32_REFRESH_SLOT_PIXELS,
            (Self::FortyCell, LineType::Active) => H40_ACCESS_SLOT_PIXELS,
            (Self::FortyCell, LineType::Blanked) => H40_REFRESH_SLOT_PIXELS,
        }
    }

    fn vdp_pixels_per_line(self) -> u16 {
        match self {
            Self::ThirtyTwoCell => H32_PIXELS_PER_LINE,
            Self::FortyCell => H40_PIXELS_PER_LINE,
        }
    }

    fn slots_per_line(self, line_type: LineType) -> u32 {
        match (self, line_type) {
            (Self::ThirtyTwoCell, LineType::Active) => H32_ACCESS_SLOT_PIXELS.len() as u32,
            (Self::ThirtyTwoCell, LineType::Blanked) => H32_SLOTS_PER_BLANK_LINE,
            (Self::FortyCell, LineType::Active) => H40_ACCESS_SLOT_PIXELS.len() as u32,
            (Self::FortyCell, LineType::Blanked) => H40_SLOTS_PER_BLANK_LINE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum LineType {
    Active,
    Blanked,
}

impl LineType {
    pub fn from_vdp(vdp: &Vdp) -> Self {
        if !vdp.registers.display_enabled || vdp.in_vblank() { Self::Blanked } else { Self::Active }
    }
}

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
        let slot_pixels = h_display_size.fifo_slot_pixels();

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

pub struct DmaInitArgs {
    pub mode: DmaMode,
    pub vram_size: VramSizeKb,
    pub data_port_location: DataPortLocation,
    pub dma_length: u32,
    pub scanline: u16,
    pub pixel: u16,
    pub line_type: LineType,
    pub h_display_size: HorizontalDisplaySize,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct DmaTracker {
    mode: DmaMode,
    bytes_remaining: u32,
    data_port_read: bool,
    last_scanline: u16,
    last_line_type: LineType,
    last_pixel: u16,
    last_slot_index: u8,
    long_dma_in_progress: bool,
}

impl DmaTracker {
    pub fn new() -> Self {
        Self {
            mode: DmaMode::default(),
            bytes_remaining: 0,
            data_port_read: false,
            last_scanline: 0,
            last_line_type: LineType::Blanked,
            last_pixel: 0,
            last_slot_index: 0,
            long_dma_in_progress: false,
        }
    }

    pub fn init(
        &mut self,
        DmaInitArgs {
            mode,
            vram_size,
            data_port_location,
            dma_length,
            scanline,
            pixel,
            line_type,
            h_display_size,
        }: DmaInitArgs,
    ) {
        let mut dma_length_bytes = match mode {
            DmaMode::MemoryToVram => {
                // Memory-to-VRAM DMAs require two access slots per word if copying to VRAM in 64KB mode
                if data_port_location == DataPortLocation::Vram
                    && vram_size == VramSizeKb::SixtyFour
                {
                    dma_length * 2
                } else {
                    dma_length
                }
            }
            DmaMode::VramCopy => {
                // VRAM copy DMAs always need to perform 2 accesses per byte, one read and one write
                dma_length * 2
            }
            DmaMode::VramFill => {
                // VRAM fill DMAs always have to write one additional word at the start to count the
                // data port write that initiates the DMA
                dma_length + 2
            }
        };

        // There is no good reason for this +1 here, but adding a small extra overhead to every memory-to-VRAM DMA
        // fixes corrupted graphics in OutRunners which is extremely sensitive to CPU/DMA/FIFO timing
        if mode == DmaMode::MemoryToVram && data_port_location == DataPortLocation::Vram {
            dma_length_bytes += 1;
        }

        self.bytes_remaining = dma_length_bytes;
        self.data_port_read = false;

        self.mode = mode;
        self.last_scanline = scanline;
        self.last_pixel = pixel;
        self.last_line_type = line_type;
        self.last_slot_index = find_slot_index(pixel, h_display_size.dma_slot_pixels(line_type));

        // Wait to set "long DMA in progress" flag until advancing to the start of a line
        self.long_dma_in_progress = false;

        log::trace!(
            "Initiated DMA in mode {mode:?} at line {scanline} pixel {pixel}; length {dma_length}, bytes {dma_length_bytes}"
        );
    }

    pub fn is_in_progress(&self) -> bool {
        self.bytes_remaining != 0
    }

    pub fn long_dma_in_progress(&self) -> bool {
        self.long_dma_in_progress
    }

    pub fn record_data_port_read(&mut self) {
        self.data_port_read = true;
    }

    pub fn should_halt_cpu(&self, pending_writes: &[PendingWrite]) -> bool {
        // Memory-to-VRAM DMA always halts the CPU; VRAM fill & VRAM copy only halt the CPU if it
        // accesses the VDP data port during the DMA
        self.bytes_remaining != 0
            && (self.mode == DmaMode::MemoryToVram
                || self.data_port_read
                || pending_writes.iter().any(|write| matches!(write, PendingWrite::Data(..))))
    }

    pub fn advance_to_pixel(
        &mut self,
        scanline: u16,
        pixel: u16,
        h_display_size: HorizontalDisplaySize,
        line_type: LineType,
    ) {
        if self.bytes_remaining == 0 {
            // No DMA in progress
            return;
        }

        // Early return if there is more than a full line's worth of bytes left; wait until the next
        // line to advance
        if scanline == self.last_scanline
            && self.bytes_remaining > h_display_size.slots_per_line(line_type)
        {
            return;
        }

        if scanline != self.last_scanline {
            let pixels_per_line = h_display_size.vdp_pixels_per_line();
            self.advance_state(pixels_per_line, h_display_size);

            self.last_scanline = scanline;
            self.last_line_type = line_type;
            self.last_pixel = 0;
            self.last_slot_index = 0;

            // Update "long DMA in progress" flag after advancing to the start of the next line and
            // before advancing to the current pixel
            self.long_dma_in_progress =
                self.bytes_remaining > h_display_size.slots_per_line(line_type);
        }

        self.advance_state(pixel, h_display_size);
        self.last_pixel = pixel;

        log::trace!(
            "Advanced DMA to line {scanline} pixel {pixel}; {} bytes remaining",
            self.bytes_remaining
        );
    }

    fn advance_state(&mut self, pixel: u16, h_display_size: HorizontalDisplaySize) {
        let slot_pixels = h_display_size.dma_slot_pixels(self.last_line_type);
        let slots_crossed = count_slots_crossed(self.last_slot_index, pixel, slot_pixels);
        self.last_slot_index += slots_crossed;

        match self.last_line_type {
            LineType::Active => {
                // Subtract one byte for every access slot crossed
                self.bytes_remaining = self.bytes_remaining.saturating_sub(slots_crossed.into());
            }
            LineType::Blanked => {
                // Add one byte for every 2 pixels crossed, minus one for each refresh slot crossed
                let refresh_slots_crossed: u32 = slots_crossed.into();
                let total_slots_crossed: u32 = ((pixel / 2) - (self.last_pixel / 2)).into();
                debug_assert!(total_slots_crossed >= refresh_slots_crossed);

                self.bytes_remaining = self
                    .bytes_remaining
                    .saturating_sub(total_slots_crossed - refresh_slots_crossed);
            }
        }
    }
}

fn find_slot_index(pixel: u16, slot_pixels: &[u16]) -> u8 {
    slot_pixels.iter().position(|&slot_pixel| pixel < slot_pixel).unwrap_or(slot_pixels.len()) as u8
}

fn count_slots_crossed(mut last_slot_index: u8, pixel: u16, slot_pixels: &[u16]) -> u8 {
    let mut count = 0;

    while last_slot_index < slot_pixels.len() as u8
        && pixel >= slot_pixels[last_slot_index as usize]
    {
        last_slot_index += 1;
        count += 1;
    }

    count
}
