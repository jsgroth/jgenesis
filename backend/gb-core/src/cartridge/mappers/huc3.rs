//! HuC-3 mapper, used by Robopon and a few Japan-only games
//!
//! References:
//!   <https://gbdev.io/pandocs/HuC3.html>
//!   <https://gbdev.gg8.se/forums/viewtopic.php?id=744>

use crate::cartridge::mappers::{basic_map_ram_address, basic_map_rom_address};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::SaveWriter;
use jgenesis_common::num::GetBit;
use jgenesis_common::timeutils;
use std::iter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum RamMapping {
    // $0
    #[default]
    RamReadOnly,
    // $A
    RamReadWrite,
    // $B
    RtcSendCommand,
    // $C
    RtcReceiveResponse,
    // $D
    RtcSemaphore,
    // $E
    Ir,
    // Other values
    OpenBus,
}

impl RamMapping {
    fn from_byte(byte: u8) -> Self {
        match byte & 0xF {
            0x0 => Self::RamReadOnly,
            0xA => Self::RamReadWrite,
            0xB => Self::RtcSendCommand,
            0xC => Self::RtcReceiveResponse,
            0xD => Self::RtcSemaphore,
            0xE => Self::Ir,
            _ => Self::OpenBus,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Huc3 {
    rom_bank: u8,
    rom_addr_mask: u32,
    ram_bank: u8,
    ram_addr_mask: u32,
    ram_mapping: RamMapping,
    rtc: Huc3Rtc,
}

impl Huc3 {
    pub fn new(rom_len: u32, ram_len: u32, rtc: Option<Huc3Rtc>) -> Self {
        Self {
            rom_bank: 0,
            rom_addr_mask: rom_len - 1,
            ram_bank: 0,
            ram_addr_mask: ram_len - 1,
            ram_mapping: RamMapping::default(),
            rtc: rtc.unwrap_or_else(Huc3Rtc::new),
        }
    }

    pub fn map_rom_address(&self, address: u16) -> u32 {
        basic_map_rom_address(address, self.rom_bank.into(), true, self.rom_addr_mask)
    }

    pub fn write_rom_address(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x1FFF => {
                // RAM mode select
                self.ram_mapping = RamMapping::from_byte(value);
                log::trace!("HuC-3 $A000-$BFFF mapping: {:?}", self.ram_mapping);
            }
            0x2000..=0x3FFF => {
                // ROM bank
                self.rom_bank = value;
            }
            0x4000..=0x5FFF => {
                // RAM bank
                self.ram_bank = value;
            }
            0x6000..=0x7FFF => {
                // Unknown functionality; ignore
            }
            0x8000..=0xFFFF => panic!("Invalid ROM address: {address:04X}"),
        }
    }

    pub fn read_ram(&self, address: u16, sram: &[u8]) -> u8 {
        match self.ram_mapping {
            RamMapping::RamReadOnly | RamMapping::RamReadWrite => {
                basic_map_ram_address(true, address, self.ram_bank.into(), self.ram_addr_mask)
                    .map_or(0xFF, |ram_addr| sram[ram_addr as usize])
            }
            RamMapping::RtcReceiveResponse => {
                let response = 0x80 | (self.rtc.command << 4) | self.rtc.response;
                log::trace!(
                    "RTC response received ({response:02X}): command {:X} response {:X}",
                    self.rtc.command,
                    self.rtc.response
                );
                response
            }
            RamMapping::RtcSemaphore => 0xFE | u8::from(self.rtc.busy_cycles_remaining == 0),
            RamMapping::Ir => {
                // Normally $C1 means "saw light" and $C0 means "didn't see light"; always return $C0
                0xC0
            }
            RamMapping::RtcSendCommand | RamMapping::OpenBus => 0xFF,
        }
    }

    pub fn write_ram(&mut self, address: u16, value: u8, sram: &mut [u8]) {
        match self.ram_mapping {
            RamMapping::RamReadWrite => {
                if let Some(ram_addr) =
                    basic_map_ram_address(true, address, self.ram_bank.into(), self.ram_addr_mask)
                {
                    sram[ram_addr as usize] = value;
                }
            }
            RamMapping::RtcSendCommand => {
                self.rtc.command = (value >> 4) & 0x07;
                self.rtc.argument = value & 0x0F;
                log::trace!(
                    "RTC command sent ({value:02X}): command {:X} argument {:X}",
                    self.rtc.command,
                    self.rtc.argument
                );
            }
            RamMapping::RtcSemaphore => {
                if !value.bit(0) {
                    self.rtc.execute_command();
                    log::trace!("Executing RTC command");
                }
            }
            RamMapping::RamReadOnly
            | RamMapping::RtcReceiveResponse
            | RamMapping::Ir
            | RamMapping::OpenBus => {}
        }
    }

    pub fn tick_cpu(&mut self) {
        self.rtc.busy_cycles_remaining = self.rtc.busy_cycles_remaining.saturating_sub(1);
    }

    pub fn update_rtc_time(&mut self) {
        self.rtc.update_time();
    }

    pub fn save_rtc_state<S: SaveWriter>(&self, save_writer: &mut S) -> Result<(), S::Err> {
        save_writer.persist_serialized("rtc", &self.rtc)
    }
}

const NANOS_PER_MINUTE: u128 = 60 * 1_000_000_000;
const MINUTES_PER_DAY: u64 = 1440;

const RTC_RAM_LEN: usize = 256;

// Arbitrary value, HuC-3 games supposedly expect a delay instead of immediate command execution
const RTC_BUSY_CYCLES: u16 = 1000;

#[derive(Debug, Clone, Encode, Decode)]
pub struct Huc3Rtc {
    memory: Box<[u8; RTC_RAM_LEN]>,
    memory_address: u8,
    last_update_time_nanos: u128,
    nanos_of_minute: u128,
    minutes_of_day: u16,
    day: u16,
    command: u8,
    argument: u8,
    response: u8,
    busy_cycles_remaining: u16,
}

impl Huc3Rtc {
    fn new() -> Self {
        let memory =
            iter::repeat_with(|| rand::random::<u8>() & 0x0F).take(RTC_RAM_LEN).collect::<Vec<_>>();

        Self {
            memory: memory.into_boxed_slice().try_into().unwrap(),
            memory_address: 0,
            last_update_time_nanos: timeutils::current_time_nanos(),
            nanos_of_minute: 0,
            minutes_of_day: rand::random(),
            day: rand::random(),
            command: 0,
            argument: 0,
            response: 0,
            busy_cycles_remaining: 0,
        }
    }

    fn update_time(&mut self) {
        let now_nanos = timeutils::current_time_nanos();
        let elapsed_nanos = now_nanos.saturating_sub(self.last_update_time_nanos);
        self.last_update_time_nanos = now_nanos;

        self.nanos_of_minute += elapsed_nanos;

        let elapsed_minutes = self.nanos_of_minute / NANOS_PER_MINUTE;
        self.nanos_of_minute %= NANOS_PER_MINUTE;

        let new_minutes_of_day = u64::from(self.minutes_of_day) + elapsed_minutes as u64;
        let elapsed_days = new_minutes_of_day / MINUTES_PER_DAY;
        self.minutes_of_day = (new_minutes_of_day % MINUTES_PER_DAY) as u16;

        self.day = self.day.wrapping_add(elapsed_days as u16);

        // These memory addresses seem to hold the current time always?
        self.memory[0x10..0x13].copy_from_slice(&to_nibbles(self.minutes_of_day));
        self.memory[0x13..0x16].copy_from_slice(&to_nibbles(self.day));
    }

    fn execute_command(&mut self) {
        self.busy_cycles_remaining = RTC_BUSY_CYCLES;

        match self.command {
            0x1 => {
                // Read value and increment address
                self.response = self.memory[self.memory_address as usize] & 0x0F;
                self.memory_address = self.memory_address.wrapping_add(1);
            }
            0x3 => {
                // Write value and increment address
                self.memory[self.memory_address as usize] = self.argument;
                self.memory_address = self.memory_address.wrapping_add(1);
            }
            0x4 => {
                // Update address low nibble
                self.memory_address = (self.memory_address & 0xF0) | self.argument;
            }
            0x5 => {
                // Update address high nibble
                self.memory_address = (self.memory_address & 0x0F) | (self.argument << 4);
            }
            0x6 => match self.argument {
                0x0 => {
                    // Copy current time to $00-$06 in RTC memory
                    self.memory[0..3].copy_from_slice(&to_nibbles(self.minutes_of_day));
                    self.memory[3..6].copy_from_slice(&to_nibbles(self.day));
                }
                0x1 => {
                    // Copy $00-$06 in RTC memory to current time
                    self.minutes_of_day = from_nibbles(&self.memory[0..3]);
                    self.day = from_nibbles(&self.memory[3..6]);

                    log::trace!(
                        "Updated RTC time: minutes={}, days={}",
                        self.minutes_of_day,
                        self.day
                    );

                    // TODO event time?
                }
                0x2 => {
                    // Some sort of status command? Games expect it to always respond with 1
                    self.response = 0x01;
                }
                0xE => {
                    log::warn!("HuC-3 tone generator is not emulated");
                }
                _ => {
                    log::warn!("Unexpected HuC-3 extended command: {:X}", self.argument);
                }
            },
            _ => {
                log::warn!(
                    "Unexpected HuC-3 command: command {:X} argument {:X}",
                    self.command,
                    self.argument
                );
            }
        }
    }
}

fn to_nibbles(value: u16) -> [u8; 3] {
    [(value & 0x0F) as u8, ((value >> 4) & 0x0F) as u8, ((value >> 8) & 0x0F) as u8]
}

fn from_nibbles(nibbles: &[u8]) -> u16 {
    u16::from(nibbles[0]) | (u16::from(nibbles[1]) << 4) | (u16::from(nibbles[2]) << 8)
}
