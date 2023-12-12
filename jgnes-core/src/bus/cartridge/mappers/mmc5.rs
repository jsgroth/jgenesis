//! Code for the MMC5 board (iNES mapper 5).

use crate::apu::pulse::{PulseChannel, SweepStatus};
use crate::apu::FrameCounter;
use crate::bus::cartridge::mappers::{BankSizeKb, CpuMapResult};
use crate::bus::cartridge::{Cartridge, MapperImpl};
use crate::num::GetBit;
use crate::{apu, bus, TimingMode};
use bincode::{Decode, Encode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum PrgBankingMode {
    Mode0,
    Mode1,
    Mode2,
    Mode3,
}

impl PrgBankingMode {
    fn map_result(bank_number: u8, bank_size: BankSizeKb, address: u16) -> CpuMapResult {
        let is_rom = bank_number.bit(7);

        let masked_bank_number = if is_rom { bank_number & 0x7F } else { bank_number & 0x0F };

        // All bank numbers are treated as 8KB banks while selectively ignoring lower bits
        let shifted_bank_number = match bank_size {
            BankSizeKb::Eight => masked_bank_number,
            BankSizeKb::Sixteen => masked_bank_number >> 1,
            BankSizeKb::ThirtyTwo => masked_bank_number >> 2,
            _ => panic!("MMC5 mapper should only have 8KB/16KB/32KB bank size, was {bank_size:?}"),
        };

        let mapped_address = bank_size.to_absolute_address(shifted_bank_number, address);
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
                Self::map_result(prg_bank_registers[0] & 0x7F, BankSizeKb::Eight, address)
            }
            0x8000..=0xFFFF => match self {
                // 1x32KB
                Self::Mode0 => {
                    Self::map_result(prg_bank_registers[4] | 0x80, BankSizeKb::ThirtyTwo, address)
                }
                // 2x16KB
                Self::Mode1 => match address {
                    0x0000..=0x7FFF => unreachable!("nested match expressions"),
                    0x8000..=0xBFFF => {
                        Self::map_result(prg_bank_registers[2], BankSizeKb::Sixteen, address)
                    }
                    0xC000..=0xFFFF => {
                        Self::map_result(prg_bank_registers[4] | 0x80, BankSizeKb::Sixteen, address)
                    }
                },
                // 1x16KB + 2x8KB
                Self::Mode2 => match address {
                    0x0000..=0x7FFF => unreachable!("nested match expressions"),
                    0x8000..=0xBFFF => {
                        Self::map_result(prg_bank_registers[2], BankSizeKb::Sixteen, address)
                    }
                    0xC000..=0xDFFF => {
                        Self::map_result(prg_bank_registers[3], BankSizeKb::Eight, address)
                    }
                    0xE000..=0xFFFF => {
                        Self::map_result(prg_bank_registers[4] | 0x80, BankSizeKb::Eight, address)
                    }
                },
                // 4x8KB
                Self::Mode3 => {
                    // 0x8000..=0x9FFF to bank 1
                    // 0xA000..=0xBFFF to bank 2
                    // 0xC000..=0xDFFF to bank 3
                    // 0xD000..=0xFFFF to bank 4
                    let bank_register = (address & 0x7FFF) / 0x2000 + 1;
                    Self::map_result(
                        prg_bank_registers[bank_register as usize],
                        BankSizeKb::Eight,
                        address,
                    )
                }
            },
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct ChrMapper {
    bank_size: BankSizeKb,
    bank_registers: [u8; 12],
    double_height_sprites: bool,
    last_register_written: usize,
    next_access_from_ppu_data: bool,
}

impl ChrMapper {
    fn new() -> Self {
        Self {
            bank_size: BankSizeKb::Eight,
            bank_registers: [0; 12],
            double_height_sprites: false,
            last_register_written: 0,
            next_access_from_ppu_data: false,
        }
    }

    fn map_sprite_chr_address(&self, address: u16) -> u32 {
        assert!((0x0000..=0x1FFF).contains(&address));

        match self.bank_size {
            BankSizeKb::Eight => {
                BankSizeKb::Eight.to_absolute_address(self.bank_registers[7], address)
            }
            BankSizeKb::Four => {
                // 0x0000..=0x0FFF to bank 3
                // 0x1000..=0x1FFF to bank 7
                let bank_register = 4 * (address / 0x1000) + 3;
                BankSizeKb::Four
                    .to_absolute_address(self.bank_registers[bank_register as usize], address)
            }
            BankSizeKb::Two => {
                // 0x0000..=0x07FF to bank 1
                // 0x0800..=0x0FFF to bank 3
                // 0x1000..=0x17FF to bank 5
                // 0x1800..=0x1FFF to bank 7
                let bank_register = 2 * (address / 0x0800) + 1;
                BankSizeKb::Two
                    .to_absolute_address(self.bank_registers[bank_register as usize], address)
            }
            BankSizeKb::One => {
                // 0x0000..=0x03FF to bank 0
                // 0x0400..=0x07FF to bank 1
                // 0x0800..=0x0BFF to bank 2
                // 0x0C00..=0x0FFF to bank 3
                // 0x1000..=0x13FF to bank 4
                // 0x1400..=0x17FF to bank 5
                // 0x1800..=0x1BFF to bank 6
                // 0x1C00..=0x1FFF to bank 7
                let bank_register = address / 0x0400;
                BankSizeKb::One
                    .to_absolute_address(self.bank_registers[bank_register as usize], address)
            }
            _ => panic!("MMC5 bank size should always be 1/2/4/8"),
        }
    }

    fn map_bg_chr_address(&self, address: u16) -> u32 {
        assert!((0x0000..=0x1FFF).contains(&address));

        match self.bank_size {
            BankSizeKb::Eight => {
                BankSizeKb::Eight.to_absolute_address(self.bank_registers[11], address)
            }
            BankSizeKb::Four => {
                BankSizeKb::Four.to_absolute_address(self.bank_registers[11], address)
            }
            BankSizeKb::Two => {
                // 0x0000..=0x07FF and 0x1000..=0x17FF to bank 9
                // 0x0800..=0x0FFF and 0x1800..=0x1FFF to bank 11
                let bank_register = 2 * ((address & 0x0FFF) / 0x0800) + 9;
                BankSizeKb::Two
                    .to_absolute_address(self.bank_registers[bank_register as usize], address)
            }
            BankSizeKb::One => {
                // 0x0000..=0x03FF and 0x1000..=0x13FF to bank 8
                // 0x0400..=0x07FF and 0x1400..=0x17FF to bank 9
                // 0x0800..=0x0BFF and 0x1800..=0x1BFF to bank 10
                // 0x0C00..=0x0FFF and 0x1C00..=0x1FFF to bank 11
                let bank_register = (address & 0x0FFF) / 0x0400 + 8;
                BankSizeKb::One
                    .to_absolute_address(self.bank_registers[bank_register as usize], address)
            }
            _ => panic!("MMC5 CHR bank size should always be 1/2/4/8"),
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
        self.double_height_sprites = ppu_ctrl_value.bit(5);
        log::trace!("Double height sprites update detected: {}", self.double_height_sprites);
    }

    fn process_bank_register_update(&mut self, address: u16, value: u8) {
        assert!((0x5120..=0x512B).contains(&address));

        let register_index = (address - 0x5120) as usize;
        self.bank_registers[register_index] = value;
        self.last_register_written = register_index;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum ExtendedRamMode {
    Nametable,
    NametableExtendedAttributes,
    ReadWrite,
    ReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum VerticalSplitMode {
    Left,
    Right,
}

#[derive(Debug, Clone, Encode, Decode)]
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
                scanline_counter.current_tile_index() < self.split_tile_index
            }
            VerticalSplitMode::Right => {
                scanline_counter.current_tile_index() >= self.split_tile_index
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum TileType {
    Background,
    Sprite,
}

#[derive(Debug, Clone, Encode, Decode)]
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
                // Reset the tile byte fetch counter on every scanline in case the game disabled
                // and re-enabled rendering mid-scanline
                self.scanline_tile_byte_fetches = 4;

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
        log::trace!("Tile byte fetched, current fetches={}", self.scanline_tile_byte_fetches);

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

#[derive(Debug, Clone, Encode, Decode)]
struct ExtendedAttributesState {
    last_nametable_addr: u16,
}

impl ExtendedAttributesState {
    fn new() -> Self {
        Self { last_nametable_addr: 0 }
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
        cartridge: &Cartridge,
    ) -> u8 {
        let extended_attributes = extended_ram[(self.last_nametable_addr & 0x03FF) as usize];
        let chr_4kb_bank = extended_attributes & 0x3F;
        let chr_addr = BankSizeKb::Four.to_absolute_address(chr_4kb_bank, pattern_table_addr);
        cartridge.get_chr_rom(chr_addr)
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct MultiplierUnit {
    operand_l: u16,
    operand_r: u16,
}

impl MultiplierUnit {
    fn new() -> Self {
        Self { operand_l: 0xFF, operand_r: 0xFF }
    }

    fn output(self) -> u16 {
        self.operand_l * self.operand_r
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum PcmMode {
    Read,
    Write,
}

impl PcmMode {
    fn bit(self) -> u8 {
        match self {
            Self::Read => 0x01,
            Self::Write => 0x00,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct PcmChannel {
    output_level: u8,
    mode: PcmMode,
    irq_enabled: bool,
    irq_pending: bool,
}

impl PcmChannel {
    fn new() -> Self {
        Self { output_level: 0, mode: PcmMode::Write, irq_enabled: false, irq_pending: false }
    }

    fn process_control_update(&mut self, value: u8) {
        self.mode = if value.bit(0) { PcmMode::Read } else { PcmMode::Write };
        self.irq_enabled = value.bit(7);

        if !self.irq_enabled {
            self.irq_pending = false;
        }
    }

    fn read_control(&mut self) -> u8 {
        let control = (u8::from(self.irq_pending) << 7) | self.mode.bit();
        self.irq_pending = false;
        control
    }

    fn process_cpu_read(&mut self, address: u16, value: u8) {
        if self.mode == PcmMode::Read && (0x8000..=0xBFFF).contains(&address) {
            if value != 0 {
                self.output_level = value;
            } else if self.irq_enabled {
                self.irq_pending = true;
            }
        }
    }

    fn process_raw_pcm_update(&mut self, value: u8) {
        if self.mode == PcmMode::Write {
            if value != 0 {
                self.output_level = value;
            } else if self.irq_enabled {
                self.irq_pending = true;
            }
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
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
    pulse_channel_1: PulseChannel,
    pulse_channel_2: PulseChannel,
    pcm_channel: PcmChannel,
    frame_counter: FrameCounter,
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
            pulse_channel_1: PulseChannel::new_channel_1(SweepStatus::Disabled),
            pulse_channel_2: PulseChannel::new_channel_2(SweepStatus::Disabled),
            pcm_channel: PcmChannel::new(),
            frame_counter: FrameCounter::new(TimingMode::Ntsc),
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
            0x5010 => self.data.pcm_channel.read_control(),
            0x5015 => {
                (u8::from(self.data.pulse_channel_2.length_counter() != 0) << 1)
                    | u8::from(self.data.pulse_channel_1.length_counter() != 0)
            }
            0x5204 => {
                log::trace!("Scanline IRQ status register read, clearing IRQ pending flag");

                let result = (u8::from(self.data.scanline_counter.irq_pending) << 7)
                    | (u8::from(self.data.scanline_counter.in_frame) << 6);
                self.data.scanline_counter.irq_pending = false;
                result
            }
            0x5205 => (self.data.multiplier.output() & 0x00FF) as u8,
            0x5206 => (self.data.multiplier.output() >> 8) as u8,
            _ => bus::cpu_open_bus(address),
        }
    }

    fn write_internal_register(&mut self, address: u16, value: u8) {
        match address {
            0x5000 => {
                self.data.pulse_channel_1.process_vol_update(value);
            }
            0x5002 => {
                self.data.pulse_channel_1.process_lo_update(value);
            }
            0x5003 => {
                self.data.pulse_channel_1.process_hi_update(value);
            }
            0x5004 => {
                self.data.pulse_channel_2.process_vol_update(value);
            }
            0x5006 => {
                self.data.pulse_channel_2.process_lo_update(value);
            }
            0x5007 => {
                self.data.pulse_channel_2.process_hi_update(value);
            }
            0x5010 => {
                self.data.pcm_channel.process_control_update(value);
            }
            0x5011 => {
                self.data.pcm_channel.process_raw_pcm_update(value);
            }
            0x5015 => {
                self.data.pulse_channel_1.process_snd_chn_update(value);
                self.data.pulse_channel_2.process_snd_chn_update(value);
            }
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
                self.data.chr_mapper.bank_size = match value & 0x03 {
                    0x00 => BankSizeKb::Eight,
                    0x01 => BankSizeKb::Four,
                    0x02 => BankSizeKb::Two,
                    0x03 => BankSizeKb::One,
                    _ => unreachable!("value & 0x03 should always be 0x00/0x01/0x02/0x03"),
                };
                log::trace!("CHR bank size set to {:?}", self.data.chr_mapper.bank_size);
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
                log::trace!("Nametable mappings set to {:?}", self.data.nametable_mappings);
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
                log::trace!("PRG bank {:02X} set to {value:02X}", address - 0x5113);
            }
            0x5120..=0x512B => {
                self.data.chr_mapper.process_bank_register_update(address, value);
                log::trace!("CHR bank {:02X} set to {value:02X}", address - 0x5120);
            }
            0x5200 => {
                self.data.vertical_split.enabled = value.bit(7);
                self.data.vertical_split.mode =
                    if value.bit(6) { VerticalSplitMode::Right } else { VerticalSplitMode::Left };
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
                self.data.scanline_counter.irq_enabled = value.bit(7);
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
            0x4020..=0x4FFF => bus::cpu_open_bus(address),
            0x5000..=0x5BFF => self.read_internal_register(address),
            0x5C00..=0x5FFF => match self.data.extended_ram_mode {
                ExtendedRamMode::ReadWrite | ExtendedRamMode::ReadOnly => {
                    self.data.extended_ram[(address - 0x5C00) as usize]
                }
                ExtendedRamMode::Nametable | ExtendedRamMode::NametableExtendedAttributes => {
                    bus::cpu_open_bus(address)
                }
            },
            0x6000..=0xFFFF => {
                let value = self
                    .data
                    .prg_banking_mode
                    .map_prg_address(self.data.prg_bank_registers, address)
                    .read(&self.cartridge);

                self.data.pcm_channel.process_cpu_read(address, value);

                value
            }
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
                if self.data.extended_ram_mode != ExtendedRamMode::ReadOnly {
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
                        &self.cartridge,
                    )
                } else if tile_type == TileType::Background
                    && self.data.vertical_split.inside_split(&self.data.scanline_counter)
                {
                    let fine_y_scroll = self.data.vertical_split.y_scroll & 0x07;
                    let pattern_table_addr =
                        (address & 0xFFF8) | ((address + u16::from(fine_y_scroll)) & 0x07);

                    let chr_addr = BankSizeKb::Four
                        .to_absolute_address(self.data.vertical_split.chr_bank, pattern_table_addr);
                    self.cartridge.get_chr_rom(chr_addr)
                } else {
                    let chr_addr = self.data.chr_mapper.map_chr_address(address, tile_type);
                    self.cartridge.get_chr_rom(chr_addr)
                };

                self.data.scanline_counter.increment_tile_bytes_fetched();

                pattern_table_byte
            }
            0x2000..=0x3EFF => {
                let relative_addr = address & 0x0FFF;
                let nametable_addr = 0x2000 | relative_addr;

                self.data.scanline_counter.nametable_address_fetched(nametable_addr);

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

                if self.data.vertical_split.inside_split(&self.data.scanline_counter)
                    && matches!(
                        self.data.extended_ram_mode,
                        ExtendedRamMode::Nametable | ExtendedRamMode::NametableExtendedAttributes
                    )
                {
                    // Ignore nametable mapping when inside the vertical split, always read from
                    // extended RAM
                    let scanline = self.data.scanline_counter.scanline;
                    let y_scroll = self.data.vertical_split.y_scroll;
                    let tile_x_index = self.data.scanline_counter.current_tile_index() & 0x1F;
                    let tile_y_index = ((u16::from(scanline) + u16::from(y_scroll)) >> 3) % 30;

                    let extended_ram_addr = if relative_addr & 0x03FF < 0x03C0 {
                        // Nametable lookup
                        (tile_y_index << 5) | u16::from(tile_x_index)
                    } else {
                        // Attribute table lookup
                        0x03C0 + (((tile_y_index >> 2) << 3) | (u16::from(tile_x_index) >> 2))
                    };

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
                        ExtendedRamMode::ReadWrite | ExtendedRamMode::ReadOnly => {
                            bus::cpu_open_bus(address)
                        }
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
        self.data.scanline_counter.interrupt_flag() || self.data.pcm_channel.irq_pending
    }

    pub(crate) fn tick_cpu(&mut self) {
        self.data.scanline_counter.tick_cpu();

        self.data.pulse_channel_1.tick_cpu();
        self.data.pulse_channel_2.tick_cpu();
        self.data.frame_counter.tick();

        if self.data.frame_counter.generate_quarter_frame_clock() {
            // MMC5 channels clock both length counter and envelope at 240Hz
            self.data.pulse_channel_1.clock_quarter_frame();
            self.data.pulse_channel_1.clock_half_frame();

            self.data.pulse_channel_2.clock_quarter_frame();
            self.data.pulse_channel_2.clock_half_frame();
        }
    }

    pub(crate) fn sample_audio(&self, mixed_apu_sample: f64) -> f64 {
        let pulse1_sample = self.data.pulse_channel_1.sample();
        let pulse2_sample = self.data.pulse_channel_2.sample();
        let mmc5_pulse_mix = apu::mix_pulse_samples(pulse1_sample, pulse2_sample);

        // Partial formula from from https://www.nesdev.org/wiki/APU_Mixer
        let pcm_sample = self.data.pcm_channel.output_level;
        let scaled_pcm_sample = if pcm_sample != 0 {
            159.79 / (1.0 / (f64::from(pcm_sample) / 22638.0) + 100.0)
        } else {
            0.0
        };

        mixed_apu_sample - mmc5_pulse_mix - scaled_pcm_sample
    }
}
