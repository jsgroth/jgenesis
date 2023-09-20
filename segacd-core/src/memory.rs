use crate::cddrive::cdd::CdDrive;
use crate::cddrive::CdController;
use crate::cdrom::reader::CdRom;
use crate::graphics::GraphicsCoprocessor;
use crate::rf5c164::Rf5c164;
use genesis_core::memory::{Memory, PhysicalMedium};
use genesis_core::GenesisRegion;
use jgenesis_traits::num::GetBit;
use m68000_emu::BusInterface;
use std::mem;

const PRG_RAM_LEN: usize = 512 * 1024;
const WORD_RAM_LEN: usize = 256 * 1024;
const PCM_RAM_LEN: usize = 16 * 1024;
const BACKUP_RAM_LEN: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordRamMode {
    TwoM,
    OneM,
}

impl WordRamMode {
    fn to_bit(self) -> bool {
        self == Self::OneM
    }

    fn from_bit(bit: bool) -> Self {
        if bit { Self::OneM } else { Self::TwoM }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordRamPriorityMode {
    Off,
    Overwrite,
    Underwrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CdcDeviceDestination {
    MainCpuRead,
    SubCpuRead,
    PcmRamDma,
    PrgRamDma,
    WordRamDma,
}

#[derive(Debug, Clone)]
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
        }
    }

    fn prg_ram_addr(&self, address: u32) -> u32 {
        (u32::from(self.prg_ram_bank) << 17) | (address & 0x1FFFF)
    }
}

#[derive(Debug, Clone)]
pub struct SegaCd {
    bios: Vec<u8>,
    disc_drive: CdController,
    prg_ram: Box<[u8; PRG_RAM_LEN]>,
    word_ram: Box<[u8; WORD_RAM_LEN]>,
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
            word_ram: vec![0; WORD_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            pcm_ram: vec![0; PCM_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            backup_ram: vec![0; BACKUP_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            backup_ram_dirty: false,
            registers: SegaCdRegisters::new(),
        }
    }

    fn read_main_cpu_register_byte(&mut self, address: u32) -> u8 {
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
                // TODO fix DMNA / RET
                (self.registers.prg_ram_bank << 6)
                    | (u8::from(self.registers.word_ram_mode.to_bit()) << 2)
                    | (u8::from(self.registers.dmna) << 1)
                    | u8::from(self.registers.ret)
            }
            0xA12006..=0xA12007 => panic!("byte-wide read of HINT vector register"),
            _ => todo!("main CPU byte register read at {address:06X}"),
        }
    }

    fn read_main_cpu_register_word(&mut self, address: u32) -> u16 {
        match address {
            0xA12000 | 0xA12002 => u16::from_be_bytes([
                self.read_main_cpu_register_byte(address),
                self.read_main_cpu_register_byte(address | 1),
            ]),
            0xA12006 => self.registers.h_interrupt_vector,
            _ => todo!("main CPU word register read at {address:06X}"),
        }
    }

    fn write_main_cpu_register_byte(&mut self, address: u32, value: u8) {
        match address {
            0xA12000 => {
                // Initialization / reset, high byte
                self.registers.software_interrupt_pending = value.bit(0);
            }
            0xA12001 => {
                // Initialization / reset, low byte
                self.registers.sub_cpu_busreq = value.bit(1);
                self.registers.sub_cpu_reset = !value.bit(0);
            }
            0xA12002 => {
                // Memory mode / write protect, high byte
                self.registers.prg_ram_write_protect = value;
            }
            0xA12003 => {
                // Memory mode / write protect, low byte
                self.registers.prg_ram_bank = value >> 6;
                self.registers.word_ram_mode = WordRamMode::from_bit(value.bit(2));
                // TODO handle DMNA / RET
                self.registers.dmna = value.bit(1);
            }
            0xA12006..=0xA12007 => panic!("byte-wide write to HINT vector register"),
            _ => todo!("main CPU register write at {address:06X}, value {value:02X}"),
        }
    }

    fn write_main_cpu_register_word(&mut self, address: u32, value: u16) {
        match address {
            0xA12000 | 0xA12002 => {
                let [msb, lsb] = value.to_be_bytes();
                self.write_main_cpu_register_byte(address, msb);
                self.write_main_cpu_register_byte(address | 1, lsb);
            }
            0xA12006 => {
                self.registers.h_interrupt_vector = value;
            }
            _ => todo!("main CPU word register write at {address:06X}, value {value:04X}"),
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
            0x200000..=0x23FFFF => todo!("word RAM byte read {address:06X}"),
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
            0x200000..=0x23FFFF => todo!("word RAM word read {address:06X}"),
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
            0x200000..=0x23FFFF => todo!("word RAM byte write {address:06X} {value:02X}"),
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
            0x200000..=0x23FFFF => todo!("word RAM word write {address:06X} {value:04X}"),
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

impl<'a> BusInterface for SubBus<'a> {
    #[inline]
    fn read_byte(&mut self, address: u32) -> u8 {
        match address {
            0x000000..=0x07FFFF => {
                // PRG RAM
                self.memory.medium().prg_ram[address as usize]
            }
            _ => todo!("sub bus read byte {address:06X}"),
        }
    }

    #[inline]
    fn read_word(&mut self, address: u32) -> u16 {
        match address {
            0x000000..=0x07FFFF => {
                // PRG RAM
                let sega_cd = self.memory.medium();
                let msb = sega_cd.prg_ram[address as usize];
                let lsb = sega_cd.prg_ram[(address + 1) as usize];
                u16::from_be_bytes([msb, lsb])
            }
            _ => todo!("sub bus read word {address:06X}"),
        }
    }

    #[inline]
    fn write_byte(&mut self, address: u32, value: u8) {
        todo!("sub bus read byte {address:06X} {value:02X}")
    }

    #[inline]
    fn write_word(&mut self, address: u32, value: u16) {
        todo!("sub bus read word {address:06X} {value:04X}")
    }

    #[inline]
    fn interrupt_level(&self) -> u8 {
        todo!("sub bus interrupt level")
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
