mod backupram;
mod font;
pub(crate) mod wordram;

use crate::api::DiscResult;
use crate::cddrive::cdc::DeviceDestination;
use crate::cddrive::{cdc, CdController, CdTickEffect};
use crate::cdrom;
use crate::cdrom::cdtime::CdTime;
use crate::cdrom::reader::CdRom;
use crate::graphics::GraphicsCoprocessor;
use crate::memory::font::FontRegisters;
use crate::rf5c164::Rf5c164;
use bincode::{Decode, Encode};
use genesis_core::memory::{CloneWithoutRom, Memory, PhysicalMedium};
use genesis_core::GenesisRegion;
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use jgenesis_traits::num::GetBit;
use m68000_emu::BusInterface;
use std::ops::Deref;
use std::path::Path;
use std::{array, mem};
use wordram::WordRam;

pub const BIOS_LEN: usize = 128 * 1024;
pub const PRG_RAM_LEN: usize = 512 * 1024;
const BACKUP_RAM_LEN: usize = 8 * 1024;

const TIMER_DIVIDER: u64 = 1536;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum ScdCpu {
    Main,
    Sub,
}

#[derive(Debug, Clone, Encode, Decode)]
struct SegaCdRegisters {
    // $FF8000/$A12000: Reset / BUSREQ
    software_interrupt_pending: bool,
    sub_cpu_busreq: bool,
    sub_cpu_reset: bool,
    led_green: bool,
    led_red: bool,
    // $FF8002/$A12002: Memory mode / PRG RAM bank select
    prg_ram_write_protect: u8,
    prg_ram_bank: u8,
    // $A12006: HINT vector
    h_interrupt_vector: u16,
    // $FF800C: Stopwatch
    stopwatch_counter: u16,
    // $FF800E: Communication flags
    sub_cpu_communication_flags: u8,
    main_cpu_communication_flags: u8,
    // $FF8010-$FF801E: Communication commands
    communication_commands: [u16; 8],
    // $FF8020-$FF802E: Communication statuses
    communication_statuses: [u16; 8],
    // $FF8030: General-purpose timer w/ INT3
    timer_counter: u8,
    timer_interval: u8,
    timer_interrupt_pending: bool,
    // $FF8032: Interrupt mask control
    subcode_interrupt_enabled: bool,
    cdc_interrupt_enabled: bool,
    cdd_interrupt_enabled: bool,
    timer_interrupt_enabled: bool,
    software_interrupt_enabled: bool,
    graphics_interrupt_enabled: bool,
    // $FF8036: CDD control
    cdd_host_clock_on: bool,
    // $FF8042-$FF804B: CDD command buffer
    cdd_command: [u8; 10],
}

