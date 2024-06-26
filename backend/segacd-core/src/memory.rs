//! Sega CD memory map and sub CPU bus interface

mod backupram;
mod font;
pub(crate) mod wordram;

use crate::api::SegaCdLoadResult;
use crate::cddrive::cdc::{DeviceDestination, Rchip};
use crate::cddrive::cdd::CdDrive;
use crate::cddrive::{cdc, CdController, CdTickEffect};
use crate::graphics::GraphicsCoprocessor;
use crate::memory::font::FontRegisters;
use crate::rf5c164::Rf5c164;
use bincode::{Decode, Encode};
use cdrom::cdtime::CdTime;
use cdrom::reader::{CdRom, CdRomFileFormat};
use genesis_core::memory::{Memory, PhysicalMedium};
use genesis_core::GenesisRegion;
use jgenesis_common::num::{GetBit, U16Ext};
use jgenesis_proc_macros::{FakeDecode, FakeEncode, PartialClone};
use m68000_emu::BusInterface;
use std::ops::Deref;
use std::path::Path;
use std::{array, mem};
use wordram::WordRam;

pub const BIOS_LEN: usize = 128 * 1024;
pub const PRG_RAM_LEN: usize = 512 * 1024;
pub const BACKUP_RAM_LEN: usize = 8 * 1024;
pub const RAM_CARTRIDGE_LEN: usize = 128 * 1024;

// RAM cartridge size byte is N in the formula 8KB * 2^N
// N=4 signals 128KB
const RAM_CARTRIDGE_SIZE_BYTE: u8 = 0x04;

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

