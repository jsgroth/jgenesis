//! Code for emulating the bus, and more generally the NES CPU and PPU address spaces.
//!
//! The NES does not have a unified bus; it has two buses, a 16-bit CPU bus and a 14-bit PPU bus.
//! The CPU can only access the PPU bus through memory-mapped I/O.
//!
//! CPU address mapping:
//! * $0000-$07FF: 2KB internal RAM
//! * $0800-$1FFF: Mirrors of internal RAM
//! * $2000-$2007: Memory-mapped PPU registers
//! * $2008-$3FFF: Mirrors of memory-mapped PPU registers
//! * $4000-$4017: Memory-mapped APU and I/O registers
//! * $4018-$401F: "Test mode" functionality that is not emulated here
//! * $4020-$FFFF: Mapped to the cartridge board
//!
//! Most cartridge boards map $6000-$7FFF to PRG RAM (if present) and $8000-$FFFF to PRG ROM. Writes
//! to $8000-$FFFF are often mapped to internal cartridge board registers.
//!
//! PPU address mapping:
//! * $0000-$3EFF: Mapped to the cartridge board
//! * $3F00-$3F1F: 32 bytes of internal palette RAM
//! * $3F20-$3FFF: Mirrors of palette RAM
//!
//! While almost the entire PPU address space is controlled by the cartridge board, the PPU does
//! expect specific address ranges to hold specific data:
//! * $0000-$1FFF: Pattern tables (2x4KB) holding tile data
//! * $2000-$2FFF: Nametables (4x1KB) holding background tile maps and background tile attributes
//! * $3000-$3EFF: Mirrors of the nametables (not directly used by the PPU but the CPU can read/write here through memory-mapped I/O)
//!
//! Most cartridge boards contain CHR ROM or CHR RAM that is mapped into $0000-$1FFF for the pattern
//! tables.
//!
//! The PPU has 2KB of internal VRAM that the cartridge board is free to map into the PPU address
//! space however it wishes. Most boards use this VRAM for nametable data, mapping it into
//! $2000-$2FFF (with some ranges mirrored).

pub mod cartridge;

use crate::bus::cartridge::Mapper;
use crate::input::{LatchedJoypadState, NesJoypadState};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::TimingMode;
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::PartialClone;
use mos6502_emu::bus::BusInterface;
use std::array;

pub const CPU_RAM_START: u16 = 0x0000;
pub const CPU_RAM_END: u16 = 0x1FFF;
pub const CPU_RAM_MASK: u16 = 0x07FF;

pub const CPU_PPU_REGISTERS_START: u16 = 0x2000;
pub const CPU_PPU_REGISTERS_END: u16 = 0x3FFF;
pub const CPU_PPU_REGISTERS_MASK: u16 = 0x0007;

pub const CPU_IO_REGISTERS_START: u16 = 0x4000;
pub const CPU_IO_REGISTERS_END: u16 = 0x4017;

pub const CPU_IO_TEST_MODE_START: u16 = 0x4018;
pub const CPU_IO_TEST_MODE_END: u16 = 0x401F;

