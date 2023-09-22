use bincode::{Decode, Encode};
use jgenesis_traits::num::GetBit;

const WORD_RAM_LEN: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum WordRamMode {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum WordRamOwner {
    Main,
    Sub,
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
    owner_2m: WordRamOwner,
    bank_0_owner_1m: WordRamOwner,
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
            mode: WordRamMode::TwoM,
            owner_2m: WordRamOwner::Main,
            bank_0_owner_1m: WordRamOwner::Main,
            swap_request: false,
        }
    }

    pub fn read_control(&self) -> u8 {
        let (dmna, ret) = match self.mode {
            WordRamMode::TwoM => {
                let dmna = self.owner_2m == WordRamOwner::Sub;
                let ret = !dmna;
                (dmna, ret)
            }
            WordRamMode::OneM => {
                let dmna = self.swap_request;
                let ret = self.bank_0_owner_1m == WordRamOwner::Sub;
                (dmna, ret)
            }
        };

        (u8::from(self.mode.to_bit()) << 2) | (u8::from(dmna) << 1) | u8::from(ret)
    }

    pub fn main_cpu_write_control(&mut self, value: u8) {
        let dmna = value.bit(1);

        // DMNA=1 always returns 2M word RAM to sub CPU, regardless of mode
        if dmna {
            self.owner_2m = WordRamOwner::Sub;
        }

        // In 1M mode, setting DMNA=0 sends a swap request to the sub CPU
        if self.mode == WordRamMode::OneM && !dmna {
            self.swap_request = true;
        }
    }

    pub fn sub_cpu_write_control(&mut self, value: u8) {
        self.mode = WordRamMode::from_bit(value.bit(2));
        let ret = value.bit(0);

        // RET=1 always returns 2M word RAM to main CPU, regardless of mode
        if ret {
            self.owner_2m = WordRamOwner::Main;
        }

        // In 1M mode, RET returns the given bank to the main CPU
        // RET=0 -> main owns bank 0, sub owns bank 1
        // RET=1 -> main owns bank 1, sub owns bank 0
        self.bank_0_owner_1m = if ret { WordRamOwner::Sub } else { WordRamOwner::Main };

        self.swap_request = false;
    }

    fn main_cpu_map_address(&self, address: u32) -> Option<u32> {
        let address = address & 0x3FFFF;
        match self.mode {
            WordRamMode::TwoM => (self.owner_2m == WordRamOwner::Main).then_some(address),
            WordRamMode::OneM => {
                if address <= 0x1FFFF {
                    Some((address << 1) | u32::from(self.bank_0_owner_1m == WordRamOwner::Sub))
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
                if self.owner_2m == WordRamOwner::Sub {
                    WordRamSubMapResult::Byte(address & 0x03FFFF)
                } else {
                    WordRamSubMapResult::None
                }
            }
            (WordRamMode::TwoM, 0x0C0000..=0x0DFFFF) => WordRamSubMapResult::None,
            (WordRamMode::OneM, 0x080000..=0x0BFFFF) => {
                let byte_addr =
                    (address & 0x03FFFE) | u32::from(self.bank_0_owner_1m == WordRamOwner::Main);
                WordRamSubMapResult::Pixel((byte_addr << 1) | (address & 0x000001))
            }
            (WordRamMode::OneM, 0x0C0000..=0x0DFFFF) => WordRamSubMapResult::Byte(
                ((address & 0x01FFFF) << 1) | u32::from(self.bank_0_owner_1m == WordRamOwner::Main),
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
                let byte_addr = (pixel_addr >> 1) as usize;
                if pixel_addr.bit(0) {
                    self.ram[byte_addr] = (self.ram[byte_addr] & 0xF0) | (value & 0x0F);
                } else {
                    self.ram[byte_addr] = (self.ram[byte_addr] & 0x0F) | (value << 4);
                }
            }
        }
    }
}