impl SegaCdRegisters {
    fn new() -> Self {
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

    fn prg_ram_addr(&self, address: u32) -> u32 {
        (u32::from(self.prg_ram_bank) << 17) | (address & 0x1FFFF)
    }
}

#[derive(Debug, Clone, Default, FakeEncode, FakeDecode)]
struct Bios(Vec<u8>);

impl Deref for Bios {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SegaCd {
    bios: Bios,
    disc_drive: CdController,
    prg_ram: Box<[u8; PRG_RAM_LEN]>,
    word_ram: WordRam,
    backup_ram: Box<[u8; BACKUP_RAM_LEN]>,
    backup_ram_dirty: bool,
    registers: SegaCdRegisters,
    font_registers: FontRegisters,
    disc_region: GenesisRegion,
    forced_region: Option<GenesisRegion>,
    timer_divider: u64,
}

impl SegaCd {
    pub fn new(
        bios: Vec<u8>,
        mut disc: Option<CdRom>,
        initial_backup_ram: Option<Vec<u8>>,
        forced_region: Option<GenesisRegion>,
    ) -> DiscResult<Self> {
        let backup_ram = match initial_backup_ram {
            Some(backup_ram) if backup_ram.len() == BACKUP_RAM_LEN => {
                backup_ram.into_boxed_slice().try_into().unwrap()
            }
            _ => backupram::new_formatted_backup_ram(),
        };

        let disc_region = match &mut disc {
            Some(disc) => {
                // Parse disc region from ROM header, which is always located in sector 0
                let mut sector_buffer = [0; cdrom::BYTES_PER_SECTOR as usize];
                disc.read_sector(1, CdTime::SECTOR_0_START, &mut sector_buffer)?;

                // Sega CD ROM header starts at $010 because the first 16 bytes are sync + CD-ROM data track header
                GenesisRegion::from_rom(&sector_buffer[0x010..]).unwrap_or_else(|| {
                    log::warn!("Unable to determine disc region from ROM header; defaulting to US");
                    GenesisRegion::Americas
                })
            }
            None => {
                // Default to US if no disc provided
                GenesisRegion::Americas
            }
        };

        log::info!("Region parsed from disc header: {disc_region:?}");

        Ok(Self {
            bios: Bios(bios),
            disc_drive: CdController::new(disc),
            prg_ram: vec![0; PRG_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            word_ram: WordRam::new(),
            backup_ram,
            backup_ram_dirty: false,
            registers: SegaCdRegisters::new(),
            font_registers: FontRegisters::new(),
            disc_region,
            forced_region,
            timer_divider: TIMER_DIVIDER,
        })
    }

    #[allow(clippy::match_same_arms)]
    fn read_main_cpu_register_byte(&mut self, address: u32) -> u8 {
        log::trace!("Main CPU register byte read: {address:06X}");
        match address {
            0xA12000 => {
                // Initialization / reset, high byte
                (u8::from(self.registers.software_interrupt_enabled) << 7)
                    | u8::from(self.registers.software_interrupt_pending)
            }
            0xA12001 => {
                // Initialization / reset, low byte
                (u8::from(self.registers.sub_cpu_busreq) << 1)
                    | u8::from(!self.registers.sub_cpu_reset)
            }
            0xA12002 => {
                // Memory mode / write protect, high byte
                self.registers.prg_ram_write_protect
            }
            0xA12003 => {
                // Memory mode / write protect, low byte
                (self.registers.prg_ram_bank << 6) | self.word_ram.read_control()
            }
            0xA12004 => {
                log::trace!("  CDC mode read (main CPU)");
                let cdc = self.disc_drive.cdc();
                let end_of_data_transfer = cdc.end_of_data_transfer();
                let data_ready = cdc.data_ready();
                let dd_bits = cdc.device_destination().to_bits();

                (u8::from(end_of_data_transfer) << 7) | (u8::from(data_ready) << 6) | dd_bits
            }
            0xA12006 => {
                // HINT vector, high byte
                (self.registers.h_interrupt_vector >> 8) as u8
            }
            0xA12007 => {
                // HINT vector, low byte
                self.registers.h_interrupt_vector as u8
            }
            0xA12008 => {
                // CDC host data, high byte
                (self.disc_drive.cdc_mut().read_host_data(ScdCpu::Main) >> 8) as u8
            }
            0xA12009 => {
                // CDC host data, low byte
                self.disc_drive.cdc_mut().read_host_data(ScdCpu::Main) as u8
            }
            0xA1200C => {
                // Stopwatch, high byte
                (self.registers.stopwatch_counter >> 8) as u8
            }
            0xA1200D => {
                // Stopwatch, low byte
                self.registers.stopwatch_counter as u8
            }
            0xA1200E => {
                // Communication flags, high byte (main CPU)
                self.registers.main_cpu_communication_flags
            }
            0xA1200F => {
                // Communication flags, low byte (sub CPU)
                self.registers.sub_cpu_communication_flags
            }
            0xA12010..=0xA1201F => {
                // Communication command buffers
                let idx = (address & 0xF) >> 1;
                let word = self.registers.communication_commands[idx as usize];
                if address.bit(0) { word as u8 } else { (word >> 8) as u8 }
            }
            0xA12020..=0xA1202F => {
                // Communication status buffers
                let idx = (address & 0xF) >> 1;
                let word = self.registers.communication_statuses[idx as usize];
                if address.bit(0) { word as u8 } else { (word >> 8) as u8 }
            }
            _ => 0,
        }
    }

    #[allow(clippy::match_same_arms)]
    fn read_main_cpu_register_word(&mut self, address: u32) -> u16 {
        log::trace!("Main CPU register word read: {address:06X}");
        match address {
            0xA12000 | 0xA12002 => u16::from_be_bytes([
                self.read_main_cpu_register_byte(address),
                self.read_main_cpu_register_byte(address | 1),
            ]),
            0xA12004 => {
                // CDC mode, only high byte has any data
                u16::from(self.read_main_cpu_register_byte(address)) << 8
            }
            0xA12006 => self.registers.h_interrupt_vector,
            0xA12008 => {
                log::trace!("  CDC host data read (main CPU)");
                self.disc_drive.cdc_mut().read_host_data(ScdCpu::Main)
            }
            0xA1200C => self.registers.stopwatch_counter,
            0xA1200E => {
                // Communication flags
                u16::from_be_bytes([
                    self.registers.main_cpu_communication_flags,
                    self.registers.sub_cpu_communication_flags,
                ])
            }
            0xA12010..=0xA1201F => {
                // Communication command buffers
                let idx = (address & 0xF) >> 1;
                self.registers.communication_commands[idx as usize]
            }
            0xA12020..=0xA1202F => {
                // Communication status buffers
                let idx = (address & 0xF) >> 1;
                self.registers.communication_statuses[idx as usize]
            }
            _ => 0,
        }
    }

    #[allow(clippy::match_same_arms)]
    fn write_main_cpu_register_byte(&mut self, address: u32, value: u8) {
        log::trace!("Main CPU register byte write: {address:06X} {value:02X}");
        match address {
            0xA12000 => {
                // Initialization / reset, high byte
                self.registers.software_interrupt_pending = value.bit(0);

                log::trace!("  INT2 pending write: {}", self.registers.software_interrupt_pending);
            }
            0xA12001 => {
                // Initialization / reset, low byte
                self.registers.sub_cpu_busreq = value.bit(1);
                self.registers.sub_cpu_reset = !value.bit(0);

                log::trace!("  Sub CPU BUSREQ: {}", self.registers.sub_cpu_busreq);
                log::trace!("  Sub CPU RESET: {}", self.registers.sub_cpu_reset);
            }
            0xA12002 => {
                // Memory mode / write protect, high byte
                self.registers.prg_ram_write_protect = value;

                log::trace!("  PRG RAM protect write: {value:02X}");
            }
            0xA12003 => {
                // Memory mode / write protect, low byte
                self.registers.prg_ram_bank = value >> 6;
                self.word_ram.main_cpu_write_control(value);

                log::trace!("  PRG RAM bank: {}", self.registers.prg_ram_bank);
            }
            0xA12006..=0xA12007 => {
                self.registers.h_interrupt_vector = u16::from_le_bytes([value, value]);
            }
            0xA1200E..=0xA1200F => {
                self.registers.main_cpu_communication_flags = value;
            }
            0xA12010..=0xA1201F => {
                // Communication command buffers
                let idx = (address & 0xF) >> 1;
                let commands = &mut self.registers.communication_commands;
                let existing_word = commands[idx as usize];
                if address.bit(0) {
                    commands[idx as usize] = (existing_word & 0xFF00) | u16::from(value);
                } else {
                    commands[idx as usize] = (existing_word & 0x00FF) | (u16::from(value) << 8);
                }
            }
            _ => {}
        }
    }

    #[allow(clippy::match_same_arms)]
    fn write_main_cpu_register_word(&mut self, address: u32, value: u16) {
        log::trace!("Main CPU register word write: {address:06X} {value:04X}");
        match address {
            0xA12000 | 0xA12002 => {
                let [msb, lsb] = value.to_be_bytes();
                self.write_main_cpu_register_byte(address, msb);
                self.write_main_cpu_register_byte(address | 1, lsb);
            }
            0xA12006 => {
                self.registers.h_interrupt_vector = value;

                log::trace!("  HINT vector set to {value:04X}");
            }
            0xA1200E => {
                // Communication flags; only main CPU flags are writable
                self.registers.main_cpu_communication_flags = (value >> 8) as u8;
            }
            0xA12010..=0xA1201F => {
                // Communication command buffers
                let idx = (address & 0xF) >> 1;
                self.registers.communication_commands[idx as usize] = value;
            }
            _ => {}
        }
    }

    fn write_prg_ram(&mut self, address: u32, value: u8, cpu: ScdCpu) {
        // PRG RAM write protection applies in multiples of $200
        let write_protection_boundary = u32::from(self.registers.prg_ram_write_protect) * 0x200;

        // PRG RAM write protection only applies to the Sub CPU.
        // The JP V2.00 BIOS freezes if Main CPU writes to PRG RAM are not always allowed through
        if cpu == ScdCpu::Main || address >= write_protection_boundary {
            self.prg_ram[address as usize] = value;
        }
    }

    pub fn tick(
        &mut self,
        master_clock_cycles: u64,
        pcm: &mut Rf5c164,
    ) -> DiscResult<CdTickEffect> {
        let cd_tick_effect = self.disc_drive.tick(
            master_clock_cycles,
            &mut self.word_ram,
            &mut self.prg_ram,
            pcm,
        )?;

        if master_clock_cycles >= self.timer_divider {
            self.clock_timers();
            self.timer_divider = TIMER_DIVIDER - (master_clock_cycles - self.timer_divider);
        } else {
            self.timer_divider -= master_clock_cycles;
        }

        Ok(cd_tick_effect)
    }

    fn clock_timers(&mut self) {
        if self.registers.timer_counter == 1 {
            self.registers.timer_interrupt_pending = true;
            self.registers.timer_counter = 0;
        } else if self.registers.timer_counter == 0 {
            self.registers.timer_counter = self.registers.timer_interval;
        } else {
            self.registers.timer_counter -= 1;
        }

        self.registers.stopwatch_counter = (self.registers.stopwatch_counter + 1) & 0x0FFF;
    }

    pub fn disc_title(&mut self) -> DiscResult<Option<String>> {
        self.disc_drive.disc_title(self.region())
    }

    pub fn word_ram_mut(&mut self) -> &mut WordRam {
        &mut self.word_ram
    }

    pub fn bios(&self) -> &[u8] {
        self.bios.0.as_slice()
    }

    pub fn backup_ram(&self) -> &[u8] {
        self.backup_ram.as_slice()
    }

    pub fn graphics_interrupt_enabled(&self) -> bool {
        self.registers.graphics_interrupt_enabled
    }

    pub fn get_and_clear_backup_ram_dirty_bit(&mut self) -> bool {
        let dirty = self.backup_ram_dirty;
        self.backup_ram_dirty = false;
        dirty
    }

    pub fn take_cdrom(&mut self) -> Option<CdRom> {
        self.disc_drive.take_disc()
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        self.bios = mem::take(&mut other.bios);
        self.disc_drive.take_disc_from(&mut other.disc_drive);
    }

    pub fn forced_region(&self) -> Option<GenesisRegion> {
        self.forced_region
    }

    pub fn set_forced_region(&mut self, forced_region: Option<GenesisRegion>) {
        self.forced_region = forced_region;
    }

    pub fn reset(&mut self) {
        self.disc_drive.reset();
        self.registers = SegaCdRegisters::new();
    }

    pub fn remove_disc(&mut self) {
        self.disc_drive.cdd_mut().remove_disc();
    }

    pub fn change_disc<P: AsRef<Path>>(&mut self, cue_path: P) -> DiscResult<()> {
        self.disc_drive.cdd_mut().change_disc(cue_path)
    }
}

impl PhysicalMedium for SegaCd {
    #[inline]
    fn read_byte(&mut self, address: u32) -> u8 {
        match address {
            0x000000..=0x01FFFF => {
                // BIOS

                // Hack: The BIOS reads the custom HINT vector from $000070-$000072, which it expects to
                // return $FFFF and the current value of $A12006 respectively
                match address {
                    0x000070 | 0x000071 => 0xFF,
                    0x000072 => (self.registers.h_interrupt_vector >> 8) as u8,
                    0x000073 => self.registers.h_interrupt_vector as u8,
                    _ => self.bios[address as usize],
                }
            }
            0x020000..=0x03FFFF => {
                // PRG RAM
                let prg_ram_addr = self.registers.prg_ram_addr(address);
                self.prg_ram[prg_ram_addr as usize]
            }
            0x200000..=0x23FFFF => self.word_ram.main_cpu_read_ram(address),
            0xA12000..=0xA1202F => {
                // Sega CD registers
                self.read_main_cpu_register_byte(address)
            }
            _ => todo!("read byte: {address:06X}"),
        }
    }

