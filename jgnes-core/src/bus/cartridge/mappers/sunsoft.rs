//! Code for the Sunsoft 5A / 5B / FME-7 boards (iNES mapper 69).

use crate::bus::cartridge::mappers::{
    BankSizeKb, ChrType, CpuMapResult, NametableMirroring, PpuMapResult,
};
use crate::bus::cartridge::{HasBasicPpuMapping, MapperImpl};
use crate::num::GetBit;
use bincode::{Decode, Encode};
use std::sync::OnceLock;

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum PrgType {
    ROM,
    RAM,
}

#[derive(Debug, Clone, Encode, Decode)]
struct Sunsoft5bChannel {
    wave_step: bool,
    divider: u8,
    timer: u16,
    period: u16,
    tone_enabled: bool,
    volume: u8,
}

const AUDIO_DIVIDER: u8 = 16;

impl Sunsoft5bChannel {
    fn new() -> Self {
        Self {
            wave_step: false,
            divider: AUDIO_DIVIDER,
            timer: 0,
            period: 0,
            tone_enabled: false,
            volume: 0,
        }
    }

    fn handle_period_low_update(&mut self, value: u8) {
        self.period = (self.period & 0xFF00) | u16::from(value);
    }

    fn handle_period_high_update(&mut self, value: u8) {
        self.period = (self.period & 0x00FF) | (u16::from(value & 0x0F) << 8);
    }

    fn handle_volume_update(&mut self, value: u8) {
        self.volume = value & 0x0F;
    }

    fn sample(&self) -> u8 {
        if !self.tone_enabled {
            self.volume
        } else {
            u8::from(self.wave_step) * self.volume
        }
    }

    fn sample_analog(&self) -> f64 {
        static DAC_LOOKUP_TABLE: OnceLock<[f64; 16]> = OnceLock::new();
        let lookup_table = DAC_LOOKUP_TABLE.get_or_init(|| {
            let mut lookup_table = [0.0; 16];

            // Arbitrary value, will get normalized later
            lookup_table[1] = 1.0;

            // https://en.wikipedia.org/wiki/Decibel
            //
            // Each step produces an output difference of 3dB which gives this equation, where L0 is the
            // previous output value on a linear scale and L1 is the current value:
            //   3dB = 20 * log10(L1/L0)
            // Simple algebra brings this to:
            //   L1 = L0 * 10^(3/20)
            for i in 2..16 {
                lookup_table[i] = lookup_table[i - 1] * 10.0_f64.powf(3.0 / 20.0);
            }

            // Hack: the 5B has some sort of compressor that appears to make the difference between
            // volume 14 and volume 15 much smaller than it should be.
            //
            // Without doing anything, volume 14 will end up around (0.707 * volume 15), so modify
            // volume 15 to be a bit higher than that.
            //
            // This also has the effect of making all of the lower volumes a little louder, which is
            // desired because otherwise they will sound too soft.
            lookup_table[15] *= 0.72;

            // Normalize the values so that the max is 1.0
            let max = lookup_table[15];
            for value in &mut lookup_table[1..] {
                *value /= max;
            }

            lookup_table
        });

        lookup_table[self.sample() as usize]
    }

    fn clock(&mut self) {
        self.timer += 1;
        if self.timer >= self.period {
            self.timer = 0;
            self.wave_step = !self.wave_step;
        }
    }

