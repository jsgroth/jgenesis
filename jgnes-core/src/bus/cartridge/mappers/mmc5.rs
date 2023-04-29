use crate::bus::cartridge::mappers::CpuMapResult;
use crate::bus::cartridge::MapperImpl;

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrgBankSize {
    EightKb,
    SixteenKb,
    ThirtyTwoKb,
}

impl PrgBankSize {
    fn bank_number_mask(self) -> u8 {
        match self {
            Self::EightKb => 0xFF,
            Self::SixteenKb => 0xFE,
            Self::ThirtyTwoKb => 0xFC,
        }
    }

    fn address_mask(self) -> u16 {
        match self {
            Self::EightKb => 0x1FFF,
            Self::SixteenKb => 0x3FFF,
            Self::ThirtyTwoKb => 0x7FFF,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrgBankingMode {
    Mode0,
    Mode1,
    Mode2,
    Mode3,
}

impl PrgBankingMode {
    fn map_result(bank_number: u8, bank_size: PrgBankSize, address: u16) -> CpuMapResult {
        let is_rom = bank_number & 0x80 != 0;

        let masked_bank_number = if is_rom {
            bank_number & 0x7F
        } else {
            bank_number & 0x0F
        };
        let masked_bank_number = masked_bank_number & bank_size.bank_number_mask();

        let masked_address = address & bank_size.address_mask();

        let mapped_address = (u32::from(masked_bank_number) << 13) | u32::from(masked_address);

        if is_rom {
            CpuMapResult::PrgROM(mapped_address)
        } else {
            CpuMapResult::PrgRAM(mapped_address)
        }
    }

    fn map_prg_address(self, prg_bank_registers: [u8; 5], address: u16) -> CpuMapResult {
        match address {
            0x0000..=0x5FFF => panic!("invalid MMC5 PRG map address: {address:04X}"),
            0x6000..=0x7FFF => {
                Self::map_result(prg_bank_registers[0] & 0x7F, PrgBankSize::EightKb, address)
            }
            0x8000..=0xFFFF => match self {
                // 1x32KB
                Self::Mode0 => Self::map_result(
                    prg_bank_registers[4] | 0x80,
                    PrgBankSize::ThirtyTwoKb,
                    address,
                ),
                // 2x16KB
                Self::Mode1 => match address {
                    0x0000..=0x7FFF => unreachable!("nested match expressions"),
                    0x8000..=0xBFFF => {
                        Self::map_result(prg_bank_registers[2], PrgBankSize::SixteenKb, address)
                    }
                    0xC000..=0xFFFF => Self::map_result(
                        prg_bank_registers[4] | 0x80,
                        PrgBankSize::SixteenKb,
                        address,
                    ),
                },
                // 1x16KB + 2x8KB
                Self::Mode2 => match address {
                    0x0000..=0x7FFF => unreachable!("nested match expressions"),
                    0x8000..=0xBFFF => {
                        Self::map_result(prg_bank_registers[2], PrgBankSize::SixteenKb, address)
                    }
                    0xC000..=0xDFFF => {
                        Self::map_result(prg_bank_registers[3], PrgBankSize::EightKb, address)
                    }
                    0xE000..=0xFFFF => Self::map_result(
                        prg_bank_registers[4] | 0x80,
                        PrgBankSize::EightKb,
                        address,
                    ),
                },
                // 4x8KB
                Self::Mode3 => match address {
                    0x0000..=0x7FFF => unreachable!("nested match expressions"),
                    0x8000..=0x9FFF => {
                        Self::map_result(prg_bank_registers[1], PrgBankSize::EightKb, address)
                    }
                    0xA000..=0xBFFF => {
                        Self::map_result(prg_bank_registers[2], PrgBankSize::EightKb, address)
                    }
                    0xC000..=0xDFFF => {
                        Self::map_result(prg_bank_registers[3], PrgBankSize::EightKb, address)
                    }
                    0xE000..=0xFFFF => Self::map_result(
                        prg_bank_registers[4] | 0x80,
                        PrgBankSize::EightKb,
                        address,
                    ),
                },
            },
        }
    }
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChrBankingMode {
    EightKb,
    FourKb,
    TwoKb,
    OneKb,
}

impl ChrBankingMode {
    fn bank_address(self, bank_number: u8) -> u32 {
        match self {
            Self::EightKb => u32::from(bank_number) << 13,
            Self::FourKb => u32::from(bank_number) << 12,
            Self::TwoKb => u32::from(bank_number) << 11,
            Self::OneKb => u32::from(bank_number) << 10,
        }
    }

    fn address_mask(self) -> u16 {
        match self {
            Self::EightKb => 0x1FFF,
            Self::FourKb => 0x0FFF,
            Self::TwoKb => 0x07FF,
            Self::OneKb => 0x03FF,
        }
    }

    fn map_address(self, bank_number: u8, address: u16) -> u32 {
        let bank_address = self.bank_address(bank_number);
        let masked_address = u32::from(address & self.address_mask());
        bank_address | masked_address
    }
}

#[derive(Debug, Clone)]
struct ChrMapper {
    mode: ChrBankingMode,
    bank_registers: [u8; 12],
    double_height_sprites: bool,
    last_register_written: usize,
    next_access_from_ppu_data: bool,
}

impl ChrMapper {
    fn new() -> Self {
        Self {
            mode: ChrBankingMode::EightKb,
            bank_registers: [0; 12],
            double_height_sprites: false,
            last_register_written: 0,
            next_access_from_ppu_data: false,
        }
    }

    fn map_sprite_chr_address(&self, address: u16) -> u32 {
        match self.mode {
            ChrBankingMode::EightKb => {
                ChrBankingMode::EightKb.map_address(self.bank_registers[7], address)
            }
            ChrBankingMode::FourKb => match address {
                0x0000..=0x0FFF => {
                    ChrBankingMode::FourKb.map_address(self.bank_registers[3], address)
                }
                0x1000..=0x1FFF => {
                    ChrBankingMode::FourKb.map_address(self.bank_registers[7], address)
                }
                0x2000..=0xFFFF => panic!("invalid MMC5 CHR map address: {address:04X}"),
            },
            ChrBankingMode::TwoKb => match address {
                0x0000..=0x07FF => {
                    ChrBankingMode::TwoKb.map_address(self.bank_registers[1], address)
                }
                0x0800..=0x0FFF => {
                    ChrBankingMode::TwoKb.map_address(self.bank_registers[3], address)
                }
                0x1000..=0x17FF => {
                    ChrBankingMode::TwoKb.map_address(self.bank_registers[5], address)
                }
                0x1800..=0x1FFF => {
                    ChrBankingMode::TwoKb.map_address(self.bank_registers[7], address)
                }
                0x2000..=0xFFFF => panic!("invalid MMC5 CHR map address {address:04X}"),
            },
            ChrBankingMode::OneKb => match address {
                0x0000..=0x03FF => {
                    ChrBankingMode::OneKb.map_address(self.bank_registers[0], address)
                }
                0x0400..=0x07FF => {
                    ChrBankingMode::OneKb.map_address(self.bank_registers[1], address)
                }
                0x0800..=0x0BFF => {
                    ChrBankingMode::OneKb.map_address(self.bank_registers[2], address)
                }
                0x0C00..=0x0FFF => {
                    ChrBankingMode::OneKb.map_address(self.bank_registers[3], address)
                }
                0x1000..=0x13FF => {
                    ChrBankingMode::OneKb.map_address(self.bank_registers[4], address)
                }
                0x1400..=0x17FF => {
                    ChrBankingMode::OneKb.map_address(self.bank_registers[5], address)
                }
                0x1800..=0x1BFF => {
                    ChrBankingMode::OneKb.map_address(self.bank_registers[6], address)
                }
                0x1C00..=0x1FFF => {
                    ChrBankingMode::OneKb.map_address(self.bank_registers[7], address)
                }
                0x2000..=0xFFFF => panic!("invalid MMC5 CHR map address: {address:04X}"),
            },
        }
    }

    fn map_bg_chr_address(&self, address: u16) -> u32 {
        match self.mode {
            ChrBankingMode::EightKb => {
                ChrBankingMode::EightKb.map_address(self.bank_registers[11], address)
            }
            ChrBankingMode::FourKb => {
                ChrBankingMode::FourKb.map_address(self.bank_registers[11], address)
            }
            ChrBankingMode::TwoKb => match address {
                0x0000..=0x07FF | 0x1000..=0x17FF => {
                    ChrBankingMode::TwoKb.map_address(self.bank_registers[9], address)
                }
                0x0800..=0x0FFF | 0x1800..=0x1FFF => {
                    ChrBankingMode::TwoKb.map_address(self.bank_registers[11], address)
                }
                0x2000..=0xFFFF => panic!("invalid MMC5 CHR map address: {address:04X}"),
            },
            ChrBankingMode::OneKb => match address {
                0x0000..=0x03FF | 0x1000..=0x13FF => {
                    ChrBankingMode::OneKb.map_address(self.bank_registers[8], address)
                }
                0x0400..=0x07FF | 0x1400..=0x17FF => {
                    ChrBankingMode::OneKb.map_address(self.bank_registers[9], address)
                }
                0x0800..=0x0BFF | 0x1800..=0x1BFF => {
                    ChrBankingMode::OneKb.map_address(self.bank_registers[10], address)
                }
                0x0C00..=0x0FFF | 0x1C00..=0x1FFF => {
                    ChrBankingMode::OneKb.map_address(self.bank_registers[11], address)
                }
                0x2000..=0xFFFF => panic!("invalid MMC5 CHR map address: {address:04X}"),
            },
        }
    }

    fn map_chr_address(&mut self, address: u16, tile_type: TileType) -> u32 {
        assert!((0x0000..=0x1FFF).contains(&address));

        if self.next_access_from_ppu_data {
            self.next_access_from_ppu_data = false;

            if self.last_register_written < 8 {
                self.map_sprite_chr_address(address)
            } else {
                self.map_bg_chr_address(address)
            }
        } else if self.double_height_sprites && tile_type == TileType::Background {
            self.map_bg_chr_address(address)
        } else {
            self.map_sprite_chr_address(address)
        }
    }

    fn process_ppu_ctrl_update(&mut self, ppu_ctrl_value: u8) {
        self.double_height_sprites = ppu_ctrl_value & 0x20 != 0;
    }

    fn process_bank_register_update(&mut self, address: u16, value: u8) {
        assert!((0x5120..=0x512B).contains(&address));

        let register_index = (address - 0x5120) as usize;
        self.bank_registers[register_index] = value;
        self.last_register_written = register_index;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtendedRamMode {
    Nametable,
    NametableExtendedAttributes,
    ReadWrite,
    ReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NametableMapping {
    VramPage0,
    VramPage1,
    ExtendedRam,
    FillMode,
}

impl NametableMapping {
    fn from_bits(bits: u8) -> Self {
        match bits {
            0x00 => Self::VramPage0,
            0x01 => Self::VramPage1,
            0x02 => Self::ExtendedRam,
            0x03 => Self::FillMode,
            _ => panic!("invalid nametable mapping bits: {bits:02X}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VerticalSplitMode {
    Left,
    Right,
}

#[derive(Debug, Clone)]
struct VerticalSplit {
    enabled: bool,
    mode: VerticalSplitMode,
    split_tile_index: u8,
    y_scroll: u8,
    chr_bank: u8,
}

impl VerticalSplit {
    fn new() -> Self {
        Self {
            enabled: false,
            mode: VerticalSplitMode::Left,
            split_tile_index: 0,
            y_scroll: 0,
            chr_bank: 0,
        }
    }

    fn inside_split(&self, scanline_counter: &ScanlineCounter) -> bool {
        if !self.enabled {
            return false;
        }

        match self.mode {
            VerticalSplitMode::Left => {
                self.split_tile_index < scanline_counter.current_tile_index()
            }
            VerticalSplitMode::Right => {
                self.split_tile_index >= scanline_counter.current_tile_index()
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TileType {
    Background,
    Sprite,
}

#[derive(Debug, Clone)]
struct ScanlineCounter {
    scanline: u8,
    scanline_tile_byte_fetches: u8,
    compare_value: u8,
    irq_enabled: bool,
    irq_pending: bool,
    in_frame: bool,
    last_nametable_address: u16,
    same_nametable_addr_fetch_count: u8,
    cpu_ticks_no_read: u32,
}

impl ScanlineCounter {
    fn new() -> Self {
        Self {
            scanline: 0,
            scanline_tile_byte_fetches: 0,
            compare_value: 0,
            irq_enabled: false,
            irq_pending: false,
            in_frame: false,
            last_nametable_address: 0,
            same_nametable_addr_fetch_count: 0,
            cpu_ticks_no_read: 0,
        }
    }

    // Should be called immediately before a PPU memory access; checks if the scanline has changed
    fn pre_fetch(&mut self) {
        if self.same_nametable_addr_fetch_count == 3 {
            self.same_nametable_addr_fetch_count = 0;

            if self.in_frame {
                log::trace!(
                    "Detected new scanline; nametable address = {:04X}",
                    self.last_nametable_address
                );

                self.scanline += 1;

                if self.scanline == 241 {
                    log::trace!("Reached VBlank scanline, resetting state");
                    self.scanline = 0;
                    self.irq_pending = false;
                    self.in_frame = false;
                } else if self.compare_value != 0 && self.scanline == self.compare_value {
                    log::trace!("Setting IRQ pending flag");
                    self.irq_pending = true;
                }
            } else {
                log::trace!("Detected new frame");
                self.scanline = 0;
                self.in_frame = true;
            }
        }
    }

    // This should be called *after* mapping the tile address in case the increment changes the
    // current tile type
    fn increment_tile_bytes_fetched(&mut self) {
        log::trace!(
            "Tile byte fetched, current fetches={}",
            self.scanline_tile_byte_fetches
        );

        self.cpu_ticks_no_read = 0;

        self.scanline_tile_byte_fetches += 1;

        // 68 BG tile bytes + 8 sprite tiles * 2 pattern table bytes per tile
        if self.scanline_tile_byte_fetches == 68 + 16 {
            self.scanline_tile_byte_fetches = 0;
        }
    }

    fn nametable_address_fetched(&mut self, address: u16) {
        assert!((0x2000..=0x2FFF).contains(&address));

        self.cpu_ticks_no_read = 0;

        if self.last_nametable_address == address && self.same_nametable_addr_fetch_count < 3 {
            self.same_nametable_addr_fetch_count += 1;
        } else if self.last_nametable_address != address {
            self.last_nametable_address = address;
            self.same_nametable_addr_fetch_count = 1;
        }
    }

    fn nmi_vector_fetched(&mut self) {
        self.scanline = 0;
        self.irq_pending = false;
        self.in_frame = false;
    }

    fn current_tile_type(&self) -> TileType {
        // 34 BG tiles * 2 pattern table bytes per tile
        let tile_type = if self.scanline_tile_byte_fetches < 68 {
            TileType::Background
        } else {
            TileType::Sprite
        };

        log::trace!("current tile type is {tile_type:?}");

        tile_type
    }

    fn current_tile_index(&self) -> u8 {
        self.scanline_tile_byte_fetches / 2
    }

    fn interrupt_flag(&self) -> bool {
        self.irq_enabled && self.irq_pending
    }

    fn tick_cpu(&mut self) {
        if self.cpu_ticks_no_read == 3 {
            log::trace!("Went 3 CPU cycles with no PPU reads, clearing in frame flag");
            self.in_frame = false;
            // Set to 4 so that the counter increments correctly starting from the pre-render scanline
            // 2 tiles * 2 bytes per tile
            self.scanline_tile_byte_fetches = 4;
        }

        self.cpu_ticks_no_read += 1;
    }
}

#[derive(Debug, Clone)]
struct ExtendedAttributesState {
    last_nametable_addr: u16,
}

impl ExtendedAttributesState {
    fn new() -> Self {
        Self {
            last_nametable_addr: 0,
        }
    }

    fn get_attribute_byte(&self, extended_ram: &[u8; 1024]) -> u8 {
        let extended_attributes = extended_ram[(self.last_nametable_addr & 0x03FF) as usize];
        let palette_index = extended_attributes >> 6;
        (palette_index << 6) | (palette_index << 4) | (palette_index << 2) | palette_index
    }

    fn get_pattern_table_byte(
        &self,
        pattern_table_addr: u16,
        extended_ram: &[u8; 1024],
        chr_rom: &[u8],
    ) -> u8 {
        let extended_attributes = extended_ram[(self.last_nametable_addr & 0x03FF) as usize];
        let chr_4kb_bank = extended_attributes & 0x3F;
        let chr_address = (u32::from(chr_4kb_bank) << 12) | u32::from(pattern_table_addr & 0x0FFF);
        chr_rom[chr_address as usize]
    }
}

#[derive(Debug, Clone, Copy)]
struct MultiplierUnit {
    operand_l: u16,
    operand_r: u16,
}

impl MultiplierUnit {
    fn new() -> Self {
        Self {
            operand_l: 0xFF,
            operand_r: 0xFF,
        }
    }

    fn output(self) -> u16 {
        self.operand_l * self.operand_r
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Mmc5 {
    extended_ram: [u8; 1024],
    extended_ram_mode: ExtendedRamMode,
    prg_banking_mode: PrgBankingMode,
    prg_bank_registers: [u8; 5],
    chr_mapper: ChrMapper,
    nametable_mappings: [NametableMapping; 4],
    fill_mode_tile_data: u8,
    fill_mode_attributes: u8,
    vertical_split: VerticalSplit,
    scanline_counter: ScanlineCounter,
    extended_attributes_state: ExtendedAttributesState,
    multiplier: MultiplierUnit,
    ram_writes_enabled_1: bool,
    ram_writes_enabled_2: bool,
}

impl Mmc5 {
    pub(crate) fn new() -> Self {
        Self {
            extended_ram: [0; 1024],
            extended_ram_mode: ExtendedRamMode::ReadOnly,
            prg_banking_mode: PrgBankingMode::Mode3,
            prg_bank_registers: [0xFF; 5],
            chr_mapper: ChrMapper::new(),
            nametable_mappings: [NametableMapping::VramPage0; 4],
            fill_mode_tile_data: 0,
            fill_mode_attributes: 0,
            vertical_split: VerticalSplit::new(),
            scanline_counter: ScanlineCounter::new(),
            extended_attributes_state: ExtendedAttributesState::new(),
            multiplier: MultiplierUnit::new(),
            ram_writes_enabled_1: false,
            ram_writes_enabled_2: false,
        }
    }
}

impl MapperImpl<Mmc5> {
    pub(crate) fn process_ppu_ctrl_update(&mut self, value: u8) {
        self.data.chr_mapper.process_ppu_ctrl_update(value);
    }

    pub(crate) fn about_to_access_ppu_data(&mut self) {
        self.data.chr_mapper.next_access_from_ppu_data = true;
    }

    fn read_internal_register(&mut self, address: u16) -> u8 {
        match address {
            0x5204 => {
                log::trace!("Scanline IRQ status register read, clearing IRQ pending flag");

                let result = (u8::from(self.data.scanline_counter.irq_pending) << 7)
                    | (u8::from(self.data.scanline_counter.in_frame) << 6);
                self.data.scanline_counter.irq_pending = false;
                result
            }
            0x5205 => (self.data.multiplier.output() & 0x00FF) as u8,
            0x5206 => (self.data.multiplier.output() >> 8) as u8,
            _ => 0xFF,
        }
    }

    fn write_internal_register(&mut self, address: u16, value: u8) {
        match address {
            0x5100 => {
                self.data.prg_banking_mode = match value & 0x03 {
                    0x00 => PrgBankingMode::Mode0,
                    0x01 => PrgBankingMode::Mode1,
                    0x02 => PrgBankingMode::Mode2,
                    0x03 => PrgBankingMode::Mode3,
                    _ => unreachable!("value & 0x03 should always be 0x00/0x01/0x02/0x03"),
                };
                log::trace!("PRG banking mode set to {:?}", self.data.prg_banking_mode);
            }
            0x5101 => {
                self.data.chr_mapper.mode = match value & 0x03 {
                    0x00 => ChrBankingMode::EightKb,
                    0x01 => ChrBankingMode::FourKb,
                    0x02 => ChrBankingMode::TwoKb,
                    0x03 => ChrBankingMode::OneKb,
                    _ => unreachable!("value & 0x03 should always be 0x00/0x01/0x02/0x03"),
                };
                log::trace!("CHR banking mode set to {:?}", self.data.chr_mapper.mode);
            }
            0x5102 => {
                self.data.ram_writes_enabled_1 = value & 0x03 == 0x02;
            }
            0x5103 => {
                self.data.ram_writes_enabled_2 = value & 0x03 == 0x01;
            }
            0x5104 => {
                self.data.extended_ram_mode = match value & 0x03 {
                    0x00 => ExtendedRamMode::Nametable,
                    0x01 => ExtendedRamMode::NametableExtendedAttributes,
                    0x02 => ExtendedRamMode::ReadWrite,
                    0x03 => ExtendedRamMode::ReadOnly,
                    _ => unreachable!("value & 0x03 should be 0x00/0x01/0x02/0x03"),
                };
                log::trace!("Extended RAM mode set to {:?}", self.data.extended_ram_mode);
            }
            0x5105 => {
                self.data.nametable_mappings[0] = NametableMapping::from_bits(value & 0x03);
                self.data.nametable_mappings[1] = NametableMapping::from_bits((value >> 2) & 0x03);
                self.data.nametable_mappings[2] = NametableMapping::from_bits((value >> 4) & 0x03);
                self.data.nametable_mappings[3] = NametableMapping::from_bits((value >> 6) & 0x03);
                log::trace!(
                    "Nametable mappings set to {:?}",
                    self.data.nametable_mappings
                );
            }
            0x5106 => {
                self.data.fill_mode_tile_data = value;
                log::trace!("Fill mode tile set to {value:02X}");
            }
            0x5107 => {
                let palette_index = value & 0x03;
                self.data.fill_mode_attributes = palette_index
                    | (palette_index << 2)
                    | (palette_index << 4)
                    | (palette_index << 6);
                log::trace!("Fill mode palette index set to {value:02X}");
            }
            0x5113..=0x5117 => {
                self.data.prg_bank_registers[(address - 0x5113) as usize] = value;
            }
            0x5120..=0x512B => {
                self.data
                    .chr_mapper
                    .process_bank_register_update(address, value);
            }
            0x5200 => {
                self.data.vertical_split.enabled = value & 0x80 != 0;
                self.data.vertical_split.mode = if value & 0x40 != 0 {
                    VerticalSplitMode::Right
                } else {
                    VerticalSplitMode::Left
                };
                self.data.vertical_split.split_tile_index = value & 0x1F;
                log::trace!(
                    "Vertical split enabled/mode/index set: {:?}",
                    self.data.vertical_split
                );
            }
            0x5201 => {
                self.data.vertical_split.y_scroll = value;
                log::trace!("Vertical split Y scroll set to {value}");
            }
            0x5202 => {
                self.data.vertical_split.chr_bank = value;
                log::trace!("Vertical split CHR bank set to {value:02X}");
            }
            0x5203 => {
                self.data.scanline_counter.compare_value = value;
                log::trace!("Scanline counter compare value set to {value}");
            }
            0x5204 => {
                self.data.scanline_counter.irq_enabled = value & 0x80 != 0;
                log::trace!(
                    "Scanline IRQ enabled set to {}",
                    self.data.scanline_counter.irq_enabled
                );
            }
            0x5205 => {
                self.data.multiplier.operand_l = value.into();
            }
            0x5206 => {
                self.data.multiplier.operand_r = value.into();
            }
            _ => {}
        }
    }

    pub(crate) fn read_cpu_address(&mut self, address: u16) -> u8 {
        if address == 0xFFFA || address == 0xFFFB {
            self.data.scanline_counter.nmi_vector_fetched();
        }

        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: {address:04X}"),
            0x4020..=0x4FFF => 0xFF,
            0x5000..=0x5BFF => self.read_internal_register(address),
            0x5C00..=0x5FFF => match self.data.extended_ram_mode {
                ExtendedRamMode::ReadWrite | ExtendedRamMode::ReadOnly => {
                    self.data.extended_ram[(address - 0x5C00) as usize]
                }
                ExtendedRamMode::Nametable | ExtendedRamMode::NametableExtendedAttributes => 0xFF,
            },
            0x6000..=0xFFFF => self
                .data
                .prg_banking_mode
                .map_prg_address(self.data.prg_bank_registers, address)
                .read(&self.cartridge),
        }
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: {address:04X}"),
            0x4020..=0x4FFF => {}
            0x5000..=0x5BFF => {
                self.write_internal_register(address, value);
            }
            0x5C00..=0x5FFF => {
                if self.data.extended_ram_mode == ExtendedRamMode::ReadWrite {
                    self.data.extended_ram[(address - 0x5C00) as usize] = value;
                }
            }
            0x6000..=0xFFFF => {
                if self.prg_ram_writes_enabled() {
                    self.data
                        .prg_banking_mode
                        .map_prg_address(self.data.prg_bank_registers, address)
                        .write(value, &mut self.cartridge);
                }
            }
        }
    }

    pub(crate) fn read_ppu_address(&mut self, address: u16, vram: &[u8; 2048]) -> u8 {
        self.data.scanline_counter.pre_fetch();

        match address {
            0x0000..=0x1FFF => {
                let tile_type = self.data.scanline_counter.current_tile_type();
                let pattern_table_byte = if tile_type == TileType::Background
                    && self.data.extended_ram_mode == ExtendedRamMode::NametableExtendedAttributes
                {
                    self.data.extended_attributes_state.get_pattern_table_byte(
                        address,
                        &self.data.extended_ram,
                        &self.cartridge.chr_rom,
                    )
                } else if tile_type == TileType::Background
                    && self
                        .data
                        .vertical_split
                        .inside_split(&self.data.scanline_counter)
                {
                    let fine_y_scroll = self.data.vertical_split.y_scroll & 0x07;
                    let pattern_table_addr =
                        (address & 0xFFF8) | ((address + u16::from(fine_y_scroll)) & 0x07);

                    let chr_4kb_bank = self.data.vertical_split.chr_bank;
                    let chr_address =
                        (u32::from(chr_4kb_bank) << 12) | u32::from(pattern_table_addr & 0x0FFF);
                    self.cartridge.chr_rom[chr_address as usize]
                } else {
                    let chr_address = self.data.chr_mapper.map_chr_address(address, tile_type);
                    self.cartridge.chr_rom[chr_address as usize]
                };

                self.data.scanline_counter.increment_tile_bytes_fetched();

                pattern_table_byte
            }
            0x2000..=0x3EFF => {
                let relative_addr = address & 0x0FFF;
                let nametable_addr = 0x2000 | relative_addr;

                self.data
                    .scanline_counter
                    .nametable_address_fetched(nametable_addr);

                let tile_type = self.data.scanline_counter.current_tile_type();
                if tile_type == TileType::Background
                    && self.data.extended_ram_mode == ExtendedRamMode::NametableExtendedAttributes
                    && address & 0x03FF >= 0x03C0
                {
                    // In extended attributes mode, replace attribute table fetches with lookups
                    // from extended RAM
                    return self
                        .data
                        .extended_attributes_state
                        .get_attribute_byte(&self.data.extended_ram);
                }
                self.data.extended_attributes_state.last_nametable_addr = address;

                if self
                    .data
                    .vertical_split
                    .inside_split(&self.data.scanline_counter)
                    && matches!(
                        self.data.extended_ram_mode,
                        ExtendedRamMode::Nametable | ExtendedRamMode::NametableExtendedAttributes
                    )
                {
                    // Ignore nametable mapping when inside the vertical split, always read from
                    // extended RAM
                    let coarse_y_scroll = self.data.vertical_split.y_scroll >> 3;
                    let extended_ram_addr = (relative_addr + u16::from(coarse_y_scroll)) & 0x03FF;
                    return self.data.extended_ram[extended_ram_addr as usize];
                }

                let nametable_mapping =
                    self.data.nametable_mappings[(relative_addr >> 10) as usize];
                match nametable_mapping {
                    NametableMapping::VramPage0 => vram[(relative_addr & 0x03FF) as usize],
                    NametableMapping::VramPage1 => {
                        vram[(0x0400 | (relative_addr & 0x03FF)) as usize]
                    }
                    NametableMapping::ExtendedRam => match self.data.extended_ram_mode {
                        ExtendedRamMode::Nametable
                        | ExtendedRamMode::NametableExtendedAttributes => {
                            self.data.extended_ram[(relative_addr & 0x03FF) as usize]
                        }
                        ExtendedRamMode::ReadWrite | ExtendedRamMode::ReadOnly => 0xFF,
                    },
                    NametableMapping::FillMode => {
                        if relative_addr & 0x03FF < 0x03C0 {
                            // Nametable fetch
                            self.data.fill_mode_tile_data
                        } else {
                            // Attribute table fetch
                            self.data.fill_mode_attributes
                        }
                    }
                }
            }
            0x3F00..=0xFFFF => panic!("invalid PPU map address: {address:04X}"),
        }
    }

    pub(crate) fn write_ppu_address(&mut self, address: u16, value: u8, vram: &mut [u8; 2048]) {
        match address {
            0x0000..=0x1FFF => {}
            0x2000..=0x3EFF => {
                let relative_addr = address & 0x0FFF;

                let nametable_mapping =
                    self.data.nametable_mappings[(relative_addr >> 10) as usize];
                match nametable_mapping {
                    NametableMapping::VramPage0 => {
                        vram[(relative_addr & 0x03FF) as usize] = value;
                    }
                    NametableMapping::VramPage1 => {
                        vram[(0x0400 | (relative_addr & 0x03FF)) as usize] = value;
                    }
                    NametableMapping::ExtendedRam => {
                        self.data.extended_ram[(relative_addr & 0x03FF) as usize] = value;
                    }
                    NametableMapping::FillMode => {}
                }
            }
            0x3F00..=0xFFFF => panic!("invalid PPU map address: {address:04X}"),
        }
    }

    fn prg_ram_writes_enabled(&self) -> bool {
        self.data.ram_writes_enabled_1 && self.data.ram_writes_enabled_2
    }

    pub(crate) fn interrupt_flag(&self) -> bool {
        self.data.scanline_counter.interrupt_flag()
    }

    pub(crate) fn tick_cpu(&mut self) {
        self.data.scanline_counter.tick_cpu();
    }
}