    #[inline]
    fn read_word(&mut self, address: u32) -> u16 {
        match address {
            0x000000..=0x01FFFF => {
                // BIOS

                // Hack: The BIOS reads the custom HINT vector from $000070-$000072, which it expects to
                // return $FFFF and the current value of $A12006 respectively
                match address {
                    0x000070 => 0xFFFF,
                    0x000072 => self.registers.h_interrupt_vector,
                    _ => {
                        let msb = self.bios[address as usize];
                        let lsb = self.bios[(address + 1) as usize];
                        u16::from_be_bytes([msb, lsb])
                    }
                }
            }
            0x020000..=0x03FFFF => {
                // PRG RAM
                let prg_ram_addr = self.registers.prg_ram_addr(address);
                let msb = self.prg_ram[prg_ram_addr as usize];
                let lsb = self.prg_ram[(prg_ram_addr + 1) as usize];
                u16::from_be_bytes([msb, lsb])
            }
            0x200000..=0x23FFFF => {
                // Word RAM
                let msb = self.word_ram.main_cpu_read_ram(address);
                let lsb = self.word_ram.main_cpu_read_ram(address | 1);
                u16::from_be_bytes([msb, lsb])
            }
            0xA12000..=0xA1202F => {
                // Sega CD registers
                self.read_main_cpu_register_word(address)
            }
            _ => todo!("read word: {address:06X}"),
        }
    }

