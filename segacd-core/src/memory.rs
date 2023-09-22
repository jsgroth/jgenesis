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
        }
    }

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
            0xA12006 => (self.registers.h_interrupt_vector >> 8) as u8,
            0xA12007 => self.registers.h_interrupt_vector as u8,
            _ => 0,
        }
    }

    fn read_main_cpu_register_word(&mut self, address: u32) -> u16 {
        log::trace!("Main CPU register word read: {address:06X}");
        match address {
            0xA12000 | 0xA12002 => u16::from_be_bytes([
                self.read_main_cpu_register_byte(address),
                self.read_main_cpu_register_byte(address | 1),
            ]),
            0xA12006 => self.registers.h_interrupt_vector,
            _ => 0,
        }
    }

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
            0xA12006..=0xA12007 => panic!("byte-wide write to HINT vector register"),
            _ => {}
        }
    }

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
            _ => {}
        }
    }

    fn write_prg_ram(&mut self, address: u32, value: u8) {
        if address >= u32::from(self.registers.prg_ram_write_protect) * 0x200 {
            self.prg_ram[address as usize] = value;
        }
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
    fn read_register_byte(&mut self, address: u32) -> u8 {
        log::trace!("Sub CPU register byte read: {address:06X}");
        match address {
            0xFF8032 => {
                // Interrupt mask control, high byte (unused)
                0x00
            }
            0xFF8033 => {
                // Interrupt mask control, low byte
                let sega_cd = self.memory.medium();
                (u8::from(sega_cd.registers.subcode_interrupt_enabled) << 6)
                    | (u8::from(sega_cd.registers.cdc_interrupt_enabled) << 5)
                    | (u8::from(sega_cd.registers.cdd_interrupt_enabled) << 4)
                    | (u8::from(sega_cd.registers.timer_interrupt_enabled) << 3)
                    | (u8::from(sega_cd.registers.software_interrupt_enabled) << 2)
                    | (u8::from(sega_cd.registers.graphics_interrupt_enabled) << 1)
            }
            0xFF8036 => {
                // CDD control, high byte
                todo!("CDD control high byte")
            }
            0xFF8037 => {
                // CDD control, low byte
                // TODO DRS/DTS bits
                let sega_cd = self.memory.medium();
                u8::from(sega_cd.registers.cdd_host_clock_on) << 2
            }
            _ => 0,
        }
    }

    fn read_register_word(&mut self, address: u32) -> u16 {
        log::trace!("Sub CPU register word read: {address:06X}");
        match address {
            0xFF8032 => {
                // Interrupt mask control
                self.read_register_byte(address + 1).into()
            }
            0xFF8036 => {
                // CDD control
                let msb = self.read_register_byte(address);
                let lsb = self.read_register_byte(address + 1);
                u16::from_be_bytes([msb, lsb])
            }
            _ => 0,
        }
    }

    fn write_register_byte(&mut self, address: u32, value: u8) {
        log::trace!("Sub CPU register byte write: {address:06X}");
        match address {
            0xFF8032 => {
                // Interrupt mask control, high byte (unused)
            }
            0xFF8033 => {
                // Interrupt mask control, low byte
                let sega_cd = self.memory.medium_mut();
                sega_cd.registers.subcode_interrupt_enabled = value.bit(6);
                sega_cd.registers.cdc_interrupt_enabled = value.bit(5);
                sega_cd.registers.cdd_interrupt_enabled = value.bit(4);
                sega_cd.registers.timer_interrupt_enabled = value.bit(3);
                sega_cd.registers.software_interrupt_enabled = value.bit(2);
                sega_cd.registers.graphics_interrupt_enabled = value.bit(1);

                log::trace!("  Interrupt mask write: {value:08b}");
            }
            0xFF8036 => {
                // CDD control, high byte (writes do nothing)
            }
            0xFF8037 => {
                // CDD control, low byte
                self.memory.medium_mut().registers.cdd_host_clock_on = value.bit(2);

                log::trace!("  CDD control write: {value:02X}");
            }
            _ => {}
        }
    }

    fn write_register_word(&mut self, address: u32, value: u16) {
        log::trace!("Sub CPU word write: {address:06X}");
        match address {
            0xFF8032 => {
                // Interrupt mask control
                self.write_register_byte(address + 1, value as u8);
            }
            0xFF8036 => {
                // CDD control
                self.write_register_byte(address + 1, value as u8);
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
        if sega_cd.registers.software_interrupt_enabled
            && sega_cd.registers.software_interrupt_pending
        {
            2
        } else {
            0
        }
    }

    #[inline]
    fn acknowledge_interrupt(&mut self) {
        todo!("sub bus acknowledge interrupt")
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
