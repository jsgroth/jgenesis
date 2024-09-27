//! Game Boy PPU (picture processing unit)

mod debug;
mod fifo;
mod registers;

use crate::HardwareMode;
use crate::dma::DmaUnit;
use crate::interrupts::InterruptRegisters;
use crate::ppu::fifo::PixelFifo;
use crate::ppu::registers::{CgbPaletteRam, Registers};
use crate::sm83::InterruptType;
use crate::speed::CpuSpeed;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::FrameSize;
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::ops::{Deref, DerefMut, Range};

const SCREEN_WIDTH: usize = 160;
const SCREEN_HEIGHT: usize = 144;

pub const FRAME_BUFFER_LEN: usize = SCREEN_WIDTH * SCREEN_HEIGHT;

pub const FRAME_SIZE: FrameSize =
    FrameSize { width: SCREEN_WIDTH as u32, height: SCREEN_HEIGHT as u32 };

// 144 rendered lines + 10 VBlank lines
const LINES_PER_FRAME: u8 = 154;
const DOTS_PER_LINE: u16 = 456;
const OAM_SCAN_DOTS: u16 = 80;

const MAX_SPRITES_PER_LINE: usize = 10;

const VRAM_LEN: usize = 16 * 1024;
const OAM_LEN: usize = 160;

type Vram = [u8; VRAM_LEN];
type Oam = [u8; OAM_LEN];

#[derive(Debug, Clone, FakeEncode, FakeDecode)]
pub struct PpuFrameBuffer(Box<[u16; FRAME_BUFFER_LEN]>);

impl PpuFrameBuffer {
    pub fn iter(&self) -> impl Iterator<Item = u16> + '_ {
        self.0.iter().copied()
    }

    fn set(&mut self, line: u8, pixel: u8, color: u16) {
        self[(line as usize) * SCREEN_WIDTH + (pixel as usize)] = color;
    }
}

impl Default for PpuFrameBuffer {
    fn default() -> Self {
        Self(vec![0; FRAME_BUFFER_LEN].into_boxed_slice().try_into().unwrap())
    }
}

impl Deref for PpuFrameBuffer {
    type Target = [u16; FRAME_BUFFER_LEN];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for PpuFrameBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum PpuMode {
    // Mode 1
    VBlank,
    // Mode 0
    HBlank,
    // Mode 2
    ScanningOam,
    // Glitched mode 2 that occurs after re-enabling the PPU
    ScanningOamGlitched,
    // Mode 3
    Rendering,
}

impl PpuMode {
    fn to_bits(self) -> u8 {
        match self {
            Self::HBlank => 0,
            Self::VBlank => 1,
            Self::ScanningOam | Self::ScanningOamGlitched => 2,
            Self::Rendering => 3,
        }
    }