#[inline]
fn map_cell_image_address(
    masked_address: u32,
    v_size_bytes: u32,
    bank_0_owner: WordRamOwner,
    base_word_ram_addr: u32,
) -> u32 {
    let row = (masked_address & (v_size_bytes - 1)) >> 2;
    let col = masked_address / v_size_bytes;

    let byte_addr =
        base_word_ram_addr | (row * CELL_IMAGE_H_SIZE_BYTES) | (col << 2) | (masked_address & 0x03);
    (byte_addr << 1) | u32::from(bank_0_owner == WordRamOwner::Sub)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_ram_main_cpu_2m_mapping() {
        let mut word_ram = WordRam::new();
        word_ram.mode = WordRamMode::TwoM;

        word_ram.owner_2m = WordRamOwner::Main;
        assert_eq!(Some(0x000000), word_ram.main_cpu_map_address(0x200000));
        assert_eq!(Some(0x03FFFF), word_ram.main_cpu_map_address(0x23FFFF));

        word_ram.owner_2m = WordRamOwner::Sub;
        assert_eq!(None, word_ram.main_cpu_map_address(0x200000));
        assert_eq!(None, word_ram.main_cpu_map_address(0x23FFFF));
    }

    #[test]
    fn word_ram_main_cpu_1m_mapping() {
        let mut word_ram = WordRam::new();
        word_ram.mode = WordRamMode::OneM;

        word_ram.bank_0_owner_1m = WordRamOwner::Main;
        assert_eq!(Some(0x000000), word_ram.main_cpu_map_address(0x200000));
        assert_eq!(Some(0x020002), word_ram.main_cpu_map_address(0x210001));
        assert_eq!(Some(0x03FFFE), word_ram.main_cpu_map_address(0x21FFFF));

        word_ram.bank_0_owner_1m = WordRamOwner::Sub;
        assert_eq!(Some(0x000001), word_ram.main_cpu_map_address(0x200000));
        assert_eq!(Some(0x020003), word_ram.main_cpu_map_address(0x210001));
        assert_eq!(Some(0x03FFFF), word_ram.main_cpu_map_address(0x21FFFF));
    }

    #[test]
    fn word_ram_main_cpu_1m_cell_image_mapping() {
        let mut word_ram = WordRam::new();
        word_ram.mode = WordRamMode::OneM;

        // V32-cell
        word_ram.bank_0_owner_1m = WordRamOwner::Main;
        assert_eq!(Some(0x000000), word_ram.main_cpu_map_address(0x220000));
        assert_eq!(Some(0x000002), word_ram.main_cpu_map_address(0x220001));
        assert_eq!(Some(0x000004), word_ram.main_cpu_map_address(0x220002));
        assert_eq!(Some(0x000006), word_ram.main_cpu_map_address(0x220003));

        assert_eq!(Some(0x000200), word_ram.main_cpu_map_address(0x220004));
        assert_eq!(Some(0x000202), word_ram.main_cpu_map_address(0x220005));
        assert_eq!(Some(0x000206), word_ram.main_cpu_map_address(0x220007));

        assert_eq!(Some(0x000008), word_ram.main_cpu_map_address(0x220400));
        assert_eq!(Some(0x00000A), word_ram.main_cpu_map_address(0x220401));
        assert_eq!(Some(0x00000C), word_ram.main_cpu_map_address(0x220402));
        assert_eq!(Some(0x000208), word_ram.main_cpu_map_address(0x220404));

        assert_eq!(Some(0x000010), word_ram.main_cpu_map_address(0x220800));

        assert_eq!(Some(0x01FFFE), word_ram.main_cpu_map_address(0x22FFFF));

        // V16-cell
        assert_eq!(Some(0x020000), word_ram.main_cpu_map_address(0x230000));
        assert_eq!(Some(0x020002), word_ram.main_cpu_map_address(0x230001));

        assert_eq!(Some(0x020200), word_ram.main_cpu_map_address(0x230004));
        assert_eq!(Some(0x020202), word_ram.main_cpu_map_address(0x230005));

        assert_eq!(Some(0x020008), word_ram.main_cpu_map_address(0x230200));
        assert_eq!(Some(0x02000A), word_ram.main_cpu_map_address(0x230201));

        assert_eq!(Some(0x020010), word_ram.main_cpu_map_address(0x230400));

        assert_eq!(Some(0x02FFFE), word_ram.main_cpu_map_address(0x237FFF));

        // V8-cell
        assert_eq!(Some(0x030000), word_ram.main_cpu_map_address(0x238000));
        assert_eq!(Some(0x030002), word_ram.main_cpu_map_address(0x238001));

        assert_eq!(Some(0x030200), word_ram.main_cpu_map_address(0x238004));
        assert_eq!(Some(0x030202), word_ram.main_cpu_map_address(0x238005));

        assert_eq!(Some(0x030008), word_ram.main_cpu_map_address(0x238100));
        assert_eq!(Some(0x03000A), word_ram.main_cpu_map_address(0x238101));

        assert_eq!(Some(0x030010), word_ram.main_cpu_map_address(0x238200));

        assert_eq!(Some(0x037FFE), word_ram.main_cpu_map_address(0x23BFFF));

        // V4-cell #1
        assert_eq!(Some(0x038000), word_ram.main_cpu_map_address(0x23C000));
        assert_eq!(Some(0x038002), word_ram.main_cpu_map_address(0x23C001));

        assert_eq!(Some(0x038200), word_ram.main_cpu_map_address(0x23C004));
        assert_eq!(Some(0x038202), word_ram.main_cpu_map_address(0x23C005));

        assert_eq!(Some(0x038008), word_ram.main_cpu_map_address(0x23C080));
        assert_eq!(Some(0x03800A), word_ram.main_cpu_map_address(0x23C081));

        assert_eq!(Some(0x038010), word_ram.main_cpu_map_address(0x23C100));

        assert_eq!(Some(0x03BFFE), word_ram.main_cpu_map_address(0x23DFFF));

        // V4-cell #2
        assert_eq!(Some(0x03C000), word_ram.main_cpu_map_address(0x23E000));
        assert_eq!(Some(0x03C002), word_ram.main_cpu_map_address(0x23E001));

        assert_eq!(Some(0x03C200), word_ram.main_cpu_map_address(0x23E004));
        assert_eq!(Some(0x03C202), word_ram.main_cpu_map_address(0x23E005));

        assert_eq!(Some(0x03C008), word_ram.main_cpu_map_address(0x23E080));
        assert_eq!(Some(0x03C00A), word_ram.main_cpu_map_address(0x23E081));

        assert_eq!(Some(0x03C010), word_ram.main_cpu_map_address(0x23E100));

        assert_eq!(Some(0x03FFFE), word_ram.main_cpu_map_address(0x23FFFF));

        // Test with main CPU having bank 1
        word_ram.bank_0_owner_1m = WordRamOwner::Sub;
        assert_eq!(Some(0x000001), word_ram.main_cpu_map_address(0x220000));
        assert_eq!(Some(0x03FFFF), word_ram.main_cpu_map_address(0x23FFFF));
    }

    #[test]
    fn word_ram_sub_cpu_2m_mapping() {
        use WordRamSubMapResult as R;

        let mut word_ram = WordRam::new();
        word_ram.mode = WordRamMode::TwoM;

        word_ram.owner_2m = WordRamOwner::Sub;
        assert_eq!(R::Byte(0x000000), word_ram.sub_cpu_map_address(0x080000));
        assert_eq!(R::Byte(0x03FFFF), word_ram.sub_cpu_map_address(0x0BFFFF));
        assert_eq!(R::None, word_ram.sub_cpu_map_address(0x0C0000));
        assert_eq!(R::None, word_ram.sub_cpu_map_address(0x0DFFFF));

        word_ram.owner_2m = WordRamOwner::Main;
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

        word_ram.bank_0_owner_1m = WordRamOwner::Sub;
        assert_eq!(R::Byte(0x000000), word_ram.sub_cpu_map_address(0x0C0000));
        assert_eq!(R::Byte(0x010000), word_ram.sub_cpu_map_address(0x0C8000));
        assert_eq!(R::Byte(0x03FFFE), word_ram.sub_cpu_map_address(0x0DFFFF));

        word_ram.bank_0_owner_1m = WordRamOwner::Main;
        assert_eq!(R::Byte(0x000001), word_ram.sub_cpu_map_address(0x0C0000));
        assert_eq!(R::Byte(0x010001), word_ram.sub_cpu_map_address(0x0C8000));
        assert_eq!(R::Byte(0x03FFFF), word_ram.sub_cpu_map_address(0x0DFFFF));
    }

    #[test]
    fn word_ram_sub_cpu_1m_pixel_mapping() {
        use WordRamSubMapResult as R;

        let mut word_ram = WordRam::new();
        word_ram.mode = WordRamMode::OneM;

        word_ram.bank_0_owner_1m = WordRamOwner::Sub;
        assert_eq!(R::Pixel(0x000000), word_ram.sub_cpu_map_address(0x080000));
        assert_eq!(R::Pixel(0x000001), word_ram.sub_cpu_map_address(0x080001));
        assert_eq!(R::Pixel(0x000004), word_ram.sub_cpu_map_address(0x080002));
        assert_eq!(R::Pixel(0x000005), word_ram.sub_cpu_map_address(0x080003));
        assert_eq!(R::Pixel(0x07FFFC), word_ram.sub_cpu_map_address(0x0BFFFE));
        assert_eq!(R::Pixel(0x07FFFD), word_ram.sub_cpu_map_address(0x0BFFFF));

        word_ram.bank_0_owner_1m = WordRamOwner::Main;
        assert_eq!(R::Pixel(0x000002), word_ram.sub_cpu_map_address(0x080000));
        assert_eq!(R::Pixel(0x000003), word_ram.sub_cpu_map_address(0x080001));
        assert_eq!(R::Pixel(0x000006), word_ram.sub_cpu_map_address(0x080002));
        assert_eq!(R::Pixel(0x000007), word_ram.sub_cpu_map_address(0x080003));
        assert_eq!(R::Pixel(0x07FFFE), word_ram.sub_cpu_map_address(0x0BFFFE));
        assert_eq!(R::Pixel(0x07FFFF), word_ram.sub_cpu_map_address(0x0BFFFF));
    }
}
