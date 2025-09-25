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

use crate::api::NesEmulatorConfig;
use crate::apu::ApuState;
use crate::bus::cartridge::Mapper;
use crate::graphics::TimingModeGraphicsExt;
use crate::input::{LatchedJoypadState, NesInputDevice, NesJoypadStateExt, ZapperState};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::TimingMode;
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::PartialClone;
use mos6502_emu::bus::BusInterface;
use nes_config::{NesJoypadState, Overscan};
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

pub const PALETTE_RAM_MASK: u16 = 0x001F;

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
    ppu_open_bus: u8,
    ppu_open_bus_decay_cycles: [u64; 8],
    total_cycles: u64,
    oam_open_bus_value: Option<u8>,
    last_accessed_register: Option<PpuTrackedRegister>,
    write_toggle: PpuWriteToggle,
    reset_flag: bool,
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
            ppu_open_bus: 0,
            ppu_open_bus_decay_cycles: [0; 8],
            total_cycles: 0,
            oam_open_bus_value: None,
            last_accessed_register: None,
            write_toggle: PpuWriteToggle::First,
            reset_flag: true,
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

    pub fn clear_reset_flag(&mut self) {
        self.reset_flag = false;
    }

    pub fn take_last_accessed_register(&mut self) -> Option<PpuTrackedRegister> {
        self.last_accessed_register.take()
    }

    pub fn get_ppu_open_bus_value(&self) -> u8 {
        self.ppu_open_bus
    }

    pub fn set_oam_addr(&mut self, oam_addr: u8) {
        self.oam_addr = oam_addr;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum IoRegister {
    #[default]
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
struct ZapperBusState {
    fire_pressed: bool,
    position: Option<(u16, u16)>,
    cpu_ticks_until_release: u32,
    cpu_ticks_until_sensor_off: u32,
}

impl ZapperBusState {
    // Roughly two frames' worth of CPU cycles
    // In actual hardware, the bit transitions from 0 to 1 when either the trigger is full pulled or
    // roughly 100ms after the half-pull (if the trigger is never full pulled)
    const HALF_PULLED_CPU_TICKS: u32 = 59_661;

    // Roughly 25 lines' worth of CPU cycles
    const SENSOR_ON_CPU_TICKS: u32 = 2_841;

    fn new(state: ZapperState) -> Self {
        Self {
            fire_pressed: state.fire,
            position: state.position(),
            cpu_ticks_until_release: 0,
            cpu_ticks_until_sensor_off: 0,
        }
    }

    fn read(&self) -> u8 {
        let no_light_sensed_bit = u8::from(self.cpu_ticks_until_sensor_off == 0) << 3;
        let released_bit = u8::from(self.cpu_ticks_until_release == 0) << 4;

        log::trace!(
            "Zapper read: light_sensed={} pressed={}",
            no_light_sensed_bit == 0,
            released_bit == 0
        );

        no_light_sensed_bit | released_bit
    }

    fn tick_cpu(&mut self) {
        self.cpu_ticks_until_release = self.cpu_ticks_until_release.saturating_sub(1);
        self.cpu_ticks_until_sensor_off = self.cpu_ticks_until_sensor_off.saturating_sub(1);
    }

    fn update_buttons(&mut self, state: ZapperState) {
        self.position = state.position();

        if !self.fire_pressed && state.fire {
            log::debug!(
                "Zapper fire button pressed; setting released bit to 0 for {} cycles",
                Self::HALF_PULLED_CPU_TICKS
            );
            self.cpu_ticks_until_release = Self::HALF_PULLED_CPU_TICKS;
        }

        self.fire_pressed = state.fire;
    }

    fn handle_pixel_rendered(
        &mut self,
        pixel: u8,
        x: u16,
        y: u16,
        display_mode: TimingMode,
        overscan: Overscan,
    ) {
        let Some((x, y)) = Self::adjust_frame_position(x, y, display_mode, overscan) else {
            return;
        };

        if self.position == Some((x, y)) {
            log::debug!("Detected pixel {pixel:02X} at Zapper position of ({x}, {y})");

            if should_trigger_zapper_sensor(pixel) {
                log::debug!("  Triggered Zapper light sensor");
                self.cpu_ticks_until_sensor_off = Self::SENSOR_ON_CPU_TICKS;
            }
        }
    }

    fn adjust_frame_position(
        mut x: u16,
        mut y: u16,
        display_mode: TimingMode,
        overscan: Overscan,
    ) -> Option<(u16, u16)> {
        let mut overflowed;

        (y, overflowed) = y.overflowing_sub(display_mode.starting_row());
        if overflowed {
            return None;
        }

        (y, overflowed) = y.overflowing_sub(overscan.top);
        if overflowed {
            return None;
        }

        (x, overflowed) = x.overflowing_sub(overscan.left);
        if overflowed {
            return None;
        }

        Some((x, y))
    }
}

fn should_trigger_zapper_sensor(pixel: u8) -> bool {
    // Fairly arbitrary definition of what constitutes a bright enough pixel to trigger
    // the sensor. On actual hardware this will vary by TV
    let pixel = pixel & 0x3F;
    pixel == 0x00
        || (0x10..0x1D).contains(&pixel)
        || (0x20..0x2E).contains(&pixel)
        || (0x30..0x3E).contains(&pixel)
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct IoRegisters {
    data: [u8; 0x18],
    dma_dirty: bool,
    dirty_register: Option<IoRegister>,
    snd_chn_read: bool,
    p1_joypad_state: NesJoypadState,
    p2_joypad_state: NesJoypadState,
    latched_joy1_bit: u8,
    latched_joy2_bit: u8,
    latched_p1_joypad_state: LatchedJoypadState,
    latched_p2_joypad_state: LatchedJoypadState,
    joypad_strobe: bool,
    joy1_read_last_cycle: bool,
    joy1_read_this_cycle: bool,
    joy2_read_last_cycle: bool,
    joy2_read_this_cycle: bool,
    zapper_state: Option<ZapperBusState>,
    // Needed for zapper positioning
    overscan: Overscan,
}

impl IoRegisters {
    fn new(overscan: Overscan) -> Self {
        Self {
            data: [0; 0x18],
            dma_dirty: false,
            dirty_register: None,
            snd_chn_read: false,
            p1_joypad_state: NesJoypadState::default(),
            p2_joypad_state: NesJoypadState::default(),
            latched_joy1_bit: 1,
            latched_joy2_bit: 1,
            latched_p1_joypad_state: LatchedJoypadState::default(),
            latched_p2_joypad_state: LatchedJoypadState::default(),
            joy1_read_last_cycle: false,
            joy1_read_this_cycle: false,
            joy2_read_last_cycle: false,
            joy2_read_this_cycle: false,
            joypad_strobe: false,
            zapper_state: None,
            overscan,
        }
    }

    fn read_address(&mut self, address: u16, cpu_open_bus: u8) -> Option<u8> {
        let relative_addr = address - CPU_IO_REGISTERS_START;
        IoRegister::from_relative_address(relative_addr)
            .map(|register| self.read_register(register, cpu_open_bus))
    }

    fn read_register(&mut self, register: IoRegister, cpu_open_bus: u8) -> u8 {
        // Highest 3 bits of JOY1/JOY2 reads are open bus (normally 0x40 but not necessarily)
        let joy_open_bus = cpu_open_bus & 0xE0;

        match register {
            IoRegister::SND_CHN => {
                self.snd_chn_read = true;
                // Bit 5 of SND_CHN reads is open bus
                self.data[register.to_relative_address()] | (cpu_open_bus & (1 << 5))
            }
            IoRegister::JOY1 => {
                self.joy1_read_this_cycle = true;

                if !self.joy1_read_last_cycle {
                    self.latched_joy1_bit = self.latched_p1_joypad_state.next_bit();
                    self.latched_p1_joypad_state = self.latched_p1_joypad_state.shift();
                }
                self.latched_joy1_bit | joy_open_bus
            }
            IoRegister::JOY2 => {
                self.joy2_read_this_cycle = true;

                match &self.zapper_state {
                    Some(zapper_state) => zapper_state.read() | joy_open_bus,
                    None => {
                        if !self.joy2_read_last_cycle {
                            self.latched_joy2_bit = self.latched_p2_joypad_state.next_bit();
                            self.latched_p2_joypad_state = self.latched_p2_joypad_state.shift();
                        }
                        self.latched_joy2_bit | joy_open_bus
                    }
                }
            }
            _ => {
                // Other I/O registers are write-only
                cpu_open_bus
            }
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
                self.joypad_strobe = value.bit(0);
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

    fn tick_cpu(&mut self, apu_odd_cycle: bool) {
        self.joy1_read_last_cycle = self.joy1_read_this_cycle;
        self.joy1_read_this_cycle = false;

        self.joy2_read_last_cycle = self.joy2_read_this_cycle;
        self.joy2_read_this_cycle = false;

        if self.joypad_strobe && apu_odd_cycle {
            self.latched_p1_joypad_state = self.p1_joypad_state.latch();
            self.latched_p2_joypad_state = self.p2_joypad_state.latch();
        }

        if let Some(zapper_state) = &mut self.zapper_state {
            zapper_state.tick_cpu();
        }
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
    cpu_internal_ram: Box<[u8; 2048]>,
    cpu_open_bus: u8,
    ppu_registers: PpuRegisters,
    io_registers: IoRegisters,
    ppu_vram: Box<[u8; 2048]>,
    ppu_palette_ram: [u8; 32],
    ppu_oam: [u8; 256],
    ppu_bus_address: u16,
    interrupt_lines: InterruptLines,
    pending_write: Option<PendingCpuWrite>,
}

impl Bus {
    pub(crate) fn from_cartridge(mapper: Mapper, overscan: Overscan) -> Self {
        const INITIAL_PALETTE_RAM: [u8; 32] = [
            0x09, 0x01, 0x00, 0x01, 0x00, 0x02, 0x02, 0x0D, 0x08, 0x10, 0x08, 0x24, 0x00, 0x00,
            0x04, 0x2C, 0x09, 0x01, 0x34, 0x03, 0x00, 0x04, 0x00, 0x14, 0x08, 0x3A, 0x00, 0x02,
            0x00, 0x20, 0x2C, 0x08,
        ];

        Self {
            mapper,
            // (Somewhat) randomize initial RAM contents
            cpu_internal_ram: Box::new(array::from_fn(
                |_| if rand::random() { 0x00 } else { 0xFF },
            )),
            cpu_open_bus: 0,
            ppu_registers: PpuRegisters::new(),
            io_registers: IoRegisters::new(overscan),
            ppu_vram: Box::new(array::from_fn(|_| 0)),
            ppu_palette_ram: INITIAL_PALETTE_RAM,
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
        p2_inputs: NesInputDevice,
        allow_opposing_inputs: bool,
    ) {
        match p2_inputs {
            NesInputDevice::Controller(joypad_state) => {
                self.io_registers.p2_joypad_state = if allow_opposing_inputs {
                    joypad_state
                } else {
                    joypad_state.sanitize_opposing_directions()
                };
                self.io_registers.zapper_state = None;
            }
            NesInputDevice::Zapper(zapper_state) => {
                match &mut self.io_registers.zapper_state {
                    Some(bus_state) => {
                        bus_state.update_buttons(zapper_state);
                    }
                    None => {
                        self.io_registers.zapper_state = Some(ZapperBusState::new(zapper_state));
                    }
                }
                self.io_registers.p2_joypad_state = NesJoypadState::default();
            }
        }
    }

    pub fn tick(&mut self) {
        self.ppu_registers.tick(&mut self.interrupt_lines);
        self.mapper.tick(self.ppu_bus_address);
    }

    pub fn tick_cpu(&mut self, apu_state: &ApuState) {
        if let Some(write) = self.pending_write.take() {
            self.cpu().apply_write(write.address, write.value);
        }

        self.mapper.tick_cpu();
        self.io_registers.tick_cpu(apu_state.is_active_cycle());

        self.ppu_registers.total_cycles += 1;
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

    pub(crate) fn reload_config(&mut self, config: &NesEmulatorConfig) {
        self.io_registers.overscan = config.overscan;
    }
}

/// A view of the bus containing methods that are appropriate for use by the CPU and APU.
pub struct CpuBus<'a>(&'a mut Bus);

impl BusInterface for CpuBus<'_> {
    #[inline]
    fn read(&mut self, address: u16) -> u8 {
        let value = match address {
            address @ CPU_RAM_START..=CPU_RAM_END => {
                let ram_address = address & CPU_RAM_MASK;
                self.0.cpu_internal_ram[ram_address as usize]
            }
            address @ CPU_PPU_REGISTERS_START..=CPU_PPU_REGISTERS_END => {
                let ppu_register_relative_addr =
                    (address - CPU_PPU_REGISTERS_START) & CPU_PPU_REGISTERS_MASK;
                self.read_ppu_register_address(ppu_register_relative_addr as usize)
            }
            address @ CPU_IO_REGISTERS_START..=CPU_IO_REGISTERS_END => self
                .0
                .io_registers
                .read_address(address, self.0.cpu_open_bus)
                .unwrap_or(self.0.cpu_open_bus),
            _address @ CPU_IO_TEST_MODE_START..=CPU_IO_TEST_MODE_END => self.0.cpu_open_bus,
            address @ CPU_CARTRIDGE_START..=CPU_CARTRIDGE_END => {
                self.0.mapper.read_cpu_address(address, self.0.cpu_open_bus)
            }
        };

        // $4015 reads (SND_CHN) do not update open bus (verified by AccuracyCoin open bus tests)
        if address != 0x4015 {
            self.0.cpu_open_bus = value;
        }

        value
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

impl CpuBus<'_> {
    fn dma_read<const DMC: bool>(&mut self, address: u16, halted_cpu_address: u16) -> u8 {
        // 2A03 register activation uses bits 0-4 from the 2A03 address bus and bits 5-15 from the
        // 6502 address bus.
        //
        // For normal CPU reads, the 2A03 and 6502 addresses are obviously always the same.
        //
        // For DMA reads, they can be different: the 2A03 address bus is set to the DMA read address,
        // while the 6502 address bus remains at the address that the CPU was reading from when DMA
        // halted it.
        //
        // This causes bus conflicts if one of the APU registers ($4015-$4017) activates while DMA
        // is trying to read from a different address.
        let register_activation_addr = (address & 0x1F) | (halted_cpu_address & !0x1F);
        if address == register_activation_addr {
            // Bits 5-15 of the DMA address match the 6502 address
            // Return early to prevent possible double reads of addresses with read side effects
            return self.read(address);
        }

        if (0x4015..=0x4017).contains(&register_activation_addr) {
            // Bus conflict!
            if DMC {
                // For DMC DMA $4015 conflicts, open bus is set to the 8-bit sample, and the frame
                // counter IRQ flag is cleared (so the $4015 read does happen).
                //
                // For DMC DMA $4016/$4017 conflicts, open bus is set to the highest 3 bits of the
                // sample and the lowest 5 bits from JOY1/JOY2.
                //
                // In both cases, give the DMC the actual 8-bit sample to prevent possible audio
                // corruption, even though this is probably not accurate to hardware.
                let sample_byte = self.read(address);
                self.read(register_activation_addr);
                sample_byte
            } else if register_activation_addr == 0x4015 {
                // For OAM DMA $4015 conflicts, the DMA address is read (which can change open bus),
                // but DMA gets the value from the $4015 read
                self.read(address);
                self.read(register_activation_addr)
            } else {
                // For OAM DMA $4016/$4017 conflicts, the controller port gets read, but the value
                // is only used if the DMA address is open bus
                self.read(register_activation_addr);
                self.read(address)
            }
        } else if (0x4015..=0x4017).contains(&address) {
            // No bus conflict, but DMA tried to read from an APU register that is not activated
            self.0.cpu_open_bus
        } else {
            // No bus conflict and read goes through
            self.read(address)
        }
    }

    pub fn oam_dma_read(&mut self, address: u16, halted_cpu_address: u16) -> u8 {
        self.dma_read::<false>(address, halted_cpu_address)
    }

    pub fn dmc_dma_read(&mut self, address: u16, halted_cpu_address: u16) -> u8 {
        self.dma_read::<true>(address, halted_cpu_address)
    }

    fn apply_write(&mut self, address: u16, value: u8) {
        self.0.cpu_open_bus = value;

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

    fn read_ppu_open_bus(&mut self) -> u8 {
        if self.0.ppu_registers.ppu_open_bus == 0 {
            return 0;
        }

        for i in 0..8 {
            if self.0.ppu_registers.ppu_open_bus.bit(i)
                && self.0.ppu_registers.ppu_open_bus_decay_cycles[i as usize]
                    <= self.0.ppu_registers.total_cycles
            {
                self.0.ppu_registers.ppu_open_bus &= !(1 << i);
            }
        }

        self.0.ppu_registers.ppu_open_bus
    }

    fn set_ppu_open_bus(&mut self, value: u8, mask: u8) {
        let masked = value & mask;
        self.0.ppu_registers.ppu_open_bus = (self.0.ppu_registers.ppu_open_bus & !mask) | masked;
        for i in 0..8 {
            if masked.bit(i) {
                // Decay to 0 after slightly more than half a second
                // Exact time varies in actual hardware but seems to always be less than a second
                self.0.ppu_registers.ppu_open_bus_decay_cycles[i as usize] =
                    self.0.ppu_registers.total_cycles + 1000000;
            }
        }
    }

    pub fn read_ppu_register(&mut self, register: PpuRegister) -> u8 {
        match register {
            PpuRegister::PPUCTRL
            | PpuRegister::PPUMASK
            | PpuRegister::OAMADDR
            | PpuRegister::PPUSCROLL
            | PpuRegister::PPUADDR => self.read_ppu_open_bus(),
            PpuRegister::PPUSTATUS => {
                self.0.ppu_registers.ppu_status_read = true;

                // PPUSTATUS reads only affect bits 7-5 of open bus, bits 4-0 remain intact
                // and are returned as part of the read
                let ppu_status_high_bits = self.0.ppu_registers.ppu_status & 0xE0;
                let open_bus_lower_bits = self.read_ppu_open_bus() & 0x1F;
                self.set_ppu_open_bus(ppu_status_high_bits, 0xE0);

                ppu_status_high_bits | open_bus_lower_bits
            }
            PpuRegister::OAMDATA => {
                let mut value = self
                    .0
                    .ppu_registers
                    .oam_open_bus_value
                    .unwrap_or(self.0.ppu_oam[self.0.ppu_registers.oam_addr as usize]);
                if self.0.ppu_registers.oam_addr & 3 == 2 {
                    // Bits 2-4 of the third byte always read 0
                    value &= !0x1C;
                }

                self.set_ppu_open_bus(value, 0xFF);
                value
            }
            PpuRegister::PPUDATA => {
                let address = self.0.ppu_bus_address;
                let (data, buffer_read_address) = if address < 0x3F00 {
                    let data = self.0.ppu_registers.ppu_data_buffer;
                    self.set_ppu_open_bus(data, 0xFF);
                    (data, address)
                } else {
                    let palette_address = address & PALETTE_RAM_MASK;
                    let palette_byte = self.0.ppu_palette_ram[palette_address as usize] & 0x3F;
                    let open_bus_bits = self.read_ppu_open_bus() & 0xC0;
                    self.set_ppu_open_bus(palette_byte, 0x3F);

                    // When PPUDATA is used to read palette RAM, buffer reads mirror the nametable
                    // data located at $2F00-$2FFF
                    (palette_byte | open_bus_bits, address - 0x1000)
                };

                self.0.mapper.about_to_access_ppu_data();

                self.0.ppu_registers.ppu_data_buffer =
                    self.0.ppu().read_address(buffer_read_address);
                // Reset the bus address in case the buffer read address was different from the
                // actual address
                self.0.ppu_bus_address = address;

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
        self.set_ppu_open_bus(value, 0xFF);

        if self.0.ppu_registers.reset_flag
            && matches!(
                register,
                PpuRegister::PPUCTRL
                    | PpuRegister::PPUMASK
                    | PpuRegister::PPUSCROLL
                    | PpuRegister::PPUADDR
            )
        {
            // These registers are not writable until the first pre-render scanline after reset
            return;
        }

        match register {
            PpuRegister::PPUCTRL => {
                self.0.ppu_registers.ppu_ctrl = value;
                self.0.ppu_registers.last_accessed_register = Some(PpuTrackedRegister::PPUCTRL);
                self.0.mapper.process_ppu_ctrl_update(value);
            }
            PpuRegister::PPUMASK => {
                log::trace!("BUS: PPUMASK set to {value:02X}");
                self.0.ppu_registers.ppu_mask = value;
                self.0.mapper.process_ppu_mask_update(value);
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

impl PpuBus<'_> {
    pub fn read_address(&mut self, address: u16) -> u8 {
        // PPU bus only has 14-bit addressing
        let address = address & 0x3FFF;

        self.0.ppu_bus_address = address;

        match address {
            0x0000..=0x3EFF => self.0.mapper.read_ppu_address(address, &self.0.ppu_vram),
            0x3F00..=0x3FFF => self.0.ppu_palette_ram[(address & PALETTE_RAM_MASK) as usize],
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
                let palette_ram_addr = (address & PALETTE_RAM_MASK) as usize;
                self.0.ppu_palette_ram[palette_ram_addr] = value;

                // Sprite backdrop colors ($3F10, $3F14, $3F18, $3F1C) mirror BG backdrop colors ($3F00, $3F04, $3F08, $3F0C)
                // Emulate this by writing to both locations whenever either is written to.
                // Super Mario Bros. and Micro Machines depend on this for correct colors
                if palette_ram_addr & 3 == 0 {
                    self.0.ppu_palette_ram[palette_ram_addr ^ 0x10] = value;
                }
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

    pub fn handle_pixel_rendered(&mut self, pixel: u8, x: u16, y: u16, display_mode: TimingMode) {
        if let Some(zapper_state) = &mut self.0.io_registers.zapper_state {
            let overscan = self.0.io_registers.overscan;
            zapper_state.handle_pixel_rendered(pixel, x, y, display_mode, overscan);
        }
    }

    pub fn reset(&mut self) {
        self.0.ppu_registers.ppu_ctrl = 0x00;
        self.0.ppu_registers.ppu_mask = 0x00;
        self.0.ppu_registers.write_toggle = PpuWriteToggle::First;
        self.0.ppu_registers.ppu_data_buffer = 0x00;
        self.0.ppu_registers.ppu_open_bus = 0x00;
        self.0.ppu_registers.reset_flag = true;
        self.0.mapper.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn randomized_ram_on_startup() {
        let mapper = cartridge::new_mmc1(vec![0; 32768]);
        let bus1 = Bus::from_cartridge(mapper.clone(), Overscan::default());
        let bus2 = Bus::from_cartridge(mapper, Overscan::default());

        assert_ne!(bus1.cpu_internal_ram, bus2.cpu_internal_ram);
    }
}
