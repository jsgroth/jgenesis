//! Code for handling word RAM, a 256KB block of RAM that can be exchanged between the main CPU
//! and the sub CPU

use crate::memory::ScdCpu;
use bincode::{Decode, Encode};
use jgenesis_traits::num::GetBit;

// Word RAM is 256KB
pub const ADDRESS_MASK: u32 = 0x03FFFF;

// Word RAM is mapped to $080000-$0BFFFF in sub CPU memory map
pub const SUB_BASE_ADDRESS: u32 = 0x080000;

const WORD_RAM_LEN: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum WordRamMode {
    #[default]
    TwoM,
    OneM,
}

impl WordRamMode {
    fn to_bit(self) -> bool {
        self == Self::OneM
    }

    fn from_bit(bit: bool) -> Self {
        if bit { Self::OneM } else { Self::TwoM }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum WordRamPriorityMode {
    #[default]
    Off,
    Underwrite,
    Overwrite,
    Invalid,
}

impl WordRamPriorityMode {
    pub fn to_bits(self) -> u8 {
        match self {
            Self::Off => 0x00,
            Self::Underwrite => 0x01,
            Self::Overwrite => 0x02,
            Self::Invalid => 0x03,
        }
    }

    pub fn from_bits(bits: u8) -> Self {
        match bits & 0x03 {
            0x00 => Self::Off,
            0x01 => Self::Underwrite,
            0x02 => Self::Overwrite,
            0x03 => Self::Invalid,
            _ => unreachable!("value & 0x03 is always <= 0x03"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nibble {
    High,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordRamSubMapResult {
    None,
    Byte(u32),
    Pixel(u32),
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct WordRam {
    ram: Box<[u8; WORD_RAM_LEN]>,
    mode: WordRamMode,
    priority_mode: WordRamPriorityMode,
    owner_2m: ScdCpu,
    bank_0_owner_1m: ScdCpu,
    swap_request: bool,
}

// N cells, 8 pixels vertically, 8 pixels horizontally, 2 pixels per byte
const CELL_IMAGE_V32_SIZE_BYTES: u32 = 32 * 8 * 8 / 2;
const CELL_IMAGE_V16_SIZE_BYTES: u32 = 16 * 8 * 8 / 2;
const CELL_IMAGE_V8_SIZE_BYTES: u32 = 8 * 8 * 8 / 2;
const CELL_IMAGE_V4_SIZE_BYTES: u32 = 4 * 8 * 8 / 2;

const CELL_IMAGE_H_SIZE_BYTES: u32 = 64 * 8 / 2;

impl WordRam {
    pub fn new() -> Self {
        Self {
            ram: vec![0; WORD_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            mode: WordRamMode::default(),
            priority_mode: WordRamPriorityMode::default(),
            owner_2m: ScdCpu::Main,
            bank_0_owner_1m: ScdCpu::Main,
            swap_request: false,
        }
    }

    pub fn mode(&self) -> WordRamMode {
        self.mode
    }

    pub fn read_control(&self) -> u8 {
        let (dmna, ret) = match self.mode {
            WordRamMode::TwoM => {
                let dmna = self.owner_2m == ScdCpu::Sub;
                let ret = !dmna;
                (dmna, ret)
            }
            WordRamMode::OneM => {
                let dmna = self.swap_request;
                let ret = self.bank_0_owner_1m == ScdCpu::Sub;
                (dmna, ret)
            }
        };

        (u8::from(self.mode.to_bit()) << 2) | (u8::from(dmna) << 1) | u8::from(ret)
    }

    pub fn main_cpu_write_control(&mut self, value: u8) {
        let dmna = value.bit(1);

        // DMNA=1 always returns 2M word RAM to sub CPU, regardless of mode
        if dmna {
            self.owner_2m = ScdCpu::Sub;
        }

        // In 1M mode, setting DMNA=0 sends a swap request to the sub CPU
        if self.mode == WordRamMode::OneM && !dmna {
            self.swap_request = true;
        }

        log::trace!("Main CPU control write; DMNA={}, mode={:?}", u8::from(dmna), self.mode);
    }

    pub fn sub_cpu_write_control(&mut self, value: u8) {
        self.mode = WordRamMode::from_bit(value.bit(2));
        let ret = value.bit(0);

        // RET=1 always returns 2M word RAM to main CPU, regardless of mode
        if ret {
            self.owner_2m = ScdCpu::Main;
        }

        let prev_bank_0_owner = self.bank_0_owner_1m;
        // In 1M mode, RET returns the given bank to the main CPU
        // RET=0 -> main owns bank 0, sub owns bank 1
        // RET=1 -> main owns bank 1, sub owns bank 0
        self.bank_0_owner_1m = if ret { ScdCpu::Sub } else { ScdCpu::Main };

        // Only clear swap request if 1M bank 0 owner changed
        if prev_bank_0_owner != self.bank_0_owner_1m {
            self.swap_request = false;
        }

        self.priority_mode = WordRamPriorityMode::from_bits(value >> 3);

        log::trace!(
            "Sub CPU control write; RET={}, mode={:?}, priority_mode={:?}",
            u8::from(ret),
            self.mode,
            self.priority_mode
        );
    }

    fn main_cpu_map_address(&self, address: u32) -> Option<u32> {
        let address = address & 0x3FFFF;
        match self.mode {
            WordRamMode::TwoM => (self.owner_2m == ScdCpu::Main).then_some(address),
            WordRamMode::OneM => {
                if address <= 0x1FFFF {
                    Some(determine_1m_address(address, ScdCpu::Main, self.bank_0_owner_1m))
                } else {
                    match address & 0x1FFFF {
                        address @ 0x00000..=0x0FFFF => {
                            // V32xH64 image
                            Some(map_cell_image_address(
                                address & 0xFFFF,
                                CELL_IMAGE_V32_SIZE_BYTES,
                                self.bank_0_owner_1m,
                                0x00000,
                            ))
                        }
                        address @ 0x10000..=0x17FFF => {
                            // V16xH64 image
                            Some(map_cell_image_address(
                                address & 0x7FFF,
                                CELL_IMAGE_V16_SIZE_BYTES,
                                self.bank_0_owner_1m,
                                0x10000,
                            ))
                        }
                        address @ 0x18000..=0x1BFFF => {
                            // V8xH64 image
                            Some(map_cell_image_address(
                                address & 0x3FFF,
                                CELL_IMAGE_V8_SIZE_BYTES,
                                self.bank_0_owner_1m,
                                0x18000,
                            ))
                        }
                        address @ 0x1C000..=0x1DFFF => {
                            // V4xH64 image #1
                            Some(map_cell_image_address(
                                address & 0x1FFF,
                                CELL_IMAGE_V4_SIZE_BYTES,
                                self.bank_0_owner_1m,
                                0x1C000,
                            ))
                        }
                        address @ 0x1E000..=0x1FFFF => {
                            // V4xH64 image #2
                            Some(map_cell_image_address(
                                address & 0x1FFF,
                                CELL_IMAGE_V4_SIZE_BYTES,
                                self.bank_0_owner_1m,
                                0x1E000,
                            ))
                        }
                        _ => unreachable!("address masked with 0x1FFFF"),
                    }
                }
            }
        }
    }

    pub fn main_cpu_read_ram(&self, address: u32) -> u8 {
        match self.main_cpu_map_address(address) {
            None => 0x00,
            Some(addr) => self.ram[addr as usize],
        }
    }

    pub fn main_cpu_write_ram(&mut self, address: u32, value: u8) {
        match self.main_cpu_map_address(address) {
            None => {}
            Some(addr) => {
                self.ram[addr as usize] = value;
            }
        }
    }

    fn sub_cpu_map_address(&self, address: u32) -> WordRamSubMapResult {
        match (self.mode, address) {
            (WordRamMode::TwoM, 0x080000..=0x0BFFFF) => {
                // Hack: On real hardware, the sub CPU accessing word RAM in 2M mode while it's
                // owned by the main CPU causes the sub CPU to lock up.
                // Allowing these accesses to go through fixes flickering / missing graphics in
                // Batman Returns, possibly related to graphics ASIC timing issues
                WordRamSubMapResult::Byte(address & ADDRESS_MASK)
            }
            (WordRamMode::TwoM, 0x0C0000..=0x0DFFFF) => WordRamSubMapResult::None,
            (WordRamMode::OneM, 0x080000..=0x0BFFFF) => {
                let byte_addr = determine_1m_address(
                    (address & 0x3FFFF) >> 1,
                    ScdCpu::Sub,
                    self.bank_0_owner_1m,
                );
                WordRamSubMapResult::Pixel((byte_addr << 1) | (address & 0x000001))
            }
            (WordRamMode::OneM, 0x0C0000..=0x0DFFFF) => WordRamSubMapResult::Byte(
                determine_1m_address(address & 0x1FFFF, ScdCpu::Sub, self.bank_0_owner_1m),
            ),
            _ => panic!("Invalid sub CPU word RAM address: {address:06X}"),
        }
    }

    pub fn sub_cpu_read_ram(&self, address: u32) -> u8 {
        match self.sub_cpu_map_address(address) {
            WordRamSubMapResult::None => 0,
            WordRamSubMapResult::Byte(addr) => self.ram[addr as usize],
            WordRamSubMapResult::Pixel(pixel_addr) => {
                let byte_addr = (pixel_addr >> 1) as usize;
                if pixel_addr.bit(0) {
                    self.ram[byte_addr] & 0x0F
                } else {
                    self.ram[byte_addr] >> 4
                }
            }
        }
    }

    pub fn sub_cpu_write_ram(&mut self, address: u32, value: u8) {
        match self.sub_cpu_map_address(address) {
            WordRamSubMapResult::None => {}
            WordRamSubMapResult::Byte(addr) => {
                self.ram[addr as usize] = value;
            }
            WordRamSubMapResult::Pixel(pixel_addr) => {
                self.write_1m_pixel(pixel_addr, value & 0x0F);
            }
        }
    }

    pub fn graphics_write_ram(&mut self, address: u32, nibble: Nibble, pixel: u8) {
        match self.sub_cpu_map_address(address) {
            WordRamSubMapResult::None => {}
            WordRamSubMapResult::Byte(addr) => {
                let current_value = self.ram[addr as usize];
                let current_nibble = match nibble {
                    Nibble::High => current_value >> 4,
                    Nibble::Low => current_value & 0x0F,
                };

                let should_write = match self.priority_mode {
                    WordRamPriorityMode::Off | WordRamPriorityMode::Invalid => true,
                    WordRamPriorityMode::Underwrite => current_nibble == 0,
                    WordRamPriorityMode::Overwrite => pixel != 0,
                };

                if should_write {
                    let new_value = match nibble {
                        Nibble::High => (pixel << 4) | (current_value & 0x0F),
                        Nibble::Low => pixel | (current_value & 0xF0),
                    };
                    self.ram[addr as usize] = new_value;
                }
            }
            WordRamSubMapResult::Pixel(addr) => {
                // High nibble is ignored for pixel writes
                if nibble == Nibble::Low {
                    self.write_1m_pixel(addr, pixel);
                }
            }
        }
    }

    fn write_1m_pixel(&mut self, pixel_addr: u32, pixel: u8) {
        let byte_addr = (pixel_addr >> 1) as usize;
        let current_byte = self.ram[byte_addr];
        let current_nibble =
            if pixel_addr.bit(0) { current_byte & 0x0F } else { current_byte >> 4 };

        let should_write = match self.priority_mode {
            WordRamPriorityMode::Off | WordRamPriorityMode::Invalid => true,
            WordRamPriorityMode::Underwrite => current_nibble == 0,
            WordRamPriorityMode::Overwrite => pixel != 0,
        };

        if should_write {
            let new_value = if pixel_addr.bit(0) {
                pixel | (current_byte & 0xF0)
            } else {
                (pixel << 4) | (current_byte & 0x0F)
            };
            self.ram[byte_addr] = new_value;
        }
    }

    pub fn dma_write(&mut self, address: u32, value: u8) {
        // Word RAM DMA writes should go to $080000 in 2M mode and $0C0000 in 1M mode
        // In 1M mode, $080000-$0BFFFF is a dot image of word RAM, and the raw bytes are at $0C0000-$0DFFFF
        let base_address = match self.mode {
            WordRamMode::TwoM => {
                assert!(address <= 0x03FFFF);
                SUB_BASE_ADDRESS
            }
            WordRamMode::OneM => {
                assert!(address <= 0x01FFFF);
                0x0C0000
            }
        };

        self.sub_cpu_write_ram(base_address | address, value);
    }

    pub fn priority_mode(&self) -> WordRamPriorityMode {
        self.priority_mode
    }
}

fn determine_1m_address(address: u32, cpu: ScdCpu, bank_0_owner: ScdCpu) -> u32 {
    ((address & !0x01) << 1) | (u32::from(cpu != bank_0_owner) << 1) | (address & 0x01)
}

#[inline]
fn map_cell_image_address(
    masked_address: u32,
    v_size_bytes: u32,
    bank_0_owner: ScdCpu,
    base_word_ram_addr: u32,
) -> u32 {
    let row = (masked_address & (v_size_bytes - 1)) >> 2;
    let col = masked_address / v_size_bytes;

    let byte_addr =
        base_word_ram_addr | (row * CELL_IMAGE_H_SIZE_BYTES) | (col << 2) | (masked_address & 0x03);
    determine_1m_address(byte_addr, ScdCpu::Main, bank_0_owner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_ram_main_cpu_2m_mapping() {
        let mut word_ram = WordRam::new();
        word_ram.mode = WordRamMode::TwoM;

        word_ram.owner_2m = ScdCpu::Main;
        assert_eq!(Some(0x000000), word_ram.main_cpu_map_address(0x200000));
        assert_eq!(Some(0x03FFFF), word_ram.main_cpu_map_address(0x23FFFF));

        word_ram.owner_2m = ScdCpu::Sub;
        assert_eq!(None, word_ram.main_cpu_map_address(0x200000));
        assert_eq!(None, word_ram.main_cpu_map_address(0x23FFFF));
    }

    #[test]
    fn word_ram_main_cpu_1m_mapping() {
        let mut word_ram = WordRam::new();
        word_ram.mode = WordRamMode::OneM;

        word_ram.bank_0_owner_1m = ScdCpu::Main;
        assert_eq!(Some(0x000000), word_ram.main_cpu_map_address(0x200000));
        assert_eq!(Some(0x020001), word_ram.main_cpu_map_address(0x210001));
        assert_eq!(Some(0x020004), word_ram.main_cpu_map_address(0x210002));
        assert_eq!(Some(0x03FFFD), word_ram.main_cpu_map_address(0x21FFFF));

        word_ram.bank_0_owner_1m = ScdCpu::Sub;
        assert_eq!(Some(0x000002), word_ram.main_cpu_map_address(0x200000));
        assert_eq!(Some(0x020003), word_ram.main_cpu_map_address(0x210001));
        assert_eq!(Some(0x03FFFF), word_ram.main_cpu_map_address(0x21FFFF));
    }

    #[test]
    fn word_ram_main_cpu_1m_cell_image_mapping() {
        let mut word_ram = WordRam::new();
        word_ram.mode = WordRamMode::OneM;

        // V32-cell
        word_ram.bank_0_owner_1m = ScdCpu::Main;
        assert_eq!(Some(0x000000), word_ram.main_cpu_map_address(0x220000));
        assert_eq!(Some(0x000001), word_ram.main_cpu_map_address(0x220001));
        assert_eq!(Some(0x000004), word_ram.main_cpu_map_address(0x220002));
        assert_eq!(Some(0x000005), word_ram.main_cpu_map_address(0x220003));

        assert_eq!(Some(0x000200), word_ram.main_cpu_map_address(0x220004));
        assert_eq!(Some(0x000201), word_ram.main_cpu_map_address(0x220005));
        assert_eq!(Some(0x000205), word_ram.main_cpu_map_address(0x220007));

        assert_eq!(Some(0x000008), word_ram.main_cpu_map_address(0x220400));
        assert_eq!(Some(0x000009), word_ram.main_cpu_map_address(0x220401));
        assert_eq!(Some(0x00000C), word_ram.main_cpu_map_address(0x220402));
        assert_eq!(Some(0x000208), word_ram.main_cpu_map_address(0x220404));

        assert_eq!(Some(0x000010), word_ram.main_cpu_map_address(0x220800));

        assert_eq!(Some(0x01FFFD), word_ram.main_cpu_map_address(0x22FFFF));

        // V16-cell
        assert_eq!(Some(0x020000), word_ram.main_cpu_map_address(0x230000));
        assert_eq!(Some(0x020001), word_ram.main_cpu_map_address(0x230001));
        assert_eq!(Some(0x020004), word_ram.main_cpu_map_address(0x230002));

        assert_eq!(Some(0x020200), word_ram.main_cpu_map_address(0x230004));
        assert_eq!(Some(0x020201), word_ram.main_cpu_map_address(0x230005));

        assert_eq!(Some(0x020008), word_ram.main_cpu_map_address(0x230200));
        assert_eq!(Some(0x020009), word_ram.main_cpu_map_address(0x230201));

        assert_eq!(Some(0x020010), word_ram.main_cpu_map_address(0x230400));

        assert_eq!(Some(0x02FFFD), word_ram.main_cpu_map_address(0x237FFF));

        // V8-cell
        assert_eq!(Some(0x030000), word_ram.main_cpu_map_address(0x238000));
        assert_eq!(Some(0x030001), word_ram.main_cpu_map_address(0x238001));

        assert_eq!(Some(0x030200), word_ram.main_cpu_map_address(0x238004));
        assert_eq!(Some(0x030201), word_ram.main_cpu_map_address(0x238005));

        assert_eq!(Some(0x030008), word_ram.main_cpu_map_address(0x238100));
        assert_eq!(Some(0x030009), word_ram.main_cpu_map_address(0x238101));

        assert_eq!(Some(0x030010), word_ram.main_cpu_map_address(0x238200));

        assert_eq!(Some(0x037FFD), word_ram.main_cpu_map_address(0x23BFFF));

        // V4-cell #1
        assert_eq!(Some(0x038000), word_ram.main_cpu_map_address(0x23C000));
        assert_eq!(Some(0x038001), word_ram.main_cpu_map_address(0x23C001));

        assert_eq!(Some(0x038200), word_ram.main_cpu_map_address(0x23C004));
        assert_eq!(Some(0x038201), word_ram.main_cpu_map_address(0x23C005));

        assert_eq!(Some(0x038008), word_ram.main_cpu_map_address(0x23C080));
        assert_eq!(Some(0x038009), word_ram.main_cpu_map_address(0x23C081));

        assert_eq!(Some(0x038010), word_ram.main_cpu_map_address(0x23C100));

        assert_eq!(Some(0x03BFFD), word_ram.main_cpu_map_address(0x23DFFF));

        // V4-cell #2
        assert_eq!(Some(0x03C000), word_ram.main_cpu_map_address(0x23E000));
        assert_eq!(Some(0x03C001), word_ram.main_cpu_map_address(0x23E001));

        assert_eq!(Some(0x03C200), word_ram.main_cpu_map_address(0x23E004));
        assert_eq!(Some(0x03C201), word_ram.main_cpu_map_address(0x23E005));

        assert_eq!(Some(0x03C008), word_ram.main_cpu_map_address(0x23E080));
        assert_eq!(Some(0x03C009), word_ram.main_cpu_map_address(0x23E081));

        assert_eq!(Some(0x03C010), word_ram.main_cpu_map_address(0x23E100));

        assert_eq!(Some(0x03FFFD), word_ram.main_cpu_map_address(0x23FFFF));

        // Test with main CPU having bank 1
        word_ram.bank_0_owner_1m = ScdCpu::Sub;
        assert_eq!(Some(0x000002), word_ram.main_cpu_map_address(0x220000));
        assert_eq!(Some(0x03FFFF), word_ram.main_cpu_map_address(0x23FFFF));
    }

    #[test]
    fn word_ram_sub_cpu_2m_mapping_sub_owner() {
        use WordRamSubMapResult as R;

        let mut word_ram = WordRam::new();
        word_ram.mode = WordRamMode::TwoM;

        word_ram.owner_2m = ScdCpu::Sub;
        assert_eq!(R::Byte(0x000000), word_ram.sub_cpu_map_address(0x080000));
        assert_eq!(R::Byte(0x03FFFF), word_ram.sub_cpu_map_address(0x0BFFFF));

        assert_eq!(R::None, word_ram.sub_cpu_map_address(0x0C0000));
        assert_eq!(R::None, word_ram.sub_cpu_map_address(0x0DFFFF));
    }

    // This test case depends on restricting sub CPU access to 2M word RAM when it's owned by
    // the main CPU, which seems to cause graphical issues in Batman Returns (most likely caused
    // by a bug elsewhere in the emulator)
    #[ignore]
    #[test]
    fn word_ram_sub_cpu_2m_mapping_main_owner() {
        use WordRamSubMapResult as R;

        let mut word_ram = WordRam::new();
        word_ram.mode = WordRamMode::TwoM;

        word_ram.owner_2m = ScdCpu::Main;
        assert_eq!(R::None, word_ram.sub_cpu_map_address(0x080000));
        assert_eq!(R::None, word_ram.sub_cpu_map_address(0x0BFFFF));

        assert_eq!(R::None, word_ram.sub_cpu_map_address(0x0C0000));
        assert_eq!(R::None, word_ram.sub_cpu_map_address(0x0DFFFF));
    }

    #[test]
    fn word_ram_sub_cpu_1m_byte_mapping() {
        use WordRamSubMapResult as R;

        let mut word_ram = WordRam::new();
        word_ram.mode = WordRamMode::OneM;

        word_ram.bank_0_owner_1m = ScdCpu::Sub;
        assert_eq!(R::Byte(0x000000), word_ram.sub_cpu_map_address(0x0C0000));
        assert_eq!(R::Byte(0x010000), word_ram.sub_cpu_map_address(0x0C8000));
        assert_eq!(R::Byte(0x03FFFD), word_ram.sub_cpu_map_address(0x0DFFFF));

        word_ram.bank_0_owner_1m = ScdCpu::Main;
        assert_eq!(R::Byte(0x000002), word_ram.sub_cpu_map_address(0x0C0000));
        assert_eq!(R::Byte(0x010002), word_ram.sub_cpu_map_address(0x0C8000));
        assert_eq!(R::Byte(0x03FFFF), word_ram.sub_cpu_map_address(0x0DFFFF));
    }

    #[test]
    fn word_ram_sub_cpu_1m_pixel_mapping() {
        use WordRamSubMapResult as R;

        let mut word_ram = WordRam::new();
        word_ram.mode = WordRamMode::OneM;

        word_ram.bank_0_owner_1m = ScdCpu::Sub;
        assert_eq!(R::Pixel(0x000000), word_ram.sub_cpu_map_address(0x080000));
        assert_eq!(R::Pixel(0x000001), word_ram.sub_cpu_map_address(0x080001));
        assert_eq!(R::Pixel(0x000002), word_ram.sub_cpu_map_address(0x080002));
        assert_eq!(R::Pixel(0x000003), word_ram.sub_cpu_map_address(0x080003));
        assert_eq!(R::Pixel(0x000008), word_ram.sub_cpu_map_address(0x080004));
        assert_eq!(R::Pixel(0x000009), word_ram.sub_cpu_map_address(0x080005));
        assert_eq!(R::Pixel(0x07FFFA), word_ram.sub_cpu_map_address(0x0BFFFE));
        assert_eq!(R::Pixel(0x07FFFB), word_ram.sub_cpu_map_address(0x0BFFFF));

        word_ram.bank_0_owner_1m = ScdCpu::Main;
        assert_eq!(R::Pixel(0x000004), word_ram.sub_cpu_map_address(0x080000));
        assert_eq!(R::Pixel(0x000005), word_ram.sub_cpu_map_address(0x080001));
        assert_eq!(R::Pixel(0x000006), word_ram.sub_cpu_map_address(0x080002));
        assert_eq!(R::Pixel(0x000007), word_ram.sub_cpu_map_address(0x080003));
        assert_eq!(R::Pixel(0x00000C), word_ram.sub_cpu_map_address(0x080004));
        assert_eq!(R::Pixel(0x00000D), word_ram.sub_cpu_map_address(0x080005));
        assert_eq!(R::Pixel(0x07FFFE), word_ram.sub_cpu_map_address(0x0BFFFE));
        assert_eq!(R::Pixel(0x07FFFF), word_ram.sub_cpu_map_address(0x0BFFFF));
    }
}