#[derive(Debug, Encode, Decode, PartialClone)]
pub struct SegaCd {
    #[partial_clone(default)]
    bios: Bios,
    #[partial_clone(partial)]
    disc_drive: CdController,
    prg_ram: Box<[u8; PRG_RAM_LEN]>,
    word_ram: WordRam,
    backup_ram: Box<[u8; BACKUP_RAM_LEN]>,
    enable_ram_cartridge: bool,
    ram_cartridge: Box<[u8; RAM_CARTRIDGE_LEN]>,
    ram_cartridge_writes_enabled: bool,
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
        initial_ram_cartridge: Option<Vec<u8>>,
        enable_ram_cartridge: bool,
        forced_region: Option<GenesisRegion>,
    ) -> SegaCdLoadResult<Self> {
        let (backup_ram, ram_cartridge) = backupram::load_initial_backup_ram(
            initial_backup_ram.as_ref(),
            initial_ram_cartridge.as_ref(),
        );

        let disc_region = match &mut disc {
            Some(disc) => parse_disc_region(disc)?,
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
            enable_ram_cartridge,
            ram_cartridge,
            ram_cartridge_writes_enabled: true,
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
                let cdc = self.cdc();
                let end_of_data_transfer = cdc.end_of_data_transfer();
                let data_ready = cdc.data_ready();
                let dd_bits = cdc.device_destination().to_bits();

                (u8::from(end_of_data_transfer) << 7) | (u8::from(data_ready) << 6) | dd_bits
            }
            0xA12006 => {
                // HINT vector, high byte
                self.registers.h_interrupt_vector.msb()
            }
            0xA12007 => {
                // HINT vector, low byte
                self.registers.h_interrupt_vector.lsb()
            }
            0xA12008 => {
                // CDC host data, high byte
                self.cdc_mut().read_host_data(ScdCpu::Main).msb()
            }
            0xA12009 => {
                // CDC host data, low byte
                self.cdc_mut().read_host_data(ScdCpu::Main).lsb()
            }
            0xA1200C => {
                // Stopwatch, high byte
                self.registers.stopwatch_counter.msb()
            }
            0xA1200D => {
                // Stopwatch, low byte
                self.registers.stopwatch_counter.lsb()
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
                if address.bit(0) { word.lsb() } else { word.msb() }
            }
            0xA12020..=0xA1202F => {
                // Communication status buffers
                let idx = (address & 0xF) >> 1;
                let word = self.registers.communication_statuses[idx as usize];
                if address.bit(0) { word.lsb() } else { word.msb() }
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
                self.cdc_mut().read_host_data(ScdCpu::Main)
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
                if address.bit(0) {
                    commands[idx as usize].set_lsb(value);
                } else {
                    commands[idx as usize].set_msb(value);
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
                self.registers.main_cpu_communication_flags = value.msb();
            }
            0xA12010..=0xA1201F => {
                // Communication command buffers
                let idx = (address & 0xF) >> 1;
                self.registers.communication_commands[idx as usize] = value;
            }
            _ => {}
        }
    }

    fn read_ram_cartridge_byte(&self, address: u32) -> u8 {
        if !self.enable_ram_cartridge {
            return 0xFF;
        }

        if !address.bit(0) {
            // RAM cartridge is mapped to odd addresses only
            return 0x00;
        }

        match address {
            0x400000..=0x4FFFFF => {
                // RAM cartridge size
                RAM_CARTRIDGE_SIZE_BYTE
            }
            0x500000..=0x5FFFFF => {
                // Unused
                0x00
            }
            0x600000..=0x6FFFFF => {
                // RAM cartridge data, mirrored every 256KB
                self.ram_cartridge[((address & 0x3FFFF) >> 1) as usize]
            }
            0x700000..=0x7FFFFF => {
                // RAM cartridge writes enabled bit
                self.ram_cartridge_writes_enabled.into()
            }
            _ => panic!("Invalid RAM cartridge address: {address:06X}"),
        }
    }

    fn write_ram_cartridge_byte(&mut self, address: u32, value: u8) {
        if !self.enable_ram_cartridge {
            return;
        }

        if !address.bit(0) {
            // RAM cartridge is mapped to odd addresses only
            return;
        }

        match address {
            0x400000..=0x5FFFFF => {
                // Unused or not writable; do nothing
            }
            0x600000..=0x6FFFFF => {
                // RAM cartridge data
                if self.ram_cartridge_writes_enabled {
                    self.ram_cartridge[((address & 0x3FFFF) >> 1) as usize] = value;
                    self.backup_ram_dirty = true;
                }
            }
            0x700000..=0x7FFFFF => {
                // RAM cartridge writes enabled bit
                self.ram_cartridge_writes_enabled = value.bit(0);
            }
            _ => panic!("Invalid RAM cartridge address: {address:06X}"),
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

    fn cdc(&self) -> &Rchip {
        self.disc_drive.cdc()
    }

    fn cdc_mut(&mut self) -> &mut Rchip {
        self.disc_drive.cdc_mut()
    }

    fn cdd(&self) -> &CdDrive {
        self.disc_drive.cdd()
    }

    fn cdd_mut(&mut self) -> &mut CdDrive {
        self.disc_drive.cdd_mut()
    }

    pub fn tick(
        &mut self,
        master_clock_cycles: u64,
        pcm: &mut Rf5c164,
    ) -> SegaCdLoadResult<CdTickEffect> {
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

    pub fn disc_title(&mut self) -> SegaCdLoadResult<Option<String>> {
        self.disc_drive.disc_title(self.region())
    }

    pub fn word_ram(&self) -> &WordRam {
        &self.word_ram
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

    pub fn ram_cartridge(&self) -> &[u8] {
        self.ram_cartridge.as_slice()
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

    pub fn set_forced_region(&mut self, forced_region: Option<GenesisRegion>) {
        self.forced_region = forced_region;
    }

    pub fn set_enable_ram_cartridge(&mut self, enable_ram_cartridge: bool) {
        self.enable_ram_cartridge = enable_ram_cartridge;
    }

    pub fn reset(&mut self) {
        self.disc_drive.reset();
        self.registers = SegaCdRegisters::new();
    }

    pub fn remove_disc(&mut self) {
        self.cdd_mut().remove_disc();
    }

    pub fn change_disc<P: AsRef<Path>>(
        &mut self,
        rom_path: P,
        format: CdRomFileFormat,
        load_disc_into_ram: bool,
    ) -> SegaCdLoadResult<()> {
        self.cdd_mut().change_disc(rom_path, format, load_disc_into_ram)
    }
}

fn parse_disc_region(disc: &mut CdRom) -> SegaCdLoadResult<GenesisRegion> {
    // ROM header is always located at track 1 sector 0
    let mut rom_header = [0; cdrom::BYTES_PER_SECTOR as usize];
    disc.read_sector(1, CdTime::SECTOR_0_START, &mut rom_header)?;

    // Sega CD ROM header starts at $010 because the first 16 bytes are sync + CD-ROM data track header
    let region = GenesisRegion::from_rom(&rom_header[0x010..]).unwrap_or_else(|| {
        log::warn!("Unable to determine region from ROM header; defaulting to US");
        GenesisRegion::Americas
    });

    // Hack to fix Snatcher (US/EU), which incorrectly reports its region as J in the header
    let serial_number = &rom_header[0x190..0x1A0];
    if region == GenesisRegion::Japan && serial_number == "GM T-95035 -00  ".as_bytes() {
        let console_name = &rom_header[0x110..0x120];
        if console_name == "SEGA GENESIS    ".as_bytes() {
            return Ok(GenesisRegion::Americas);
        } else if console_name == "SEGA MEGA DRIVE ".as_bytes() {
            return Ok(GenesisRegion::Europe);
        }
        // Any other console name is unexpected, leave region as-is
    }

    Ok(region)
}

impl PhysicalMedium for SegaCd {
    #[inline]
    fn read_byte(&mut self, address: u32) -> u8 {
        match address {
            0x000000..=0x1FFFFF => {
                // Mirrors of BIOS at $000000-$01FFFF and PRG RAM at $020000-$03FFFF
                if !address.bit(17) {
                    // BIOS

                    // Hack: The BIOS reads the custom HINT vector from $000070-$000072, which it expects to
                    // return $FFFF and the current value of $A12006 respectively
                    match address {
                        0x000070 | 0x000071 => 0xFF,
                        0x000072 => self.registers.h_interrupt_vector.msb(),
                        0x000073 => self.registers.h_interrupt_vector.lsb(),
                        _ => self.bios[(address & 0x1FFFF) as usize],
                    }
                } else {
                    // PRG RAM
                    let prg_ram_addr = self.registers.prg_ram_addr(address);
                    self.prg_ram[prg_ram_addr as usize]
                }
            }
            0x200000..=0x3FFFFF => self.word_ram.main_cpu_read_ram(address),
            0x400000..=0x7FFFFF => self.read_ram_cartridge_byte(address),
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
            0x000000..=0x1FFFFF => {
                // Mirrors of BIOS at $000000-$01FFFF and PRG RAM at $020000-$03FFFF
                if !address.bit(17) {
                    // BIOS

                    // Hack: The BIOS reads the custom HINT vector from $000070-$000072, which it expects to
                    // return $FFFF and the current value of $A12006 respectively
                    match address {
                        0x000070 => 0xFFFF,
                        0x000072 => self.registers.h_interrupt_vector,
                        _ => {
                            let address = address & 0x1FFFF;
                            let msb = self.bios[address as usize];
                            let lsb = self.bios[(address + 1) as usize];
                            u16::from_be_bytes([msb, lsb])
                        }
                    }
                } else {
                    // PRG RAM
                    let prg_ram_addr = self.registers.prg_ram_addr(address);
                    let msb = self.prg_ram[prg_ram_addr as usize];
                    let lsb = self.prg_ram[(prg_ram_addr + 1) as usize];
                    u16::from_be_bytes([msb, lsb])
                }
            }
            0x200000..=0x3FFFFF => {
                // Word RAM; located at $200000-$23FFFF, mirrors at $240000-$3FFFFF
                let msb = self.word_ram.main_cpu_read_ram(address);
                let lsb = self.word_ram.main_cpu_read_ram(address | 1);
                u16::from_be_bytes([msb, lsb])
            }
            0x400000..=0x7FFFFF => {
                // RAM cartridge; only odd addresses are mapped
                self.read_ram_cartridge_byte(address | 1).into()
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
            // End range at $240000, one word past the last word address in word RAM
            address @ 0x200000..=0x240000 => self.read_word(address.wrapping_sub(2)),
            address => self.read_word(address),
        }
    }

    #[inline]
    fn write_byte(&mut self, address: u32, value: u8) {
        match address {
            0x000000..=0x1FFFFF => {
                // Mirrors of BIOS at $000000-$01FFFF and PRG RAM at $020000-$03FFFF
                if address.bit(17) {
                    // PRG RAM
                    let prg_ram_addr = self.registers.prg_ram_addr(address);
                    self.write_prg_ram(prg_ram_addr, value, ScdCpu::Main);
                } else {
                    // BIOS, ignore
                }
            }
            0x200000..=0x3FFFFF => {
                self.word_ram.main_cpu_write_ram(address, value);
            }
            0x400000..=0x7FFFFF => {
                self.write_ram_cartridge_byte(address, value);
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
            0x000000..=0x1FFFFF => {
                // Mirrors of BIOS at $000000-$01FFFF and PRG RAM at $020000-$03FFFF
                if address.bit(17) {
                    // PRG RAM
                    let prg_ram_addr = self.registers.prg_ram_addr(address);
                    let [msb, lsb] = value.to_be_bytes();
                    self.write_prg_ram(prg_ram_addr, msb, ScdCpu::Main);
                    self.write_prg_ram(prg_ram_addr + 1, lsb, ScdCpu::Main);
                } else {
                    // BIOS, ignore
                }
            }
            0x200000..=0x3FFFFF => {
                let [msb, lsb] = value.to_be_bytes();
                self.word_ram.main_cpu_write_ram(address, msb);
                self.word_ram.main_cpu_write_ram(address | 1, lsb);
            }
            0x400000..=0x7FFFFF => {
                // RAM cartridge; only odd addresses are mapped
                self.write_ram_cartridge_byte(address | 1, value as u8);
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

const SUB_REGISTER_ADDRESS_MASK: u32 = 0x1FF;

pub struct SubBus<'a> {
    pub memory: &'a mut Memory<SegaCd>,
    pub graphics_coprocessor: &'a mut GraphicsCoprocessor,
    pub pcm: &'a mut Rf5c164,
}

impl<'a> SubBus<'a> {
    #[inline]
    pub fn new(
        memory: &'a mut Memory<SegaCd>,
        graphics_coprocessor: &'a mut GraphicsCoprocessor,
        pcm: &'a mut Rf5c164,
    ) -> Self {
        Self { memory, graphics_coprocessor, pcm }
    }

    fn sega_cd(&self) -> &SegaCd {
        self.memory.medium()
    }

    fn sega_cd_mut(&mut self) -> &mut SegaCd {
        self.memory.medium_mut()
    }

    #[allow(clippy::match_same_arms)]
    fn read_register_byte(&mut self, address: u32) -> u8 {
        log::trace!("Sub CPU register byte read: {address:06X}");
        match address & SUB_REGISTER_ADDRESS_MASK {
            0x0000 => {
                let registers = &self.sega_cd().registers;
                (u8::from(registers.led_green) << 1) | u8::from(registers.led_red)
            }
            0x0001 => {
                // Reset
                // TODO version in bits 7-4
                // Bit 0 (CD drive operable) hardcoded to 1
                0x01
            }
            0x0002 => {
                // PRG RAM write protect
                self.sega_cd().registers.prg_ram_write_protect
            }
            0x0003 => {
                // Memory mode
                let word_ram = &self.sega_cd().word_ram;
                word_ram.read_control() | (word_ram.priority_mode().to_bits() << 3)
            }
            0x0004 => {
                // CDC mode
                log::trace!("  CDC mode read (sub CPU)");
                let cdc = self.sega_cd().cdc();
                let end_of_data_transfer = cdc.end_of_data_transfer();
                let data_ready = cdc.data_ready();
                let dd_bits = cdc.device_destination().to_bits();
                (u8::from(end_of_data_transfer) << 7) | (u8::from(data_ready) << 6) | dd_bits
            }
            0x0005 => {
                // CDC register address
                log::trace!("  CDC register address read");
                self.sega_cd().cdc().register_address()
            }
            0x0007 => {
                // CDC register data
                log::trace!("  CDC register data read");
                self.sega_cd_mut().cdc_mut().read_register()
            }
            0x0008 => {
                // CDC host data, high byte
                self.sega_cd_mut().cdc_mut().read_host_data(ScdCpu::Sub).msb()
            }
            0x0009 => {
                // CDC host data, low byte
                self.sega_cd_mut().cdc_mut().read_host_data(ScdCpu::Sub).lsb()
            }
            0x000C => {
                // Stopwatch, high byte
                self.sega_cd().registers.stopwatch_counter.msb()
            }
            0x000D => {
                // Stopwatch, low byte
                self.sega_cd().registers.stopwatch_counter.lsb()
            }
            0x000E => {
                // Communication flags, high byte (main CPU)
                self.sega_cd().registers.main_cpu_communication_flags
            }
            0x000F => {
                // Communication flags, low byte (sub CPU)
                self.sega_cd().registers.sub_cpu_communication_flags
            }
            0x0010..=0x001F => {
                // Communication command buffers
                let idx = (address & 0xF) >> 1;
                let word = self.sega_cd().registers.communication_commands[idx as usize];
                if address.bit(0) { word.lsb() } else { word.msb() }
            }
            0x0020..=0x002F => {
                // Communication status buffers
                let idx = (address & 0xF) >> 1;
                let word = self.sega_cd().registers.communication_statuses[idx as usize];
                if address.bit(0) { word.lsb() } else { word.msb() }
            }
            0x0031 => {
                // Timer
                self.sega_cd().registers.timer_interval
            }
            0x0033 => {
                // Interrupt mask control
                let sega_cd = self.sega_cd();
                (u8::from(sega_cd.registers.subcode_interrupt_enabled) << 6)
                    | (u8::from(sega_cd.registers.cdc_interrupt_enabled) << 5)
                    | (u8::from(sega_cd.registers.cdd_interrupt_enabled) << 4)
                    | (u8::from(sega_cd.registers.timer_interrupt_enabled) << 3)
                    | (u8::from(sega_cd.registers.software_interrupt_enabled) << 2)
                    | (u8::from(sega_cd.registers.graphics_interrupt_enabled) << 1)
            }
            0x0034..=0x0035 => {
                // CDD fader, only bit 15 (fader processing) is readable and it's fine to always
                // set it to 0
                0x00
            }
            0x0036 => {
                log::trace!("  CDD control read");
                u8::from(!self.sega_cd().cdd().playing_audio())
            }
            0x0037 => {
                // CDD control, low byte
                // TODO DRS/DTS bits
                u8::from(self.sega_cd().registers.cdd_host_clock_on) << 2
            }
            0x0038..=0x0041 => {
                // CDD status
                let relative_addr = (address - 8) & 0xF;
                self.sega_cd().cdd().status()[relative_addr as usize]
            }
            0x0042..=0x004B => {
                // CDD command
                let relative_addr = (address - 2) & 0xF;
                self.sega_cd().registers.cdd_command[relative_addr as usize]
            }
            0x004C..=0x004D => {
                // Font color
                self.sega_cd().font_registers.read_color()
            }
            0x004E => {
                // Font bits, high byte
                self.sega_cd().font_registers.font_bits().msb()
            }
            0x004F => {
                // Font bits, low byte
                self.sega_cd().font_registers.font_bits().lsb()
            }
            0x0050..=0x0057 => {
                // Font data
                let font_data_word = self.sega_cd().font_registers.read_font_data(address);
                if address.bit(0) { font_data_word.lsb() } else { font_data_word.msb() }
            }
            0x0058..=0x0067 => self.graphics_coprocessor.read_register_byte(address),
            _ => 0x00,
        }
    }

    #[allow(clippy::match_same_arms)]
    fn read_register_word(&mut self, address: u32) -> u16 {
        log::trace!("Sub CPU register word read: {address:06X}");
        match address & SUB_REGISTER_ADDRESS_MASK {
            0x0000 | 0x0002 | 0x0004 | 0x0036 => {
                let msb = self.read_register_byte(address);
                let lsb = self.read_register_byte(address | 1);
                u16::from_be_bytes([msb, lsb])
            }
            0x0006 => {
                // CDC register data; stored in low byte
                self.read_register_byte(address | 1).into()
            }
            0x0008 => {
                // CDC host data
                log::trace!("  CDC host data read (sub CPU)");
                self.sega_cd_mut().cdc_mut().read_host_data(ScdCpu::Sub)
            }
            0x000A => {
                // CDC DMA address
                (self.sega_cd().cdc().dma_address() >> 3) as u16
            }
            0x000C => self.sega_cd().registers.stopwatch_counter,
            0x000E => {
                // Communication flags
                let registers = &self.sega_cd().registers;
                u16::from_be_bytes([
                    registers.main_cpu_communication_flags,
                    registers.sub_cpu_communication_flags,
                ])
            }
            0x0010..=0x001F => {
                // Communication command buffers
                let idx = (address & 0xF) >> 1;
                self.sega_cd().registers.communication_commands[idx as usize]
            }
            0x0020..=0x002F => {
                // Communication status buffers
                let idx = (address & 0xF) >> 1;
                self.sega_cd().registers.communication_statuses[idx as usize]
            }
            0x0030 => {
                // Timer
                self.sega_cd().registers.timer_interval.into()
            }
            0x0032 => {
                // Interrupt mask control; all bits in low byte
                self.read_register_byte(address | 1).into()
            }
            0x0034 => {
                // CDD fader, only bit 15 (fader processing) is significant and it's fine to always
                // set it to 0
                0x0000
            }
            0x0038..=0x0041 => {
                // CDD status
                let relative_addr = (address - 8) & 0xF;
                let cdd_status = self.sega_cd().cdd().status();
                u16::from_be_bytes([
                    cdd_status[relative_addr as usize],
                    cdd_status[(relative_addr + 1) as usize],
                ])
            }
            0x0042..=0x004B => {
                // CDD command
                let relative_addr = (address - 2) & 0xF;
                let cdd_command = self.sega_cd().registers.cdd_command;
                u16::from_be_bytes([
                    cdd_command[relative_addr as usize],
                    cdd_command[(relative_addr + 1) as usize],
                ])
            }
            0x004C => {
                // Font color (all bits in low byte)
                self.sega_cd().font_registers.read_color().into()
            }
            0x004E => {
                // Font bits
                self.sega_cd().font_registers.font_bits()
            }
            0x0050..=0x0057 => {
                // Font data registers
                self.sega_cd().font_registers.read_font_data(address)
            }
            0x0058..=0x0067 => self.graphics_coprocessor.read_register_word(address),
            _ => 0x0000,
        }
    }

    #[allow(clippy::match_same_arms)]
    fn write_register_byte(&mut self, address: u32, value: u8) {
        log::trace!("Sub CPU register byte write: {address:06X} {value:02X}");
        match address & SUB_REGISTER_ADDRESS_MASK {
            0x0000 => {
                let registers = &mut self.sega_cd_mut().registers;
                registers.led_green = value.bit(1);
                registers.led_red = value.bit(0);
            }
            0x0001 => {
                // TODO reset CDD/CDC
            }
            0x0002..=0x0003 => {
                // Memory mode
                self.sega_cd_mut().word_ram.sub_cpu_write_control(value);
            }
            0x0004 => {
                // CDC mode
                log::trace!("  CDC mode write: {value:02X}");
                let device_destination = DeviceDestination::from_bits(value & 0x07);
                self.sega_cd_mut().cdc_mut().set_device_destination(device_destination);
            }
            0x0005 => {
                // CDC register address
                log::trace!("  CDC register address write: {value:02X}");
                let register_address = value & cdc::REGISTER_ADDRESS_MASK;
                self.sega_cd_mut().cdc_mut().set_register_address(register_address);
            }
            0x0007 => {
                // CDC register data
                log::trace!("  CDC register data write: {value:02X}");
                self.sega_cd_mut().cdc_mut().write_register(value);
            }
            0x000A..=0x000B => {
                // CDC DMA address (bits 18-3)
                // Byte-size writes to this register are erroneous
                let word = u16::from_le_bytes([value, value]);
                let dma_address = u32::from(word) << 3;
                self.sega_cd_mut().cdc_mut().set_dma_address(dma_address);
            }
            0x000C..=0x000D => {
                // Stopwatch (12 bits)
                self.sega_cd_mut().registers.stopwatch_counter =
                    u16::from_be_bytes([value, value]) & 0x0FFF;
            }
            0x000E..=0x000F => {
                // Communication flags
                self.sega_cd_mut().registers.sub_cpu_communication_flags = value;
            }
            0x0020..=0x002F => {
                // Communication status buffers
                let idx = (address & 0xF) >> 1;
                let statuses = &mut self.sega_cd_mut().registers.communication_statuses;
                if address.bit(0) {
                    statuses[idx as usize].set_lsb(value);
                } else {
                    statuses[idx as usize].set_msb(value);
                }
            }
            0x0030..=0x0031 => {
                // Timer
                let registers = &mut self.sega_cd_mut().registers;
                registers.timer_interval = value;
                registers.timer_counter = value;
            }
            0x0033 => {
                // Interrupt mask control
                let sega_cd = self.sega_cd_mut();
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
            0x0034..=0x0035 => {
                // CDD fader; word access only, byte access is erroneous

                self.sega_cd_mut().cdd_mut().set_fader_volume(u16::from_le_bytes([value, value]));

                log::trace!("  CDD fader write: {value:02X}");
            }
            0x0037 => {
                // CDD control
                self.sega_cd_mut().registers.cdd_host_clock_on = value.bit(2);

                log::trace!("  CDD control write: {value:02X}");
            }
            0x0042..=0x004B => {
                // CDD command
                let relative_addr = (address - 2) & 0xF;

                let sega_cd = self.sega_cd_mut();
                sega_cd.registers.cdd_command[relative_addr as usize] = value & 0x0F;

                // Byte-size writes to $FF804B trigger a CDD command send
                if address == 0xFF804B {
                    sega_cd.disc_drive.cdd_mut().send_command(sega_cd.registers.cdd_command);
                }
            }
            0x004C..=0x004D => {
                // Font color
                self.sega_cd_mut().font_registers.write_color(value);
            }
            0x004E => {
                // Font bits, high byte
                self.sega_cd_mut().font_registers.write_font_bits_msb(value);
            }
            0x004F => {
                // Font bits, low byte
                self.sega_cd_mut().font_registers.write_font_bits_lsb(value);
            }
            0x0058..=0x0067 => {
                self.graphics_coprocessor.write_register_byte(address, value);
            }
            _ => {}
        }
    }

    #[allow(clippy::match_same_arms)]
    fn write_register_word(&mut self, address: u32, value: u16) {
        log::trace!("Sub CPU register word write: {address:06X} {value:04X}");
        match address & SUB_REGISTER_ADDRESS_MASK {
            0x0000 => {
                // LEDs / CD drive reset
                let [msb, lsb] = value.to_be_bytes();
                self.write_register_byte(address, msb);
                self.write_register_byte(address | 1, lsb);
            }
            0x0002 => {
                // Memory mode, only low byte is writable
                self.write_register_byte(address | 1, value as u8);
            }
            0x0004 => {
                // CDC mode & register address
                let [msb, lsb] = value.to_be_bytes();
                self.write_register_byte(address, msb);
                self.write_register_byte(address | 1, lsb);
            }
            0x0006 => {
                // CDC data, only low byte is writable
                self.write_register_byte(address | 1, value as u8);
            }
            0x000A => {
                // CDC DMA address (bits 18-3)
                log::trace!("  CDC DMA address write: {value:04X}");
                let dma_address = u32::from(value) << 3;
                self.sega_cd_mut().cdc_mut().set_dma_address(dma_address);
            }
            0x000C => {
                // Stopwatch (12 bits)
                self.sega_cd_mut().registers.stopwatch_counter = value & 0x0FFF;
            }
            0x000E => {
                // Communication flags, only low byte (sub CPU) is writable
                self.sega_cd_mut().registers.sub_cpu_communication_flags = value as u8;
            }
            0x0020..=0x002F => {
                // Communication status buffers
                let idx = (address & 0xF) >> 1;
                self.sega_cd_mut().registers.communication_statuses[idx as usize] = value;
            }
            0x0030 => {
                // Timer, only low byte is writable
                let registers = &mut self.sega_cd_mut().registers;
                registers.timer_interval = value as u8;
                registers.timer_counter = value as u8;
            }
            0x0032 => {
                // Interrupt mask control, only low byte is writable
                self.write_register_byte(address | 1, value as u8);
            }
            0x0034 => {
                // CDD fader
                // Bits 14-4 are volume bits 10-0
                let fader_volume = (value >> 4) & 0x7FF;
                self.sega_cd_mut().cdd_mut().set_fader_volume(fader_volume);

                log::trace!("  CDD fader write: {value:04X}");
            }
            0x0036 => {
                // CDD control, only low byte is writable
                self.write_register_byte(address | 1, value as u8);
            }
            0x0042..=0x004B => {
                // CDD command
                let relative_addr = (address - 2) & 0xF;

                let sega_cd = self.sega_cd_mut();
                let [msb, lsb] = value.to_be_bytes();
                sega_cd.registers.cdd_command[relative_addr as usize] = msb & 0x0F;
                sega_cd.registers.cdd_command[(relative_addr + 1) as usize] = lsb & 0x0F;

                // Word-size writes to $FF804A trigger a CDD command send
                if address == 0xFF804A {
                    sega_cd.disc_drive.cdd_mut().send_command(sega_cd.registers.cdd_command);
                }
            }
            0x004C => {
                // Font color, only low byte is writable
                self.write_register_byte(address | 1, value as u8);
            }
            0x004E => {
                // Font bits
                self.sega_cd_mut().font_registers.write_font_bits(value);
            }
            0x0058..=0x0067 => {
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
                self.sega_cd().prg_ram[address as usize]
            }
            0x080000..=0x0DFFFF => {
                // Word RAM
                self.sega_cd_mut().word_ram.sub_cpu_read_ram(address)
            }
            0xFE0000..=0xFEFFFF => {
                // Backup RAM (odd addresses)
                if address.bit(0) {
                    let backup_ram_addr = (address & 0x3FFF) >> 1;
                    self.sega_cd().backup_ram[backup_ram_addr as usize]
                } else {
                    0x00
                }
            }
            0xFF0000..=0xFF7FFF => {
                // PCM sound chip (odd addresses); canonically located at $FF0000-$FF3FFF and mirrored at $FF4000-$FF7FFF
                if address.bit(0) { self.pcm.read((address & 0x3FFF) >> 1) } else { 0x00 }
            }
            0xFF8000..=0xFFFFFF => {
                // Sub CPU registers; canonically located at $FF8000-$FF81FF, but mirrored throughout the range
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
                let sega_cd = self.sega_cd();
                let msb = sega_cd.prg_ram[address as usize];
                let lsb = sega_cd.prg_ram[(address + 1) as usize];
                u16::from_be_bytes([msb, lsb])
            }
            0x080000..=0x0DFFFF => {
                // Word RAM
                let word_ram = &mut self.sega_cd_mut().word_ram;
                let msb = word_ram.sub_cpu_read_ram(address);
                let lsb = word_ram.sub_cpu_read_ram(address | 1);
                u16::from_be_bytes([msb, lsb])
            }
            0xFE0000..=0xFEFFFF => {
                // Backup RAM (odd addresses)
                let backup_ram_addr = (address & 0x3FFF) >> 1;
                self.sega_cd().backup_ram[backup_ram_addr as usize].into()
            }
            0xFF0000..=0xFF7FFF => {
                // PCM sound chip (odd addresses); canonically located at $FF0000-$FF3FFF and mirrored at $FF4000-$FF7FFF
                self.pcm.read((address & 0x3FFF) >> 1).into()
            }
            0xFF8000..=0xFFFFFF => {
                // Sub CPU registers; canonically located at $FF8000-$FF81FF, but mirrored throughout the range
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
                self.sega_cd_mut().write_prg_ram(address, value, ScdCpu::Sub);
            }
            0x080000..=0x0DFFFF => {
                // Word RAM
                self.sega_cd_mut().word_ram.sub_cpu_write_ram(address, value);
            }
            0xFE0000..=0xFEFFFF => {
                // Backup RAM (odd addresses)
                if address.bit(0) {
                    let backup_ram_addr = (address & 0x3FFF) >> 1;
                    let sega_cd = self.sega_cd_mut();
                    sega_cd.backup_ram[backup_ram_addr as usize] = value;
                    sega_cd.backup_ram_dirty = true;
                }
            }
            0xFF0000..=0xFF7FFF => {
                // PCM sound chip (odd addresses); canonically located at $FF0000-$FF3FFF and mirrored at $FF4000-$FF7FFF
                if address.bit(0) {
                    self.pcm.write((address & 0x3FFF) >> 1, value);
                }
            }
            0xFF8000..=0xFFFFFF => {
                // Sub CPU registers; canonically located at $FF8000-$FF81FF, but mirrored throughout the range
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
                let sega_cd = self.sega_cd_mut();
                sega_cd.write_prg_ram(address, msb, ScdCpu::Sub);
                sega_cd.write_prg_ram(address + 1, lsb, ScdCpu::Sub);
            }
            0x080000..=0x0DFFFF => {
                // Word RAM
                let [msb, lsb] = value.to_be_bytes();
                let word_ram = &mut self.sega_cd_mut().word_ram;
                word_ram.sub_cpu_write_ram(address, msb);
                word_ram.sub_cpu_write_ram(address | 1, lsb);
            }
            0xFE0000..=0xFEFFFF => {
                // Backup RAM (odd addresses)
                let backup_ram_addr = (address & 0x3FFF) >> 1;
                let sega_cd = self.sega_cd_mut();
                sega_cd.backup_ram[backup_ram_addr as usize] = value as u8;
                sega_cd.backup_ram_dirty = true;
            }
            0xFF0000..=0xFF7FFF => {
                // PCM sound chip (odd addresses); canonically located at $FF0000-$FF3FFF and mirrored at $FF4000-$FF7FFF
                self.pcm.write((address & 0x3FFF) >> 1, value as u8);
            }
            0xFF8000..=0xFFFFFF => {
                // Sub CPU registers; canonically located at $FF8000-$FF81FF, but mirrored throughout the range
                self.write_register_word(address, value);
            }
            _ => todo!("sub bus read word {address:06X} {value:04X}"),
        }
    }

    #[allow(clippy::bool_to_int_with_if)]
    #[inline]
    fn interrupt_level(&self) -> u8 {
        let sega_cd = self.sega_cd();
        if sega_cd.registers.cdc_interrupt_enabled && sega_cd.cdc().interrupt_pending() {
            // INT5: CDC interrupt
            5
        } else if sega_cd.registers.cdd_interrupt_enabled
            && sega_cd.registers.cdd_host_clock_on
            && sega_cd.cdd().interrupt_pending()
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
    fn acknowledge_interrupt(&mut self, interrupt_level: u8) {
        // Unlike the Genesis VDP, the Sega CD does appear to acknowledge the correct interrupt
        // when the sub CPU acknowledges an interrupt. Not doing this causes some mcd-verificator
        // tests to fail
        match interrupt_level {
            1 => {
                self.graphics_coprocessor.acknowledge_interrupt();
            }
            2 => {
                self.sega_cd_mut().registers.software_interrupt_pending = false;
            }
            3 => {
                self.sega_cd_mut().registers.timer_interrupt_pending = false;
            }
            4 => {
                self.sega_cd_mut().cdd_mut().acknowledge_interrupt();
            }
            5 => {
                self.sega_cd_mut().cdc_mut().acknowledge_interrupt();
            }
            _ => {}
        }
    }

    #[inline]
    fn halt(&self) -> bool {
        self.sega_cd().registers.sub_cpu_busreq
    }

    #[inline]
    fn reset(&self) -> bool {
        self.sega_cd().registers.sub_cpu_reset
    }
}
