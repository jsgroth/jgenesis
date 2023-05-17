use crate::bus;
use crate::bus::cartridge::mappers::{BankSizeKb, ChrType, PpuMapResult};
use crate::bus::cartridge::MapperImpl;
use crate::num::GetBit;
use bincode::{Decode, Encode};

#[derive(Debug, Clone, Encode, Decode)]
struct IrqCounter {
    enabled: bool,
    counter: u16,
}

// 15-bit counter
const MAX_IRQ_COUNTER: u16 = 0x7FFF;

impl IrqCounter {
    fn new() -> Self {
        Self {
            enabled: false,
            counter: 0,
        }
    }

    fn get_counter_low_bits(&self) -> u8 {
        self.counter as u8
    }

    fn get_counter_high_bits(&self) -> u8 {
        (u8::from(self.enabled) << 7) | ((self.counter >> 8) as u8)
    }

    fn update_counter_low_bits(&mut self, value: u8) {
        self.counter = (self.counter & 0xFF00) | u16::from(value);
    }

    fn update_counter_high_bits(&mut self, value: u8) {
        self.enabled = value.bit(7);
        self.counter = (self.counter & 0x00FF) | (u16::from(value & 0x7F) << 8);
    }

    fn tick_cpu(&mut self) {
        if self.enabled && self.counter < MAX_IRQ_COUNTER {
            self.counter += 1;
        }
    }