    fn read_word_for_dma(&mut self, address: u32) -> u16 {
        // VDP DMA reads from word RAM are delayed by a cycle, effectively meaning the read should
        // be from (address - 2)
        match address & ADDRESS_MASK {
            address @ 0x200000..=0x23FFFF => self.read_word(address.wrapping_sub(2)),
            address => self.read_word(address),
        }
    }

    #[inline]
    fn write_byte(&mut self, address: u32, value: u8) {
        match address {
            0x000000..=0x01FFFF => {
                // BIOS, ignore
            }
            0x020000..=0x03FFFF => {
                // PRG RAM
                let prg_ram_addr = self.registers.prg_ram_addr(address);
                self.write_prg_ram(prg_ram_addr, value, ScdCpu::Main);
            }
            0x200000..=0x23FFFF => {
                self.word_ram.main_cpu_write_ram(address, value);
            }
            0xA12000..=0xA1202F => {
                self.write_main_cpu_register_byte(address, value);
            }
            _ => todo!("write byte: {address:06X}, {value:02X}"),
        }
    }

    #[inline]
    fn write_word(&mut self, address: u32, value: u16) {
        match address {
            0x000000..=0x01FFFF => {
                // BIOS, ignore
            }
            0x020000..=0x03FFFF => {
                // PRG RAM
                let prg_ram_addr = self.registers.prg_ram_addr(address);
                let [msb, lsb] = value.to_be_bytes();
                self.write_prg_ram(prg_ram_addr, msb, ScdCpu::Main);
                self.write_prg_ram(prg_ram_addr + 1, lsb, ScdCpu::Main);
            }
            0x200000..=0x23FFFF => {
                let [msb, lsb] = value.to_be_bytes();
                self.word_ram.main_cpu_write_ram(address, msb);
                self.word_ram.main_cpu_write_ram(address | 1, lsb);
            }
            0xA12000..=0xA1202F => {
                self.write_main_cpu_register_word(address, value);
            }
            _ => todo!("write word: {address:06X}, {value:04X}"),
        }
    }

