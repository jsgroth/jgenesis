//! Code for the Namco 129 and Namco 163 boards (iNES mapper 19).

use crate::bus;
use crate::bus::cartridge::mappers::{BankSizeKb, ChrType, PpuMapResult};
use crate::bus::cartridge::{HasBasicPpuMapping, MapperImpl};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::array;

// 15-bit counter
const MAX_IRQ_COUNTER: u16 = 0x7FFF;

const AUDIO_DIVIDER: u8 = 15;

// Mix instead of multiplex when 6+ channels are playing to avoid high-pitched ringing noise
const CHANNEL_MULTIPLEX_THRESHOLD: u8 = 6;

#[derive(Debug, Clone, Encode, Decode)]
struct IrqCounter {
    enabled: bool,
    counter: u16,
}

impl IrqCounter {
    fn new() -> Self {
        Self { enabled: false, counter: 0 }
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
struct Namco163AudioChannel {
    channel_idx: u8,
    current_output: f64,
}

impl Namco163AudioChannel {
    fn new(channel_idx: u8) -> Self {
        Self { channel_idx, current_output: 0.0 }
    }

    fn clock(&mut self, internal_ram: &mut [u8; 128]) {
        // Channel config/state stored in $40-$7F, 8 bytes per channel
        let config_addr = 0x40 | (8 * self.channel_idx as usize);

        let frequency = u32::from_le_bytes([
            internal_ram[config_addr],
            internal_ram[config_addr + 2],
            internal_ram[config_addr + 4] & 0x03,
            0,
        ]);

        let mut phase = u32::from_le_bytes([
            internal_ram[config_addr + 1],
            internal_ram[config_addr + 3],
            internal_ram[config_addr + 5],
            0,
        ]);

        let length = 256 - u32::from(internal_ram[config_addr + 4] & 0xFC);
        let base_address: u32 = internal_ram[config_addr + 6].into();
        let volume: i16 = (internal_ram[config_addr + 7] & 0x0F).into();

        phase = (phase + frequency) & ((1 << 24) - 1);
        while phase >= (length << 16) {
            phase -= length << 16;
        }

        let relative_sample_idx = phase >> 16;
        let sample_idx = (base_address + relative_sample_idx) & 0xFF;
        let sample_byte = internal_ram[(sample_idx >> 1) as usize];

        // Samples are 4-bit nibbles in little-endian: 0=low nibble, 1=high nibble
        let sample = (sample_byte >> (4 * (sample_idx & 1))) & 0xF;

        // Volume should act as if the waveform is centered at sample value 8
        // This will produce a value in the range [-120, 105]
        let sample = (i16::from(sample) - 8) * volume;

        self.current_output = f64::from(sample) / 120.0;

        // Write updated phase back to wavetable RAM
        let [phase_low, phase_mid, phase_high, _] = phase.to_le_bytes();
        internal_ram[config_addr + 1] = phase_low;
        internal_ram[config_addr + 3] = phase_mid;
        internal_ram[config_addr + 5] = phase_high;
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct Namco163AudioUnit {
    enabled: bool,
    channels: [Namco163AudioChannel; 8],
    divider: u8,
    current_channel: u8,
    enabled_channel_count: u8,
}

impl Namco163AudioUnit {
    fn new() -> Self {
        Self {
            enabled: false,
            channels: array::from_fn(|i| Namco163AudioChannel::new(i as u8)),
            divider: AUDIO_DIVIDER,
            current_channel: 0,
            enabled_channel_count: 0,
        }
    }

    fn process_internal_ram_update(&mut self, address: u8, value: u8) {
        if address == 0x7F {
            // Bits 6-4 of $7F control which channels are enabled in addition to channel 8 volume
            self.enabled_channel_count = ((value & 0x70) >> 4) + 1;

            log::trace!("# channels enabled: {}", self.enabled_channel_count);
        }
    }

    fn clock(&mut self, internal_ram: &mut [u8; 128]) {
        if !self.enabled {
            return;
        }

        self.current_channel = self.current_channel.wrapping_sub(1) & 0x07;
        if self.current_channel < 8 - self.enabled_channel_count {
            self.current_channel = 7;
        }
        self.channels[self.current_channel as usize].clock(internal_ram);
    }

    fn tick_cpu(&mut self, internal_ram: &mut [u8; 128]) {
        self.divider -= 1;
        if self.divider == 0 {
            self.clock(internal_ram);
            self.divider = AUDIO_DIVIDER;
        }
    }

    fn sample(&self) -> f64 {
        if self.enabled_channel_count < CHANNEL_MULTIPLEX_THRESHOLD {
            self.channels[self.current_channel as usize].current_output
        } else {
            // Special case 6-8 enabled channels because an accurate implementation sounds horrible
            // without a low-pass filter with a low cutoff frequency
            let channel_sum = self
                .channels
                .iter()
                .rev()
                .take(self.enabled_channel_count as usize)
                .map(|channel| channel.current_output)
                .sum::<f64>();
            channel_sum / f64::from(self.enabled_channel_count)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum VolumeVariantDb {
    Twelve,
    Sixteen,
    Eighteen,
}

impl VolumeVariantDb {
    const fn n163_coefficient(self) -> f64 {
        match self {
            Self::Twelve => {
                // APU pulse volume * 10^(12/20) / (1 + 105/120)
                0.31716257177124485
            }
            Self::Sixteen => {
                // APU pulse volume * 10^(16.5/20) / (1 + 105/120)
                0.5324537998876507
            }
            Self::Eighteen => {
                // APU pulse volume * 10^(18.75/20) / (1 + 105/120)
                0.6898933055568182
            }
        }
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
    audio: Namco163AudioUnit,
    volume_variant: VolumeVariantDb,
}

impl Namco163 {
    pub(crate) fn new(
        sub_mapper_number: u8,
        chr_type: ChrType,
        has_battery: bool,
        prg_ram_len: u32,
        sav_bytes: Option<Vec<u8>>,
    ) -> Self {
        let volume_variant = match sub_mapper_number {
            4 => VolumeVariantDb::Sixteen,
            5 => VolumeVariantDb::Eighteen,
            _ => VolumeVariantDb::Twelve,
        };

        let mut internal_ram = [0; 128];

        if has_battery && prg_ram_len == 0 {
            if let Some(sav_bytes) = sav_bytes {
                if sav_bytes.len() == internal_ram.len() {
                    internal_ram.copy_from_slice(&sav_bytes);
                }
            }
        }

        log::info!("Namco 163 volume variant: {volume_variant:?}");

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
            audio: Namco163AudioUnit::new(),
            volume_variant,
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
                let ram_addr = self.data.internal_ram_addr;
                self.data.internal_ram[ram_addr as usize] = value;
                self.data.internal_ram_dirty_bit = true;

                self.data.audio.process_internal_ram_update(ram_addr, value);

                if self.data.internal_ram_auto_increment {
                    self.data.internal_ram_addr = (ram_addr + 1) & 0x7F;
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
                // This register doubles as both PRG RAM write protection and the internal RAM address
                self.data.ram_writes_enabled = value & 0xF0 == 0x40;
                for bit in 0..=3 {
                    self.data.ram_window_writes_enabled[bit as usize] = !value.bit(bit);
                }

                self.data.internal_ram_auto_increment = value.bit(7);
                self.data.internal_ram_addr = value & 0x7F;
            }
        }
    }

    pub(crate) fn tick_cpu(&mut self) {
        self.data.irq.tick_cpu();
        self.data.audio.tick_cpu(&mut self.data.internal_ram);
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

    pub(crate) fn sample_audio(&self, mixed_apu_sample: f64) -> f64 {
        if !self.data.audio.enabled {
            return mixed_apu_sample;
        }

        let n163_sample = self.data.audio.sample() * self.data.volume_variant.n163_coefficient();
        (mixed_apu_sample + n163_sample).clamp(-1.0, 1.0)
    }
}

impl HasBasicPpuMapping for MapperImpl<Namco163> {
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
}
