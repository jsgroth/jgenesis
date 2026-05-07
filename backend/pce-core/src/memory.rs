use bincode::{Decode, Encode};
use huc6280_emu::bus::{ClockSpeed, InterruptLines};
use jgenesis_common::boxedarray::BoxedByteArray;
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::{FakeDecode, FakeEncode, PartialClone};
use std::ops::Deref;
use std::{cmp, mem};

const WORKING_RAM_LEN: usize = 8 * 1024;

// Timer ticks every 1024 cycles at ~7.16 MHz regardless of current CPU clock speed
const TIMER_PRESCALER_DIVIDER: u64 = 3 * 1024;

#[derive(Debug, Clone, FakeEncode, FakeDecode)]
pub struct Rom(pub Box<[u8]>);

impl Default for Rom {
    fn default() -> Self {
        Self(vec![].into_boxed_slice())
    }
}

impl Deref for Rom {
    type Target = Box<[u8]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, PartialClone, Encode, Decode)]
pub struct HuCard {
    #[partial_clone(default)]
    rom: Rom,
}

impl HuCard {
    pub fn new(mut rom: Vec<u8>) -> Self {
        rom = mirror_hucard_rom(rom);

        Self { rom: Rom(rom.into_boxed_slice()) }
    }

    pub fn read_rom(&self, address: u32) -> u8 {
        self.rom[(address as usize) & (self.rom.len() - 1)]
    }

    pub fn clone_rom(&self) -> Vec<u8> {
        self.rom.0.to_vec()
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        self.rom.0 = mem::take(&mut other.rom.0);
    }
}

fn mirror_hucard_rom(mut rom: Vec<u8>) -> Vec<u8> {
    let mut new_rom = if rom.len() == 384 * 1024 {
        // 384KB HuCards contain two ROM chips, a 256KB chip and a 128KB chip, mapped like so:
        //   $000000-$07FFFF (banks $00-$3F): First 256KB of ROM, mirrored 2x
        //   $080000-$0FFFFF (banks $40-$7F): Last 128KB of ROM, mirrored 4x
        let mut new_rom = Vec::with_capacity(1024 * 1024);

        for _ in 0..2 {
            new_rom.extend(&rom[..256 * 1024]);
        }
        new_rom.extend(&rom[256 * 1024..]);

        new_rom
    } else if rom.len() == 512 * 1024 {
        // 512KB HuCards can apparently be one of two mappings.
        // Mapping A (2x 256KB chips):
        //   $000000-$07FFFF (banks $00-$3F): First 256KB of ROM, mirrored 2x
        //   $080000-$0FFFFF (banks $40-$7F): Last 256KB of ROM, mirrored 2x
        // Mapping B (1x 512KB chip):
        //   $000000-$0FFFFF (banks $00-$7F): Full 512KB of ROM, mirrored 2x
        // It's virtually impossible to detect which mapping a game expects, so for highest
        // compatibility, mirror the last 256KB of ROM 3x (inspired by what Mednafen does).
        // Explicitly:
        //   $00-$1F: First 256KB
        //   $20-$3F: Second 256KB (important for games with 1x 512KB chip)
        //   $40-$5F: Second 256KB (important for games with 2x 256KB chips)
        //   $60-$7F: Second 256KB (probably never used?)
        if rom.capacity() < 1024 * 1024 {
            rom.reserve(1024 * 1024 - rom.capacity());
        }

        for i in 0..256 * 1024 {
            rom.push(rom[i]);
        }

        rom
    } else {
        // For other sizes (e.g. 768KB or 1MB), normal mirroring up to the next power of two works
        rom
    };

    jgenesis_common::rom::mirror_to_next_power_of_two(&mut new_rom);

    new_rom
}

#[derive(Debug, Clone, Encode, Decode)]
struct Timer {
    counter: u8,
    reload: u8,
    prescaler: u64,
    enabled: bool,
    cycles: u64,
}

impl Timer {
    fn new() -> Self {
        Self {
            reload: 0,
            counter: 0,
            prescaler: TIMER_PRESCALER_DIVIDER,
            enabled: false,
            cycles: 0,
        }
    }

    fn step_to(&mut self, cycles: u64, tiq_pending: &mut bool) {
        if !self.enabled {
            self.cycles = cycles;
            return;
        }

        let mut elapsed_cycles = cycles.saturating_sub(self.cycles);
        self.cycles = cycles;

        while elapsed_cycles != 0 {
            let timer_elapsed = cmp::min(elapsed_cycles, self.prescaler);
            self.prescaler -= timer_elapsed;
            elapsed_cycles -= timer_elapsed;

            if self.prescaler == 0 {
                self.prescaler = TIMER_PRESCALER_DIVIDER;

                if self.counter == 0 {
                    *tiq_pending = true;
                    self.counter = self.reload;
                } else {
                    self.counter -= 1;
                }
            }
        }
    }
}

trait ClockSpeedExt {
    fn mclk_divider(self) -> u64;
}

