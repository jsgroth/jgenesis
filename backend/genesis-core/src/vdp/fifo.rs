use crate::vdp::{DataPortLocation, DataPortMode};
use bincode::{Decode, Encode};
use std::array;

// Direct color DMA demos demonstrate that there's a ~3-slot latency between when an entry is
// written to the FIFO and when it can be popped
const INITIAL_FIFO_LATENCY: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum VramWriteFlag {
    First,
    Second,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum VramWriteSize {
    Word,
    Byte,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct VdpFifoEntry {
    pub mode: DataPortMode,
    pub location: DataPortLocation,
    pub address: u32,
    pub word: u16,
    pub size: VramWriteSize,
    pub vram_write: VramWriteFlag,
    pub latency: u8,
}

impl VdpFifoEntry {
    pub fn new(
        mode: DataPortMode,
        location: DataPortLocation,
        address: u32,
        word: u16,
        size: VramWriteSize,
    ) -> Self {
        Self {
            mode,
            location,
            address,
            word,
            size,
            vram_write: VramWriteFlag::First,
            latency: INITIAL_FIFO_LATENCY,
        }
    }
}

impl Default for VdpFifoEntry {
    fn default() -> Self {
        Self {
            mode: DataPortMode::Read,
            location: DataPortLocation::Vram,
            address: 0,
            word: 0,
            size: VramWriteSize::Word,
            vram_write: VramWriteFlag::First,
            latency: INITIAL_FIFO_LATENCY,
        }
    }
}

const FIFO_LEN: u8 = 4;

#[derive(Debug, Clone, Encode, Decode)]
pub struct VdpFifo {
    buffer: [VdpFifoEntry; FIFO_LEN as usize],
    push_idx: u8,
    pop_idx: u8,
    len: u8,
}

impl VdpFifo {
    pub fn new() -> Self {
        Self {
            buffer: array::from_fn(|_| VdpFifoEntry::default()),
            push_idx: 0,
            pop_idx: 0,
            len: 0,
        }
    }

    pub fn push(&mut self, entry: VdpFifoEntry) {
        debug_assert!(self.len < FIFO_LEN);

        self.buffer[self.push_idx as usize] = entry;
        self.push_idx = (self.push_idx + 1) % FIFO_LEN;
        self.len += 1;
    }

    pub fn front(&self) -> &VdpFifoEntry {
        &self.buffer[self.pop_idx as usize]
    }

    pub fn next_slot_word(&self) -> u16 {
        self.buffer[self.push_idx as usize].word
    }

    pub fn pop(&mut self) {
        debug_assert_ne!(self.len, 0);

        // Clue depends on FIFO entries for invalid targets taking 2 slots to pop, same as VRAM writes.
        // Otherwise main menu graphics will be randomly corrupted
        let front = &mut self.buffer[self.pop_idx as usize];
        if matches!(front.location, DataPortLocation::Vram | DataPortLocation::Invalid)
            && front.size == VramWriteSize::Word
            && front.vram_write == VramWriteFlag::First
        {
            front.vram_write = VramWriteFlag::Second;
            return;
        }

        self.pop_idx = (self.pop_idx + 1) % FIFO_LEN;
        self.len -= 1;
    }

    pub fn decrement_latency(&mut self) {
        for i in 0..self.len {
            let idx = (self.pop_idx + i) % 4;
            let entry = &mut self.buffer[idx as usize];
            entry.latency = entry.latency.saturating_sub(1);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn is_full(&self) -> bool {
        self.len == FIFO_LEN
    }

    pub fn len(&self) -> u8 {
        self.len
    }

    pub fn iter(&self) -> VdpFifoIter<'_> {
        VdpFifoIter { buffer: &self.buffer, idx: self.pop_idx, remaining: self.len }
    }
}

pub struct VdpFifoIter<'fifo> {
    buffer: &'fifo [VdpFifoEntry; FIFO_LEN as usize],
    idx: u8,
    remaining: u8,
}

impl<'fifo> Iterator for VdpFifoIter<'fifo> {
    type Item = &'fifo VdpFifoEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }

        let value = &self.buffer[self.idx as usize];

        self.idx = (self.idx + 1) % FIFO_LEN;
        self.remaining -= 1;

        Some(value)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining.into(), Some(self.remaining.into()))
    }
}
