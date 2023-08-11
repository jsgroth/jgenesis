use crate::num::GetBit;
use z80_emu::traits::InterruptLine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VdpVersion {
    MasterSystem,
    MasterSystem2,
    GameGearSmsMode,
    GameGearGgMode,
}

impl VdpVersion {
    const fn cram_address_mask(self) -> u16 {
        match self {
            Self::GameGearGgMode => 0x003F,
            _ => 0x001F,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlWriteFlag {
    First,
    Second,
}

impl ControlWriteFlag {
    fn toggle(self) -> Self {
        match self {
            Self::First => Self::Second,
            Self::Second => Self::First,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DataWriteLocation {
    Vram,
    Cram,
}

#[derive(Debug, Clone)]
struct Registers {
    version: VdpVersion,
    control_write_flag: ControlWriteFlag,
    latched_control_byte: u8,
    data_write_location: DataWriteLocation,
    data_address: u16,
    data_read_buffer: u8,
    display_enabled: bool,
    frame_interrupt_enabled: bool,
    frame_interrupt_pending: bool,
    line_interrupt_enabled: bool,
    line_interrupt_pending: bool,
    sprite_overflow: bool,
    sprite_collision: bool,
    vertical_scroll_lock: bool,
    horizontal_scroll_lock: bool,
    hide_left_column: bool,
    shift_sprites_left: bool,
    double_sprite_height: bool,
    double_sprite_size: bool,
    base_name_table_address: u16,
    base_sprite_table_address: u16,
    base_sprite_pattern_address: u16,
    backdrop_color: u8,
    x_scroll: u8,
    y_scroll: u8,
    line_counter_reload_value: u8,
}

// Data address is 14 bits
const DATA_ADDRESS_MASK: u16 = 0x3FFF;

impl Registers {
    fn new(version: VdpVersion) -> Self {
        Self {
            version,
            control_write_flag: ControlWriteFlag::First,
            latched_control_byte: 0,
            data_write_location: DataWriteLocation::Vram,
            data_address: 0,
            data_read_buffer: 0,
            display_enabled: false,
            frame_interrupt_enabled: false,
            frame_interrupt_pending: false,
            line_interrupt_enabled: false,
            line_interrupt_pending: false,
            sprite_overflow: false,
            sprite_collision: false,
            vertical_scroll_lock: false,
            horizontal_scroll_lock: false,
            hide_left_column: false,
            shift_sprites_left: false,
            double_sprite_height: false,
            double_sprite_size: false,
            base_name_table_address: 0x3800,
            base_sprite_table_address: 0x3F00,
            base_sprite_pattern_address: 0x2000,
            backdrop_color: 0,
            x_scroll: 0,
            y_scroll: 0,
            line_counter_reload_value: 0,
        }
    }

    fn read_control(&mut self) -> u8 {
        let status_flags = (u8::from(self.frame_interrupt_pending) << 7)
            | (u8::from(self.sprite_overflow) << 6)
            | (u8::from(self.sprite_collision) << 5);

        // Control reads clear all status/interrupt flags and reset the control write toggle
        self.frame_interrupt_pending = false;
        self.line_interrupt_pending = false;
        self.sprite_overflow = false;
        self.sprite_collision = false;
        self.control_write_flag = ControlWriteFlag::First;

        status_flags
    }

    fn write_control(&mut self, value: u8, vram: &[u8]) {
        let write_flag = self.control_write_flag;

        log::trace!("VDP control write with flag {write_flag:?}");

        match write_flag {
            ControlWriteFlag::First => {
                self.latched_control_byte = value;

                // Set low byte of data address
                self.data_address = (self.data_address & 0xFF00) | u16::from(value);
            }
            ControlWriteFlag::Second => {
                self.data_address = (self.data_address & 0x00FF) | (u16::from(value & 0x3F) << 8);

                log::trace!("VRAM address set to {:04X?}", self.data_address);

                match value & 0xC0 {
                    0x00 => {
                        // VRAM read
                        self.data_read_buffer = vram[self.data_address as usize];
                        self.data_address = (self.data_address + 1) & DATA_ADDRESS_MASK;

                        self.data_write_location = DataWriteLocation::Vram;

                        log::trace!("VRAM read");
                    }
                    0x40 => {
                        // VRAM write
                        self.data_write_location = DataWriteLocation::Vram;

                        log::trace!("VRAM write");
                    }
                    0x80 => {
                        // Internal register write
                        let register = value & 0x0F;
                        self.write_internal_register(register, self.latched_control_byte);

                        self.data_write_location = DataWriteLocation::Vram;

                        log::trace!(
                            "Internal register write: {register} set to {:02X}",
                            self.latched_control_byte
                        );
                    }
                    0xC0 => {
                        // CRAM write
                        self.data_write_location = DataWriteLocation::Cram;

                        log::trace!("CRAM write");
                    }
                    _ => unreachable!("value & 0xC0 is always 0x00/0x40/0x80/0xC0"),
                }
            }
        }

        self.control_write_flag = write_flag.toggle();
    }

    fn read_data(&mut self, vram: &[u8]) -> u8 {
        let buffered_byte = self.data_read_buffer;

        self.data_read_buffer = vram[self.data_address as usize];
        self.data_address = (self.data_address + 1) & DATA_ADDRESS_MASK;

        // All data accesses reset the write toggle
        self.control_write_flag = ControlWriteFlag::First;

        buffered_byte
    }

    fn write_data(&mut self, value: u8, vram: &mut [u8], cram: &mut [u8]) {
        log::trace!("VDP data write with address {:04X}", self.data_address);

        match self.data_write_location {
            DataWriteLocation::Vram => {
                vram[self.data_address as usize] = value;
            }
            DataWriteLocation::Cram => {
                // CRAM only uses the lowest 5 or 6 address bits
                let cram_addr = self.data_address & self.version.cram_address_mask();
                cram[cram_addr as usize] = value;
            }
        }

        self.data_address = (self.data_address + 1) & DATA_ADDRESS_MASK;

        // All data accesses reset the write toggle
        self.control_write_flag = ControlWriteFlag::First;

        // Hardware quirk: writing to the data port also updates the read buffer
        self.data_read_buffer = value;
    }

    fn write_internal_register(&mut self, register: u8, value: u8) {
        match register {
            0 => {
                // Mode control #1
                self.vertical_scroll_lock = value.bit(7);
                self.horizontal_scroll_lock = value.bit(6);
                self.hide_left_column = value.bit(5);
                self.line_interrupt_enabled = value.bit(4);
                self.shift_sprites_left = value.bit(3);
                // TODO mode select bits M2/M4 and sync/monochrome bit
            }
            1 => {
                // Mode control #2
                self.display_enabled = value.bit(6);
                self.frame_interrupt_enabled = value.bit(5);
                // TODO mode select bits M1/M3
                self.double_sprite_height = value.bit(1);
                self.double_sprite_size = value.bit(0);
            }
            2 => {
                // Base name table address
                // TODO SMS1 hardware quirk - bit 0 is ANDed with A10 when doing name table lookups
                self.base_name_table_address = u16::from(value & 0x0E) << 10;
            }
            // Registers 3 and 4 are effectively unused outside of SMS1 quirks
            5 => {
                // Sprite attribute table base address
                // TODO SMS1 hardware quirk - if bit 0 is cleared then X position and tile index are
                // fetched from the lower half of the table instead of the upper half
                self.base_sprite_table_address = u16::from(value & 0x7E) << 7;
            }
            6 => {
                // Sprite pattern table base address
                // TODO SMS1 hardware quirk - bits 1 and 0 are ANDed with bits 8 and 6 of the tile index
                self.base_sprite_pattern_address = u16::from(value.bit(2)) << 13;
            }
            7 => {
                // Backdrop color
                self.backdrop_color = value & 0x0F;
            }
            8 => {
                // X scroll
                self.x_scroll = value;
            }
            9 => {
                // Y scroll
                self.y_scroll = value;
            }
            10 => {
                // Line counter
                self.line_counter_reload_value = value;
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Palette {
    Palette0,
    Palette1,
}

impl Palette {
    fn base_cram_addr(self) -> u8 {
        match self {
            Self::Palette0 => 0x00,
            Self::Palette1 => 0x10,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct BgTileData {
    priority: bool,
    palette: Palette,
    vertical_flip: bool,
    horizontal_flip: bool,
    tile_index: u16,
}

#[derive(Debug, Clone, Copy)]
struct SpriteData {
    y: u8,
    x: u8,
    tile_index: u8,
}

const VRAM_SIZE: usize = 16 * 1024;
const COLOR_RAM_SIZE: usize = 32;

const SCREEN_WIDTH: u16 = 256;
const FRAME_BUFFER_HEIGHT: u16 = 240;

pub type FrameBuffer = [[u8; SCREEN_WIDTH as usize]; FRAME_BUFFER_HEIGHT as usize];

#[derive(Debug, Clone)]
pub struct Vdp {
    frame_buffer: FrameBuffer,
    registers: Registers,
    vram: [u8; VRAM_SIZE],
    color_ram: [u8; COLOR_RAM_SIZE],
    scanline: u16,
    dot: u16,
}

const DOTS_PER_SCANLINE: u16 = 342;
const NTSC_SCANLINES_PER_FRAME: u16 = 262;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickEffect {
    None,
    FrameComplete,
}

impl Vdp {
    pub fn new(version: VdpVersion) -> Self {
        Self {
            frame_buffer: [[0; SCREEN_WIDTH as usize]; FRAME_BUFFER_HEIGHT as usize],
            registers: Registers::new(version),
            vram: [0; VRAM_SIZE],
            color_ram: [0; COLOR_RAM_SIZE],
            scanline: 0,
            dot: 0,
        }
    }

    fn read_name_table_word(&self, row: u16, col: u16) -> BgTileData {
        let name_table_addr = self.registers.base_name_table_address | (row << 6) | (col << 1);
        let high_byte = self.vram[name_table_addr as usize];
        let low_byte = self.vram[(name_table_addr + 1) as usize];

        let priority = high_byte.bit(4);
        let palette = if !high_byte.bit(3) {
            Palette::Palette0
        } else {
            Palette::Palette1
        };
        let vertical_flip = high_byte.bit(2);
        let horizontal_flip = high_byte.bit(1);
        let tile_index = (u16::from(high_byte.bit(0)) << 8) | u16::from(low_byte);

        BgTileData {
            priority,
            palette,
            vertical_flip,
            horizontal_flip,
            tile_index,
        }
    }

    fn render_scanline(&mut self) {
        let scanline = self.scanline;

        let (coarse_x_scroll, fine_x_scroll) =
            if scanline < 16 && self.registers.horizontal_scroll_lock {
                (0, 0)
            } else {
                (
                    u16::from(self.registers.x_scroll >> 3),
                    u16::from(self.registers.x_scroll & 0x07),
                )
            };

        let color_0 = self.color_ram[0];
        for dot in 0..fine_x_scroll {
            self.frame_buffer[scanline as usize][dot as usize] = color_0;
        }

        // Backdrop color always reads from the second half of CRAM
        let backdrop_color = self.color_ram[0x10 | self.registers.backdrop_color as usize];

        // TODO sprites

        for column in 0..32 {
            let (coarse_y_scroll, fine_y_scroll) =
                if column >= 24 && self.registers.vertical_scroll_lock {
                    (0, 0)
                } else {
                    (
                        u16::from(self.registers.y_scroll >> 3),
                        u16::from(self.registers.y_scroll & 0x07),
                    )
                };

            // TODO hide leftmost column

            // TODO 224-line and 240-line modes - nametable is 32 rows intead of 28
            let name_table_row = (scanline / 8 + coarse_y_scroll) % 28;
            let name_table_col = (column + (32 - coarse_x_scroll)) % 32;
            let bg_tile_data = self.read_name_table_word(name_table_row, name_table_col);

            let bg_tile_addr = (bg_tile_data.tile_index * 32) as usize;
            let bg_tile = &self.vram[bg_tile_addr..bg_tile_addr + 32];

            let base_cram_addr = bg_tile_data.palette.base_cram_addr();

            // TODO vertical flip
            let tile_row = (scanline + fine_y_scroll) % 8;

            for tile_col in 0..8 {
                let dot = 8 * column + fine_x_scroll + tile_col;
                if dot == SCREEN_WIDTH {
                    break;
                }

                // TODO horizontal flip
                let shift = 7 - tile_col;
                let mask = 1 << shift;
                let color_id = ((bg_tile[(4 * tile_row) as usize] & mask) >> shift)
                    | (((bg_tile[(4 * tile_row + 1) as usize] & mask) >> shift) << 1)
                    | (((bg_tile[(4 * tile_row + 2) as usize] & mask) >> shift) << 2)
                    | (((bg_tile[(4 * tile_row + 3) as usize] & mask) >> shift) << 3);

                // TODO properly support GG mode's extended palette RAM

                let pixel_color = if color_id != 0 {
                    self.color_ram[(base_cram_addr | color_id) as usize]
                } else {
                    backdrop_color
                };
                self.frame_buffer[scanline as usize][dot as usize] = pixel_color;
            }
        }
    }

    fn debug_log(&self) {
        log::trace!("Registers: {:04X?}", self.registers);

        log::trace!("CRAM:");
        for (i, value) in self.color_ram.into_iter().enumerate() {
            log::trace!("  {i:02X}: {value:02X}");
        }

        log::trace!(
            "Nametable ({:04X}):",
            self.registers.base_name_table_address
        );
        for row in 0..28 {
            for col in 0..32 {
                let name_table_word = self.read_name_table_word(row, col);
                log::trace!("  ({row}, {col}): {name_table_word:03X?}");

                let name_table_addr =
                    (self.registers.base_name_table_address | (row << 6) | (col << 1)) as usize;
                let memory = &self.vram[name_table_addr..name_table_addr + 2];
                log::trace!("  RAM bytes: {memory:02X?}");
            }
        }

        log::trace!("Tiles:");
        for i in 0..512 {
            let address = i * 32;
            let values = &self.vram[address..address + 32];
            log::trace!("  {i:03X}: {values:02X?}");
        }
    }

    #[must_use]
    pub fn tick(&mut self) -> TickEffect {
        // TODO 224-line / 240-line modes
        if self.registers.display_enabled && self.scanline < 192 && self.dot == 0 {
            self.render_scanline();
        }

        if log::log_enabled!(log::Level::Trace) && self.scanline == 0 && self.dot == 0 {
            self.debug_log();
        }

        // TODO 224-line / 240-line modes
        let vblank_start = self.scanline == 193 && self.dot == 0;
        if vblank_start {
            self.registers.frame_interrupt_pending = true;
        }

        let tick_effect = if vblank_start {
            TickEffect::FrameComplete
        } else {
            TickEffect::None
        };

        // TODO line interrupts

        self.dot += 1;
        if self.dot == DOTS_PER_SCANLINE {
            self.scanline += 1;
            self.dot = 0;

            if self.scanline == NTSC_SCANLINES_PER_FRAME {
                self.scanline = 0;
            }
        }

        tick_effect
    }

    pub fn frame_buffer(&self) -> &FrameBuffer {
        &self.frame_buffer
    }

    pub fn read_control(&mut self) -> u8 {
        self.registers.read_control()
    }

    pub fn write_control(&mut self, value: u8) {
        let prev_display_enabled = self.registers.display_enabled;

        self.registers.write_control(value, &self.vram);

        if prev_display_enabled && !self.registers.display_enabled {
            // Display was just disabled; clear frame buffer
            for row in &mut self.frame_buffer {
                for pixel in row {
                    *pixel = 0x00;
                }
            }
        }
    }

    pub fn read_data(&mut self) -> u8 {
        self.registers.read_data(&self.vram)
    }

    pub fn write_data(&mut self, value: u8) {
        self.registers
            .write_data(value, &mut self.vram, &mut self.color_ram);
    }

    pub fn v_counter(&self) -> u8 {
        // TODO NTSC/PAL, 224-line and 240-line modes
        if self.scanline <= 0xDA {
            self.scanline as u8
        } else {
            (self.scanline - 6) as u8
        }
    }

    pub fn interrupt_line(&self) -> InterruptLine {
        if (self.registers.frame_interrupt_enabled && self.registers.frame_interrupt_pending)
            || (self.registers.line_interrupt_enabled && self.registers.line_interrupt_pending)
        {
            InterruptLine::Low
        } else {
            InterruptLine::High
        }
    }
}
