mod wordram;

use crate::cddrive::cdd::CdDrive;
use crate::cddrive::CdController;
use crate::cdrom::reader::CdRom;
use crate::graphics::GraphicsCoprocessor;
use crate::rf5c164::Rf5c164;
use bincode::{Decode, Encode};
use genesis_core::memory::{Memory, PhysicalMedium};
use genesis_core::GenesisRegion;
use jgenesis_traits::num::GetBit;
use m68000_emu::BusInterface;
use std::mem;
use wordram::{WordRam, WordRamMode};

const PRG_RAM_LEN: usize = 512 * 1024;
const PCM_RAM_LEN: usize = 16 * 1024;
const BACKUP_RAM_LEN: usize = 8 * 1024;

const TIMER_DIVIDER: u64 = 1536;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum WordRamPriorityMode {
    Off,
    Overwrite,
    Underwrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum CdcDeviceDestination {
    MainCpuRead,
    SubCpuRead,
    PcmRamDma,
    PrgRamDma,
    WordRamDma,
}

#[derive(Debug, Clone, Encode, Decode)]
struct SegaCdRegisters {
    // $FF8000/$A12000: Reset / BUSREQ
    software_interrupt_pending: bool,
    sub_cpu_busreq: bool,
    sub_cpu_reset: bool,
    // $FF8002/$A12002: Memory mode / PRG RAM bank select
    prg_ram_write_protect: u8,
    prg_ram_bank: u8,
    word_ram_priority_mode: WordRamPriorityMode,
    word_ram_mode: WordRamMode,
    dmna: bool,
    ret: bool,
    // $FF8004: CDC mode & register address
    cdc_device_destination: CdcDeviceDestination,
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
}

impl SegaCdRegisters {
    fn new() -> Self {
        Self {
            software_interrupt_pending: false,
            sub_cpu_busreq: true,
            sub_cpu_reset: true,
            prg_ram_write_protect: 0,
            prg_ram_bank: 0,
            word_ram_priority_mode: WordRamPriorityMode::Off,
            word_ram_mode: WordRamMode::TwoM,
            dmna: false,
            ret: true,
            cdc_device_destination: CdcDeviceDestination::MainCpuRead,
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
        }
    }

    fn prg_ram_addr(&self, address: u32) -> u32 {
        (u32::from(self.prg_ram_bank) << 17) | (address & 0x1FFFF)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SegaCd {
    bios: Vec<u8>,
    disc_drive: CdController,
    prg_ram: Box<[u8; PRG_RAM_LEN]>,
    word_ram: WordRam,
    pcm_ram: Box<[u8; PCM_RAM_LEN]>,
    backup_ram: Box<[u8; BACKUP_RAM_LEN]>,
    backup_ram_dirty: bool,
    registers: SegaCdRegisters,
    timer_divider: u64,
}

impl SegaCd {
    pub fn new(bios: Vec<u8>, disc: CdRom) -> Self {
        Self {
            bios,
            disc_drive: CdController::new(CdDrive::new(Some(disc))),
            prg_ram: vec![0; PRG_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            word_ram: WordRam::new(),
            pcm_ram: vec![0; PCM_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            backup_ram: vec![0; BACKUP_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            backup_ram_dirty: false,
            registers: SegaCdRegisters::new(),
            timer_divider: TIMER_DIVIDER,
        }
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
                (self.registers.prg_ram_bank << 6) | self.word_ram.control_read()
            }
            0xA12004 => {
                // TODO CDC mode
                0
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
                // TODO CDC host data, high byte
                0
            }
            0xA12009 => {
                // TODO CDC host data, low byte
                0
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
                // TODO CDC host data
                0
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
        log::trace!("Main CPU register byte write: {address:06X}");
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
                self.word_ram.main_cpu_control_write(value);

                log::trace!("  PRG RAM bank: {}", self.registers.prg_ram_bank);
                log::trace!("  Word RAM mode: {:?}", self.registers.word_ram_mode);
                log::trace!("  DMNA: {}", self.registers.dmna);
            }
            0xA12006..=0xA12007 => {
                self.registers.h_interrupt_vector = u16::from_le_bytes([value, value]);
            }
            0xA1200E => {
                self.registers.main_cpu_communication_flags = value;
            }
            0xA12010..=0xA1201F => {
                // Communication command buffers
                let idx = (address & 0xF) >> 1;
                let commands = &mut self.registers.communication_commands;
                let existing_word = commands[idx as usize];
                if address.bit(1) {
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
        log::trace!("Main CPU register word write: {address:06X}");
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

    fn write_prg_ram(&mut self, address: u32, value: u8) {
        if address >= u32::from(self.registers.prg_ram_write_protect) * 0x200 {
            self.prg_ram[address as usize] = value;
        }
    }

    pub fn tick(&mut self, master_clock_cycles: u64) {
        if master_clock_cycles >= self.timer_divider {
            self.clock_timers();
            self.timer_divider = TIMER_DIVIDER - (master_clock_cycles - self.timer_divider);
        } else {
            self.timer_divider -= master_clock_cycles;
        }
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
}

impl PhysicalMedium for SegaCd {
    type Rom = CdRom;

    #[inline]
    fn read_byte(&mut self, address: u32) -> u8 {
        match address {
            0x000000..=0x01FFFF => {
                // BIOS
                self.bios[address as usize]
            }
            0x020000..=0x03FFFF => {
                // PRG RAM
                let prg_ram_addr = self.registers.prg_ram_addr(address);
                self.prg_ram[prg_ram_addr as usize]
            }
            0x200000..=0x23FFFF => self.word_ram.main_cpu_ram_read(address),
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

                // Hack: If reading the second word of the interrupt vector for level 2 (HINT),
                // ignore what's in BIOS and return the current contents of $A12006
                if address == 0x000072 {
                    self.registers.h_interrupt_vector
                } else {
                    let msb = self.bios[address as usize];
                    let lsb = self.bios[(address + 1) as usize];
                    u16::from_be_bytes([msb, lsb])
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
                let msb = self.word_ram.main_cpu_ram_read(address);
                let lsb = self.word_ram.main_cpu_ram_read(address | 1);
                u16::from_be_bytes([msb, lsb])
            }
            0xA12000..=0xA1202F => {
                // Sega CD registers
                self.read_main_cpu_register_word(address)
            }
            _ => todo!("read word: {address:06X}"),
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
                self.write_prg_ram(prg_ram_addr, value);
            }
            0x200000..=0x23FFFF => {
                self.word_ram.main_cpu_ram_write(address, value);
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
                self.write_prg_ram(prg_ram_addr, msb);
                self.write_prg_ram(prg_ram_addr + 1, lsb);
            }
            0x200000..=0x23FFFF => {
                let [msb, lsb] = value.to_be_bytes();
                self.word_ram.main_cpu_ram_write(address, msb);
                self.word_ram.main_cpu_ram_write(address | 1, lsb);
            }
            0xA12000..=0xA1202F => {
                self.write_main_cpu_register_word(address, value);
            }
            _ => todo!("write word: {address:06X}, {value:04X}"),
        }
    }

    fn clone_without_rom(&self) -> Self {
        todo!("clone without ROM")
    }

    fn take_rom(&mut self) -> Self::Rom {
        todo!("take ROM")
    }

    fn take_rom_from(&mut self, other: &mut Self) {
        todo!("take ROM from")
    }

    #[inline]
    fn external_ram(&self) -> &[u8] {
        self.backup_ram.as_slice()
    }

    #[inline]
    fn is_ram_persistent(&self) -> bool {
        true
    }

    fn take_ram_if_persistent(&mut self) -> Option<Vec<u8>> {
        let ram_box = mem::replace(
            &mut self.backup_ram,
            vec![0; BACKUP_RAM_LEN].into_boxed_slice().try_into().unwrap(),
        );
        Some(<[u8]>::into_vec(ram_box))
    }

    #[inline]
    fn get_and_clear_ram_dirty(&mut self) -> bool {
        let dirty = self.backup_ram_dirty;
        self.backup_ram_dirty = false;
        dirty
    }

    fn program_title(&self) -> String {
        // TODO
        "PLACEHOLDER".into()
    }

    fn region(&self) -> GenesisRegion {
        // TODO
        GenesisRegion::Americas
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
                // TODO LEDs
                0
            }
            0xFF8001 => {
                // Reset
                // TODO version
                0x01
            }
            0xFF8002 => {
                // PRG RAM write protect
                self.memory.medium().registers.prg_ram_write_protect
            }
            0xFF8003 => {
                // Memory mode
                // TODO word RAM graphics write priority
                self.memory.medium().word_ram.control_read()
            }
            0xFF8004 => {
                // TODO CDC mode
                0x00
            }
            0xFF8005 => {
                // TODO CDC register address
                0x00
            }
            0xFF8007 => {
                // TODO CDC register data
                0x00
            }
            0xFF8008 => {
                // TODO CDC host data, high byte
                0x00
            }
            0xFF8009 => {
                // TODO CDC host data, low byte
                0x00
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
                if address.bit(1) { word as u8 } else { (word >> 8) as u8 }
            }
            0xFF8020..=0xFF802F => {
                // Communication status buffers
                let idx = (address & 0xF) >> 1;
                let word = self.memory.medium().registers.communication_statuses[idx as usize];
                if address.bit(1) { word as u8 } else { (word >> 8) as u8 }
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
            0xFF8034 => {
                // TODO CDD fader, high byte
                0x00
            }
            0xFF8035 => {
                // TODO CDD fader, low byte
                0x00
            }
            0xFF8036 => {
                // TODO CDD control, high byte
                0x00
            }
            0xFF8037 => {
                // CDD control, low byte
                // TODO DRS/DTS bits
                let sega_cd = self.memory.medium();
                u8::from(sega_cd.registers.cdd_host_clock_on) << 2
            }
            0xFF8038..=0xFF804B => {
                // TODO CDD communication buffers
                0x00
            }
            0xFF804C..=0xFF8067 => {
                // TODO graphics registers
                0x00
            }
            _ => 0,
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
                // TODO CDC host data
                0x0000
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
                // TODO CDD fader
                0x0000
            }
            0xFF8038..=0xFF804B => {
                // TODO CDD communication buffers
                0x0000
            }
            0xFF804C..=0xFF8067 => {
                // TODO graphics registers
                0x0000
            }
            _ => 0,
        }
    }

    #[allow(clippy::match_same_arms)]
    fn write_register_byte(&mut self, address: u32, value: u8) {
        log::trace!("Sub CPU register byte write: {address:06X} {value:02X}");
        match address {
            0xFF8003 => {
                // Memory mode
                // TODO word RAM graphics priority mode
                self.memory.medium_mut().word_ram.sub_cpu_control_write(value);
            }
            0xFF8004 => {
                // TODO CDC mode
            }
            0xFF8005 => {
                // TODO CDC register address
            }
            0xFF8007 => {
                // TODO CDC register data
            }
            0xFF800A => {
                // TODO CDC DMA address
            }
            0xFF800C..=0xFF800D => {
                // Stopwatch (12 bits)
                self.memory.medium_mut().registers.stopwatch_counter =
                    u16::from_be_bytes([value, value]) & 0x0FFF;
            }
            0xFF800F => {
                // Communication flags, low byte (sub CPU)
                self.memory.medium_mut().registers.sub_cpu_communication_flags = value;
            }
            0xFF8020..=0xFF802F => {
                // Communication status buffers
                let idx = (address & 0xF) >> 1;
                let statuses = &mut self.memory.medium_mut().registers.communication_statuses;
                let existing_word = statuses[idx as usize];
                if address.bit(1) {
                    statuses[idx as usize] = (existing_word & 0xFF00) | u16::from(value);
                } else {
                    statuses[idx as usize] = (existing_word & 0x00FF) | (u16::from(value) << 8);
                }
            }
            0xFF8031 => {
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

                log::trace!("  Interrupt mask write: {value:08b}");
            }
            0xFF8034..=0xFF8035 => {
                // TODO CDD fader
            }
            0xFF8037 => {
                // CDD control
                self.memory.medium_mut().registers.cdd_host_clock_on = value.bit(2);

                log::trace!("  CDD control write: {value:02X}");
            }
            0xFF8038..=0xFF804B => {
                // TODO CDD communication buffers
            }
            0xFF804C..=0xFF8067 => {
                // TODO graphics registers
            }
            _ => {}
        }
    }

    #[allow(clippy::match_same_arms)]
    fn write_register_word(&mut self, address: u32, value: u16) {
        log::trace!("Sub CPU register word write: {address:06X} {value:04X}");
        match address {
            0xFF8004 => {
                let [msb, lsb] = value.to_be_bytes();
                self.write_register_byte(address, msb);
                self.write_register_byte(address | 1, lsb);
            }
            0xFF8002 => {
                // Memory mode, only low byte is writable
                self.write_register_byte(address | 1, value as u8);
            }
            0xFF8006 => {
                // CDC data, only low byte is writable
                self.write_register_byte(address | 1, value as u8);
            }
            0xFF800A => {
                // TODO CDC DMA address
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
                // TODO CDD fader
            }
            0xFF8036 => {
                // CDD control, only low byte is writable
                self.write_register_byte(address | 1, value as u8);
            }
            0xFF8038..=0xFF804B => {
                // TODO CDD communication buffers
            }
            0xFF804C..=0xFF8067 => {
                // TODO graphics registers
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
                self.memory.medium().word_ram.sub_cpu_ram_read(address)
            }
            0xFE0000..=0xFE3FFF => {
                // Backup RAM (odd addresses)
                if address.bit(0) {
                    let backup_ram_addr = (address & 0x3FFF) >> 1;
                    self.memory.medium().backup_ram[backup_ram_addr as usize]
                } else {
                    0x00
                }
            }
            0xFF0000..=0xFFFFFF => {
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
                let msb = word_ram.sub_cpu_ram_read(address);
                let lsb = word_ram.sub_cpu_ram_read(address | 1);
                u16::from_be_bytes([msb, lsb])
            }
            0xFE0000..=0xFE3FFF => {
                // Backup RAM (odd addresses)
                let backup_ram_addr = (address & 0x3FFF) >> 1;
                self.memory.medium().backup_ram[backup_ram_addr as usize].into()
            }
            0xFF0000..=0xFFFFFF => {
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
                self.memory.medium_mut().write_prg_ram(address, value);
            }
            0x080000..=0x0DFFFF => {
                // Word RAM
                self.memory.medium_mut().word_ram.sub_cpu_ram_write(address, value);
            }
            0xFE0000..=0xFE3FFF => {
                // Backup RAM (odd addresses)
                if address.bit(0) {
                    let backup_ram_addr = (address & 0x3FFF) >> 1;
                    let sega_cd = self.memory.medium_mut();
                    sega_cd.backup_ram[backup_ram_addr as usize] = value;
                    sega_cd.backup_ram_dirty = true;
                }
            }
            0xFF0000..=0xFFFFFF => {
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
                sega_cd.write_prg_ram(address, msb);
                sega_cd.write_prg_ram(address + 1, lsb);
            }
            0x080000..=0x0DFFFF => {
                // Word RAM
                let [msb, lsb] = value.to_be_bytes();
                let word_ram = &mut self.memory.medium_mut().word_ram;
                word_ram.sub_cpu_ram_write(address, msb);
                word_ram.sub_cpu_ram_write(address | 1, lsb);
            }
            0xFE0000..=0xFE3FFF => {
                // Backup RAM (odd addresses)
                let backup_ram_addr = (address & 0x3FFF) >> 1;
                let sega_cd = self.memory.medium_mut();
                sega_cd.backup_ram[backup_ram_addr as usize] = value as u8;
                sega_cd.backup_ram_dirty = true;
            }
            0xFF0000..=0xFFFFFF => {
                // Sub CPU registers
                self.write_register_word(address, value);
            }
            _ => todo!("sub bus read word {address:06X} {value:04X}"),
        }
    }

    #[inline]
    fn interrupt_level(&self) -> u8 {
        // TODO other interrupts
        let sega_cd = self.memory.medium();
        if sega_cd.registers.timer_interrupt_enabled && sega_cd.registers.timer_interrupt_pending {
            3
        } else if sega_cd.registers.software_interrupt_enabled
            && sega_cd.registers.software_interrupt_pending
        {
            2
        } else {
            0
        }
    }

    #[inline]
    fn acknowledge_interrupt(&mut self) {
        // TODO other interrupts
        match self.interrupt_level() {
            2 => {
                self.memory.medium_mut().registers.software_interrupt_pending = false;
            }
            3 => {
                self.memory.medium_mut().registers.timer_interrupt_pending = false;
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
