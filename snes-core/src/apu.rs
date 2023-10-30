mod bootrom;

use bincode::{Decode, Encode};
use jgenesis_traits::frontend::TimingMode;
use spc700_emu::traits::BusInterface;
use spc700_emu::Spc700;

const AUDIO_RAM_LEN: usize = 64 * 1024;

// Main SNES master clock frequencies
const NTSC_MASTER_CLOCK_FREQUENCY: u64 = 21_477_270;
const PAL_MASTER_CLOCK_FREQUENCY: u64 = 21_281_370;

const APU_MASTER_CLOCK_FREQUENCY: u64 = 24_576_000;

type AudioRam = [u8; AUDIO_RAM_LEN];

#[derive(Debug, Clone, Encode, Decode)]
struct ApuRegisters {
    boot_rom_mapped: bool,
    main_cpu_communication: [u8; 4],
    spc700_communication: [u8; 4],
}

impl ApuRegisters {
    fn new() -> Self {
        Self { boot_rom_mapped: true, main_cpu_communication: [0; 4], spc700_communication: [0; 4] }
    }

    fn read(&mut self, register: u16) -> u8 {
        match register {
            4 => self.main_cpu_communication[0],
            5 => self.main_cpu_communication[1],
            6 => self.main_cpu_communication[2],
            7 => self.main_cpu_communication[3],
            _ => {
                // TODO other registers
                0xFF
            }
        }
    }

    fn write(&mut self, register: u16, value: u8) {
        match register {
            4 => {
                self.spc700_communication[0] = value;
            }
            5 => {
                self.spc700_communication[1] = value;
            }
            6 => {
                self.spc700_communication[2] = value;
            }
            7 => {
                self.spc700_communication[3] = value;
            }
            _ => {
                // TODO other registers
            }
        }
    }
}

struct Spc700Bus<'a> {
    audio_ram: &'a mut Box<AudioRam>,
    registers: &'a mut ApuRegisters,
}

impl<'a> BusInterface for Spc700Bus<'a> {
    #[inline]
    fn read(&mut self, address: u16) -> u8 {
        match address {
            0x0000..=0x00EF | 0x0100..=0xFFBF => self.audio_ram[address as usize],
            0x00F0..=0x00FF => self.registers.read(address & 0xF),
            0xFFC0..=0xFFFF => {
                if self.registers.boot_rom_mapped {
                    bootrom::SPC700_BOOT_ROM[(address & 0x003F) as usize]
                } else {
                    self.audio_ram[address as usize]
                }
            }
        }
    }

    #[inline]
    fn write(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x00EF | 0x0100..=0xFFBF => {
                self.audio_ram[address as usize] = value;
            }
            0x00F0..=0x00FF => {
                self.registers.write(address & 0xF, value);
            }
            0xFFC0..=0xFFFF => {
                if !self.registers.boot_rom_mapped {
                    self.audio_ram[address as usize] = value;
                }
            }
        }
    }

    #[inline]
    fn idle(&mut self) {}
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Apu {
    spc700: Spc700,
    audio_ram: Box<AudioRam>,
    registers: ApuRegisters,
    main_master_clock_frequency: u64,
    master_cycles_product: u64,
}

macro_rules! new_spc700_bus {
    ($self:expr) => {
        Spc700Bus { audio_ram: &mut $self.audio_ram, registers: &mut $self.registers }
    };
}

impl Apu {
    pub fn new(timing_mode: TimingMode) -> Self {
        let main_master_clock_frequency = match timing_mode {
            TimingMode::Ntsc => NTSC_MASTER_CLOCK_FREQUENCY,
            TimingMode::Pal => PAL_MASTER_CLOCK_FREQUENCY,
        };

        let mut apu = Self {
            spc700: Spc700::new(),
            audio_ram: vec![0; AUDIO_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            registers: ApuRegisters::new(),
            main_master_clock_frequency,
            master_cycles_product: 0,
        };

        let mut bus = new_spc700_bus!(apu);
        apu.spc700.reset(&mut bus);

        apu
    }

    pub fn tick(&mut self, main_master_cycles: u64) {
        self.master_cycles_product += main_master_cycles * APU_MASTER_CLOCK_FREQUENCY;

        while self.master_cycles_product >= 24 * self.main_master_clock_frequency {
            self.clock();
            self.master_cycles_product -= 24 * self.main_master_clock_frequency;
        }
    }

    fn clock(&mut self) {
        self.spc700.tick(&mut new_spc700_bus!(self));
    }

    pub fn read_port(&mut self, address: u32) -> u8 {
        self.registers.spc700_communication[(address & 0x3) as usize]
    }

    pub fn write_port(&mut self, address: u32, value: u8) {
        self.registers.main_cpu_communication[(address & 0x3) as usize] = value;
    }

    pub fn reset(&mut self) {
        self.registers.boot_rom_mapped = true;
        self.spc700.reset(&mut new_spc700_bus!(self));
    }
}
