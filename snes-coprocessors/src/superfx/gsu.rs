mod codecache;
mod instructions;

use crate::superfx::gsu::codecache::CodeCache;
use crate::superfx::gsu::instructions::PlotState;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::EnumDisplay;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay)]
enum MultiplierSpeed {
    #[default]
    Standard,
    High,
}

impl MultiplierSpeed {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::High } else { Self::Standard }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum ClockSpeed {
    #[default]
    Slow,
    Fast,
}

impl ClockSpeed {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Fast } else { Self::Slow }
    }

    const fn mclk_divider(self) -> u64 {
        match self {
            // mclk/2 = 10.74 MHz
            Self::Slow => 2,
            // mclk/1 = 21.47 MHz
            Self::Fast => 1,
        }
    }

    const fn memory_access_cycles(self) -> u8 {
        match self {
            Self::Slow => 3,
            Self::Fast => 5,
        }
    }

    const fn rom_buffer_wait_cycles(self) -> u8 {
        // TODO are these numbers right?
        match self {
            Self::Slow => 4,
            Self::Fast => 7,
        }
    }
}

impl Display for ClockSpeed {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Slow => write!(f, "10.74 MHz"),
            Self::Fast => write!(f, "21.47 MHz"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum ColorGradientColors {
    #[default]
    Four,
    Sixteen,
    TwoFiftySix,
}

impl ColorGradientColors {
    fn from_byte(byte: u8) -> Self {
        match byte & 0x03 {
            0x00 => Self::Four,
            0x01 => Self::Sixteen,
            0x02 | 0x03 => Self::TwoFiftySix,
            _ => unreachable!("value & 0x03 is always <= 0x03"),
        }
    }

    const fn tile_size(self) -> u32 {
        match self {
            Self::Four => 16,
            Self::Sixteen => 32,
            Self::TwoFiftySix => 64,
        }
    }

    const fn bitplanes(self) -> u32 {
        match self {
            Self::Four => 2,
            Self::Sixteen => 4,
            Self::TwoFiftySix => 8,
        }
    }

    const fn color_mask(self) -> u8 {
        match self {
            Self::Four => 0x03,
            Self::Sixteen => 0x0F,
            Self::TwoFiftySix => 0xFF,
        }
    }
}

impl Display for ColorGradientColors {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Four => write!(f, "4-color"),
            Self::Sixteen => write!(f, "16-color"),
            Self::TwoFiftySix => write!(f, "256-color"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum ScreenHeight {
    #[default]
    Bg128Pixel,
    Bg160Pixel,
    Bg192Pixel,
    ObjMode,
}

impl ScreenHeight {
    fn from_byte(byte: u8) -> Self {
        match (byte.bit(5), byte.bit(2)) {
            (false, false) => Self::Bg128Pixel,
            (false, true) => Self::Bg160Pixel,
            (true, false) => Self::Bg192Pixel,
            (true, true) => Self::ObjMode,
        }
    }
}

impl Display for ScreenHeight {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bg128Pixel => write!(f, "128-pixel"),
            Self::Bg160Pixel => write!(f, "160-pixel"),
            Self::Bg192Pixel => write!(f, "192-pixel"),
            Self::ObjMode => write!(f, "OBJ mode"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum BusAccess {
    #[default]
    Snes,
    Gsu,
}

impl BusAccess {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Gsu } else { Self::Snes }
    }
}

impl Display for BusAccess {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Snes => write!(f, "SNES"),
            Self::Gsu => write!(f, "GSU"),
        }
    }
}

const NOP_OPCODE: u8 = 0x01;

#[derive(Debug, Clone, Encode, Decode)]
struct GsuState {
    opcode_buffer: u8,
    rom_buffer: u8,
    rom_buffer_wait_cycles: u8,
    ram_buffer_wait_cycles: u8,
    ram_address_buffer: u16,
    rom_pointer_changed: bool,
    ram_buffer_written: bool,
    just_jumped: bool,
}

impl GsuState {
    fn new() -> Self {
        Self {
            opcode_buffer: NOP_OPCODE,
            rom_buffer: 0,
            rom_buffer_wait_cycles: 0,
            ram_buffer_wait_cycles: 0,
            ram_address_buffer: 0,
            rom_pointer_changed: false,
            ram_buffer_written: false,
            just_jumped: false,
        }
    }
}

const VERSION_REGISTER: u8 = 0x04;

#[derive(Debug, Clone, Encode, Decode)]
pub struct GraphicsSupportUnit {
    r: [u16; 16],
    r_latch: u8,
    pbr: u8,
    rombr: u8,
    code_cache: CodeCache,
    state: GsuState,
    plot_state: PlotState,
    zero_flag: bool,
    carry_flag: bool,
    sign_flag: bool,
    overflow_flag: bool,
    go: bool,
    alt1: bool,
    alt2: bool,
    b: bool,
    sreg: u8,
    dreg: u8,
    irq: bool,
    irq_enabled: bool,
    multiplier_speed: MultiplierSpeed,
    clock_speed: ClockSpeed,
    screen_base: u32,
    color: u8,
    color_gradient: ColorGradientColors,
    screen_height: ScreenHeight,
    plot_transparent_pixels: bool,
    dither_on: bool,
    por_high_nibble_flag: bool,
    por_freeze_high_nibble: bool,
    force_obj_mode: bool,
    rom_access: BusAccess,
    ram_access: BusAccess,
    wait_cycles: u8,
}

impl GraphicsSupportUnit {
    pub fn new() -> Self {
        Self {
            r: [0; 16],
            r_latch: 0,
            pbr: 0,
            rombr: 0,
            code_cache: CodeCache::new(),
            state: GsuState::new(),
            plot_state: PlotState::new(),
            zero_flag: false,
            carry_flag: false,
            sign_flag: false,
            overflow_flag: false,
            go: false,
            alt1: false,
            alt2: false,
            b: false,
            sreg: 0,
            dreg: 0,
            irq: false,
            irq_enabled: false,
            multiplier_speed: MultiplierSpeed::Standard,
            clock_speed: ClockSpeed::Slow,
            screen_base: 0,
            color: 0,
            color_gradient: ColorGradientColors::default(),
            screen_height: ScreenHeight::default(),
            plot_transparent_pixels: false,
            dither_on: false,
            por_high_nibble_flag: false,
            por_freeze_high_nibble: false,
            force_obj_mode: false,
            rom_access: BusAccess::default(),
            ram_access: BusAccess::default(),
            wait_cycles: 0,
        }
    }

    pub fn read_register(&mut self, address: u32) -> Option<u8> {
        log::trace!("GSU register read {:04X}", address & 0xFFFF);

        if self.go {
            // Only SFR and VCR can be read while the GSU is running
            return match address & 0x3F {
                0x30 => Some(self.read_sfr_low()),
                0x31 => Some(self.read_sfr_high()),
                0x3B => Some(VERSION_REGISTER),
                _ => None,
            };
        }

        match address & 0x3F {
            0x00..=0x1F => Some(self.read_r(address)),
            0x20 | 0x30 => Some(self.read_sfr_low()),
            0x21 | 0x31 => Some(self.read_sfr_high()),
            0x24 | 0x34 => Some(self.pbr),
            0x26 | 0x36 => Some(self.rombr),
            0x2B | 0x3B => Some(VERSION_REGISTER),
            0x2E | 0x3E => Some((self.code_cache.cbr() >> 8) as u8),
            0x2F | 0x3F => Some(self.code_cache.cbr() as u8),
            _ => Some(0x00),
        }
    }

    #[allow(clippy::match_same_arms)]
    pub fn write_register(&mut self, address: u32, value: u8) {
        log::trace!("GSU register write {:04X} {value:02X}", address & 0xFFFF);

        if self.go {
            // Only SFR and SCMR can be written while the GSU is running
            match address & 0xFFFF {
                0x3030 => self.write_sfr(value),
                0x303A => self.write_scmr(value),
                _ => {}
            }
            return;
        }

        match address & 0xFFFF {
            0x3000..=0x301F => self.write_r(address, value),
            0x3030 => self.write_sfr(value),
            0x3034 => self.write_pbr(value),
            0x3037 => self.write_cfgr(value),
            0x3038 => self.write_scbr(value),
            0x3039 => self.write_clsr(value),
            0x303A => self.write_scmr(value),
            _ => {}
        }
    }

    pub fn read_code_cache_ram(&self, address: u32) -> Option<u8> {
        if self.go {
            return None;
        }

        let ram_addr = map_snes_code_cache_address(address, self.code_cache.cbr());
        Some(self.code_cache.read_ram(ram_addr as u16))
    }

    pub fn write_code_cache_ram(&mut self, address: u32, value: u8) {
        if self.go {
            return;
        }

        let ram_addr = map_snes_code_cache_address(address, self.code_cache.cbr());
        self.code_cache.write_ram(ram_addr as u16, value);
    }

    pub fn is_running(&self) -> bool {
        self.go
    }

    pub fn rom_access(&self) -> BusAccess {
        self.rom_access
    }

    pub fn ram_access(&self) -> BusAccess {
        self.ram_access
    }

    pub fn tick(&mut self, master_cycles_elapsed: u64, rom: &[u8], ram: &mut [u8]) {
        if !self.go {
            self.wait_cycles = 0;
            return;
        }

        let mut gsu_cycles = master_cycles_elapsed / self.clock_speed.mclk_divider();
        while gsu_cycles >= u64::from(self.wait_cycles) {
            gsu_cycles -= u64::from(self.wait_cycles);
            self.wait_cycles = instructions::execute(self, rom, ram);

            // Check if a STOP was executed
            if !self.go {
                self.wait_cycles = 0;
                return;
            }
        }

        self.wait_cycles -= gsu_cycles as u8;
    }

    pub fn irq(&self) -> bool {
        self.irq_enabled && self.irq
    }

    pub fn reset(&mut self) {
        self.go = false;
        self.code_cache.full_clear();
        self.irq = false;
    }

    fn read_r(&self, address: u32) -> u8 {
        let idx = (address & 0x1F) >> 1;
        if !address.bit(0) { self.r[idx as usize] as u8 } else { (self.r[idx as usize] >> 8) as u8 }
    }

    fn read_sfr_low(&self) -> u8 {
        (u8::from(self.zero_flag) << 1)
            | (u8::from(self.carry_flag) << 2)
            | (u8::from(self.sign_flag) << 3)
            | (u8::from(self.overflow_flag) << 4)
            | (u8::from(self.go) << 5)
            | (u8::from(self.state.rom_buffer_wait_cycles != 0) << 6)
    }

    fn read_sfr_high(&mut self) -> u8 {
        let value = (u8::from(self.alt1))
            | (u8::from(self.alt2) << 1)
            | (u8::from(self.b) << 4)
            | (u8::from(self.irq) << 7);

        // Reading SFR high byte clears pending IRQ
        self.irq = false;

        value
    }

    fn write_r(&mut self, address: u32, value: u8) {
        // R0-R15: General registers (16x 16-bit)
        // R14 is always ROM pointer and R15 is always program counter

        // Writing LSB latches the write
        // Writing MSB writes the register using the latched value + incoming value
        if !address.bit(0) {
            self.r_latch = value;
        } else {
            let idx = ((address & 0x1F) >> 1) as usize;
            self.r[idx] = u16::from_le_bytes([self.r_latch, value]);
            log::trace!("  R{idx}: {:04X}", self.r[idx]);
        }

        // Writing R15 MSB ($301F) causes the GSU to begin execution
        if address & 0x1F == 0x1F {
            self.go = true;
            self.state.just_jumped = true;
            log::trace!("  Started GSU execution");
        }
    }

    fn write_sfr(&mut self, value: u8) {
        // SFR: Status/flag register
        // Bits 1-5 are R/W, rest are read-only
        self.zero_flag = value.bit(1);
        self.carry_flag = value.bit(2);
        self.sign_flag = value.bit(3);
        self.overflow_flag = value.bit(4);

        let prev_go = self.go;
        self.go = value.bit(5);

        if !self.go {
            // Writing GO=0 sets CBR=0 and clears all code cache lines
            self.code_cache.full_clear();
        }

        if !prev_go && self.go {
            self.state.just_jumped = true;
        }

        log::trace!("  GO: {}", self.go);
    }

    fn write_pbr(&mut self, value: u8) {
        self.pbr = value;
        log::trace!("  PBR: {value:02X}");
    }

    fn write_cfgr(&mut self, value: u8) {
        // CFGR: Config register
        self.multiplier_speed = MultiplierSpeed::from_bit(value.bit(5));
        self.irq_enabled = !value.bit(7);

        log::trace!("  Multiplier speed: {}", self.multiplier_speed);
        log::trace!("  GSU IRQ enabled: {}", self.irq_enabled);
    }

    fn write_clsr(&mut self, value: u8) {
        // CLSR: Clock select register
        self.clock_speed = ClockSpeed::from_bit(value.bit(0));

        log::trace!("  Clock speed: {}", self.clock_speed);
    }

    fn write_scbr(&mut self, value: u8) {
        // SCBR: Screen base register
        // value is in 1KB units
        self.screen_base = u32::from(value) << 10;

        log::trace!("  Screen base: {:05X}", self.screen_base);
    }

    fn write_scmr(&mut self, value: u8) {
        // SCMR: Screen mode register
        self.ram_access = BusAccess::from_bit(value.bit(3));
        self.rom_access = BusAccess::from_bit(value.bit(4));

        // TODO it doesn't seem like these fields can be altered while the GSU is running?
        if !self.go {
            self.color_gradient = ColorGradientColors::from_byte(value);
            self.screen_height = ScreenHeight::from_byte(value);
        }

        log::trace!("  ROM bus access: {}", self.rom_access);
        log::trace!("  RAM bus access: {}", self.ram_access);
        log::trace!("  Color gradient: {}", self.color_gradient);
        log::trace!("  Screen height: {}", self.screen_height);
    }
}

fn map_snes_code_cache_address(address: u32, cbr: u16) -> u32 {
    let snes_offset = (address & 0xFFFF) - 0x3100;
    snes_offset.wrapping_sub((cbr & 0x1FF).into()) & 0x1FF
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snes_code_cache_mapping() {
        assert_eq!(0, map_snes_code_cache_address(0x003100, 0));
        assert_eq!(0x1FF, map_snes_code_cache_address(0x0032FF, 0));

        assert_eq!(0, map_snes_code_cache_address(0xC03100, 0));
        assert_eq!(0x1FF, map_snes_code_cache_address(0xC032FF, 0));

        assert_eq!(0, map_snes_code_cache_address(0x003250, 0x9150));
        assert_eq!(0x0AF, map_snes_code_cache_address(0xBF32FF, 0x9150));
        assert_eq!(0x0B0, map_snes_code_cache_address(0x003100, 0x9150));

        assert_eq!(0, map_snes_code_cache_address(0x003250, 0x9B50));
        assert_eq!(0x0AF, map_snes_code_cache_address(0xBF32FF, 0x9B50));
        assert_eq!(0x0B0, map_snes_code_cache_address(0x003100, 0x9B50));
    }
}