    fn tick_cpu(&mut self) {
        self.divider -= 1;
        if self.divider == 0 {
            self.divider = AUDIO_DIVIDER;
            self.clock();
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct Sunsoft5bAudioUnit {
    register_select: u8,
    register_writes_enabled: bool,
    channel_1: Sunsoft5bChannel,
    channel_2: Sunsoft5bChannel,
    channel_3: Sunsoft5bChannel,
}

impl Sunsoft5bAudioUnit {
    fn new() -> Self {
        Self {
            register_select: 0,
            register_writes_enabled: false,
            channel_1: Sunsoft5bChannel::new(),
            channel_2: Sunsoft5bChannel::new(),
            channel_3: Sunsoft5bChannel::new(),
        }
    }

    fn handle_select_update(&mut self, value: u8) {
        self.register_select = value & 0x0F;
        self.register_writes_enabled = value & 0xF0 == 0;
    }

    fn handle_write(&mut self, value: u8) {
        if !self.register_writes_enabled {
            return;
        }

        match self.register_select {
            0x00 => {
                self.channel_1.handle_period_low_update(value);
            }
            0x01 => {
                self.channel_1.handle_period_high_update(value);
            }
            0x02 => {
                self.channel_2.handle_period_low_update(value);
            }
            0x03 => {
                self.channel_2.handle_period_high_update(value);
            }
            0x04 => {
                self.channel_3.handle_period_low_update(value);
            }
            0x05 => {
                self.channel_3.handle_period_high_update(value);
            }
            0x07 => {
                self.channel_3.tone_enabled = !value.bit(2);
                self.channel_2.tone_enabled = !value.bit(1);
                self.channel_1.tone_enabled = !value.bit(0);
            }
            0x08 => {
                self.channel_1.handle_volume_update(value);
            }
            0x09 => {
                self.channel_2.handle_volume_update(value);
            }
            0x0A => {
                self.channel_3.handle_volume_update(value);
            }
            _ => {}
        }
    }

    fn tick_cpu(&mut self) {
        self.channel_1.tick_cpu();
        self.channel_2.tick_cpu();
        self.channel_3.tick_cpu();
    }

    fn sample(&self) -> f64 {
        (self.channel_1.sample_analog()
            + self.channel_2.sample_analog()
            + self.channel_3.sample_analog())
            / 3.0
    }

    fn enabled(&self) -> bool {
        self.channel_1.tone_enabled
            || self.channel_2.tone_enabled
            || self.channel_3.tone_enabled
            || self.channel_1.volume != 0
            || self.channel_2.volume != 0
            || self.channel_3.volume != 0
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct Sunsoft {
    prg_banks: [u8; 4],
    prg_bank_0_type: PrgType,
    prg_ram_enabled: bool,
    chr_type: ChrType,
    chr_banks: [u8; 8],
    nametable_mirroring: NametableMirroring,
    command_register: u8,
    irq_enabled: bool,
    irq_counter_enabled: bool,
    irq_counter: u16,
    irq_triggered: bool,
    audio: Sunsoft5bAudioUnit,
}

impl Sunsoft {
    pub(crate) fn new(chr_type: ChrType) -> Self {
        Self {
            prg_banks: [0; 4],
            prg_bank_0_type: PrgType::ROM,
            prg_ram_enabled: false,
            chr_type,
            chr_banks: [0; 8],
            nametable_mirroring: NametableMirroring::Vertical,
            command_register: 0,
            irq_enabled: false,
            irq_counter_enabled: false,
            irq_counter: 0,
            irq_triggered: false,
            audio: Sunsoft5bAudioUnit::new(),
        }
    }
}

impl MapperImpl<Sunsoft> {
    fn map_cpu_address(&self, address: u16) -> CpuMapResult {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: {address:04X}"),
            0x4020..=0x5FFF => CpuMapResult::None {
                original_address: address,
            },
            0x6000..=0x7FFF => match self.data.prg_bank_0_type {
                PrgType::ROM => {
                    let prg_rom_addr =
                        BankSizeKb::Eight.to_absolute_address(self.data.prg_banks[0], address);
                    CpuMapResult::PrgROM(prg_rom_addr)
                }
                PrgType::RAM => {
                    if self.data.prg_ram_enabled && !self.cartridge.prg_ram.is_empty() {
                        let prg_ram_addr =
                            BankSizeKb::Eight.to_absolute_address(self.data.prg_banks[0], address);
                        CpuMapResult::PrgRAM(prg_ram_addr)
                    } else {
                        CpuMapResult::None {
                            original_address: address,
                        }
                    }
                }
            },
            0x8000..=0xDFFF => {
                // 0x8000..=0x9FFF to bank index 1
                // 0xA000..=0xBFFF to bank index 2
                // 0xC000..=0xDFFF to bank index 3
                let bank_index = (address - 0x6000) / 0x2000;
                let prg_rom_addr = BankSizeKb::Eight
                    .to_absolute_address(self.data.prg_banks[bank_index as usize], address);
                CpuMapResult::PrgROM(prg_rom_addr)
            }
            0xE000..=0xFFFF => {
                // Fixed to last bank
                let prg_rom_addr = BankSizeKb::Eight
                    .to_absolute_address_last_bank(self.cartridge.prg_rom.len() as u32, address);
                CpuMapResult::PrgROM(prg_rom_addr)
            }
        }
    }

    pub(crate) fn read_cpu_address(&self, address: u16) -> u8 {
        self.map_cpu_address(address).read(&self.cartridge)
    }

    pub(crate) fn write_cpu_address(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: {address:04X}"),
            0x4020..=0x5FFF => {}
            0x6000..=0x7FFF => {
                self.map_cpu_address(address)
                    .write(value, &mut self.cartridge);
            }
            0x8000..=0x9FFF => {
                self.data.command_register = value & 0x0F;
            }
            0xA000..=0xBFFF => match self.data.command_register {
                0x00..=0x07 => {
                    self.data.chr_banks[self.data.command_register as usize] = value;
                }
                0x08 => {
                    self.data.prg_banks[0] = value & 0x3F;
                    self.data.prg_bank_0_type = if value.bit(6) {
                        PrgType::RAM
                    } else {
                        PrgType::ROM
                    };
                    self.data.prg_ram_enabled = value.bit(7);
                }
                0x09..=0x0B => {
                    let prg_bank_index = self.data.command_register - 0x08;
                    self.data.prg_banks[prg_bank_index as usize] = value & 0x3F;
                }
                0x0C => {
                    self.data.nametable_mirroring = match value & 0x03 {
                        0x00 => NametableMirroring::Vertical,
                        0x01 => NametableMirroring::Horizontal,
                        0x02 => NametableMirroring::SingleScreenBank0,
                        0x03 => NametableMirroring::SingleScreenBank1,
                        _ => unreachable!("value & 0x03 should always be 0x00/0x01/0x02/0x03"),
                    };
                }
                0x0D => {
                    self.data.irq_enabled = value.bit(0);
                    self.data.irq_counter_enabled = value.bit(7);
                    self.data.irq_triggered = false;
                }
                0x0E => {
                    self.data.irq_counter = (self.data.irq_counter & 0xFF00) | u16::from(value);
                }
                0x0F => {
                    self.data.irq_counter =
                        (self.data.irq_counter & 0x00FF) | (u16::from(value) << 8);
                }
                _ => panic!("command register should always contain 0-15"),
            },
            0xC000..=0xDFFF => {
                self.data.audio.handle_select_update(value);
            }
            0xE000..=0xFFFF => {
                self.data.audio.handle_write(value);
            }
        }
    }

    pub(crate) fn interrupt_flag(&self) -> bool {
        self.data.irq_triggered
    }

    pub(crate) fn tick_cpu(&mut self) {
        self.data.audio.tick_cpu();

        if !self.data.irq_counter_enabled {
            return;
        }

        if self.data.irq_enabled && self.data.irq_counter == 0 {
            self.data.irq_triggered = true;
        }
        self.data.irq_counter = self.data.irq_counter.wrapping_sub(1);
    }

    pub(crate) fn sample_audio(&self, mixed_apu_sample: f64) -> f64 {
        if !self.data.audio.enabled() {
            return mixed_apu_sample;
        }

        let sunsoft_5b_sample = self.data.audio.sample();

        // This audio chip appears to slightly decrease APU channel volume
        0.7 * mixed_apu_sample - sunsoft_5b_sample
    }
}

impl HasBasicPpuMapping for MapperImpl<Sunsoft> {
    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        match address {
            0x0000..=0x1FFF => {
                let chr_bank_index = address / 0x0400;
                let chr_addr = BankSizeKb::One
                    .to_absolute_address(self.data.chr_banks[chr_bank_index as usize], address);
                self.data.chr_type.to_map_result(chr_addr)
            }
            0x2000..=0x3EFF => {
                PpuMapResult::Vram(self.data.nametable_mirroring.map_to_vram(address))
            }
            0x3F00..=0xFFFF => panic!("invalid PPU map address: {address:04X}"),
        }
    }
}