    fn region(&self) -> GenesisRegion {
        self.forced_region.unwrap_or(self.disc_region)
    }
}

impl CloneWithoutRom for SegaCd {
    fn clone_without_rom(&self) -> Self {
        Self {
            bios: Bios(vec![]),
            disc_drive: self.disc_drive.clone_without_disc(),
            ..self.clone()
        }
    }
}

pub struct SubBus<'a> {
    memory: &'a mut Memory<SegaCd>,
    graphics_coprocessor: &'a mut GraphicsCoprocessor,
    pcm: &'a mut Rf5c164,
}

impl<'a> SubBus<'a> {
    pub fn new(
        memory: &'a mut Memory<SegaCd>,
        graphics_coprocessor: &'a mut GraphicsCoprocessor,
        pcm: &'a mut Rf5c164,
    ) -> Self {
        Self { memory, graphics_coprocessor, pcm }
    }
}

impl<'a> SubBus<'a> {
    #[allow(clippy::match_same_arms)]
    fn read_register_byte(&mut self, address: u32) -> u8 {
        log::trace!("Sub CPU register byte read: {address:06X}");
        match address {
            0xFF8000 => {
                let registers = &self.memory.medium().registers;
                (u8::from(registers.led_green) << 1) | u8::from(registers.led_red)
            }
            0xFF8001 => {
                // Reset
                // TODO version in bits 7-4
                // Bit 0 (CD drive operable) hardcoded to 1
                0x01
            }
            0xFF8002 => {
                // PRG RAM write protect
                self.memory.medium().registers.prg_ram_write_protect
            }
            0xFF8003 => {
                // Memory mode
                let word_ram = &self.memory.medium().word_ram;
                word_ram.read_control() | (word_ram.priority_mode().to_bits() << 3)
            }
            0xFF8004 => {
                // CDC mode
                log::trace!("  CDC mode read (sub CPU)");
                let cdc = self.memory.medium().disc_drive.cdc();
                let end_of_data_transfer = cdc.end_of_data_transfer();
                let data_ready = cdc.data_ready();
                let dd_bits = cdc.device_destination().to_bits();
                (u8::from(end_of_data_transfer) << 7) | (u8::from(data_ready) << 6) | dd_bits
            }
            0xFF8005 => {
                // CDC register address
                log::trace!("  CDC register address read");
                self.memory.medium().disc_drive.cdc().register_address()
            }
            0xFF8007 => {
                // CDC register data
                log::trace!("  CDC register data read");
                self.memory.medium_mut().disc_drive.cdc_mut().read_register()
            }
            0xFF8008 => {
                // CDC host data, high byte
                let word =
                    self.memory.medium_mut().disc_drive.cdc_mut().read_host_data(ScdCpu::Sub);
                (word >> 8) as u8
            }
            0xFF8009 => {
                // CDC host data, low byte
                self.memory.medium_mut().disc_drive.cdc_mut().read_host_data(ScdCpu::Sub) as u8
            }
            0xFF800C => {
                // Stopwatch, high byte
                (self.memory.medium().registers.stopwatch_counter >> 8) as u8
            }
            0xFF800D => {
                // Stopwatch, low byte
                self.memory.medium().registers.stopwatch_counter as u8
            }
            0xFF800E => {
                // Communication flags, high byte (main CPU)
                self.memory.medium().registers.main_cpu_communication_flags
            }
            0xFF800F => {
                // Communication flags, low byte (sub CPU)
                self.memory.medium().registers.sub_cpu_communication_flags
            }
            0xFF8010..=0xFF801F => {
                // Communication command buffers
                let idx = (address & 0xF) >> 1;
                let word = self.memory.medium().registers.communication_commands[idx as usize];
                if address.bit(0) { word as u8 } else { (word >> 8) as u8 }
            }
            0xFF8020..=0xFF802F => {
                // Communication status buffers
                let idx = (address & 0xF) >> 1;
                let word = self.memory.medium().registers.communication_statuses[idx as usize];
                if address.bit(0) { word as u8 } else { (word >> 8) as u8 }
            }
            0xFF8031 => {
                // Timer
                self.memory.medium().registers.timer_interval
            }
            0xFF8033 => {
                // Interrupt mask control
                let sega_cd = self.memory.medium();
                (u8::from(sega_cd.registers.subcode_interrupt_enabled) << 6)
                    | (u8::from(sega_cd.registers.cdc_interrupt_enabled) << 5)
                    | (u8::from(sega_cd.registers.cdd_interrupt_enabled) << 4)
                    | (u8::from(sega_cd.registers.timer_interrupt_enabled) << 3)
                    | (u8::from(sega_cd.registers.software_interrupt_enabled) << 2)
                    | (u8::from(sega_cd.registers.graphics_interrupt_enabled) << 1)
            }
            0xFF8034..=0xFF8035 => {
                // CDD fader, only bit 15 (fader processing) is readable and it's fine to always
                // set it to 0
                0x00
            }
            0xFF8036 => {
                log::trace!("  CDD control read");
                u8::from(!self.memory.medium().disc_drive.cdd().playing_audio())
            }
            0xFF8037 => {
                // CDD control, low byte
                // TODO DRS/DTS bits
                let sega_cd = self.memory.medium();
                u8::from(sega_cd.registers.cdd_host_clock_on) << 2
            }
            0xFF8038..=0xFF8041 => {
                // CDD status
                let relative_addr = (address - 8) & 0xF;
                self.memory.medium().disc_drive.cdd().status()[relative_addr as usize]
            }
            0xFF8042..=0xFF804B => {
                // CDD command
                let relative_addr = (address - 2) & 0xF;
                self.memory.medium().registers.cdd_command[relative_addr as usize]
            }
            0xFF804C..=0xFF804D => {
                // Font color
                self.memory.medium().font_registers.read_color()
            }
            0xFF804E => {
                // Font bits, high byte
                (self.memory.medium().font_registers.font_bits() >> 8) as u8
            }
            0xFF804F => {
                // Font bits, low byte
                self.memory.medium().font_registers.font_bits() as u8
            }
            0xFF8050..=0xFF8057 => {
                // Font data
                let font_data_word = self.memory.medium().font_registers.read_font_data(address);
                if address.bit(0) { font_data_word as u8 } else { (font_data_word >> 8) as u8 }
            }
            0xFF8058..=0xFF8067 => self.graphics_coprocessor.read_register_byte(address),
            _ => 0x00,
        }
    }

    #[allow(clippy::match_same_arms)]
    fn read_register_word(&mut self, address: u32) -> u16 {
        log::trace!("Sub CPU register word read: {address:06X}");
        match address {
            0xFF8000 | 0xFF8002 | 0xFF8004 | 0xFF8036 => {
                let msb = self.read_register_byte(address);
                let lsb = self.read_register_byte(address | 1);
                u16::from_be_bytes([msb, lsb])
            }
            0xFF8006 => {
                // CDC register data; stored in low byte
                self.read_register_byte(address | 1).into()
            }
            0xFF8008 => {
                // CDC host data
                log::trace!("  CDC host data read (sub CPU)");
                self.memory.medium_mut().disc_drive.cdc_mut().read_host_data(ScdCpu::Sub)
            }
            0xFF800C => self.memory.medium().registers.stopwatch_counter,
            0xFF800E => {
                // Communication flags
                let registers = &self.memory.medium().registers;
                u16::from_be_bytes([
                    registers.main_cpu_communication_flags,
                    registers.sub_cpu_communication_flags,
                ])
            }
            0xFF8010..=0xFF801F => {
                // Communication command buffers
                let idx = (address & 0xF) >> 1;
                self.memory.medium().registers.communication_commands[idx as usize]
            }
            0xFF8020..=0xFF802F => {
                // Communication status buffers
                let idx = (address & 0xF) >> 1;
                self.memory.medium().registers.communication_statuses[idx as usize]
            }
            0xFF8030 => {
                // Timer
                self.memory.medium().registers.timer_interval.into()
            }
            0xFF8032 => {
                // Interrupt mask control; all bits in low byte
                self.read_register_byte(address | 1).into()
            }
            0xFF8034 => {
                // CDD fader, only bit 15 (fader processing) is significant and it's fine to always
                // set it to 0
                0x0000
            }
            0xFF8038..=0xFF8041 => {
                // CDD status
                let relative_addr = (address - 8) & 0xF;
                let cdd_status = self.memory.medium().disc_drive.cdd().status();
                u16::from_be_bytes([
                    cdd_status[relative_addr as usize],
                    cdd_status[(relative_addr + 1) as usize],
                ])
            }
            0xFF8042..=0xFF804B => {
                // CDD command
                let relative_addr = (address - 2) & 0xF;
                let cdd_command = self.memory.medium().registers.cdd_command;
                u16::from_be_bytes([
                    cdd_command[relative_addr as usize],
                    cdd_command[(relative_addr + 1) as usize],
                ])
            }
            0xFF804C => {
                // Font color (all bits in low byte)
                self.memory.medium().font_registers.read_color().into()
            }
            0xFF804E => {
                // Font bits
                self.memory.medium().font_registers.font_bits()
            }
            0xFF8050..=0xFF8057 => {
                // Font data registers
                self.memory.medium().font_registers.read_font_data(address)
            }
            0xFF8058..=0xFF8067 => self.graphics_coprocessor.read_register_word(address),
            _ => 0x0000,
        }
    }

