use crate::api::{SegaCdAudioOutput, SegaCdLoadResult};
use crate::cddrive::cdc::{DeviceDestination, Rchip};
use crate::cddrive::cdd::{CdDrive, CdModel};
use crate::cddrive::{CdController, cdc};
use crate::font::FontRegisters;
use crate::graphics::GraphicsCoprocessor;
use crate::memory::{BACKUP_RAM_LEN, Bios, PRG_RAM_LEN, RAM_CARTRIDGE_LEN, SegaCdRegisters};
use crate::rf5c164::Rf5c164;
use crate::{ScdCpu, WordRam, api, backupram};
use bincode::{Decode, Encode};
use cdrom::reader::CdRom;
use genesis_config::{GenesisEmulatorConfig, GenesisRegion};
use jgenesis_common::boxedarray::BoxedByteArray;
use jgenesis_common::num::{GetBit, U16Ext};
use jgenesis_proc_macros::PartialClone;
use m68000_emu::BusInterface;
use m68000_emu::debug::DummyM68000Debugger;
use std::mem;

pub mod debug;

// RAM cartridge size byte is N in the formula 8KB * 2^N
// N=4 signals 128KB
const RAM_CARTRIDGE_SIZE_BYTE: u8 = 0x04;

const TIMER_DIVIDER: u64 = 1536;

const SUB_REGISTER_ADDRESS_MASK: u32 = 0x1FF;

#[derive(Debug, Clone, Copy, Encode, Decode)]
enum BufferedWrite {
    Byte(u8),
    Word(u16),
}

#[derive(Debug, Encode, Decode, PartialClone)]
pub struct SegaCdBus {
    pub graphics_coprocessor: GraphicsCoprocessor,
    pub pcm: Rf5c164,
    #[partial_clone(default)]
    bios: Bios,
    #[partial_clone(partial)]
    disc_drive: CdController,
    prg_ram: BoxedByteArray<PRG_RAM_LEN>,
    word_ram: WordRam,
    backup_ram: BoxedByteArray<BACKUP_RAM_LEN>,
    enable_ram_cartridge: bool,
    ram_cartridge: BoxedByteArray<RAM_CARTRIDGE_LEN>,
    ram_cartridge_writes_enabled: bool,
    backup_ram_dirty: bool,
    registers: SegaCdRegisters,
    font_registers: FontRegisters,
    disc_region: GenesisRegion,
    forced_region: Option<GenesisRegion>,
    timer_divider: u64,
    buffered_sub_register_writes: Vec<(u32, BufferedWrite)>,
}

impl SegaCdBus {
    pub fn new(
        bios: Vec<u8>,
        mut disc: Option<CdRom>,
        initial_backup_ram: Option<Vec<u8>>,
        initial_ram_cartridge: Option<Vec<u8>>,
        config: &GenesisEmulatorConfig,
    ) -> SegaCdLoadResult<Self> {
        let (backup_ram, ram_cartridge) = backupram::load_initial_backup_ram(
            initial_backup_ram.as_ref(),
            initial_ram_cartridge.as_ref(),
        );

        let disc_region = match &mut disc {
            Some(disc) => api::parse_disc_region(disc)?,
            None => {
                // Default to US if no disc provided
                GenesisRegion::Americas
            }
        };

        log::info!("Region parsed from disc header: {disc_region:?}");

        let cd_model = guess_cd_model(&bios);
        log::info!("Detected CD model {cd_model:?} based on BIOS ROM");

        Ok(Self {
            graphics_coprocessor: GraphicsCoprocessor::new(),
            pcm: Rf5c164::new(&config.sega_cd),
            bios: Bios(bios.into_boxed_slice()),
            disc_drive: CdController::new(disc, cd_model, &config.sega_cd),
            prg_ram: BoxedByteArray::new(),
            word_ram: WordRam::new(),
            backup_ram: backup_ram.into(),
            enable_ram_cartridge: config.sega_cd.enable_ram_cartridge,
            ram_cartridge: ram_cartridge.into(),
            ram_cartridge_writes_enabled: true,
            backup_ram_dirty: false,
            registers: SegaCdRegisters::new(),
            font_registers: FontRegisters::new(),
            disc_region,
            forced_region: config.forced_region,
            timer_divider: TIMER_DIVIDER,
            buffered_sub_register_writes: Vec::with_capacity(5),
        })
    }

