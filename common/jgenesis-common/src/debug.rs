use crate::num::{GetBit, U16Ext};

#[allow(clippy::len_without_is_empty)]
pub trait DebugMemoryView {
    fn len(&self) -> usize;

    fn read(&self, address: usize) -> u8;

    fn write(&mut self, address: usize, value: u8);
}

pub struct DebugBytesView<'a>(pub &'a mut [u8]);

impl DebugMemoryView for DebugBytesView<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }

    fn read(&self, address: usize) -> u8 {
        self.0.get(address).copied().unwrap_or(0)
    }

    fn write(&mut self, address: usize, value: u8) {
        if address >= self.len() {
            return;
        }

        self.0[address] = value;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endian {
    Little,
    Big,
}

pub struct DebugWordsView<'a>(pub &'a mut [u16], pub Endian);

impl DebugMemoryView for DebugWordsView<'_> {
    fn len(&self) -> usize {
        2 * self.0.len()
    }

    fn read(&self, mut address: usize) -> u8 {
        if address >= self.len() {
            return 0;
        }

        if self.1 == Endian::Little {
            address ^= 1;
        }

        let word = self.0[address >> 1];
        word.to_be_bytes()[address & 1]
    }

    fn write(&mut self, mut address: usize, value: u8) {
        if address >= self.len() {
            return;
        }

        if self.1 == Endian::Little {
            address ^= 1;
        }

        let word_addr = address >> 1;
        let word = &mut self.0[word_addr];
        if !address.bit(0) {
            word.set_msb(value);
        } else {
            word.set_lsb(value);
        }
    }
}

pub struct DebugLongwordsView<'a>(pub &'a mut [u32], pub Endian);

impl DebugMemoryView for DebugLongwordsView<'_> {
    fn len(&self) -> usize {
        4 * self.0.len()
    }

    fn read(&self, mut address: usize) -> u8 {
        if address >= self.len() {
            return 0;
        }

        if self.1 == Endian::Little {
            address ^= 3;
        }

        let longword_addr = address >> 2;
        let longword_addr = self.0[longword_addr];
        longword_addr.to_be_bytes()[address & 3]
    }

    fn write(&mut self, mut address: usize, value: u8) {
        if address >= self.len() {
            return;
        }

        if self.1 == Endian::Little {
            address ^= 3;
        }

        let longword_addr = address >> 2;
        let mut longword_bytes = self.0[longword_addr].to_be_bytes();
        longword_bytes[address & 3] = value;
        self.0[longword_addr] = u32::from_be_bytes(longword_bytes);
    }
}

pub struct EmptyDebugView;

impl DebugMemoryView for EmptyDebugView {
    fn len(&self) -> usize {
        0
    }

    fn read(&self, _address: usize) -> u8 {
        0
    }

    fn write(&mut self, _address: usize, _value: u8) {}
}