    fn interrupt_flag(&self) -> bool {
        self.enabled && self.counter == MAX_IRQ_COUNTER
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct Namco163Audio {
    enabled: bool,
}

impl Namco163Audio {
    fn new() -> Self {
        Self { enabled: false }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct Namco163 {
    chr_type: ChrType,
    internal_ram: [u8; 128],
    internal_ram_addr: u8,
    internal_ram_auto_increment: bool,
    internal_ram_dirty_bit: bool,
    prg_banks: [u8; 3],
    pattern_table_chr_banks: [u8; 8],
    nametable_chr_banks: [u8; 4],
    vram_chr_banks_enabled: [bool; 2],
    ram_writes_enabled: bool,
    ram_window_writes_enabled: [bool; 4],
    irq: IrqCounter,
    audio: Namco163Audio,
}

impl Namco163 {
    pub(crate) fn new(
        _sub_mapper_number: u8,
        chr_type: ChrType,
        has_battery: bool,
        prg_ram_len: u32,
        sav_bytes: Option<Vec<u8>>,
    ) -> Self {
        let mut internal_ram = [0; 128];

        if has_battery && prg_ram_len == 0 {
            if let Some(sav_bytes) = sav_bytes {
                if sav_bytes.len() == internal_ram.len() {
                    internal_ram.copy_from_slice(&sav_bytes);
                }
            }
        }

        Self {
            chr_type,
            internal_ram,
            internal_ram_addr: 0,
            internal_ram_auto_increment: false,
            internal_ram_dirty_bit: true,
            prg_banks: [0; 3],
            pattern_table_chr_banks: [0; 8],
            nametable_chr_banks: [0; 4],
            vram_chr_banks_enabled: [false; 2],
            ram_writes_enabled: false,
            ram_window_writes_enabled: [false; 4],
            irq: IrqCounter::new(),
            audio: Namco163Audio::new(),
        }
    }
}

impl MapperImpl<Namco163> {
    pub(crate) fn read_cpu_address(&mut self, address: u16) -> u8 {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: {address:04X}"),
            0x4020..=0x47FF => bus::cpu_open_bus(address),
            0x4800..=0x4FFF => {
                let byte = self.data.internal_ram[self.data.internal_ram_addr as usize];
                if self.data.internal_ram_auto_increment {
                    self.data.internal_ram_addr = (self.data.internal_ram_addr + 1) & 0x7F;
                }
                byte
            }
            0x5000..=0x57FF => self.data.irq.get_counter_low_bits(),
            0x5800..=0x5FFF => self.data.irq.get_counter_high_bits(),
            0x6000..=0x7FFF => {
                if !self.cartridge.prg_ram.is_empty() {
                    self.cartridge.get_prg_ram((address & 0x1FFF).into())
                } else {
                    bus::cpu_open_bus(address)
                }
            }
            0x8000..=0xDFFF => {
                // $8000-$9FFF to bank index 0
                // $A000-$BFFF to bank index 1
                // $C000-$DFFF to bank index 2
                let bank_index = (address & 0x7FFF) / 0x2000;
                let bank_number = self.data.prg_banks[bank_index as usize];
                let prg_rom_addr = BankSizeKb::Eight.to_absolute_address(bank_number, address);
                self.cartridge.get_prg_rom(prg_rom_addr)
            }
            0xE000..=0xFFFF => {
                let prg_rom_addr = BankSizeKb::Eight
                    .to_absolute_address_last_bank(self.cartridge.prg_rom.len() as u32, address);
                self.cartridge.get_prg_rom(prg_rom_addr)
            }
        }
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: {address:04X}"),
            0x4020..=0x47FF => {}
            0x4800..=0x4FFF => {
                if self.data.ram_writes_enabled {
                    self.data.internal_ram[self.data.internal_ram_addr as usize] = value;
                    self.data.internal_ram_dirty_bit = true;

                    if self.data.internal_ram_auto_increment {
                        self.data.internal_ram_addr = (self.data.internal_ram_addr + 1) & 0x7F;
                    }
                }
            }
            0x5000..=0x57FF => {
                self.data.irq.update_counter_low_bits(value);
            }
            0x5800..=0x5FFF => {
                self.data.irq.update_counter_high_bits(value);
            }
            0x6000..=0x7FFF => {
                if !self.cartridge.prg_ram.is_empty() && self.data.ram_writes_enabled {
                    let prg_ram_addr = address & 0x1FFF;
                    let window_index = prg_ram_addr / 0x0800;
                    if self.data.ram_window_writes_enabled[window_index as usize] {
                        self.cartridge.set_prg_ram(prg_ram_addr.into(), value);
                    }
                }
            }
            0x8000..=0xBFFF => {
                let bank_index = (address & 0x7FFF) / 0x0800;
                self.data.pattern_table_chr_banks[bank_index as usize] = value;
            }
            0xC000..=0xDFFF => {
                let bank_index = (address & 0x3FFF) / 0x0800;
                self.data.nametable_chr_banks[bank_index as usize] = value;
            }
            0xE000..=0xE7FF => {
                self.data.audio.enabled = !value.bit(6);
                self.data.prg_banks[0] = value & 0x3F;
            }
            0xE800..=0xEFFF => {
                self.data.vram_chr_banks_enabled[1] = !value.bit(7);
                self.data.vram_chr_banks_enabled[0] = !value.bit(6);
                self.data.prg_banks[1] = value & 0x3F;
            }
            0xF000..=0xF7FF => {
                self.data.prg_banks[2] = value & 0x3F;
            }
            0xF800..=0xFFFF => {
                self.data.ram_writes_enabled = value & 0xF0 == 0x40;
                for bit in 0..3 {
                    self.data.ram_window_writes_enabled[bit as usize] = !value.bit(bit);
                }
            }
        }
    }

    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        match address {
            0x0000..=0x1FFF => {
                let bank_index = address / 0x0400;
                let bank_number = self.data.pattern_table_chr_banks[bank_index as usize];
                let pattern_table_index = address / 0x1000;
                if bank_number >= 0xE0
                    && self.data.vram_chr_banks_enabled[pattern_table_index as usize]
                {
                    let vram_bank = u16::from(bank_number & 0x01);
                    PpuMapResult::Vram((vram_bank * 0x0400) | (address & 0x03FF))
                } else {
                    let chr_addr = BankSizeKb::One.to_absolute_address(bank_number, address);
                    self.data.chr_type.to_map_result(chr_addr)
                }
            }
            0x2000..=0x3EFF => {
                let relative_addr = address & 0x0FFF;
                let bank_index = relative_addr / 0x0400;
                let bank_number = self.data.nametable_chr_banks[bank_index as usize];
                if bank_number >= 0xE0 {
                    let vram_bank = u16::from(bank_number & 0x01);
                    PpuMapResult::Vram((vram_bank * 0x0400) | (address & 0x03FF))
                } else {
                    let chr_addr = BankSizeKb::One.to_absolute_address(bank_number, address);
                    self.data.chr_type.to_map_result(chr_addr)
                }
            }
            0x3F00..=0xFFFF => {
                panic!("invalid PPU map address: {address:04X}")
            }
        }
    }

    pub(crate) fn read_ppu_address(&self, address: u16, vram: &[u8; 2048]) -> u8 {
        self.map_ppu_address(address).read(&self.cartridge, vram)
    }

    pub(crate) fn write_ppu_address(&mut self, address: u16, value: u8, vram: &mut [u8; 2048]) {
        self.map_ppu_address(address)
            .write(value, &mut self.cartridge, vram);
    }

    pub(crate) fn tick_cpu(&mut self) {
        self.data.irq.tick_cpu();
    }

    pub(crate) fn interrupt_flag(&self) -> bool {
        self.data.irq.interrupt_flag()
    }

    pub(crate) fn has_battery_backed_internal_ram(&self) -> bool {
        self.cartridge.has_ram_battery && self.cartridge.prg_ram.is_empty()
    }

    pub(crate) fn get_and_clear_internal_ram_dirty_bit(&mut self) -> bool {
        let dirty_bit = self.data.internal_ram_dirty_bit;
        self.data.internal_ram_dirty_bit = false;
        dirty_bit
    }

    pub(crate) fn get_internal_ram(&self) -> &[u8; 128] {
        &self.data.internal_ram
    }
}