    #[inline]
    pub fn tick_components(
        &mut self,
        mclk_cycles: u64,
        pcm_cycles: u64,
        audio_output: &mut impl SegaCdAudioOutput,
    ) -> SegaCdLoadResult<()> {
        // CDC DMA can only write to PRG RAM while the sub CPU is on the bus
        let prg_ram_accessible = !(self.registers.sub_cpu_busreq || self.registers.sub_cpu_reset);
        self.disc_drive.tick(
            mclk_cycles,
            &mut self.word_ram,
            &mut self.prg_ram,
            prg_ram_accessible,
            &mut self.pcm,
            |sample_l, sample_r| audio_output.collect_cd((sample_l, sample_r)),
        )?;

        self.tick_timers(mclk_cycles);

        if !self.word_ram.is_sub_access_blocked() {
            self.graphics_coprocessor.tick(
                mclk_cycles,
                &mut self.word_ram,
                self.registers.graphics_interrupt_enabled,
            );
        }

        self.pcm.tick(pcm_cycles, |sample| audio_output.collect_pcm(sample));

        Ok(())
    }

    fn tick_timers(&mut self, mut mclk_cycles: u64) {
        while mclk_cycles >= self.timer_divider {
            self.clock_timers();
            mclk_cycles -= self.timer_divider;
            self.timer_divider = TIMER_DIVIDER;
        }
        self.timer_divider -= mclk_cycles;
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

    // $000000-$1FFFFF: BIOS at $000000-$01FFFF, PRG RAM at $020000-$03FFFF, mirrored repeatedly
    pub fn main_read_bios_prg_ram(&self, address: u32) -> u8 {
        if address & 0x20000 == 0 {
            // BIOS ROM
            // HINT vector ($000070-$000073) should read out the register value
            match address & 0x1FFFF {
                0x70 | 0x71 => 0xFF,
                0x72 => self.registers.h_interrupt_vector.msb(),
                0x73 => self.registers.h_interrupt_vector.lsb(),
                bios_addr => self.bios[bios_addr as usize],
            }
        } else {
            // PRG RAM
            let prg_ram_addr = self.registers.main_prg_ram_addr(address);
            self.prg_ram[prg_ram_addr as usize]
        }
    }

    // $000000-$1FFFFF: BIOS at $000000-$01FFFF, PRG RAM at $020000-$03FFFF, mirrored repeatedly
    pub fn main_write_bios_prg_ram(&mut self, address: u32, value: u8) {
        if address & 0x20000 != 0 {
            // PRG RAM
            let prg_ram_addr = self.registers.main_prg_ram_addr(address);
            self.write_prg_ram(prg_ram_addr, value, ScdCpu::Main);
        } // else BIOS ROM, ignore
    }

    fn write_prg_ram(&mut self, address: u32, value: u8, cpu: ScdCpu) {
        if cpu == ScdCpu::Main && !(self.registers.sub_cpu_busreq || self.registers.sub_cpu_reset) {
            // The Genesis hardware cannot write to PRG RAM while the sub CPU is on the bus.
            // Dungeon Explorer depends on this or the Z80 will trash PRG RAM while the sub CPU is using it
            log::trace!(
                "Main CPU write to PRG RAM without removing sub CPU from bus: {address:06X} {value:02X}"
            );
            return;
        }

        // PRG RAM write protection applies in multiples of $200
        let write_protection_boundary = u32::from(self.registers.prg_ram_write_protect) * 0x200;

        // PRG RAM write protection only applies to the Sub CPU.
        // The JP V2.00 BIOS freezes if Main CPU writes to PRG RAM are not always allowed through
        if cpu == ScdCpu::Main || address >= write_protection_boundary {
            self.prg_ram[address as usize] = value;
        }
    }

    // $200000-$3FFFFF: Word RAM
    pub fn main_read_word_ram(&self, address: u32) -> u8 {
        self.word_ram.main_cpu_read_ram(address)
    }

    // $200000-$3FFFFF: Word RAM
    pub fn main_write_word_ram(&mut self, address: u32, value: u8) {
        self.word_ram.main_cpu_write_ram(address, value);
    }

    // $400000-$7FFFFF: RAM cartridge
    pub fn read_ram_cartridge(&self, address: u32) -> u8 {
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

    // $400000-$7FFFFF: RAM cartridge
    pub fn write_ram_cartridge(&mut self, address: u32, value: u8) {
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

    // $A12000-$A1202F: Sega CD gate array registers
    pub fn main_read_register<const WORD: bool>(&mut self, address: u32) -> u16 {
        log::trace!("Main CPU register {} read: {address:06X}", if WORD { "word" } else { "byte" });

        let word = match address {
            0xA12000 | 0xA12001 => {
                // Initialization / reset
                (u16::from(self.registers.software_interrupt_enabled) << 15)
                    | (u16::from(self.registers.software_interrupt_pending) << 8)
                    | (u16::from(self.registers.sub_cpu_busreq) << 1)
                    | u16::from(!self.registers.sub_cpu_reset)
            }
            0xA12002 | 0xA12003 => {
                // Memory mode / write protect
                (u16::from(self.registers.prg_ram_write_protect) << 8)
                    | (u16::from(self.registers.prg_ram_bank) << 6)
                    | u16::from(self.word_ram.read_control())
            }
            0xA12004 | 0xA12005 => {
                log::trace!("  CDC mode read (main CPU)");
                let cdc = self.cdc();
                let end_of_data_transfer = cdc.end_of_data_transfer();
                let data_ready = cdc.data_ready();
                let dd_bits = cdc.device_destination().to_bits();

                (u16::from(end_of_data_transfer) << 15)
                    | (u16::from(data_ready) << 14)
                    | (u16::from(dd_bits) << 8)
            }
            0xA12006 | 0xA12007 => {
                // HINT vector
                self.registers.h_interrupt_vector
            }
            0xA12008 | 0xA12009 => {
                // CDC host data
                self.cdc_mut().read_host_data(ScdCpu::Main)
            }
            0xA1200C | 0xA1200D => {
                // Stopwatch
                self.registers.stopwatch_counter
            }
            0xA1200E | 0xA1200F => {
                // Communication flags
                u16::from_be_bytes([
                    self.registers.main_cpu_communication_flags,
                    self.registers.sub_cpu_communication_flags,
                ])
            }
            0xA12010..=0xA1201F => {
                // Communication command buffers
                self.registers.communication_commands[((address & 0xF) >> 1) as usize]
            }
            0xA12020..=0xA1202F => {
                // Communication status buffers
                self.registers.communication_statuses[((address & 0xF) >> 1) as usize]
            }
            _ => 0,
        };

        if WORD { word } else { word.be_byte(address & 1).into() }
    }

    // $A12000-$A1202F: Sega CD gate array registers
    pub fn main_write_register<const WORD: bool>(&mut self, address: u32, value: u16) {
        if WORD {
            log::trace!("Main CPU register word write: {address:06X} {value:04X}");
        } else {
            log::trace!("Main CPU register byte write: {address:06X} {:02X}", value & 0xFF);
        }

        let value_msb = if WORD { value.msb() } else { value as u8 };
        let value_lsb = value.lsb();

        match address {
            0xA12000 | 0xA12001 => {
                // Initialization / reset
                if WORD || !address.bit(0) {
                    self.registers.software_interrupt_pending = value_msb.bit(0);

                    log::trace!(
                        "  INT2 pending write: {}",
                        self.registers.software_interrupt_pending
                    );
                }

                if WORD || address.bit(0) {
                    self.registers.sub_cpu_busreq = value_lsb.bit(1);
                    self.registers.sub_cpu_reset = !value_lsb.bit(0);

                    log::trace!("  Sub CPU BUSREQ: {}", self.registers.sub_cpu_busreq);
                    log::trace!("  Sub CPU RESET: {}", self.registers.sub_cpu_reset);
                }
            }
            0xA12002 | 0xA12003 => {
                // Memory mode / write protect
                if WORD || !address.bit(0) {
                    self.registers.prg_ram_write_protect = value_msb;
                    log::trace!("  PRG RAM protect write: {value:02X}");
                }

                if WORD || address.bit(0) {
                    self.registers.prg_ram_bank = value_lsb >> 6;
                    self.word_ram.main_cpu_write_control(value_lsb);

                    log::trace!("  PRG RAM bank: {}", self.registers.prg_ram_bank);
                }
            }
            0xA12006 | 0xA12007 => {
                // HINT vector
                // Byte-size writes copy the byte into both halves
                self.registers.h_interrupt_vector =
                    if WORD { value } else { u16::from_ne_bytes([value as u8; 2]) };
            }
            0xA12008 | 0xA12009 => {
                // CDC host data
                self.cdc_mut().write_host_data(ScdCpu::Main);
            }
            0xA1200E | 0xA1200F => {
                // Communication flags; only main CPU flags are writable
                // Byte-size writes always write the flags regardless of address
                self.registers.main_cpu_communication_flags = value_msb;
            }
            0xA12010..=0xA1201F => {
                // Communication command buffers
                let idx = (address & 0xF) >> 1;
                let command = &mut self.registers.communication_commands[idx as usize];
                if WORD {
                    *command = value;
                } else if !address.bit(0) {
                    command.set_msb(value as u8);
                } else {
                    command.set_lsb(value as u8);
                }
            }
            _ => {}
        }
    }

    pub fn flush_buffered_sub_writes(&mut self) {
        if self.buffered_sub_register_writes.is_empty() {
            return;
        }

        let mut writes = mem::take(&mut self.buffered_sub_register_writes);
        for &(address, value) in &writes {
            match value {
                BufferedWrite::Byte(byte) => {
                    self.sub_write_register::<false>(address, byte.into());
                }
                BufferedWrite::Word(word) => {
                    self.sub_write_register::<true>(address, word);
                }
            }
        }

        writes.clear();
        self.buffered_sub_register_writes = writes;
    }

    #[allow(clippy::match_same_arms)]
    fn sub_read_register<const WORD: bool>(&mut self, address: u32) -> u16 {
        log::trace!("Sub CPU register {} read: {address:06X}", if WORD { "word" } else { "byte" });

        let word = match address & SUB_REGISTER_ADDRESS_MASK {
            0x000 | 0x001 => {
                // LED / reset
                // TODO version in bits 7-4
                // Bit 0 (CD drive operable) hardcoded to 1
                (u16::from(self.registers.led_green) << 9)
                    | (u16::from(self.registers.led_red) << 8)
                    | 1
            }
            0x002 | 0x003 => {
                // PRG RAM write protect / memory mode
                (u16::from(self.registers.prg_ram_write_protect) << 8)
                    | (u16::from(self.word_ram.priority_mode().to_bits()) << 3)
                    | u16::from(self.word_ram.read_control())
            }
            0x004 | 0x005 => {
                // CDC mode / register address
                log::trace!("  CDC mode read (sub CPU)");

                let cdc = self.cdc();
                let end_of_data_transfer = cdc.end_of_data_transfer();
                let data_ready = cdc.data_ready();
                let dd_bits = cdc.device_destination().to_bits();

                (u16::from(end_of_data_transfer) << 15)
                    | (u16::from(data_ready) << 14)
                    | (u16::from(dd_bits) << 8)
                    | u16::from(cdc.register_address())
            }
            0x006 | 0x007 if WORD || address.bit(0) => {
                // CDC register data
                self.cdc_mut().read_register().into()
            }
            0x008 | 0x009 => {
                // CDC host data
                self.cdc_mut().read_host_data(ScdCpu::Sub)
            }
            0x00A | 0x00B => {
                // CDC DMA address (bits 18-3)
                (self.cdc().dma_address() >> 3) as u16
            }
            0x00C | 0x00D => {
                // Stopwatch
                self.registers.stopwatch_counter
            }
            0x00E | 0x00F => {
                // Communication flags
                u16::from_be_bytes([
                    self.registers.main_cpu_communication_flags,
                    self.registers.sub_cpu_communication_flags,
                ])
            }
            0x010..=0x01F => {
                // Communication command buffers
                self.registers.communication_commands[((address & 0xF) >> 1) as usize]
            }
            0x020..=0x02F => {
                // Communication status buffers
                self.registers.communication_statuses[((address & 0xF) >> 1) as usize]
            }
            0x030 | 0x031 if WORD || address.bit(0) => {
                // Timer
                self.registers.timer_interval.into()
            }
            0x032 | 0x033 if WORD || address.bit(0) => {
                // Interrupt mask control
                (u16::from(self.registers.subcode_interrupt_enabled) << 6)
                    | (u16::from(self.registers.cdc_interrupt_enabled) << 5)
                    | (u16::from(self.registers.cdd_interrupt_enabled) << 4)
                    | (u16::from(self.registers.timer_interrupt_enabled) << 3)
                    | (u16::from(self.registers.software_interrupt_enabled) << 2)
                    | (u16::from(self.registers.graphics_interrupt_enabled) << 1)
            }
            0x034 | 0x035 => {
                // CDD fader, only bit 15 (fader processing) is readable and it's fine to always
                // set it to 0
                0
            }
            0x036 | 0x037 => {
                // CDD control
                (u16::from(!self.cdd().playing_audio()) << 8)
                    | (u16::from(self.registers.cdd_host_clock_on) << 2)
            }
            0x038..=0x041 => {
                // CDD status
                let relative_addr = ((address - 8) & 0xE) as usize;
                let cdd_status = self.cdd().status();
                u16::from_be_bytes([cdd_status[relative_addr], cdd_status[relative_addr + 1]])
            }
            0x042..=0x04B => {
                // CDD command
                let relative_addr = ((address - 2) & 0xE) as usize;
                let cdd_command = &self.registers.cdd_command;
                u16::from_be_bytes([cdd_command[relative_addr], cdd_command[relative_addr + 1]])
            }
            0x04C | 0x04D if WORD || address.bit(0) => {
                // Font color
                self.font_registers.read_color().into()
            }
            0x04E | 0x04F => {
                // Font bits
                self.font_registers.font_bits()
            }
            0x050..=0x057 => {
                // Font data
                self.font_registers.read_font_data(address)
            }
            0x058..=0x067 => {
                // Graphics coprocessor
                self.graphics_coprocessor.read_register(address)
            }
            _ => 0,
        };

        if WORD { word } else { word.be_byte(address & 1).into() }
    }

    fn sub_write_register<const WORD: bool>(&mut self, address: u32, value: u16) {
        if WORD {
            log::trace!("Sub CPU register word write: {address:06X} {value:04X}");
        } else {
            log::trace!("Sub CPU register byte write: {address:06X} {:02X}", value & 0xFF);
        }

        let value_msb = if WORD { value.msb() } else { value as u8 };
        let value_lsb = value.lsb();

        match address & SUB_REGISTER_ADDRESS_MASK {
            0x000 | 0x001 => {
                // LED / reset
                if WORD || !address.bit(0) {
                    self.registers.led_green = value_msb.bit(1);
                    self.registers.led_red = value_msb.bit(0);
                }

                if WORD || address.bit(0) {
                    log::trace!("  CDD reset write: {value_lsb:02X}");

                    if !value_lsb.bit(0) {
                        // TODO official documentation says that this reset takes about 100ms - unclear what happens during that time
                        self.cdd_mut().reset();
                    }
                }
            }
            0x002 | 0x003 => {
                // Memory mode
                self.word_ram.sub_cpu_write_control(value_lsb);
            }
            0x004 | 0x005 => {
                // CDC mode / register address
                if WORD || !address.bit(0) {
                    log::trace!("  CDC mode write: {value_msb:02X}");
                    let device_destination = DeviceDestination::from_bits(value_msb & 7);
                    self.cdc_mut().set_device_destination(device_destination);
                }

                if WORD || address.bit(0) {
                    log::trace!("  CDC register address write: {value_lsb:02X}");
                    let register_address = value_lsb & cdc::REGISTER_ADDRESS_MASK;
                    self.cdc_mut().set_register_address(register_address);
                }
            }
            0x006 | 0x007 if WORD || address.bit(0) => {
                // CDC register data
                log::trace!("  CDC register data write: {value_lsb:02X}");
                self.cdc_mut().write_register(value_lsb);
            }
            0x008 | 0x009 => {
                // CDC host data
                self.cdc_mut().write_host_data(ScdCpu::Sub);
            }
            0x00A | 0x00B => {
                // CDC DMA address (bits 18-3)
                let word = if WORD { value } else { u16::from_ne_bytes([value as u8; 2]) };
                let dma_address = u32::from(word) << 3;
                self.cdc_mut().set_dma_address(dma_address);
            }
            0x00C | 0x00D => {
                // Stopwatch (12 bits)
                let word = if WORD { value } else { u16::from_ne_bytes([value as u8; 2]) };
                self.registers.stopwatch_counter = word & 0xFFF;
            }
            0x00E | 0x00F => {
                // Communication flags
                // Only low byte (sub CPU) is writable, but byte-size writes always write the flags
                self.registers.sub_cpu_communication_flags = value as u8;
            }
            0x020..=0x02F => {
                // Communication status buffers
                let idx = (address & 0xF) >> 1;
                let status = &mut self.registers.communication_statuses[idx as usize];
                if WORD {
                    *status = value;
                } else if !address.bit(0) {
                    status.set_msb(value as u8);
                } else {
                    status.set_lsb(value as u8);
                }
            }
            0x030 | 0x031 => {
                // Timer
                self.registers.timer_interval = value as u8;
                self.registers.timer_counter = value as u8;
            }
            0x032 | 0x033 if WORD || address.bit(0) => {
                // Interrupt mask control
                self.registers.subcode_interrupt_enabled = value.bit(6);
                self.registers.cdc_interrupt_enabled = value.bit(5);
                self.registers.cdd_interrupt_enabled = value.bit(4);
                self.registers.timer_interrupt_enabled = value.bit(3);
                self.registers.software_interrupt_enabled = value.bit(2);
                self.registers.graphics_interrupt_enabled = value.bit(1);

                // Disabling the graphics interrupt should clear any pending interrupt
                if !self.registers.graphics_interrupt_enabled {
                    self.graphics_coprocessor.acknowledge_interrupt();
                }

                log::trace!("  Interrupt mask write: {value_lsb:08b}");
            }
            0x034 | 0x035 => {
                // CDD fader
                let word = if WORD { value } else { u16::from_ne_bytes([value as u8; 2]) };
                self.cdd_mut().set_fader_volume(word);

                log::trace!("  CDD fader write: {value:04X}");
            }
            0x036 | 0x037 if WORD || address.bit(0) => {
                // CDD control
                self.registers.cdd_host_clock_on = value.bit(2);
                log::trace!("  CDD control write: {value:02X}");
            }
            0x042..=0x04B => {
                // CDD command
                let relative_addr = ((address - 2) & 0xF) as usize;

                if WORD {
                    self.registers.cdd_command[relative_addr] = value.msb();
                    self.registers.cdd_command[relative_addr + 1] = value.lsb();
                } else {
                    self.registers.cdd_command[relative_addr] = value as u8;
                }

                // Writes to the last byte trigger a CDD command send
                if (WORD && relative_addr == 8) || (!WORD && relative_addr == 9) {
                    self.disc_drive.cdd_mut().send_command(self.registers.cdd_command);
                }
            }
            0x04C | 0x04D => {
                // Font color
                self.font_registers.write_color(value as u8);
            }
            0x04E | 0x04F => {
                // Font bits
                if WORD {
                    self.font_registers.write_font_bits(value);
                } else if !address.bit(0) {
                    self.font_registers.write_font_bits_msb(value as u8);
                } else {
                    self.font_registers.write_font_bits_lsb(value as u8);
                }
            }
            0x058..=0x067 => {
                // Graphics coprocessor
                if WORD {
                    self.graphics_coprocessor.write_register_word(address, value);
                } else {
                    self.graphics_coprocessor.write_register_byte(address, value as u8);
                }
            }
            _ => {}
        }
    }

    fn sub_read<const WORD: bool>(&mut self, address: u32) -> u16 {
        // Only A0-A19 are connected for the sub CPU:
        //   https://gendev.spritesmind.net/forum/viewtopic.php?p=18935#p18935
        match address & 0xFFFFF {
            0x00000..=0x7FFFF => {
                // PRG RAM
                if WORD {
                    let msb = self.prg_ram[address as usize];
                    let lsb = self.prg_ram[(address + 1) as usize];
                    u16::from_be_bytes([msb, lsb])
                } else {
                    self.prg_ram[address as usize].into()
                }
            }
            0x80000..=0xDFFFF => {
                // Word RAM
                if WORD {
                    let msb = self.word_ram.sub_cpu_read_ram(address);
                    let lsb = self.word_ram.sub_cpu_read_ram(address + 1);
                    u16::from_be_bytes([msb, lsb])
                } else {
                    self.word_ram.sub_cpu_read_ram(address).into()
                }
            }
            0xE0000..=0xEFFFF => {
                // Backup RAM (odd addresses)
                // Canonically located at $E0000-$E3FFF, mirrored up to $EFFFF
                if WORD || address.bit(0) {
                    let backup_ram_addr = (address & 0x3FFF) >> 1;
                    self.backup_ram[backup_ram_addr as usize].into()
                } else {
                    0
                }
            }
            0xF0000..=0xF7FFF => {
                // PCM sound chip (odd addresses)
                // Canonically located at $F0000-$F3FFF, mirrored at $F4000-$F7FFF
                if WORD || address.bit(0) {
                    self.pcm.read((address & 0x3FFF) >> 1).into()
                } else {
                    0
                }
            }
            0xF8000..=0xFFFFF => {
                // Sub CPU registers
                // Canonically located at $F8000-$F81FF, mirrored up to $FFFFF
                self.sub_read_register::<WORD>(address)
            }
            _ => unreachable!("Value & 0xFFFFF is always <= 0xFFFFF"),
        }
    }

    fn sub_write<const WORD: bool>(&mut self, address: u32, value: u16) {
        // Only A0-A19 are connected for the sub CPU:
        //   https://gendev.spritesmind.net/forum/viewtopic.php?p=18935#p18935
        match address & 0xFFFFF {
            0x00000..=0x7FFFF => {
                // PRG RAM
                if WORD {
                    self.write_prg_ram(address, value.msb(), ScdCpu::Sub);
                    self.write_prg_ram(address + 1, value.lsb(), ScdCpu::Sub);
                } else {
                    self.write_prg_ram(address, value as u8, ScdCpu::Sub);
                }
            }
            0x80000..=0xDFFFF => {
                // Word RAM
                if WORD {
                    self.word_ram.sub_cpu_write_ram(address, value.msb());
                    self.word_ram.sub_cpu_write_ram(address + 1, value.lsb());
                } else {
                    self.word_ram.sub_cpu_write_ram(address, value as u8);
                }
            }
            0xE0000..=0xEFFFF => {
                // Backup RAM (odd addresses)
                // Canonically located at $E0000-$E3FFF, mirrored up to $EFFFF
                if WORD || address.bit(0) {
                    let backup_ram_addr = (address & 0x3FFF) >> 1;
                    self.backup_ram[backup_ram_addr as usize] = value as u8;
                    self.backup_ram_dirty = true;
                }
            }
            0xF0000..=0xF7FFF => {
                // PCM sound chip (odd addresses)
                // Canonically located at $F0000-$F3FFF, mirrored at $F4000-$F7FFF
                if WORD || address.bit(0) {
                    self.pcm.write((address & 0x3FFF) >> 1, value as u8);
                }
            }
            0xF8000..=0xFFFFF => {
                // Sub CPU registers
                // Canonically located at $F8000-$F81FF, mirrored up to $FFFFF
                let register_addr = address & SUB_REGISTER_ADDRESS_MASK;
                if matches!(register_addr, 0x002 | 0x003) {
                    // Hack: Buffer writes to the word RAM control register until the next sub CPU instruction
                    // Fixes possible crashing in Silpheed due to a race condition in its word RAM handoff code
                    self.buffered_sub_register_writes.push((
                        address,
                        if WORD {
                            BufferedWrite::Word(value)
                        } else {
                            BufferedWrite::Byte(value as u8)
                        },
                    ));
                } else {
                    self.sub_write_register::<WORD>(address, value);
                }
            }
            _ => unreachable!("value & 0xFFFFF is always <= 0xFFFFF"),
        }
    }

    pub(crate) fn word_ram(&self) -> &WordRam {
        &self.word_ram
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

    pub fn reload_config(&mut self, config: &GenesisEmulatorConfig) {
        self.forced_region = config.forced_region;
        self.enable_ram_cartridge = config.sega_cd.enable_ram_cartridge;
        self.cdd_mut().reload_config(&config.sega_cd);
        self.pcm.reload_config(&config.sega_cd);
    }

    pub fn reset(&mut self) {
        self.disc_drive.reset();
        self.registers = SegaCdRegisters::new();
        self.pcm.disable();
    }

    pub fn region(&self) -> GenesisRegion {
        self.forced_region.unwrap_or(self.disc_region)
    }

    pub fn disc_title(&mut self) -> SegaCdLoadResult<Option<String>> {
        self.disc_drive.disc_title(self.region())
    }

    pub fn has_six_button_incompatible_game(&mut self) -> SegaCdLoadResult<bool> {
        self.disc_drive.cdd_mut().has_six_button_incompatible_game()
    }

    pub fn take_backup_ram_dirty(&mut self) -> bool {
        mem::take(&mut self.backup_ram_dirty)
    }

    pub fn backup_ram(&self) -> &[u8] {
        self.backup_ram.as_slice()
    }

    pub fn ram_cartridge(&self) -> &[u8] {
        self.ram_cartridge.as_slice()
    }

    pub fn take_bios_and_disc(mut self) -> (Vec<u8>, Option<CdRom>) {
        let bios_rom = self.bios.0.into_vec();
        let disc = self.disc_drive.take_disc();

        (bios_rom, disc)
    }

    pub fn take_bios_and_disc_from(&mut self, other: &mut Self) {
        self.bios.0 = mem::take(&mut other.bios.0);
        self.disc_drive.take_disc_from(&mut other.disc_drive);
    }

    pub fn change_disc(&mut self, disc: CdRom) {
        self.cdd_mut().change_disc(disc);
    }

    pub fn remove_disc(&mut self) {
        self.cdd_mut().remove_disc();
    }
}

impl BusInterface for SegaCdBus {
    type DebugView<'a>
        = DummyM68000Debugger
    where
        Self: 'a;

    fn read_byte(&mut self, address: u32) -> u8 {
        self.sub_read::<false>(address) as u8
    }

    fn read_word(&mut self, address: u32) -> u16 {
        self.sub_read::<true>(address)
    }

    fn write_byte(&mut self, address: u32, value: u8) {
        self.sub_write::<false>(address, value.into());
    }

    fn write_word(&mut self, address: u32, value: u16) {
        self.sub_write::<true>(address, value);
    }

    #[allow(clippy::bool_to_int_with_if)]
    fn interrupt_level(&self) -> u8 {
        if self.registers.cdc_interrupt_enabled && self.cdc().interrupt_pending() {
            // INT5: CDC interrupt
            5
        } else if self.registers.cdd_interrupt_enabled && self.cdd().interrupt_pending() {
            // INT4: CDD interrupt
            4
        } else if self.registers.timer_interrupt_enabled && self.registers.timer_interrupt_pending {
            // INT3: Timer interrupt
            3
        } else if self.registers.software_interrupt_enabled
            && self.registers.software_interrupt_pending
        {
            // INT2: Software interrupt from main CPU
            2
        } else if self.registers.graphics_interrupt_enabled
            && self.graphics_coprocessor.interrupt_pending()
        {
            // INT1: Graphics interrupt
            1
        } else {
            0
        }
    }

    fn acknowledge_interrupt(&mut self, interrupt_level: u8) {
        // Unlike the Genesis VDP, the Sega CD does appear to acknowledge the correct interrupt
        // when the sub CPU acknowledges an interrupt. Not doing this causes some mcd-verificator
        // tests to fail
        match interrupt_level {
            5 => {
                self.cdc_mut().acknowledge_interrupt();
            }
            4 => {
                self.cdd_mut().acknowledge_interrupt();
            }
            3 => {
                self.registers.timer_interrupt_pending = false;
            }
            2 => {
                self.registers.software_interrupt_pending = false;
            }
            1 => {
                self.graphics_coprocessor.acknowledge_interrupt();
            }
            _ => {}
        }
    }

    fn halt(&self) -> bool {
        self.registers.sub_cpu_busreq
    }

    fn reset(&self) -> bool {
        self.registers.sub_cpu_reset
    }
}

fn guess_cd_model(bios: &[u8]) -> CdModel {
    // Official BIOS versions have the version number at the end of the serial number, e.g.:
    //   "BR 000003-1.10" (Model 1 V1.10)
    if &bios[0x18A..0x18C] == b"1." { CdModel::One } else { CdModel::Two }
}