    #[allow(clippy::match_same_arms)]
    fn write_register_byte(&mut self, address: u32, value: u8) {
        log::trace!("Sub CPU register byte write: {address:06X} {value:02X}");
        match address {
            0xFF8000 => {
                let registers = &mut self.memory.medium_mut().registers;
                registers.led_green = value.bit(1);
                registers.led_red = value.bit(0);
            }
            0xFF8001 => {
                // TODO reset CDD/CDC
            }
            0xFF8002..=0xFF8003 => {
                // Memory mode
                self.memory.medium_mut().word_ram.sub_cpu_write_control(value);
            }
            0xFF8004 => {
                // CDC mode
                log::trace!("  CDC mode write: {value:02X}");
                let device_destination = DeviceDestination::from_bits(value & 0x07);
                self.memory
                    .medium_mut()
                    .disc_drive
                    .cdc_mut()
                    .set_device_destination(device_destination);
            }
            0xFF8005 => {
                // CDC register address
                log::trace!("  CDC register address write: {value:02X}");
                let register_address = value & cdc::REGISTER_ADDRESS_MASK;
                self.memory
                    .medium_mut()
                    .disc_drive
                    .cdc_mut()
                    .set_register_address(register_address);
            }
            0xFF8007 => {
                // CDC register data
                log::trace!("  CDC register data write: {value:02X}");
                self.memory.medium_mut().disc_drive.cdc_mut().write_register(value);
            }
            0xFF800A..=0xFF800B => {
                // CDC DMA address (bits 18-3)
                // Byte-size writes to this register are erroneous
                let word = u16::from_le_bytes([value, value]);
                let dma_address = u32::from(word) << 3;
                self.memory.medium_mut().disc_drive.cdc_mut().set_dma_address(dma_address);
            }
            0xFF800C..=0xFF800D => {
                // Stopwatch (12 bits)
                self.memory.medium_mut().registers.stopwatch_counter =
                    u16::from_be_bytes([value, value]) & 0x0FFF;
            }
            0xFF800E..=0xFF800F => {
                // Communication flags
                self.memory.medium_mut().registers.sub_cpu_communication_flags = value;
            }
            0xFF8020..=0xFF802F => {
                // Communication status buffers
                let idx = (address & 0xF) >> 1;
                let statuses = &mut self.memory.medium_mut().registers.communication_statuses;
                let existing_word = statuses[idx as usize];
                if address.bit(0) {
                    statuses[idx as usize] = (existing_word & 0xFF00) | u16::from(value);
                } else {
                    statuses[idx as usize] = (existing_word & 0x00FF) | (u16::from(value) << 8);
                }
            }
            0xFF8030..=0xFF8031 => {
                // Timer
                let registers = &mut self.memory.medium_mut().registers;
                registers.timer_interval = value;
                registers.timer_counter = value;
            }
            0xFF8033 => {
                // Interrupt mask control
                let sega_cd = self.memory.medium_mut();
                sega_cd.registers.subcode_interrupt_enabled = value.bit(6);
                sega_cd.registers.cdc_interrupt_enabled = value.bit(5);
                sega_cd.registers.cdd_interrupt_enabled = value.bit(4);
                sega_cd.registers.timer_interrupt_enabled = value.bit(3);
                sega_cd.registers.software_interrupt_enabled = value.bit(2);
                sega_cd.registers.graphics_interrupt_enabled = value.bit(1);

                // Disabling the graphics interrupt should clear any pending interrupt
                if !sega_cd.registers.graphics_interrupt_enabled {
                    self.graphics_coprocessor.acknowledge_interrupt();
                }

                log::trace!("  Interrupt mask write: {value:08b}");
            }
            0xFF8034..=0xFF8035 => {
                // CDD fader; word access only, byte access is erroneous

                self.memory
                    .medium_mut()
                    .disc_drive
                    .cdd_mut()
                    .set_fader_volume(u16::from_le_bytes([value, value]));

                log::trace!("  CDD fader write: {value:02X}");
            }
            0xFF8037 => {
                // CDD control
                self.memory.medium_mut().registers.cdd_host_clock_on = value.bit(2);

                log::trace!("  CDD control write: {value:02X}");
            }
            0xFF8042..=0xFF804B => {
                // CDD command
                let relative_addr = (address - 2) & 0xF;

                let sega_cd = self.memory.medium_mut();
                sega_cd.registers.cdd_command[relative_addr as usize] = value & 0x0F;

                // Byte-size writes to $FF804B trigger a CDD command send
                if address == 0xFF804B {
                    sega_cd.disc_drive.cdd_mut().send_command(sega_cd.registers.cdd_command);
                }
            }
            0xFF804C..=0xFF804D => {
                // Font color
                self.memory.medium_mut().font_registers.write_color(value);
            }
            0xFF804E => {
                // Font bits, high byte
                self.memory.medium_mut().font_registers.write_font_bits_msb(value);
            }
            0xFF804F => {
                // Font bits, low byte
                self.memory.medium_mut().font_registers.write_font_bits_lsb(value);
            }
            0xFF8058..=0xFF8067 => {
                self.graphics_coprocessor.write_register_byte(address, value);
            }
            _ => {}
        }
    }

