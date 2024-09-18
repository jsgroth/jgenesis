//! Code for Konami's VRC7 board (iNES mapper 85).
//!
//! This board has a full-blown FM synthesizer chip as expansion audio, containing a stripped-down
//! Yamaha OPLL core. The mapper excluding audio is a bit less complicated than MMC3.

use crate::bus;
use crate::bus::cartridge::mappers::konami::irq::VrcIrqCounter;
use crate::bus::cartridge::mappers::{
    konami, BankSizeKb, ChrType, NametableMirroring, PpuMapResult,
};
use crate::bus::cartridge::{HasBasicPpuMapping, MapperImpl};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use ym_opll::Vrc7AudioUnit;

#[derive(Debug, Clone, Encode, Decode)]
pub(crate) struct Vrc7 {
    variant: Variant,
    prg_bank_0: u8,
    prg_bank_1: u8,
    prg_bank_2: u8,
    chr_banks: [u8; 8],
    chr_type: ChrType,
    nametable_mirroring: NametableMirroring,
    irq: VrcIrqCounter,
    ram_enabled: bool,
    audio: Vrc7AudioUnit,
    audio_enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum Variant {
    Vrc7a,
    Vrc7b,
    Unknown,
}

// VRC7 has its own oscillator, but the frequency is almost an exact division of the NES CPU clock speed
const VRC7_AUDIO_CLOCK_INTERVAL: u8 = 36;

impl Vrc7 {
    pub(crate) fn new(sub_mapper_number: u8, chr_type: ChrType) -> Self {
        let variant = match sub_mapper_number {
            1 => Variant::Vrc7b,
            2 => Variant::Vrc7a,
            0 => Variant::Unknown,
            _ => panic!("invalid VRC7 sub mapper: {sub_mapper_number}"),
        };

        log::info!("VRC7 variant: {variant:?}");

        Self {
            variant,
            prg_bank_0: 0,
            prg_bank_1: 0,
            prg_bank_2: 0,
            chr_banks: [0; 8],
            chr_type,
            nametable_mirroring: NametableMirroring::Vertical,
            irq: VrcIrqCounter::new(),
            ram_enabled: false,
            audio: ym_opll::new_vrc7(VRC7_AUDIO_CLOCK_INTERVAL),
            audio_enabled: false,
        }
    }
}

impl MapperImpl<Vrc7> {
    pub(crate) fn read_cpu_address(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x401F => panic!("invalid CPU map address: {address:04X}"),
            0x4020..=0x5FFF => bus::cpu_open_bus(address),
            0x6000..=0x7FFF => {
                if self.data.ram_enabled {
                    self.cartridge.get_prg_ram((address & 0x1FFF).into())
                } else {
                    bus::cpu_open_bus(address)
                }
            }
            0x8000..=0x9FFF => {
                let prg_rom_addr =
                    BankSizeKb::Eight.to_absolute_address(self.data.prg_bank_0, address);
                self.cartridge.get_prg_rom(prg_rom_addr)
            }
            0xA000..=0xBFFF => {
                let prg_rom_addr =
                    BankSizeKb::Eight.to_absolute_address(self.data.prg_bank_1, address);
                self.cartridge.get_prg_rom(prg_rom_addr)
            }
            0xC000..=0xDFFF => {
                let prg_rom_addr =
                    BankSizeKb::Eight.to_absolute_address(self.data.prg_bank_2, address);
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
            0x4020..=0x5FFF => {}
            0x6000..=0x7FFF => {
                if self.data.ram_enabled {
                    self.cartridge.set_prg_ram((address & 0x1FFF).into(), value);
                }
            }
            0x8000..=0xFFFF => match (self.data.variant, address) {
                (_, 0x8000) => {
                    self.data.prg_bank_0 = value & 0x3F;
                }
                (Variant::Vrc7a | Variant::Unknown, 0x8010)
                | (Variant::Vrc7b | Variant::Unknown, 0x8008) => {
                    self.data.prg_bank_1 = value & 0x3F;
                }
                (_, 0x9000) => {
                    self.data.prg_bank_2 = value & 0x3F;
                }
                (Variant::Vrc7a | Variant::Unknown, 0x9010) => {
                    self.data.audio.select_register(value);
                }
                (Variant::Vrc7a | Variant::Unknown, 0x9030) => {
                    if self.data.audio_enabled {
                        self.data.audio.write_data(value);
                    }
                }
                (_, 0xA000..=0xD010) => {
                    let address_mask = match self.data.variant {
                        Variant::Vrc7a => 0x0010,
                        Variant::Vrc7b => 0x0008,
                        Variant::Unknown => 0x0018,
                    };
                    let chr_bank_index =
                        2 * ((address - 0xA000) / 0x1000) + u16::from(address & address_mask != 0);
                    self.data.chr_banks[chr_bank_index as usize] = value;
                }
                (_, 0xE000) => {
                    self.data.nametable_mirroring = match value & 0x03 {
                        0x00 => NametableMirroring::Vertical,
                        0x01 => NametableMirroring::Horizontal,
                        0x02 => NametableMirroring::SingleScreenBank0,
                        0x03 => NametableMirroring::SingleScreenBank1,
                        _ => unreachable!("value & 0x03 should always be 0x00/0x01/0x02/0x03"),
                    };
                    self.data.ram_enabled = value.bit(7);

                    self.data.audio_enabled = !value.bit(6);
                    if !self.data.audio_enabled {
                        // Clear all audio state when audio is disabled
                        self.data.audio = ym_opll::new_vrc7(VRC7_AUDIO_CLOCK_INTERVAL);
                    }
                }
                (Variant::Vrc7a | Variant::Unknown, 0xE010)
                | (Variant::Vrc7b | Variant::Unknown, 0xE008) => {
                    self.data.irq.set_reload_value(value);
                }
                (_, 0xF000) => {
                    self.data.irq.set_control(value);
                }
                (Variant::Vrc7a | Variant::Unknown, 0xF010)
                | (Variant::Vrc7b | Variant::Unknown, 0xF008) => {
                    self.data.irq.acknowledge();
                }
                _ => {}
            },
        }
    }

    pub(crate) fn tick_cpu(&mut self) {
        self.data.irq.tick_cpu();
        self.data.audio.tick();
    }

    pub(crate) fn interrupt_flag(&self) -> bool {
        self.data.irq.interrupt_flag()
    }

    pub(crate) fn sample_audio(&self, mixed_apu_sample: f64) -> f64 {
        if !self.data.audio_enabled {
            return mixed_apu_sample;
        }

        let vrc7_sample = self.data.audio.sample();

        // Amplify the VRC7 samples by ~4dB because otherwise this chip is very quiet
        let amplified_sample = vrc7_sample * 1.5848931924611136;
        let clamped_sample = amplified_sample.clamp(-1.0, 1.0);

        mixed_apu_sample - clamped_sample
    }
}

impl HasBasicPpuMapping for MapperImpl<Vrc7> {
    fn map_ppu_address(&self, address: u16) -> PpuMapResult {
        konami::map_ppu_address(
            address,
            &self.data.chr_banks,
            self.data.chr_type,
            self.data.nametable_mirroring,
        )
    }
}