pub const CPU_CARTRIDGE_START: u16 = 0x4020;
pub const CPU_CARTRIDGE_END: u16 = 0xFFFF;

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct PendingCpuWrite {
    address: u16,
    value: u8,
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PpuRegister {
    PPUCTRL,
    PPUMASK,
    PPUSTATUS,
    OAMADDR,
    OAMDATA,
    PPUSCROLL,
    PPUADDR,
    PPUDATA,
}

impl PpuRegister {
    fn from_relative_address(relative_addr: usize) -> Option<Self> {
        match relative_addr {
            0x00 => Some(Self::PPUCTRL),
            0x01 => Some(Self::PPUMASK),
            0x02 => Some(Self::PPUSTATUS),
            0x03 => Some(Self::OAMADDR),
            0x04 => Some(Self::OAMDATA),
            0x05 => Some(Self::PPUSCROLL),
            0x06 => Some(Self::PPUADDR),
            0x07 => Some(Self::PPUDATA),
            _ => None,
        }
    }

    pub const fn to_address(self) -> u16 {
        match self {
            Self::PPUCTRL => 0x2000,
            Self::PPUMASK => 0x2001,
            Self::PPUSTATUS => 0x2002,
            Self::OAMADDR => 0x2003,
            Self::OAMDATA => 0x2004,
            Self::PPUSCROLL => 0x2005,
            Self::PPUADDR => 0x2006,
            Self::PPUDATA => 0x2007,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum PpuWriteToggle {
    First,
    Second,
}

impl PpuWriteToggle {
    fn toggle(self) -> Self {
        match self {
            Self::First => Self::Second,
            Self::Second => Self::First,
        }
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum PpuTrackedRegister {
    PPUCTRL,
    PPUSCROLL,
    PPUADDR,
    PPUDATA,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct PpuRegisters {
    ppu_ctrl: u8,
    ppu_mask: u8,
    ppu_status: u8,
    oam_addr: u8,
    ppu_data_buffer: u8,
    ppu_status_read: bool,
    ppu_open_bus_value: u8,
    oam_open_bus_value: Option<u8>,
    last_accessed_register: Option<PpuTrackedRegister>,
    write_toggle: PpuWriteToggle,
}

impl PpuRegisters {
    pub fn new() -> Self {
        Self {
            ppu_ctrl: 0,
            ppu_mask: 0,
            ppu_status: 0xA0,
            oam_addr: 0,
            ppu_data_buffer: 0,
            ppu_status_read: false,
            ppu_open_bus_value: 0,
            oam_open_bus_value: None,
            last_accessed_register: None,
            write_toggle: PpuWriteToggle::First,
        }
    }

    pub fn ppu_ctrl(&self) -> u8 {
        self.ppu_ctrl
    }

    pub fn nmi_enabled(&self) -> bool {
        self.ppu_ctrl.bit(7)
    }

    pub fn double_height_sprites(&self) -> bool {
        self.ppu_ctrl.bit(5)
    }

    pub fn bg_pattern_table_address(&self) -> u16 {
        if self.ppu_ctrl.bit(4) { 0x1000 } else { 0x0000 }
    }

    pub fn sprite_pattern_table_address(&self) -> u16 {
        if self.ppu_ctrl.bit(3) { 0x1000 } else { 0x0000 }
    }

    pub fn ppu_data_addr_increment(&self) -> u16 {
        if self.ppu_ctrl.bit(2) { 32 } else { 1 }
    }

    pub fn emphasize_blue(&self) -> bool {
        self.ppu_mask.bit(7)
    }

    pub fn emphasize_green(&self, timing_mode: TimingMode) -> bool {
        match timing_mode {
            TimingMode::Ntsc => self.ppu_mask.bit(6),
            TimingMode::Pal => self.ppu_mask.bit(5),
        }
    }

    pub fn emphasize_red(&self, timing_mode: TimingMode) -> bool {
        match timing_mode {
            TimingMode::Ntsc => self.ppu_mask.bit(5),
            TimingMode::Pal => self.ppu_mask.bit(6),
        }
    }

    pub fn greyscale(&self) -> bool {
        self.ppu_mask.bit(0)
    }

    pub fn sprites_enabled(&self) -> bool {
        self.ppu_mask.bit(4)
    }

    pub fn bg_enabled(&self) -> bool {
        self.ppu_mask.bit(3)
    }

    pub fn left_edge_sprites_enabled(&self) -> bool {
        self.ppu_mask.bit(2)
    }

    pub fn left_edge_bg_enabled(&self) -> bool {
        self.ppu_mask.bit(1)
    }

    pub fn vblank_flag(&self) -> bool {
        self.ppu_status.bit(7)
    }

    pub fn set_vblank_flag(&mut self, vblank: bool) {
        if vblank {
            self.ppu_status |= 1 << 7;
        } else {
            self.ppu_status &= !(1 << 7);
        }
    }

    pub fn set_sprite_0_hit(&mut self, sprite_0_hit: bool) {
        if sprite_0_hit {
            self.ppu_status |= 1 << 6;
        } else {
            self.ppu_status &= !(1 << 6);
        }
    }

    pub fn set_sprite_overflow(&mut self, sprite_overflow: bool) {
        if sprite_overflow {
            self.ppu_status |= 1 << 5;
        } else {
            self.ppu_status &= !(1 << 5);
        }
    }

    pub fn take_last_accessed_register(&mut self) -> Option<PpuTrackedRegister> {
        self.last_accessed_register.take()
    }

    pub fn get_ppu_open_bus_value(&self) -> u8 {
        self.ppu_open_bus_value
    }

    pub fn set_oam_open_bus(&mut self, value: Option<u8>) {
        self.oam_open_bus_value = value;
    }

    pub fn get_write_toggle(&self) -> PpuWriteToggle {
        self.write_toggle
    }

    fn tick(&mut self, interrupt_lines: &mut InterruptLines) {
        if self.ppu_status_read {
            self.ppu_status_read = false;

            self.set_vblank_flag(false);
            self.write_toggle = PpuWriteToggle::First;
        }

        let nmi_line = if self.vblank_flag() && self.nmi_enabled() {
            InterruptLine::Low
        } else {
            InterruptLine::High
        };
        interrupt_lines.ppu_set_nmi_line(nmi_line);
    }
}

#[allow(clippy::upper_case_acronyms)]
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum IoRegister {
    SQ1_VOL,
    SQ1_SWEEP,
    SQ1_LO,
    SQ1_HI,
    SQ2_VOL,
    SQ2_SWEEP,
    SQ2_LO,
    SQ2_HI,
    TRI_LINEAR,
    TRI_LO,
    TRI_HI,
    NOISE_VOL,
    NOISE_LO,
    NOISE_HI,
    DMC_FREQ,
    DMC_RAW,
    DMC_START,
    DMC_LEN,
    OAMDMA,
    SND_CHN,
    JOY1,
    JOY2,
}

// Needed for ArrayVec
impl Default for IoRegister {
    fn default() -> Self {
        Self::SQ1_VOL
    }
}

impl IoRegister {
    const fn to_relative_address(self) -> usize {
        match self {
            Self::SQ1_VOL => 0x00,
            Self::SQ1_SWEEP => 0x01,
            Self::SQ1_LO => 0x02,
            Self::SQ1_HI => 0x03,
            Self::SQ2_VOL => 0x04,
            Self::SQ2_SWEEP => 0x05,
            Self::SQ2_LO => 0x06,
            Self::SQ2_HI => 0x07,
            Self::TRI_LINEAR => 0x08,
            Self::TRI_LO => 0x0A,
            Self::TRI_HI => 0x0B,
            Self::NOISE_VOL => 0x0C,
            Self::NOISE_LO => 0x0E,
            Self::NOISE_HI => 0x0F,
            Self::DMC_FREQ => 0x10,
            Self::DMC_RAW => 0x11,
            Self::DMC_START => 0x12,
            Self::DMC_LEN => 0x13,
            Self::OAMDMA => 0x14,
            Self::SND_CHN => 0x15,
            Self::JOY1 => 0x16,
            Self::JOY2 => 0x17,
        }
    }

    fn from_relative_address(relative_addr: u16) -> Option<Self> {
        match relative_addr {
            0x00 => Some(Self::SQ1_VOL),
            0x01 => Some(Self::SQ1_SWEEP),
            0x02 => Some(Self::SQ1_LO),
            0x03 => Some(Self::SQ1_HI),
            0x04 => Some(Self::SQ2_VOL),
            0x05 => Some(Self::SQ2_SWEEP),
            0x06 => Some(Self::SQ2_LO),
            0x07 => Some(Self::SQ2_HI),
            0x08 => Some(Self::TRI_LINEAR),
            0x0A => Some(Self::TRI_LO),
            0x0B => Some(Self::TRI_HI),
            0x0C => Some(Self::NOISE_VOL),
            0x0E => Some(Self::NOISE_LO),
            0x0F => Some(Self::NOISE_HI),
            0x10 => Some(Self::DMC_FREQ),
            0x11 => Some(Self::DMC_RAW),
            0x12 => Some(Self::DMC_START),
            0x13 => Some(Self::DMC_LEN),
            0x14 => Some(Self::OAMDMA),
            0x15 => Some(Self::SND_CHN),
            0x16 => Some(Self::JOY1),
            0x17 => Some(Self::JOY2),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct IoRegisters {
    data: [u8; 0x18],
    dma_dirty: bool,
    dirty_register: Option<IoRegister>,
    snd_chn_read: bool,
    p1_joypad_state: NesJoypadState,
    p2_joypad_state: NesJoypadState,
    latched_joypad_state: Option<(LatchedJoypadState, LatchedJoypadState)>,
}

impl IoRegisters {
    // All I/O registers are at $40xx, and JOY1/JOY2 leave the highest 3 bits unused
    const IO_OPEN_BUS_BITS: u8 = 0x40;

    fn new() -> Self {
        Self {
            data: [0; 0x18],
            dma_dirty: false,
            dirty_register: None,
            snd_chn_read: false,
            p1_joypad_state: NesJoypadState::default(),
            p2_joypad_state: NesJoypadState::default(),
            latched_joypad_state: None,
        }
    }

    fn read_address(&mut self, address: u16) -> u8 {
        let relative_addr = address - CPU_IO_REGISTERS_START;
        let Some(register) = IoRegister::from_relative_address(relative_addr) else {
            return cpu_open_bus(address);
        };

        self.read_register(register)
    }

    fn read_register(&mut self, register: IoRegister) -> u8 {
        match register {
            IoRegister::SND_CHN => {
                self.snd_chn_read = true;
                self.data[register.to_relative_address()]
            }
            IoRegister::JOY1 => {
                if let Some((p1_joypad_state, p2_joypad_state)) = self.latched_joypad_state {
                    self.latched_joypad_state = Some((p1_joypad_state.shift(), p2_joypad_state));
                    p1_joypad_state.next_bit() | Self::IO_OPEN_BUS_BITS
                } else {
                    u8::from(self.p1_joypad_state.a) | Self::IO_OPEN_BUS_BITS
                }
            }
            IoRegister::JOY2 => {
                if let Some((p1_joypad_state, p2_joypad_state)) = self.latched_joypad_state {
                    self.latched_joypad_state = Some((p1_joypad_state, p2_joypad_state.shift()));
                    p2_joypad_state.next_bit() | Self::IO_OPEN_BUS_BITS
                } else {
                    u8::from(self.p2_joypad_state.a) | Self::IO_OPEN_BUS_BITS
                }
            }
            _ => Self::IO_OPEN_BUS_BITS,
        }
    }

    fn write_address(&mut self, address: u16, value: u8) {
        let relative_addr = address - CPU_IO_REGISTERS_START;
        let Some(register) = IoRegister::from_relative_address(relative_addr) else {
            return;
        };

        self.write_register(register, value);
    }

    #[allow(clippy::manual_assert)]
    fn write_register(&mut self, register: IoRegister, value: u8) {
        self.data[register.to_relative_address()] = value;

        if self.dirty_register.replace(register).is_some() {
            panic!("Attempted to write an I/O register twice in the same cycle");
        }

        match register {
            IoRegister::JOY1 => {
                if value.bit(0) {
                    self.latched_joypad_state = None;
                } else if self.latched_joypad_state.is_none() {
                    self.latched_joypad_state =
                        Some((self.p1_joypad_state.latch(), self.p2_joypad_state.latch()));
                }
            }
            IoRegister::OAMDMA => {
                self.dma_dirty = true;
            }
            _ => {}
        }
    }

    pub fn take_dirty_register(&mut self) -> Option<(IoRegister, u8)> {
        self.dirty_register
            .take()
            .map(|register| (register, self.data[register.to_relative_address()]))
    }

    pub fn set_apu_status(&mut self, apu_status: u8) {
        self.data[IoRegister::SND_CHN.to_relative_address()] = apu_status;
    }

    pub fn get_and_clear_snd_chn_read(&mut self) -> bool {
        let snd_chn_read = self.snd_chn_read;
        self.snd_chn_read = false;
        snd_chn_read
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum InterruptLine {
    High,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum IrqSource {
    ApuDmc,
    ApuFrameCounter,
    Mapper,
}

impl IrqSource {
    fn to_low_pull_bit(self) -> u8 {
        match self {
            Self::ApuDmc => 0x01,
            Self::ApuFrameCounter => 0x02,
            Self::Mapper => 0x04,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum IrqStatus {
    None,
    Pending,
    Triggered,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct InterruptLines {
    nmi_line: InterruptLine,
    next_nmi_line: InterruptLine,
    nmi_triggered: bool,
    irq_status: IrqStatus,
    irq_low_pulls: u8,
}

impl InterruptLines {
    fn new() -> Self {
        Self {
            nmi_line: InterruptLine::High,
            next_nmi_line: InterruptLine::High,
            nmi_triggered: false,
            irq_status: IrqStatus::None,
            irq_low_pulls: 0x00,
        }
    }

    fn tick(&mut self) {
        if self.nmi_line == InterruptLine::High && self.next_nmi_line == InterruptLine::Low {
            self.nmi_triggered = true;
        }
        self.nmi_line = self.next_nmi_line;

        let irq_line =
            if self.irq_low_pulls != 0 { InterruptLine::Low } else { InterruptLine::High };

        match (irq_line, self.irq_status) {
            (InterruptLine::High, _) => {
                self.irq_status = IrqStatus::None;
            }
            (InterruptLine::Low, IrqStatus::None) => {
                // IRQ interrupts need to be delayed by 1 CPU cycle
                self.irq_status = IrqStatus::Pending;
            }
            (InterruptLine::Low, IrqStatus::Pending) => {
                self.irq_status = IrqStatus::Triggered;
            }
            (InterruptLine::Low, IrqStatus::Triggered) => {}
        }
    }

    pub fn nmi_triggered(&self) -> bool {
        self.nmi_triggered
    }

    pub fn clear_nmi_triggered(&mut self) {
        self.nmi_triggered = false;
    }

    pub fn ppu_set_nmi_line(&mut self, interrupt_line: InterruptLine) {
        self.next_nmi_line = interrupt_line;
    }

    pub fn irq_triggered(&self) -> bool {
        self.irq_status == IrqStatus::Triggered
    }

    pub fn set_irq_low_pull(&mut self, source: IrqSource, value: bool) {
        if value {
            self.irq_low_pulls |= source.to_low_pull_bit();
        } else {
            self.irq_low_pulls &= !source.to_low_pull_bit();
        }
    }
}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct Bus {
    #[partial_clone(partial)]
    mapper: Mapper,
    cpu_internal_ram: [u8; 2048],
    ppu_registers: PpuRegisters,
    io_registers: IoRegisters,
    ppu_vram: [u8; 2048],
    ppu_palette_ram: [u8; 32],
    ppu_oam: [u8; 256],
    ppu_bus_address: u16,
    interrupt_lines: InterruptLines,
    pending_write: Option<PendingCpuWrite>,
}

impl Bus {
    pub(crate) fn from_cartridge(mapper: Mapper) -> Self {
        Self {
            mapper,
            // (Somewhat) randomize initial RAM contents
            cpu_internal_ram: array::from_fn(|_| if rand::random() { 0x00 } else { 0xFF }),
            ppu_registers: PpuRegisters::new(),
            io_registers: IoRegisters::new(),
            ppu_vram: [0; 2048],
            ppu_palette_ram: [0; 32],
            ppu_oam: [0; 256],
            ppu_bus_address: 0,
            interrupt_lines: InterruptLines::new(),
            pending_write: None,
        }
    }

    pub fn cpu(&mut self) -> CpuBus<'_> {
        CpuBus(self)
    }

    pub fn ppu(&mut self) -> PpuBus<'_> {
        PpuBus(self)
    }

    pub fn update_p1_joypad_state(
        &mut self,
        p1_joypad_state: NesJoypadState,
        allow_opposing_inputs: bool,
    ) {
        self.io_registers.p1_joypad_state = if allow_opposing_inputs {
            p1_joypad_state
        } else {
            p1_joypad_state.sanitize_opposing_directions()
        };
    }

    pub fn update_p2_joypad_state(
        &mut self,
        p2_joypad_state: NesJoypadState,
        allow_opposing_inputs: bool,
    ) {
        self.io_registers.p2_joypad_state = if allow_opposing_inputs {
            p2_joypad_state
        } else {
            p2_joypad_state.sanitize_opposing_directions()
        };
    }

    pub fn tick(&mut self) {
        self.ppu_registers.tick(&mut self.interrupt_lines);
        self.mapper.tick(self.ppu_bus_address);
    }

    pub fn tick_cpu(&mut self) {
        if let Some(write) = self.pending_write.take() {
            self.cpu().apply_write(write.address, write.value);
        }

        self.mapper.tick_cpu();
    }

    // Poll NMI/IRQ interrupt lines; this should be called once per CPU cycle, between the first
    // and second PPU ticks
    pub fn poll_interrupt_lines(&mut self) {
        self.interrupt_lines.set_irq_low_pull(IrqSource::Mapper, self.mapper.interrupt_flag());

        self.interrupt_lines.tick();
    }

    pub(crate) fn mapper(&self) -> &Mapper {
        &self.mapper
    }

    pub(crate) fn mapper_mut(&mut self) -> &mut Mapper {
        &mut self.mapper
    }

    pub(crate) fn move_rom_from(&mut self, other: &mut Self) {
        self.mapper.move_rom_from(&mut other.mapper);
    }
}

/// A view of the bus containing methods that are appropriate for use by the CPU and APU.
pub struct CpuBus<'a>(&'a mut Bus);

impl<'a> BusInterface for CpuBus<'a> {
    #[inline]
    fn read(&mut self, address: u16) -> u8 {
        match address {
            address @ CPU_RAM_START..=CPU_RAM_END => {
                let ram_address = address & CPU_RAM_MASK;
                self.0.cpu_internal_ram[ram_address as usize]
            }
            address @ CPU_PPU_REGISTERS_START..=CPU_PPU_REGISTERS_END => {
                let ppu_register_relative_addr =
                    (address - CPU_PPU_REGISTERS_START) & CPU_PPU_REGISTERS_MASK;
                self.read_ppu_register_address(ppu_register_relative_addr as usize)
            }
            address @ CPU_IO_REGISTERS_START..=CPU_IO_REGISTERS_END => {
                self.0.io_registers.read_address(address)
            }
            _address @ CPU_IO_TEST_MODE_START..=CPU_IO_TEST_MODE_END => cpu_open_bus(address),
            address @ CPU_CARTRIDGE_START..=CPU_CARTRIDGE_END => {
                self.0.mapper.read_cpu_address(address)
            }
        }
    }

    #[inline]
    #[allow(clippy::manual_assert)]
    fn write(&mut self, address: u16, value: u8) {
        if self.0.pending_write.replace(PendingCpuWrite { address, value }).is_some() {
            panic!("Attempted to write twice in the same cycle");
        }
    }

    #[inline]
    fn nmi(&self) -> bool {
        self.0.interrupt_lines.nmi_triggered()
    }

    #[inline]
    fn acknowledge_nmi(&mut self) {
        self.0.interrupt_lines.clear_nmi_triggered();
    }

    #[inline]
    fn irq(&self) -> bool {
        self.0.interrupt_lines.irq_triggered()
    }
}

impl<'a> CpuBus<'a> {
    fn apply_write(&mut self, address: u16, value: u8) {
        match address {
            address @ CPU_RAM_START..=CPU_RAM_END => {
                let ram_address = address & CPU_RAM_MASK;
                self.0.cpu_internal_ram[ram_address as usize] = value;
            }
            address @ CPU_PPU_REGISTERS_START..=CPU_PPU_REGISTERS_END => {
                let ppu_register_relative_addr =
                    (address - CPU_PPU_REGISTERS_START) & CPU_PPU_REGISTERS_MASK;
                self.write_ppu_register_address(ppu_register_relative_addr as usize, value);
            }
            address @ CPU_IO_REGISTERS_START..=CPU_IO_REGISTERS_END => {
                self.0.io_registers.write_address(address, value);
            }
            _address @ CPU_IO_TEST_MODE_START..=CPU_IO_TEST_MODE_END => {}
            address @ CPU_CARTRIDGE_START..=CPU_CARTRIDGE_END => {
                self.0.mapper.write_cpu_address(address, value);
            }
        }
    }

    fn read_ppu_register_address(&mut self, relative_addr: usize) -> u8 {
        let Some(register) = PpuRegister::from_relative_address(relative_addr) else {
            panic!("invalid PPU register address: {relative_addr}");
        };

        self.read_ppu_register(register)
    }

    pub fn read_ppu_register(&mut self, register: PpuRegister) -> u8 {
        match register {
            PpuRegister::PPUCTRL
            | PpuRegister::PPUMASK
            | PpuRegister::OAMADDR
            | PpuRegister::PPUSCROLL
            | PpuRegister::PPUADDR => self.0.ppu_registers.ppu_open_bus_value,
            PpuRegister::PPUSTATUS => {
                self.0.ppu_registers.ppu_status_read = true;

                // PPUSTATUS reads only affect bits 7-5 of open bus, bits 4-0 remain intact
                // and are returned as part of the read
                let ppu_status_high_bits = self.0.ppu_registers.ppu_status & 0xE0;
                let open_bus_lower_bits = self.0.ppu_registers.ppu_open_bus_value & 0x1F;
                self.0.ppu_registers.ppu_open_bus_value =
                    ppu_status_high_bits | open_bus_lower_bits;

                self.0.ppu_registers.ppu_open_bus_value
            }
            PpuRegister::OAMDATA => {
                let value = self
                    .0
                    .ppu_registers
                    .oam_open_bus_value
                    .unwrap_or(self.0.ppu_oam[self.0.ppu_registers.oam_addr as usize]);
                self.0.ppu_registers.ppu_open_bus_value = value;
                value
            }
            PpuRegister::PPUDATA => {
                let address = self.0.ppu_bus_address;
                let (data, buffer_read_address) = if address < 0x3F00 {
                    (self.0.ppu_registers.ppu_data_buffer, address)
                } else {
                    let palette_address = map_palette_address(address);
                    let palette_byte = self.0.ppu_palette_ram[palette_address];
                    // When PPUDATA is used to read palette RAM, buffer reads mirror the nametable
                    // data located at $2F00-$2FFF
                    (palette_byte, address - 0x1000)
                };

                self.0.mapper.about_to_access_ppu_data();

                self.0.ppu_registers.ppu_data_buffer =
                    self.0.ppu().read_address(buffer_read_address);
                // Reset the bus address in case the buffer read address was different from the
                // actual address
                self.0.ppu_bus_address = address;

                self.0.ppu_registers.ppu_open_bus_value = data;

                self.0.ppu_registers.last_accessed_register = Some(PpuTrackedRegister::PPUDATA);

                data
            }
        }
    }

    fn write_ppu_register_address(&mut self, relative_addr: usize, value: u8) {
        let Some(register) = PpuRegister::from_relative_address(relative_addr) else {
            panic!("invalid PPU register address: {relative_addr}");
        };

        // Writes to any memory-mapped PPU register put the value on open bus
        self.0.ppu_registers.ppu_open_bus_value = value;

        match register {
            PpuRegister::PPUCTRL => {
                self.0.ppu_registers.ppu_ctrl = value;
                self.0.ppu_registers.last_accessed_register = Some(PpuTrackedRegister::PPUCTRL);
                self.0.mapper.process_ppu_ctrl_update(value);
            }
            PpuRegister::PPUMASK => {
                log::trace!("BUS: PPUMASK set to {value:02X}");
                self.0.ppu_registers.ppu_mask = value;
            }
            PpuRegister::PPUSTATUS => {}
            PpuRegister::OAMADDR => {
                self.0.ppu_registers.oam_addr = value;
            }
            PpuRegister::OAMDATA => {
                if self.0.ppu_registers.oam_open_bus_value.is_none() {
                    let oam_addr = self.0.ppu_registers.oam_addr;
                    self.0.ppu_oam[oam_addr as usize] = value;
                    self.0.ppu_registers.oam_addr = self.0.ppu_registers.oam_addr.wrapping_add(1);
                }
            }
            PpuRegister::PPUSCROLL => {
                self.0.ppu_registers.last_accessed_register = Some(PpuTrackedRegister::PPUSCROLL);
                self.0.ppu_registers.write_toggle = self.0.ppu_registers.write_toggle.toggle();
            }
            PpuRegister::PPUADDR => {
                self.0.ppu_registers.last_accessed_register = Some(PpuTrackedRegister::PPUADDR);
                self.0.ppu_registers.write_toggle = self.0.ppu_registers.write_toggle.toggle();
            }
            PpuRegister::PPUDATA => {
                self.0.mapper.about_to_access_ppu_data();

                let address = self.0.ppu_bus_address;
                self.0.ppu().write_address(address & 0x3FFF, value);

                self.0.ppu_registers.last_accessed_register = Some(PpuTrackedRegister::PPUDATA);
            }
        }
    }

    pub fn is_oamdma_dirty(&self) -> bool {
        self.0.io_registers.dma_dirty
    }

    pub fn clear_oamdma_dirty(&mut self) {
        self.0.io_registers.dma_dirty = false;
    }

    pub fn read_oamdma_for_transfer(&self) -> u8 {
        self.0.io_registers.data[IoRegister::OAMDMA.to_relative_address()]
    }

    pub fn get_io_registers_mut(&mut self) -> &mut IoRegisters {
        &mut self.0.io_registers
    }

    pub fn interrupt_lines(&mut self) -> &mut InterruptLines {
        &mut self.0.interrupt_lines
    }
}

/// A view of the bus containing methods that are appropriate for use by the PPU.
pub struct PpuBus<'a>(&'a mut Bus);

impl<'a> PpuBus<'a> {
    pub fn read_address(&mut self, address: u16) -> u8 {
        // PPU bus only has 14-bit addressing
        let address = address & 0x3FFF;

        self.0.ppu_bus_address = address;

        match address {
            0x0000..=0x3EFF => self.0.mapper.read_ppu_address(address, &self.0.ppu_vram),
            0x3F00..=0x3FFF => {
                let palette_relative_addr = map_palette_address(address);

                self.0.ppu_palette_ram[palette_relative_addr]
            }
            0x4000..=0xFFFF => {
                unreachable!("{address} should be <= 0x3FFF after masking with 0x3FFF")
            }
        }
    }

    pub fn write_address(&mut self, address: u16, value: u8) {
        let address = address & 0x3FFF;
        match address {
            0x0000..=0x3EFF => {
                self.0.mapper.write_ppu_address(address, value, &mut self.0.ppu_vram);
            }
            0x3F00..=0x3FFF => {
                let palette_relative_addr = map_palette_address(address);
                self.0.ppu_palette_ram[palette_relative_addr] = value;
            }
            0x4000..=0xFFFF => {
                unreachable!("{address} should be <= 0x3FFF after masking with 0x3FFF")
            }
        }
    }

    pub fn get_ppu_registers(&self) -> &PpuRegisters {
        &self.0.ppu_registers
    }

    pub fn get_ppu_registers_mut(&mut self) -> &mut PpuRegisters {
        &mut self.0.ppu_registers
    }

    pub fn get_oam(&self) -> &[u8; 256] {
        &self.0.ppu_oam
    }

    pub fn get_palette_ram(&self) -> &[u8; 32] {
        &self.0.ppu_palette_ram
    }

    pub fn set_bus_address(&mut self, address: u16) {
        self.0.ppu_bus_address = address;
    }

    pub fn reset(&mut self) {
        self.0.ppu_registers.ppu_ctrl = 0x00;
        self.0.ppu_registers.ppu_mask = 0x00;
        self.0.ppu_registers.write_toggle = PpuWriteToggle::First;
        self.0.ppu_registers.ppu_data_buffer = 0x00;
        self.0.ppu_registers.ppu_open_bus_value = 0x00;
        self.0.mapper.reset();
    }
}

fn map_palette_address(address: u16) -> usize {
    let palette_relative_addr = (address & 0x001F) as usize;
    if palette_relative_addr >= 0x10 && palette_relative_addr.trailing_zeros() >= 2 {
        // 0x10, 0x14, 0x18, 0x1C are mirrored to 0x00, 0x04, 0x08, 0x0C
        palette_relative_addr - 0x10
    } else {
        palette_relative_addr
    }
}

#[cfg(test)]
mod tests {
    use crate::bus::{cartridge, Bus};

    #[test]
    fn randomized_ram_on_startup() {
        let mapper = cartridge::new_mmc1(vec![0; 32768]);
        let bus1 = Bus::from_cartridge(mapper.clone());
        let bus2 = Bus::from_cartridge(mapper);

        assert_ne!(bus1.cpu_internal_ram, bus2.cpu_internal_ram);
    }
}

pub(crate) fn cpu_open_bus(address: u16) -> u8 {
    (address >> 8) as u8
}