    #[allow(clippy::match_same_arms)]
    fn write_register_word(&mut self, address: u32, value: u16) {
        log::trace!("Sub CPU register word write: {address:06X} {value:04X}");
        match address {
            0xFF8000 => {
                // LEDs / CD drive reset
                let [msb, lsb] = value.to_be_bytes();
                self.write_register_byte(address, msb);
                self.write_register_byte(address | 1, lsb);
            }
            0xFF8002 => {
                // Memory mode, only low byte is writable
                self.write_register_byte(address | 1, value as u8);
            }
            0xFF8004 => {
                // CDC mode & register address
                let [msb, lsb] = value.to_be_bytes();
                self.write_register_byte(address, msb);
                self.write_register_byte(address | 1, lsb);
            }
            0xFF8006 => {
                // CDC data, only low byte is writable
                self.write_register_byte(address | 1, value as u8);
            }
            0xFF800A => {
                // CDC DMA address (bits 18-3)
                log::trace!("  CDC DMA address write: {value:04X}");
                let dma_address = u32::from(value) << 3;
                self.memory.medium_mut().disc_drive.cdc_mut().set_dma_address(dma_address);
            }
            0xFF800C => {
                // Stopwatch (12 bits)
                self.memory.medium_mut().registers.stopwatch_counter = value & 0x0FFF;
            }
            0xFF800E => {
                // Communication flags, only low byte (sub CPU) is writable
                self.memory.medium_mut().registers.sub_cpu_communication_flags = value as u8;
            }
            0xFF8020..=0xFF802F => {
                // Communication status buffers
                let idx = (address & 0xF) >> 1;
                self.memory.medium_mut().registers.communication_statuses[idx as usize] = value;
            }
            0xFF8030 => {
                // Timer, only low byte is writable
                let registers = &mut self.memory.medium_mut().registers;
                registers.timer_interval = value as u8;
                registers.timer_counter = value as u8;
            }
            0xFF8032 => {
                // Interrupt mask control, only low byte is writable
                self.write_register_byte(address | 1, value as u8);
            }
            0xFF8034 => {
                // CDD fader
                // Bits 14-4 are volume bits 10-0
                let fader_volume = (value >> 4) & 0x7FF;
                self.memory.medium_mut().disc_drive.cdd_mut().set_fader_volume(fader_volume);

                log::trace!("  CDD fader write: {value:04X}");
            }
            0xFF8036 => {
                // CDD control, only low byte is writable
                self.write_register_byte(address | 1, value as u8);
            }
            0xFF8042..=0xFF804B => {
                // CDD command
                let relative_addr = (address - 2) & 0xF;

                let sega_cd = self.memory.medium_mut();
                let [msb, lsb] = value.to_be_bytes();
                sega_cd.registers.cdd_command[relative_addr as usize] = msb & 0x0F;
                sega_cd.registers.cdd_command[(relative_addr + 1) as usize] = lsb & 0x0F;

                // Word-size writes to $FF804A trigger a CDD command send
                if address == 0xFF804A {
                    sega_cd.disc_drive.cdd_mut().send_command(sega_cd.registers.cdd_command);
                }
            }
            0xFF804C => {
                // Font color, only low byte is writable
                self.write_register_byte(address | 1, value as u8);
            }
            0xFF804E => {
                // Font bits
                self.memory.medium_mut().font_registers.write_font_bits(value);
            }
            0xFF8058..=0xFF8067 => {
                self.graphics_coprocessor.write_register_word(address, value);
            }
            _ => {}
        }
    }
}

// Sega CD / 68000 only has a 24-bit address bus
const ADDRESS_MASK: u32 = 0xFFFFFF;

impl<'a> BusInterface for SubBus<'a> {
    #[inline]
    fn read_byte(&mut self, address: u32) -> u8 {
        let address = address & ADDRESS_MASK;
        match address {
            0x000000..=0x07FFFF => {
                // PRG RAM
                self.memory.medium().prg_ram[address as usize]
            }
            0x080000..=0x0DFFFF => {
                // Word RAM
                self.memory.medium().word_ram.sub_cpu_read_ram(address)
            }
            0xFE0000..=0xFEFFFF => {
                // Backup RAM (odd addresses)
                if address.bit(0) {
                    let backup_ram_addr = (address & 0x3FFF) >> 1;
                    self.memory.medium().backup_ram[backup_ram_addr as usize]
                } else {
                    0x00
                }
            }
            0xFF0000..=0xFF3FFF => {
                // PCM sound chip (odd addresses)
                if address.bit(0) { self.pcm.read((address & 0x3FFF) >> 1) } else { 0x00 }
            }
            0xFF8000..=0xFF81FF => {
                // Sub CPU registers
                self.read_register_byte(address)
            }
            _ => todo!("sub bus read byte {address:06X}"),
        }
    }

