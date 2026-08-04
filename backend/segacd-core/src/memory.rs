//! Sega CD memory map and sub CPU bus interface

use bincode::{Decode, Encode};
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::array;
use std::ops::Deref;

pub const BIOS_LEN: usize = 128 * 1024;
pub const PRG_RAM_LEN_WORDS: usize = 512 * 1024 / 2;
pub const BACKUP_RAM_LEN: usize = 8 * 1024;
pub const RAM_CARTRIDGE_LEN: usize = 128 * 1024;

#[derive(Debug, Clone, Encode, Decode)]
pub struct SegaCdRegisters {
    // $FF8000/$A12000: Reset / BUSREQ
    pub software_interrupt_pending: bool,
    pub sub_cpu_busreq: bool,
    pub sub_cpu_reset: bool,
    pub led_green: bool,
    pub led_red: bool,
    // $FF8002/$A12002: Memory mode / PRG RAM bank select
    pub prg_ram_write_protect: u8,
    pub prg_ram_bank: u8,
    // $A12006: HINT vector
    pub h_interrupt_vector: u16,
    // $FF800C: Stopwatch
    pub stopwatch_counter: u16,
    // $FF800E: Communication flags
    pub sub_cpu_communication_flags: u8,
    pub main_cpu_communication_flags: u8,
    // $FF8010-$FF801E: Communication commands
    pub communication_commands: [u16; 8],
    // $FF8020-$FF802E: Communication statuses
    pub communication_statuses: [u16; 8],
    // $FF8030: General-purpose timer w/ INT3
    pub timer_counter: u8,
    pub timer_interval: u8,
    pub timer_interrupt_pending: bool,
    // $FF8032: Interrupt mask control
    pub subcode_interrupt_enabled: bool,
    pub cdc_interrupt_enabled: bool,
    pub cdd_interrupt_enabled: bool,
    pub timer_interrupt_enabled: bool,
    pub software_interrupt_enabled: bool,
    pub graphics_interrupt_enabled: bool,
    // $FF8036: CDD control
    pub cdd_host_clock_on: bool,
    // $FF8042-$FF804B: CDD command buffer
    pub cdd_command: [u8; 10],
}

impl SegaCdRegisters {
    pub fn new() -> Self {
        Self {
            software_interrupt_pending: false,
            sub_cpu_busreq: true,
            sub_cpu_reset: true,
            led_green: true,
            led_red: false,
            prg_ram_write_protect: 0,
            prg_ram_bank: 0,
            h_interrupt_vector: 0xFFFF,
            stopwatch_counter: 0,
            sub_cpu_communication_flags: 0,
            main_cpu_communication_flags: 0,
            communication_commands: [0; 8],
            communication_statuses: [0; 8],
            timer_counter: 0,
            timer_interval: 0,
            timer_interrupt_pending: false,
            subcode_interrupt_enabled: false,
            cdc_interrupt_enabled: false,
            cdd_interrupt_enabled: false,
            timer_interrupt_enabled: false,
            software_interrupt_enabled: false,
            graphics_interrupt_enabled: false,
            cdd_host_clock_on: false,
            cdd_command: array::from_fn(|_| 0),
        }
    }

    pub fn main_prg_ram_addr(&self, address: u32) -> u32 {
        (u32::from(self.prg_ram_bank) << 17) | (address & 0x1FFFF)
    }
}

#[derive(Debug, Clone, Default, FakeEncode, FakeDecode)]
pub struct Bios(pub Box<[u16]>);

impl Deref for Bios {
    type Target = Box<[u16]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
