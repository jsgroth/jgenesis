use crate::bus::cartridge::mappers::{ChrType, NametableMirroring, PpuMapResult};
use crate::bus::cartridge::MapperImpl;
use crate::bus::PpuWriteToggle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mmc3PrgMode {
    Mode0,
    Mode1,
}

impl Mmc3PrgMode {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mmc3ChrMode {
    Mode0,
    Mode1,
}

impl Mmc3ChrMode {}

#[derive(Debug, Clone)]
struct Mmc3BankMapping {
    prg_mode: Mmc3PrgMode,
    chr_mode: Mmc3ChrMode,
    prg_rom_len: u32,
    chr_len: u32,
    prg_bank_0: u8,
    prg_bank_1: u8,
    chr_banks: [u8; 6],
}

impl Mmc3BankMapping {
    fn new(prg_rom_len: u32, chr_len: u32) -> Self {
        Self {
            prg_mode: Mmc3PrgMode::Mode0,
            chr_mode: Mmc3ChrMode::Mode0,
            prg_rom_len,
            chr_len,
            prg_bank_0: 0,
            prg_bank_1: 0,
            chr_banks: [0; 6],
        }
    }

    fn prg_bank_address(bank_number: u8, address: u16) -> u32 {
        u32::from(bank_number & 0x3F) * 8192 + u32::from(address & 0x1FFF)
    }

    fn chr_1kb_bank_address(bank_number: u8, address: u16) -> u32 {
        u32::from(bank_number) * 1024 + u32::from(address & 0x03FF)
    }

    fn chr_2kb_bank_address(bank_number: u8, address: u16) -> u32 {
        u32::from(bank_number & 0xFE) * 1024 + u32::from(address & 0x07FF)
    }

    fn map_prg_rom_address(&self, address: u16) -> u32 {
        match (self.prg_mode, address) {
            (_, 0x0000..=0x7FFF) => panic!("invalid MMC3 PRG ROM address: 0x{address:04X}"),
            (Mmc3PrgMode::Mode0, 0x8000..=0x9FFF) | (Mmc3PrgMode::Mode1, 0xC000..=0xDFFF) => {
                Self::prg_bank_address(self.prg_bank_0, address)
            }
            (_, 0xA000..=0xBFFF) => Self::prg_bank_address(self.prg_bank_1, address),
            (Mmc3PrgMode::Mode0, 0xC000..=0xDFFF) | (Mmc3PrgMode::Mode1, 0x8000..=0x9FFF) => {
                Self::prg_bank_address(((self.prg_rom_len >> 13) - 2) as u8, address)
            }
            (_, 0xE000..=0xFFFF) => {
                Self::prg_bank_address(((self.prg_rom_len >> 13) - 1) as u8, address)
            }
        }
    }