    #[inline]
    fn read_word(&mut self, address: u32) -> u16 {
        let address = address & ADDRESS_MASK;
        match address {
            0x000000..=0x07FFFF => {
                // PRG RAM
                let sega_cd = self.memory.medium();
                let msb = sega_cd.prg_ram[address as usize];
                let lsb = sega_cd.prg_ram[(address + 1) as usize];
                u16::from_be_bytes([msb, lsb])
            }
            0x080000..=0x0DFFFF => {
                // Word RAM
                let word_ram = &self.memory.medium().word_ram;
                let msb = word_ram.sub_cpu_read_ram(address);
                let lsb = word_ram.sub_cpu_read_ram(address | 1);
                u16::from_be_bytes([msb, lsb])
            }
            0xFE0000..=0xFEFFFF => {
                // Backup RAM (odd addresses)
                let backup_ram_addr = (address & 0x3FFF) >> 1;
                self.memory.medium().backup_ram[backup_ram_addr as usize].into()
            }
            0xFF0000..=0xFF3FFF => {
                // PCM sound chip (odd addresses)
                self.pcm.read((address & 0x3FFF) >> 1).into()
            }
            0xFF8000..=0xFF81FF => {
                // Sub CPU registers
                self.read_register_word(address)
            }
            _ => todo!("sub bus read word {address:06X}"),
        }
    }

    #[inline]
    fn write_byte(&mut self, address: u32, value: u8) {
        let address = address & ADDRESS_MASK;
        match address {
            0x000000..=0x07FFFF => {
                // PRG RAM
                self.memory.medium_mut().write_prg_ram(address, value, ScdCpu::Sub);
            }
            0x080000..=0x0DFFFF => {
                // Word RAM
                self.memory.medium_mut().word_ram.sub_cpu_write_ram(address, value);
            }
            0xFE0000..=0xFEFFFF => {
                // Backup RAM (odd addresses)
                if address.bit(0) {
                    let backup_ram_addr = (address & 0x3FFF) >> 1;
                    let sega_cd = self.memory.medium_mut();
                    sega_cd.backup_ram[backup_ram_addr as usize] = value;
                    sega_cd.backup_ram_dirty = true;
                }
            }
            0xFF0000..=0xFF3FFF => {
                // PCM sound chip (odd addresses)
                if address.bit(0) {
                    self.pcm.write((address & 0x3FFF) >> 1, value);
                }
            }
            0xFF8000..=0xFF81FF => {
                // Sub CPU registers
                self.write_register_byte(address, value);
            }
            _ => todo!("sub bus read byte {address:06X} {value:02X}"),
        }
    }

    #[inline]
    fn write_word(&mut self, address: u32, value: u16) {
        let address = address & ADDRESS_MASK;
        match address {
            0x000000..=0x07FFFF => {
                // PRG RAM
                let [msb, lsb] = value.to_be_bytes();
                let sega_cd = self.memory.medium_mut();
                sega_cd.write_prg_ram(address, msb, ScdCpu::Sub);
                sega_cd.write_prg_ram(address + 1, lsb, ScdCpu::Sub);
            }
            0x080000..=0x0DFFFF => {
                // Word RAM
                let [msb, lsb] = value.to_be_bytes();
                let word_ram = &mut self.memory.medium_mut().word_ram;
                word_ram.sub_cpu_write_ram(address, msb);
                word_ram.sub_cpu_write_ram(address | 1, lsb);
            }
            0xFE0000..=0xFEFFFF => {
                // Backup RAM (odd addresses)
                let backup_ram_addr = (address & 0x3FFF) >> 1;
                let sega_cd = self.memory.medium_mut();
                sega_cd.backup_ram[backup_ram_addr as usize] = value as u8;
                sega_cd.backup_ram_dirty = true;
            }
            0xFF0000..=0xFF3FFF => {
                // PCM sound chip (odd addresses)
                self.pcm.write((address & 0x3FFF) >> 1, value as u8);
            }
            0xFF8000..=0xFF81FF => {
                // Sub CPU registers
                self.write_register_word(address, value);
            }
            _ => todo!("sub bus read word {address:06X} {value:04X}"),
        }
    }

    #[allow(clippy::bool_to_int_with_if)]
    #[inline]
    fn interrupt_level(&self) -> u8 {
        let sega_cd = self.memory.medium();
        if sega_cd.registers.cdc_interrupt_enabled && sega_cd.disc_drive.cdc().interrupt_pending() {
            // INT5: CDC interrupt
            5
        } else if sega_cd.registers.cdd_interrupt_enabled
            && sega_cd.registers.cdd_host_clock_on
            && sega_cd.disc_drive.cdd().interrupt_pending()
        {
            // INT4: CDD interrupt
            4
        } else if sega_cd.registers.timer_interrupt_enabled
            && sega_cd.registers.timer_interrupt_pending
        {
            // INT3: Timer interrupt
            3
        } else if sega_cd.registers.software_interrupt_enabled
            && sega_cd.registers.software_interrupt_pending
        {
            // INT2: Software interrupt from main CPU
            2
        } else if sega_cd.registers.graphics_interrupt_enabled
            && self.graphics_coprocessor.interrupt_pending()
        {
            // INT1: Graphics interrupt
            1
        } else {
            0
        }
    }

    #[inline]
    fn acknowledge_interrupt(&mut self) {
        match self.interrupt_level() {
            1 => {
                self.graphics_coprocessor.acknowledge_interrupt();
            }
            2 => {
                self.memory.medium_mut().registers.software_interrupt_pending = false;
            }
            3 => {
                self.memory.medium_mut().registers.timer_interrupt_pending = false;
            }
            4 => {
                self.memory.medium_mut().disc_drive.cdd_mut().acknowledge_interrupt();
            }
            5 => {
                self.memory.medium_mut().disc_drive.cdc_mut().acknowledge_interrupt();
            }
            _ => {}
        }
    }

    #[inline]
    fn halt(&self) -> bool {
        self.memory.medium().registers.sub_cpu_busreq
    }

    #[inline]
    fn reset(&self) -> bool {
        self.memory.medium().registers.sub_cpu_reset
    }
}