    fn is_scanning_oam(self) -> bool {
        matches!(self, Self::ScanningOam | Self::ScanningOamGlitched)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct State {
    scanline: u8,
    dot: u16,
    mode: PpuMode,
    prev_stat_interrupt_line: bool,
    stat_interrupt_pending: bool,
    previously_enabled: bool,
    // LY=LYC bit in STAT does not change while PPU is disabled, per:
    // https://gbdev.gg8.se/wiki/articles/Tricky-to-emulate_games
    frozen_ly_lyc_bit: bool,
    skip_next_frame: bool,
    frame_complete: bool,
    dots_until_frame_clear: u32,
}

impl State {
    fn new() -> Self {
        Self {
            scanline: 0,
            dot: 0,
            mode: PpuMode::ScanningOam,
            prev_stat_interrupt_line: false,
            stat_interrupt_pending: false,
            previously_enabled: true,
            frozen_ly_lyc_bit: false,
            skip_next_frame: true,
            frame_complete: false,
            dots_until_frame_clear: 0,
        }
    }

    fn ly(&self) -> u8 {
        if self.scanline == LINES_PER_FRAME - 1 && self.dot >= 4 {
            // LY=0 starts 4 dots into the final scanline. Kirby's Dream Land 2 and Wario Land 2 depend on this for
            // minor top-of-screen effects
            0
        } else {
            self.scanline
        }
    }

    fn ly_for_compare(&self, cpu_speed: CpuSpeed) -> u8 {
        // This handles two edge cases for LY=LYC interrupts:
        //
        // 1. HBlank interrupts should not block LY=LYC interrupts; Ken Griffey Jr.'s Slugfest
        // depends on this
        //
        // 2. When LYC=0, the LY=LYC interrupt should not trigger before dot 9 in single speed
        // or before dot 13 in CGB double speed. The demo Mental Respirator depends on this for the
        // "gin & tonic trick" effect
        match self.scanline {
            0 => 0,
            line @ 1..=152 => {
                if self.dot != 0 {
                    line
                } else {
                    line - 1
                }
            }
            153 => match (self.dot, cpu_speed) {
                (0, _) => 152,
                (1..=8, _) | (9..=12, CpuSpeed::Double) => 153,
                _ => 0,
            },
            _ => panic!("Invalid scanline in state: {}", self.scanline),
        }
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct SpriteData {
    oam_index: u8,
    x: u8,
    y: u8,
    tile_number: u8,
    vram_bank: u8,
    palette: u8,
    horizontal_flip: bool,
    vertical_flip: bool,
    low_priority: bool,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Ppu {
    hardware_mode: HardwareMode,
    frame_buffer: PpuFrameBuffer,
    vram: Box<Vram>,
    oam: Box<Oam>,
    registers: Registers,
    bg_palette_ram: CgbPaletteRam,
    sprite_palette_ram: CgbPaletteRam,
    state: State,
    sprite_buffer: Vec<SpriteData>,
    fifo: PixelFifo,
}

macro_rules! cgb_only_read {
    ($ppu:ident.$($op:tt)*) => {
        match $ppu.hardware_mode {
            HardwareMode::Dmg => 0xFF,
            HardwareMode::Cgb => $ppu.$($op)*,
        }
    }
}

macro_rules! cgb_only_write {
    ($ppu:ident.$($op:tt)*) => {
        if $ppu.hardware_mode == HardwareMode::Cgb {
            $ppu.$($op)*;
        }
    }
}

impl Ppu {
    pub fn new(hardware_mode: HardwareMode, rom: &[u8]) -> Self {
        let mut vram = vec![0; VRAM_LEN];
        initialize_vram(hardware_mode, rom, &mut vram);

        Self {
            hardware_mode,
            frame_buffer: PpuFrameBuffer::default(),
            vram: vram.into_boxed_slice().try_into().unwrap(),
            oam: vec![0; OAM_LEN].into_boxed_slice().try_into().unwrap(),
            registers: Registers::new(),
            bg_palette_ram: CgbPaletteRam::new(),
            sprite_palette_ram: CgbPaletteRam::new(),
            state: State::new(),
            sprite_buffer: Vec::with_capacity(MAX_SPRITES_PER_LINE),
            fifo: PixelFifo::new(hardware_mode),
        }
    }

    pub fn tick_dot(
        &mut self,
        cpu_speed: CpuSpeed,
        dma_unit: &DmaUnit,
        interrupt_registers: &mut InterruptRegisters,
    ) {
        if !self.registers.ppu_enabled {
            if self.state.previously_enabled {
                match self.hardware_mode {
                    HardwareMode::Dmg => {
                        self.clear_frame_buffer();
                    }
                    HardwareMode::Cgb => {
                        if self.state.scanline < SCREEN_HEIGHT as u8 {
                            self.clear_frame_buffer();
                        } else {
                            // On CGB, disabling the PPU only seems to clear the screen if it's
                            // left disabled for a long enough time.
                            // A Bug's Life depends on this or the screen will flash in some parts
                            // of the game
                            // TODO is this really a CGB-only behavior?
                            self.state.dots_until_frame_clear =
                                (SCREEN_HEIGHT as u32) * u32::from(DOTS_PER_LINE);
                        }
                    }
                }

                // Disabling PPU freezes the LY=LYC bit until it's re-enabled, per:
                // https://gbdev.gg8.se/wiki/articles/Tricky-to-emulate_games
                self.state.frozen_ly_lyc_bit = self.state.scanline == self.registers.ly_compare;

                // Disabling the PPU moves it to line 0 + mode 0 and clears the display
                self.state.scanline = 0;
                self.state.dot = 0;
                self.state.mode = PpuMode::HBlank;

                self.sprite_buffer.clear();
                self.fifo.reset_window_state();
                self.fifo.start_new_line(0, &self.registers, &[]);

                self.state.previously_enabled = false;
                self.state.stat_interrupt_pending = false;
                self.state.prev_stat_interrupt_line = false;

                return;
            }

            if self.state.dots_until_frame_clear != 0 {
                self.state.dots_until_frame_clear -= 1;
                if self.state.dots_until_frame_clear == 0 {
                    self.clear_frame_buffer();
                }
            }

            // Unlike TV-based systems, the PPU does not process at all when display is disabled
            return;
        } else if !self.state.previously_enabled {
            self.state.previously_enabled = true;

            // When the PPU is re-enabled, the next frame is not displayed
            self.state.skip_next_frame = true;

            self.state.mode = PpuMode::ScanningOamGlitched;
        }

        // STAT interrupts don't seem to fire during the first 4 dots of line 0
        if self.state.stat_interrupt_pending && (self.state.scanline != 0 || self.state.dot >= 4) {
            log::trace!(
                "Generating STAT interrupt at line {} dot {}",
                self.state.scanline,
                self.state.dot
            );

            interrupt_registers.set_flag(InterruptType::LcdStatus);
            self.state.stat_interrupt_pending = false;
        }

        if self.state.mode == PpuMode::Rendering {
            self.fifo.tick(
                &self.vram,
                &self.registers,
                &self.bg_palette_ram,
                &self.sprite_palette_ram,
                &mut self.frame_buffer,
            );
            if self.fifo.done_with_line() {
                log::trace!(
                    "Pixel FIFO finished line {} after dot {}",
                    self.state.scanline,
                    self.state.dot
                );
                self.state.mode = PpuMode::HBlank;
            }
        }

        self.state.dot += 1;
        if self.state.dot == DOTS_PER_LINE {
            // Check the window Y condition again before moving to the next line.
            // The fairylake.gb test ROM depends on this because it enables the window during mode 3
            // with WY==LY
            self.fifo.check_window_y(self.state.scanline, &self.registers);

            self.state.dot = 0;
            self.state.scanline += 1;
            if self.state.scanline == LINES_PER_FRAME {
                self.state.scanline = 0;
                self.fifo.reset_window_state();
            }

            if self.state.scanline < SCREEN_HEIGHT as u8 {
                self.state.mode = PpuMode::ScanningOam;

                self.sprite_buffer.clear();

                // PPU cannot read OAM while an OAM DMA is in progress
                if !dma_unit.oam_dma_in_progress() {
                    scan_oam(
                        self.hardware_mode,
                        self.state.scanline,
                        self.registers.double_height_sprites,
                        &self.oam,
                        &mut self.sprite_buffer,
                    );
                }
            } else {
                self.state.mode = PpuMode::VBlank;
            }
        } else if self.state.scanline < SCREEN_HEIGHT as u8 && self.state.dot == OAM_SCAN_DOTS {
            self.fifo.start_new_line(self.state.scanline, &self.registers, &self.sprite_buffer);
            self.state.mode = PpuMode::Rendering;
        }

        // TODO timing
        if self.state.scanline == SCREEN_HEIGHT as u8 && self.state.dot == 1 {
            interrupt_registers.set_flag(InterruptType::VBlank);
            if self.state.skip_next_frame {
                self.state.skip_next_frame = false;
            } else {
                self.state.frame_complete = true;
            }
        }

        let stat_interrupt_line = self.stat_interrupt_line(cpu_speed);
        if !self.state.prev_stat_interrupt_line && stat_interrupt_line {
            self.state.stat_interrupt_pending = true;
            log::trace!(
                "Setting STAT pending: LY={}, LYC={}, mode={:?}",
                self.state.ly(),
                self.registers.ly_compare,
                self.state.mode
            );
        }
        self.state.prev_stat_interrupt_line = stat_interrupt_line;
    }

    fn clear_frame_buffer(&mut self) {
        log::trace!("Clearing PPU frame buffer");

        // Disabling display makes the entire display white, which is color 0 on DMG
        // and color 31/31/31 ($7FFF) on CGB
        let fill_color = match self.hardware_mode {
            HardwareMode::Dmg => 0,
            HardwareMode::Cgb => 0b11111_11111_11111,
        };
        self.frame_buffer.fill(fill_color);

        // Signal that the frame should be displayed
        self.state.frame_complete = true;
    }

    fn stat_interrupt_line(&self, cpu_speed: CpuSpeed) -> bool {
        let lyc_interrupt_enabled = self.registers.lyc_interrupt_enabled;
        let mode_2_interrupt_enabled = self.registers.mode_2_interrupt_enabled;
        let mode_1_interrupt_enabled = self.registers.mode_1_interrupt_enabled;
        let mode_0_interrupt_enabled = self.registers.mode_0_interrupt_enabled;

        (lyc_interrupt_enabled && self.state.ly_for_compare(cpu_speed) == self.registers.ly_compare)
            || (mode_2_interrupt_enabled && self.state.mode.is_scanning_oam())
            || (mode_1_interrupt_enabled && self.state.mode == PpuMode::VBlank)
            || (mode_0_interrupt_enabled && self.state.mode == PpuMode::HBlank)
    }

    pub fn frame_buffer(&self) -> &PpuFrameBuffer {
        &self.frame_buffer
    }

    pub fn frame_complete(&self) -> bool {
        self.state.frame_complete
    }

    pub fn clear_frame_complete(&mut self) {
        self.state.frame_complete = false;
    }

    pub fn read_vram(&self, address: u16) -> u8 {
        if self.cpu_can_access_vram() {
            let vram_addr = map_vram_address(address, self.registers.vram_bank);
            self.vram[vram_addr as usize]
        } else {
            0xFF
        }
    }

    pub fn write_vram(&mut self, address: u16, value: u8) {
        if self.cpu_can_access_vram() {
            let vram_addr = map_vram_address(address, self.registers.vram_bank);
            self.vram[vram_addr as usize] = value;
        }
    }

    pub fn read_oam(&self, address: u16) -> u8 {
        if self.cpu_can_access_oam() { self.oam[(address & 0xFF) as usize] } else { 0xFF }
    }

    pub fn write_oam(&mut self, address: u16, value: u8) {
        if self.cpu_can_access_oam() {
            self.oam[(address & 0xFF) as usize] = value;
        }
    }

    // OAM DMA can write to OAM at any time, even during Modes 2 and 3
    pub fn write_oam_for_dma(&mut self, address: u16, value: u8) {
        self.oam[(address & 0xFF) as usize] = value;
    }

    fn cpu_can_access_oam(&self) -> bool {
        !matches!(self.state.mode, PpuMode::ScanningOam | PpuMode::Rendering)
    }

    fn cpu_can_access_vram(&self) -> bool {
        // Allow access even during mode 3 if dot == 80.
        // Because of how the CPU and PPU are executed, a write at dot == 80 would have occurred
        // on dot 78 (single-speed) or dot 79 (double-speed) on actual hardware and would not have
        // been blocked
        self.state.mode != PpuMode::Rendering || self.state.dot == OAM_SCAN_DOTS
    }

    pub fn mode(&self) -> PpuMode {
        self.state.mode
    }

    pub fn read_register(&self, address: u16) -> u8 {
        match address & 0xFF {
            0x40 => self.registers.read_lcdc(),
            0x41 => self.registers.read_stat(&self.state),
            0x42 => self.registers.bg_y_scroll,
            0x43 => self.registers.bg_x_scroll,
            // LY: Line number
            0x44 => self.state.ly(),
            0x45 => self.registers.ly_compare,
            0x47 => self.registers.read_bgp(),
            0x48 => self.registers.read_obp0(),
            0x49 => self.registers.read_obp1(),
            0x4A => self.registers.window_y,
            0x4B => self.registers.window_x,
            0x4F => cgb_only_read!(self.registers.read_vbk()),
            0x68 => cgb_only_read!(self.bg_palette_ram.read_data_port_address()),
            0x69 => cgb_only_read!(self.bg_palette_ram.read_data_port(self.cpu_can_access_vram())),
            0x6A => cgb_only_read!(self.sprite_palette_ram.read_data_port_address()),
            0x6B => {
                cgb_only_read!(self.sprite_palette_ram.read_data_port(self.cpu_can_access_vram()))
            }
            _ => {
                log::warn!("PPU register read {address:04X}");
                0xFF
            }
        }
    }

    pub fn write_register(
        &mut self,
        address: u16,
        value: u8,
        interrupt_registers: &mut InterruptRegisters,
    ) {
        log::trace!(
            "PPU register write on line {} dot {}: {address:04X} set to {value:02X}",
            self.state.scanline,
            self.state.dot
        );

        match address & 0xFF {
            0x40 => self.registers.write_lcdc(value),
            0x41 => self.write_stat(value, interrupt_registers),
            0x42 => self.registers.write_scy(value),
            0x43 => self.registers.write_scx(value),
            // LY, not writable
            0x44 => {}
            0x45 => self.registers.write_lyc(value),
            0x47 => self.registers.write_bgp(value),
            0x48 => self.registers.write_obp0(value),
            0x49 => self.registers.write_obp1(value),
            0x4A => self.registers.write_wy(value),
            0x4B => self.registers.write_wx(value),
            0x4F => cgb_only_write!(self.registers.write_vbk(value)),
            0x68 => cgb_only_write!(self.bg_palette_ram.write_data_port_address(value)),
            0x69 => cgb_only_write!(
                self.bg_palette_ram.write_data_port(value, self.cpu_can_access_vram())
            ),
            0x6A => cgb_only_write!(self.sprite_palette_ram.write_data_port_address(value)),
            0x6B => cgb_only_write!(
                self.sprite_palette_ram.write_data_port(value, self.cpu_can_access_vram())
            ),
            _ => log::warn!("PPU register write {address:04X} {value:02X}"),
        }
    }

    fn write_stat(&mut self, value: u8, interrupt_registers: &mut InterruptRegisters) {
        if self.hardware_mode == HardwareMode::Dmg && self.registers.ppu_enabled {
            // DMG STAT bug: If STAT is written while any of the 4 STAT conditions are true, the
            // hardware behaves as if all 4 STAT interrupts are enabled for a single M-cycle.
            // Road Rash (GB version) and Zerd no Densetsu depend on this
            let dmg_stat_bug_triggered = self.state.mode != PpuMode::Rendering
                || self.state.ly_for_compare(CpuSpeed::Normal) == self.registers.ly_compare;

            if dmg_stat_bug_triggered {
                // It seems that the DMG STAT bug does not trigger if HBlank interrupts were previously
                // enabled and the current mode is 2 (OAM scan). This doesn't really make sense, but
                // this fixes Initial D Gaiden and doesn't break Road Rash or Zerd no Densetsu
                let suppress_oam_interrupts = self.registers.mode_0_interrupt_enabled
                    && self.state.mode == PpuMode::ScanningOam;
                let bugged_stat_write = !(u8::from(suppress_oam_interrupts) << 5);

                self.registers.write_stat(bugged_stat_write);

                let stat_interrupt_line = self.stat_interrupt_line(CpuSpeed::Normal);
                if !self.state.prev_stat_interrupt_line && stat_interrupt_line {
                    interrupt_registers.set_flag(InterruptType::LcdStatus);
                }
                self.state.prev_stat_interrupt_line = stat_interrupt_line;
            }
        }

        self.registers.write_stat(value);
    }
}

fn map_vram_address(address: u16, vram_bank: u8) -> u16 {
    (u16::from(vram_bank) << 13) | (address & 0x1FFF)
}

fn scan_oam(
    hardware_mode: HardwareMode,
    scanline: u8,
    double_height_sprites: bool,
    oam: &Oam,
    sprite_buffer: &mut Vec<SpriteData>,
) {
    let sprite_height = if double_height_sprites { 16 } else { 8 };

    for oam_idx in 0..OAM_LEN / 4 {
        let oam_addr = 4 * oam_idx;

        let y = oam[oam_addr];

        // Check if sprite overlaps current line
        let sprite_top = i16::from(y) - 16;
        let sprite_bottom = sprite_top + sprite_height;
        if !(sprite_top..sprite_bottom).contains(&scanline.into()) {
            continue;
        }

        let x = oam[oam_addr + 1];
        let tile_number = oam[oam_addr + 2];

        let attributes = oam[oam_addr + 3];
        let horizontal_flip = attributes.bit(5);
        let vertical_flip = attributes.bit(6);
        let low_priority = attributes.bit(7);

        // VRAM bank is only valid in CGB mode, and palette is read from different bits
        let (vram_bank, palette) = match hardware_mode {
            HardwareMode::Dmg => (0, attributes.bit(4).into()),
            HardwareMode::Cgb => (attributes.bit(3).into(), attributes & 0x07),
        };

        sprite_buffer.push(SpriteData {
            oam_index: oam_idx as u8,
            x,
            y,
            tile_number,
            vram_bank,
            palette,
            horizontal_flip,
            vertical_flip,
            low_priority,
        });
        if sprite_buffer.len() == MAX_SPRITES_PER_LINE {
            break;
        }
    }

    sprite_buffer.sort_by(|a, b| a.x.cmp(&b.x).then(a.oam_index.cmp(&b.oam_index)));
}

const NINTENDO_LOGO_ADDR: Range<usize> = 0x0104..0x0134;
const LOGO_TILE_DATA_ADDR: usize = 0x0010;

const TRADEMARK_TILE_DATA_ADDR: usize = 0x0190;
const TRADEMARK_SYMBOL: [u8; 16] = [
    0x3C, 0x00, 0x42, 0x00, 0xB9, 0x00, 0xA5, 0x00, 0xB9, 0x00, 0xA5, 0x00, 0x42, 0x00, 0x3C, 0x00,
];

// Initialize VRAM the way that the DMG boot ROM would. The Nintendo logo is copied out of the
// cartridge header.
// Some games depend on this by assuming that VRAM initially contains the Nintendo logo and a
// trademark symbol, e.g. X for its intro animation
fn initialize_vram(hardware_mode: HardwareMode, rom: &[u8], vram: &mut [u8]) {
    if hardware_mode != HardwareMode::Dmg {
        // Only write the logo to VRAM on DMG
        return;
    }

    if rom.len() < NINTENDO_LOGO_ADDR.end {
        // Invalid ROM; don't try to initialize VRAM
        return;
    }

    // Write logo to tile data area
    let logo = &rom[NINTENDO_LOGO_ADDR];
    for (i, logo_byte) in logo.iter().copied().enumerate() {
        for nibble_idx in 0..2 {
            let nibble = logo_byte >> (4 * (1 - nibble_idx));

            // Duplicate pixels horizontally
            let vram_byte = ((nibble & 8) << 4)
                | ((nibble & 8) << 3)
                | ((nibble & 4) << 3)
                | ((nibble & 4) << 2)
                | ((nibble & 2) << 2)
                | ((nibble & 2) << 1)
                | ((nibble & 1) << 1)
                | (nibble & 1);

            // Duplicate pixels vertically
            let vram_addr = LOGO_TILE_DATA_ADDR + 4 * (2 * i + nibble_idx);
            vram[vram_addr] = vram_byte;
            vram[vram_addr + 2] = vram_byte;
        }
    }

    // Write trademark to tile data area
    vram[TRADEMARK_TILE_DATA_ADDR..TRADEMARK_TILE_DATA_ADDR + TRADEMARK_SYMBOL.len()]
        .copy_from_slice(&TRADEMARK_SYMBOL);

    // Populate tile map
    // The upscaled logo is 12x2 tiles and should be centered, ranging from (X=4, Y=8) to (X=16, Y=10)
    for tile_row in 0..2 {
        for tile_col in 0..12 {
            let vram_addr = 0x1800 + (8 + tile_row) * 32 + (4 + tile_col);
            vram[vram_addr] = (1 + (12 * tile_row) + tile_col) as u8;
        }
    }

    // Trademark symbol should be in the top row just to the right of the logo, at (X=16, Y=8)
    vram[0x1800 + 8 * 32 + 16] = (TRADEMARK_TILE_DATA_ADDR / 16) as u8;
}