impl ClockSpeedExt for ClockSpeed {
    fn mclk_divider(self) -> u64 {
        match self {
            Self::Low => 12, // ~1.79 MHz
            Self::High => 3, // ~7.16 MHz
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptType {
    Tiq,
    Irq1,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct CpuRegisters {
    clock_speed: ClockSpeed,
    tiq_disabled: bool,
    tiq_pending: bool,
    irq1_disabled: bool,
    irq1_pending: bool,
    irq2_disabled: bool,
    irq2_pending: bool,
    timer: Timer,
    io_buffer: u8,
}

impl CpuRegisters {
    fn new() -> Self {
        Self {
            clock_speed: ClockSpeed::default(),
            tiq_disabled: false,
            tiq_pending: false,
            irq1_disabled: false,
            irq1_pending: false,
            irq2_disabled: false,
            irq2_pending: false,
            timer: Timer::new(),
            io_buffer: 0xFF,
        }
    }

    pub fn io_buffer(&self) -> u8 {
        self.io_buffer
    }

    pub fn update_io_buffer(&mut self, value: u8, mask: u8) -> u8 {
        self.io_buffer = (value & mask) | (self.io_buffer & !mask);
        self.io_buffer
    }

    pub fn set_irq1(&mut self, irq1_pending: bool) {
        self.irq1_pending = irq1_pending;
    }

    pub fn step_to(&mut self, cycles: u64) {
        self.timer.step_to(cycles, &mut self.tiq_pending);
    }

    // $1FF400-$1FF403: Interrupt registers
    pub fn read_interrupt_register(&mut self, address: u32) -> u8 {
        match address & 3 {
            0 | 1 => {} // Unused
            2 => {
                self.io_buffer = (self.io_buffer & 0xF8)
                    | (u8::from(self.tiq_disabled) << 2)
                    | (u8::from(self.irq1_disabled) << 1)
                    | u8::from(self.irq2_disabled);
            }
            3 => {
                self.io_buffer = (self.io_buffer & 0xF8)
                    | (u8::from(self.tiq_pending) << 2)
                    | (u8::from(self.irq1_pending) << 1)
                    | u8::from(self.irq2_pending);
            }
            _ => unreachable!("value & 3 is always <= 3"),
        }

        self.io_buffer
    }

    // $1FF400-$1FF403: Interrupt registers
    pub fn write_interrupt_register(&mut self, address: u32, value: u8) {
        log::trace!("Interrupt register write: {address:06X} {value:02X}");

        self.io_buffer = value;

        match address & 3 {
            0 | 1 => {} // Unused
            2 => {
                self.tiq_disabled = value.bit(2);
                self.irq1_disabled = value.bit(1);
                self.irq2_disabled = value.bit(0);

                log::trace!("TIQ enabled: {}", !self.tiq_disabled);
                log::trace!("IRQ1 enabled: {}", !self.irq1_disabled);
                log::trace!("IRQ2 enabled: {}", !self.irq2_disabled);
            }
            3 => {
                // All writes acknowledge the timer interrupt
                self.tiq_pending = false;

                log::trace!("TIQ acknowledged");
            }
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }

    // $1FEC00-$1FEC01: Timer registers
    pub fn read_timer_register(&mut self) -> u8 {
        self.io_buffer = (self.io_buffer & !0x7F) | (self.timer.counter & 0x7F);
        self.io_buffer
    }

    // $1FEC00-$1FEC01: Timer registers
    pub fn write_timer_register(&mut self, address: u32, value: u8) {
        self.io_buffer = value;

        match address & 1 {
            0 => {
                self.timer.reload = value & 0x7F;

                log::trace!("Timer reload: {}", self.timer.reload);
            }
            1 => {
                let prev_enabled = self.timer.enabled;
                self.timer.enabled = value.bit(0);

                if !prev_enabled && self.timer.enabled {
                    self.timer.counter = self.timer.reload;
                    self.timer.prescaler = TIMER_PRESCALER_DIVIDER;
                }

                log::trace!("Timer enabled: {}", self.timer.enabled);
            }
            _ => unreachable!("value & 1 is always <= 1"),
        }
    }

    pub fn interrupt_lines(&self) -> InterruptLines {
        InterruptLines {
            irq1: self.irq1_pending && !self.irq1_disabled,
            irq2: self.irq2_pending && !self.irq2_disabled,
            tiq: self.tiq_pending && !self.tiq_disabled,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Memory {
    working_ram: BoxedByteArray<WORKING_RAM_LEN>,
    cpu_registers: CpuRegisters,
}

impl Memory {
    pub fn new() -> Self {
        Self { working_ram: BoxedByteArray::new_random(), cpu_registers: CpuRegisters::new() }
    }

    pub fn read_working_ram(&self, address: u32) -> u8 {
        self.working_ram[(address as usize) & (WORKING_RAM_LEN - 1)]
    }

    pub fn write_working_ram(&mut self, address: u32, value: u8) {
        self.working_ram[(address as usize) & (WORKING_RAM_LEN - 1)] = value;
    }

    pub fn cpu_clock_divider(&self) -> u64 {
        self.cpu_registers.clock_speed.mclk_divider()
    }

    pub fn set_clock_speed(&mut self, speed: ClockSpeed) {
        log::trace!("Clock speed set to {speed:?}");
        self.cpu_registers.clock_speed = speed;
    }

    pub fn cpu_registers(&mut self) -> &mut CpuRegisters {
        &mut self.cpu_registers
    }

    pub fn interrupt_lines(&self) -> InterruptLines {
        self.cpu_registers.interrupt_lines()
    }
}