    fn map_pattern_table_address(&self, address: u16) -> u32 {
        let mapped_address = match (self.chr_mode, address) {
            (Mmc3ChrMode::Mode0, 0x0000..=0x07FF) | (Mmc3ChrMode::Mode1, 0x1000..=0x17FF) => {
                Self::chr_2kb_bank_address(self.chr_banks[0], address)
            }
            (Mmc3ChrMode::Mode0, 0x0800..=0x0FFF) | (Mmc3ChrMode::Mode1, 0x1800..=0x1FFF) => {
                Self::chr_2kb_bank_address(self.chr_banks[1], address)
            }
            (Mmc3ChrMode::Mode0, 0x1000..=0x13FF) | (Mmc3ChrMode::Mode1, 0x0000..=0x03FF) => {
                Self::chr_1kb_bank_address(self.chr_banks[2], address)
            }
            (Mmc3ChrMode::Mode0, 0x1400..=0x17FF) | (Mmc3ChrMode::Mode1, 0x0400..=0x07FF) => {
                Self::chr_1kb_bank_address(self.chr_banks[3], address)
            }
            (Mmc3ChrMode::Mode0, 0x1800..=0x1BFF) | (Mmc3ChrMode::Mode1, 0x0800..=0x0BFF) => {
                Self::chr_1kb_bank_address(self.chr_banks[4], address)
            }
            (Mmc3ChrMode::Mode0, 0x1C00..=0x1FFF) | (Mmc3ChrMode::Mode1, 0x0C00..=0x0FFF) => {
                Self::chr_1kb_bank_address(self.chr_banks[5], address)
            }
            (_, 0x2000..=0xFFFF) => {
                panic!("invalid MMC3 CHR pattern table address: 0x{address:04X}")
            }
        };
        mapped_address & (self.chr_len - 1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mmc3BankUpdate {
    PrgBank0,
    PrgBank1,
    ChrBank(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mmc3SubMapper {
    StandardMmc3,
    Mmc6,
}

impl Mmc3SubMapper {
    fn name(self) -> &'static str {
        match self {
            Self::StandardMmc3 => "MMC3",
            Self::Mmc6 => "MMC6",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mmc3RamMode {
    Mmc3Enabled,
    Mmc3WritesDisabled,
    Mmc6Enabled {
        first_half_reads: bool,
        first_half_writes: bool,
        second_half_reads: bool,
        second_half_writes: bool,
    },
    Disabled,
}

impl Mmc3RamMode {
    fn reads_enabled(self, address: u16) -> bool {
        match self {
            Self::Mmc3Enabled | Self::Mmc3WritesDisabled => true,
            Self::Mmc6Enabled {
                first_half_reads,
                second_half_reads,
                ..
            } => {
                if address & 0x0200 != 0 {
                    second_half_reads
                } else {
                    first_half_reads
                }
            }
            Self::Disabled => false,
        }
    }

    fn writes_enabled(self, address: u16) -> bool {
        match self {
            Self::Mmc3Enabled => true,
            Self::Mmc6Enabled {
                first_half_writes,
                second_half_writes,
                ..
            } => {
                if address & 0x0200 != 0 {
                    second_half_writes
                } else {
                    first_half_writes
                }
            }
            Self::Disabled | Self::Mmc3WritesDisabled => false,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Mmc3 {
    sub_mapper: Mmc3SubMapper,
    chr_type: ChrType,
    bank_mapping: Mmc3BankMapping,
    nametable_mirroring: NametableMirroring,
    bank_update_select: Mmc3BankUpdate,
    ram_mode: Mmc3RamMode,
    interrupt_flag: bool,
    irq_counter: u8,
    irq_reload_value: u8,
    irq_reload_flag: bool,
    irq_enabled: bool,
    last_a12_read: u16,
    a12_low_cycles: u32,
}

impl Mmc3 {
    pub(crate) fn new(
        chr_type: ChrType,
        prg_rom_len: u32,
        chr_size: u32,
        sub_mapper_number: u8,
    ) -> Self {
        let sub_mapper = match sub_mapper_number {
            1 => Mmc3SubMapper::Mmc6,
            _ => Mmc3SubMapper::StandardMmc3,
        };
        log::info!("MMC3 sub mapper: {}", sub_mapper.name());
        Self {
            sub_mapper,
            chr_type,
            bank_mapping: Mmc3BankMapping::new(prg_rom_len, chr_size),
            nametable_mirroring: NametableMirroring::Vertical,
            bank_update_select: Mmc3BankUpdate::ChrBank(0),
            ram_mode: Mmc3RamMode::Disabled,
            interrupt_flag: false,
            irq_counter: 0,
            irq_reload_value: 0,
            irq_reload_flag: false,
            irq_enabled: false,
            last_a12_read: 0,
            a12_low_cycles: 0,
        }
    }
}

impl MapperImpl<Mmc3> {
    pub(crate) fn read_cpu_address(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: 0x{address:04X}"),
            0x4020..=0x5FFF => 0xFF,
            0x6000..=0x7FFF => {
                if self.data.ram_mode.reads_enabled(address) && !self.cartridge.prg_ram.is_empty() {
                    let prg_ram_addr = address & (self.cartridge.prg_ram.len() as u16 - 1);
                    self.cartridge.prg_ram[prg_ram_addr as usize]
                } else {
                    0xFF
                }
            }
            0x8000..=0xFFFF => {
                self.cartridge.prg_rom[self.data.bank_mapping.map_prg_rom_address(address) as usize]
            }
        }
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: 0x{address:04X}"),
            0x4020..=0x5FFF => {}
            0x6000..=0x7FFF => {
                if self.data.ram_mode.writes_enabled(address) && !self.cartridge.prg_ram.is_empty()
                {
                    let prg_ram_addr = address & (self.cartridge.prg_ram.len() as u16 - 1);
                    self.cartridge.prg_ram[prg_ram_addr as usize] = value;
                }
            }
            0x8000..=0x9FFF => {
                if address & 0x01 == 0 {
                    self.data.bank_mapping.chr_mode = if value & 0x80 != 0 {
                        Mmc3ChrMode::Mode1
                    } else {
                        Mmc3ChrMode::Mode0
                    };
                    self.data.bank_mapping.prg_mode = if value & 0x40 != 0 {
                        Mmc3PrgMode::Mode1
                    } else {
                        Mmc3PrgMode::Mode0
                    };
                    self.data.bank_update_select = match value & 0x07 {
                        masked_value @ 0x00..=0x05 => Mmc3BankUpdate::ChrBank(masked_value),
                        0x06 => Mmc3BankUpdate::PrgBank0,
                        0x07 => Mmc3BankUpdate::PrgBank1,
                        _ => unreachable!(
                            "masking with 0x07 should always be in the range 0x00..=0x07"
                        ),
                    };

                    if self.data.sub_mapper == Mmc3SubMapper::Mmc6 {
                        let ram_enabled = value & 0x20 != 0;
                        if !ram_enabled {
                            self.data.ram_mode = Mmc3RamMode::Disabled;
                        } else if ram_enabled && self.data.ram_mode == Mmc3RamMode::Disabled {
                            self.data.ram_mode = Mmc3RamMode::Mmc6Enabled {
                                first_half_reads: false,
                                first_half_writes: false,
                                second_half_reads: false,
                                second_half_writes: false,
                            };
                        }
                    }
                } else {
                    match self.data.bank_update_select {
                        Mmc3BankUpdate::ChrBank(chr_bank) => {
                            self.data.bank_mapping.chr_banks[chr_bank as usize] = value;
                        }
                        Mmc3BankUpdate::PrgBank0 => {
                            self.data.bank_mapping.prg_bank_0 = value;
                        }
                        Mmc3BankUpdate::PrgBank1 => {
                            self.data.bank_mapping.prg_bank_1 = value;
                        }
                    }
                }
            }
            0xA000..=0xBFFF => {
                if address & 0x01 == 0 {
                    self.data.nametable_mirroring = if value & 0x01 != 0 {
                        NametableMirroring::Horizontal
                    } else {
                        NametableMirroring::Vertical
                    };
                } else {
                    match self.data.sub_mapper {
                        Mmc3SubMapper::Mmc6 => {
                            self.data.ram_mode = if self.data.ram_mode == Mmc3RamMode::Disabled {
                                // $A001 writes are ignored if RAM is disabled via $8000
                                Mmc3RamMode::Disabled
                            } else {
                                let first_half_writes = value & 0x10 != 0;
                                let first_half_reads = value & 0x20 != 0;
                                let second_half_writes = value & 0x40 != 0;
                                let second_half_reads = value & 0x80 != 0;
                                Mmc3RamMode::Mmc6Enabled {
                                    first_half_reads,
                                    first_half_writes,
                                    second_half_reads,
                                    second_half_writes,
                                }
                            };
                        }
                        Mmc3SubMapper::StandardMmc3 => {
                            self.data.ram_mode = if value & 0x80 == 0 {
                                Mmc3RamMode::Disabled
                            } else if value & 0x40 != 0 {
                                Mmc3RamMode::Mmc3WritesDisabled
                            } else {
                                Mmc3RamMode::Mmc3Enabled
                            };
                        }
                    }
                }
            }
            0xC000..=0xDFFF => {
                if address & 0x01 == 0 {
                    self.data.irq_reload_value = value;
                } else {
                    self.data.irq_reload_flag = true;
                }
            }
            0xE000..=0xFFFF => {
                if address & 0x01 == 0 {
                    self.data.irq_enabled = false;
                    self.data.interrupt_flag = false;
                } else {
                    self.data.irq_enabled = true;
                }
            }
        }
    }

    fn clock_irq(&mut self) {
        if self.data.irq_counter == 0 || self.data.irq_reload_flag {
            self.data.irq_counter = self.data.irq_reload_value;
            self.data.irq_reload_flag = false;
        } else {
            self.data.irq_counter -= 1;
        }

        if self.data.irq_counter == 0 && self.data.irq_enabled {
            self.data.interrupt_flag = true;
        }
    }

    fn process_ppu_address(&mut self, address: u16) {
        let a12 = address & (1 << 12);
        if a12 != 0 && self.data.last_a12_read == 0 && self.data.a12_low_cycles >= 10 {
            self.clock_irq();
        }
        self.data.last_a12_read = a12;
    }

    fn map_ppu_address(&mut self, address: u16) -> PpuMapResult {
        self.process_ppu_address(address);

        match address & 0x3FFF {
            0x0000..=0x1FFF => self
                .data
                .chr_type
                .to_map_result(self.data.bank_mapping.map_pattern_table_address(address)),
            0x2000..=0x3EFF => {
                PpuMapResult::Vram(self.data.nametable_mirroring.map_to_vram(address))
            }
            _ => panic!("invalid PPU map address: 0x{address:04X}"),
        }
    }

    pub(crate) fn read_ppu_address(&mut self, address: u16, vram: &[u8; 2048]) -> u8 {
        self.map_ppu_address(address).read(&self.cartridge, vram)
    }

    pub(crate) fn write_ppu_address(&mut self, address: u16, value: u8, vram: &mut [u8; 2048]) {
        self.map_ppu_address(address)
            .write(value, &mut self.cartridge, vram);
    }

    pub(crate) fn interrupt_flag(&self) -> bool {
        self.data.interrupt_flag
    }

    pub(crate) fn tick(&mut self) {
        if self.data.last_a12_read == 0 {
            self.data.a12_low_cycles += 1;
        } else {
            self.data.a12_low_cycles = 0;
        }
    }

    pub(crate) fn process_ppu_addr_update(&mut self, value: u8, write_toggle: PpuWriteToggle) {
        if write_toggle == PpuWriteToggle::First {
            // This mapper only cares about bit 12
            self.process_ppu_address(u16::from(value) << 8);
        }
    }

    pub(crate) fn process_ppu_addr_increment(&mut self, new_ppu_addr: u16) {
        self.process_ppu_address(new_ppu_addr);
    }
}
