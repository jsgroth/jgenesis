use bincode::{Decode, Encode};
use jgenesis_traits::num::GetBit;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use z80_emu::traits::InterruptLine;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct ViewportSize {
    pub width: u16,
    pub height: u16,
    pub top: u16,
    pub left: u16,
    pub top_border_height: u16,
    pub bottom_border_height: u16,
    pub left_border_width: u16,
}

impl ViewportSize {
    const NTSC_SMS2: Self = Self {
        width: 256,
        height: 224,
        top: 0,
        left: 0,
        top_border_height: 16,
        bottom_border_height: 16,
        left_border_width: 8,
    };

    const PAL_SMS2: Self = Self {
        width: 256,
        height: 240,
        top: 0,
        left: 0,
        top_border_height: 24,
        bottom_border_height: 24,
        left_border_width: 8,
    };

    const GAME_GEAR: Self = Self {
        width: 160,
        height: 144,
        top: 24,
        left: 48,
        top_border_height: 0,
        bottom_border_height: 0,
        left_border_width: 0,
    };

    pub fn height_without_border(self) -> u16 {
        self.height - self.top_border_height - self.bottom_border_height
    }

    pub fn width_without_border(self) -> u16 {
        self.width - self.left_border_width
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum VdpVersion {
    #[default]
    NtscMasterSystem2,
    PalMasterSystem2,
    GameGear,
}

impl VdpVersion {
    #[must_use]
    pub fn is_master_system(self) -> bool {
        matches!(self, Self::NtscMasterSystem2 | Self::PalMasterSystem2)
    }

    const fn cram_address_mask(self) -> u16 {
        match self {
            Self::NtscMasterSystem2 | Self::PalMasterSystem2 => 0x001F,
            Self::GameGear => 0x003F,
        }
    }

    #[must_use]
    pub const fn viewport_size(self) -> ViewportSize {
        match self {
            Self::NtscMasterSystem2 => ViewportSize::NTSC_SMS2,
            Self::PalMasterSystem2 => ViewportSize::PAL_SMS2,
            Self::GameGear => ViewportSize::GAME_GEAR,
        }
    }
}

impl Display for VdpVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NtscMasterSystem2 => write!(f, "NtscMasterSystem2"),
            Self::PalMasterSystem2 => write!(f, "PalMasterSystem2"),
            Self::GameGear => write!(f, "GameGear"),
        }
    }
}

impl FromStr for VdpVersion {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "NtscMasterSystem2" => Ok(Self::NtscMasterSystem2),
            "PalMasterSystem2" => Ok(Self::PalMasterSystem2),
            "GameGear" => Ok(Self::GameGear),
            _ => Err(format!("invalid VDP version string: {s}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum DataWriteLocation {
    Vram,
    Cram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum Mode {
    #[default]
    Four,
    Four224Line,
}

impl Mode {
    fn from_mode_bits(mode_bits: [bool; 4]) -> Self {
        match mode_bits {
            [false, _, false, true] | [false, false, true, true] | [true, true, true, true] => {
                Self::Four
            }
            [true, true, false, true] => Self::Four224Line,
            _ => {
                log::warn!("Unsupported mode, defaulting to mode 4: {mode_bits:?}");
                Self::Four
            }
        }
    }

    const fn name_table_rows(self) -> u16 {
        match self {
            Self::Four => 28,
            Self::Four224Line => 32,
        }
    }

    const fn active_scanlines(self) -> u16 {
        match self {
            Self::Four => 192,
            Self::Four224Line => 224,
        }
    }

    // The number of scanlines to remove from each of the top and bottom borders when in this modet
    const fn vertical_border_offset(self) -> u16 {
        match self {
            Self::Four => 0,
            Self::Four224Line => 16,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
struct Registers {
    version: VdpVersion,
    mode: Mode,
    mode_bits: [bool; 4],
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
            mode: Mode::Four,
            mode_bits: [false, false, false, true],
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
                self.mode_bits[3] = value.bit(2);
                self.mode_bits[1] = value.bit(1);
                self.mode = Mode::from_mode_bits(self.mode_bits);
                // TODO sync/monochrome bit
            }
            1 => {
                // Mode control #2
                self.display_enabled = value.bit(6);
                self.frame_interrupt_enabled = value.bit(5);
                self.mode_bits[0] = value.bit(4);
                self.mode_bits[2] = value.bit(3);
                self.mode = Mode::from_mode_bits(self.mode_bits);
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
                // TODO updates to Y scroll should only take effect at end-of-frame
                self.y_scroll = value;
            }
            10 => {
                // Line counter
                self.line_counter_reload_value = value;
            }
            _ => {}
        }
    }

    fn sprite_height(&self) -> u8 {
        match (self.double_sprite_size, self.double_sprite_height) {
            (true, true) => 32,
            (true, false) | (false, true) => 16,
            (false, false) => 8,
        }
    }

    fn sprite_width(&self) -> u8 {
        if self.double_sprite_size {
            16
        } else {
            8
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

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct SpriteData {
    y: u8,
    x: u8,
    tile_index: u16,
}

#[derive(Debug, Clone, Encode, Decode)]
struct SpriteBuffer {
    sprites: [SpriteData; 64],
    len: usize,
    overflow: bool,
}

impl SpriteBuffer {
    fn new() -> Self {
        Self {
            sprites: [SpriteData::default(); 64],
            len: 0,
            overflow: false,
        }
    }

    fn iter(&self) -> BufferIter<'_, SpriteData> {
        BufferIter {
            buffer: &self.sprites,
            idx: 0,
            len: self.len,
        }
    }

    fn clear(&mut self) {
        self.len = 0;
        self.overflow = false;
    }
}

#[derive(Debug, Clone)]
struct BufferIter<'a, T> {
    buffer: &'a [T],
    idx: usize,
    len: usize,
}

impl<'a, T: Copy> Iterator for BufferIter<'a, T> {
    type Item = T;

    #[allow(clippy::if_then_some_else_none)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.idx < self.len {
            let data = self.buffer[self.idx];
            self.idx += 1;
            Some(data)
        } else {
            None
        }
    }
}

fn find_sprites_on_scanline(
    scanline: u8,
    registers: &Registers,
    vram: &[u8],
    sprite_buffer: &mut SpriteBuffer,
    remove_sprite_limit: bool,
) {
    sprite_buffer.clear();

    let sprite_height = registers.sprite_height();

    let base_sat_addr = registers.base_sprite_table_address;
    for i in 0..64 {
        let y = vram[(base_sat_addr | i) as usize];
        if registers.mode != Mode::Four224Line && y == 0xD0 {
            return;
        }

        let x = vram[(base_sat_addr | 0x80 | (2 * i)) as usize];
        let tile_index = vram[(base_sat_addr | 0x80 | (2 * i + 1)) as usize];

        let sprite_top = y.saturating_add(1);
        let sprite_bottom = sprite_top.saturating_add(sprite_height);
        if (sprite_top..sprite_bottom).contains(&scanline) {
            if sprite_buffer.len == 8 {
                sprite_buffer.overflow = true;
                if !remove_sprite_limit {
                    return;
                }
            }

            sprite_buffer.sprites[sprite_buffer.len] = SpriteData {
                y,
                x,
                tile_index: tile_index.into(),
            };
            sprite_buffer.len += 1;
        }
    }
}

const VRAM_SIZE: usize = 16 * 1024;
const COLOR_RAM_SIZE: usize = 64;

const SCREEN_WIDTH: u16 = 256;
const SCREEN_HEIGHT: u16 = 240;
const FRAME_BUFFER_LEN: usize = SCREEN_WIDTH as usize * SCREEN_HEIGHT as usize;

#[derive(Debug, Clone, Encode, Decode)]
pub struct FrameBuffer {
    buffer: [u16; FRAME_BUFFER_LEN],
    viewport: ViewportSize,
}

impl FrameBuffer {
    fn new(version: VdpVersion) -> Self {
        Self {
            buffer: [0; FRAME_BUFFER_LEN],
            viewport: version.viewport_size(),
        }
    }

    #[inline]
    fn idx(&self, row: u16, col: u16) -> usize {
        (self.viewport.top as usize + row as usize) * SCREEN_WIDTH as usize
            + self.viewport.left as usize
            + col as usize
    }

    #[inline]
    pub fn get(&self, row: u16, col: u16) -> u16 {
        self.buffer[self.idx(row, col)]
    }

    #[inline]
    fn set(&mut self, row: u16, col: u16, value: u16) {
        self.buffer[row as usize * SCREEN_WIDTH as usize + col as usize] = value;
    }

    pub fn iter(&self) -> FrameBufferRowIter<'_> {
        FrameBufferRowIter {
            buffer: self,
            row: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FrameBufferRowIter<'a> {
    buffer: &'a FrameBuffer,
    row: u16,
}

impl<'a> Iterator for FrameBufferRowIter<'a> {
    type Item = &'a [u16];

    #[inline]
    #[allow(clippy::if_then_some_else_none)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.row < self.buffer.viewport.height {
            let start_idx = self.buffer.idx(self.row, 0);
            let row_slice =
                &self.buffer.buffer[start_idx..start_idx + self.buffer.viewport.width as usize];
            self.row += 1;
            Some(row_slice)
        } else {
            None
        }
    }
}

impl<'a> IntoIterator for &'a FrameBuffer {
    type Item = &'a [u16];
    type IntoIter = FrameBufferRowIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Vdp {
    frame_buffer: FrameBuffer,
    registers: Registers,
    vram: [u8; VRAM_SIZE],
    color_ram: [u8; COLOR_RAM_SIZE],
    scanline: u16,
    dot: u16,
    sprite_buffer: SpriteBuffer,
    remove_sprite_limit: bool,
    line_counter: u8,
}

const DOTS_PER_SCANLINE: u16 = 342;
const NTSC_SCANLINES_PER_FRAME: u16 = 262;
const PAL_SCANLINES_PER_FRAME: u16 = 313;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VdpTickEffect {
    None,
    FrameComplete,
}

impl Vdp {
    pub fn new(version: VdpVersion, remove_sprite_limit: bool) -> Self {
        Self {
            frame_buffer: FrameBuffer::new(version),
            registers: Registers::new(version),
            vram: [0; VRAM_SIZE],
            color_ram: [0; COLOR_RAM_SIZE],
            scanline: 0,
            dot: 0,
            sprite_buffer: SpriteBuffer::new(),
            remove_sprite_limit,
            line_counter: 0xFF,
        }
    }

    fn read_color_ram_word(&self, address: u8) -> u16 {
        match self.registers.version {
            VdpVersion::NtscMasterSystem2 | VdpVersion::PalMasterSystem2 => {
                self.color_ram[address as usize].into()
            }
            VdpVersion::GameGear => u16::from_le_bytes([
                self.color_ram[(2 * address) as usize],
                self.color_ram[(2 * address + 1) as usize],
            ]),
        }
    }

    fn read_name_table_word(&self, row: u16, col: u16) -> BgTileData {
        let base_name_table_addr = match self.registers.mode {
            Mode::Four => self.registers.base_name_table_address,
            // Mask out bit 10 and offset by $0700
            Mode::Four224Line => (self.registers.base_name_table_address & 0xF000) | 0x0700,
        };
        let name_table_addr = base_name_table_addr + (row << 6) + (col << 1);
        let low_byte = self.vram[name_table_addr as usize];
        let high_byte = self.vram[(name_table_addr + 1) as usize];

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
        let frame_buffer_row = scanline + self.frame_buffer.viewport.top_border_height
            - self.registers.mode.vertical_border_offset();

        let (coarse_x_scroll, fine_x_scroll) =
            if scanline < 16 && self.registers.horizontal_scroll_lock {
                (0, 0)
            } else {
                (
                    u16::from(self.registers.x_scroll >> 3),
                    u16::from(self.registers.x_scroll & 0x07),
                )
            };

        // Backdrop color always reads from the second half of CRAM
        let backdrop_color = self.read_color_ram_word(0x10 | self.registers.backdrop_color);

        for dot in 0..fine_x_scroll {
            self.frame_buffer.set(frame_buffer_row, dot, backdrop_color);
        }

        find_sprites_on_scanline(
            scanline as u8,
            &self.registers,
            &self.vram,
            &mut self.sprite_buffer,
            self.remove_sprite_limit,
        );
        if self.sprite_buffer.overflow {
            self.registers.sprite_overflow = true;
        }

        let sprite_width = self.registers.sprite_width();
        let sprite_pixel_size = if self.registers.double_sprite_size {
            2
        } else {
            1
        };

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

            let name_table_rows = self.registers.mode.name_table_rows();
            let name_table_row =
                ((scanline + fine_y_scroll) / 8 + coarse_y_scroll) % name_table_rows;
            let name_table_col = (column + (32 - coarse_x_scroll)) % 32;
            let bg_tile_data = self.read_name_table_word(name_table_row, name_table_col);

            let bg_tile_addr = (bg_tile_data.tile_index * 32) as usize;
            let bg_tile = &self.vram[bg_tile_addr..bg_tile_addr + 32];

            let bg_base_cram_addr = bg_tile_data.palette.base_cram_addr();

            let bg_tile_row = if bg_tile_data.vertical_flip {
                7 - ((scanline + fine_y_scroll) % 8)
            } else {
                (scanline + fine_y_scroll) % 8
            };

            for bg_tile_col in 0..8 {
                let dot = 8 * column + fine_x_scroll + bg_tile_col;
                if dot == SCREEN_WIDTH {
                    break;
                }

                if self.registers.hide_left_column && dot < 8 {
                    self.frame_buffer.set(frame_buffer_row, dot, backdrop_color);
                    continue;
                }

                let bg_color_id = get_color_id(
                    bg_tile,
                    bg_tile_row,
                    bg_tile_col,
                    bg_tile_data.horizontal_flip,
                );

                let mut found_sprite_color_id = None;
                for sprite in self.sprite_buffer.iter() {
                    let sprite_right_inclusive = sprite.x.saturating_add(sprite_width - 1);
                    if !(sprite.x..=sprite_right_inclusive).contains(&(dot as u8)) {
                        continue;
                    }

                    let sprite_tile_row =
                        (scanline - (u16::from(sprite.y) + 1)) / sprite_pixel_size;
                    let sprite_tile_col = (dot - u16::from(sprite.x)) / sprite_pixel_size;

                    let tile_index = if self.registers.double_sprite_height {
                        let top_tile = sprite.tile_index & 0xFE;
                        top_tile | u16::from(sprite_tile_row >= 8)
                    } else {
                        sprite.tile_index
                    };

                    let sprite_tile_addr =
                        (self.registers.base_sprite_pattern_address | (tile_index * 32)) as usize;
                    let sprite_tile = &self.vram[sprite_tile_addr..sprite_tile_addr + 32];

                    let sprite_color_id =
                        get_color_id(sprite_tile, sprite_tile_row & 0x07, sprite_tile_col, false);
                    if sprite_color_id != 0 {
                        match found_sprite_color_id {
                            None => {
                                found_sprite_color_id = Some(sprite_color_id);
                            }
                            Some(_) => {
                                self.registers.sprite_collision = true;
                                break;
                            }
                        }
                    }
                }

                let sprite_color_id = found_sprite_color_id.unwrap_or(0);
                let pixel_color =
                    if sprite_color_id != 0 && (bg_color_id == 0 || !bg_tile_data.priority) {
                        // Sprites can only use palette 1
                        self.read_color_ram_word(0x10 | sprite_color_id)
                    } else {
                        self.read_color_ram_word(bg_base_cram_addr | bg_color_id)
                    };
                self.frame_buffer.set(frame_buffer_row, dot, pixel_color);
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
    pub fn tick(&mut self) -> VdpTickEffect {
        if log::log_enabled!(log::Level::Trace) && self.scanline == 0 && self.dot == 0 {
            self.debug_log();
        }

        let active_scanlines = self.registers.mode.active_scanlines();
        if self.registers.display_enabled && self.scanline < active_scanlines && self.dot == 0 {
            self.render_scanline();
        }

        // The apparent off-by-one in this comparison is intentional. The line counter is
        // decremented on every active scanline *and* on the scanline immediately following the
        // active period.
        if self.scanline <= active_scanlines && self.dot == 0 {
            let (new_counter, overflowed) = self.line_counter.overflowing_sub(1);
            if overflowed {
                self.line_counter = self.registers.line_counter_reload_value;
                self.registers.line_interrupt_pending = true;
            } else {
                self.line_counter = new_counter;
            }
        } else if self.scanline > active_scanlines {
            // Line counter is constantly reloaded outside of the active display period
            self.line_counter = self.registers.line_counter_reload_value;
        }

        let vblank_start = self.scanline == active_scanlines + 1 && self.dot == 0;
        if vblank_start {
            self.registers.frame_interrupt_pending = true;

            self.fill_vertical_border();
        }

        let tick_effect = if vblank_start {
            VdpTickEffect::FrameComplete
        } else {
            VdpTickEffect::None
        };

        self.dot += 1;
        if self.dot == DOTS_PER_SCANLINE {
            self.scanline += 1;
            self.dot = 0;

            let scanlines_per_frame = match self.registers.version {
                VdpVersion::NtscMasterSystem2 | VdpVersion::GameGear => NTSC_SCANLINES_PER_FRAME,
                VdpVersion::PalMasterSystem2 => PAL_SCANLINES_PER_FRAME,
            };
            if self.scanline == scanlines_per_frame {
                self.scanline = 0;
            }
        }

        tick_effect
    }

    fn fill_vertical_border(&mut self) {
        let backdrop_color = self.read_color_ram_word(0x10 | self.registers.backdrop_color);

        let ViewportSize {
            top_border_height,
            height,
            bottom_border_height,
            ..
        } = self.frame_buffer.viewport;

        let mode_border_offset = self.registers.mode.vertical_border_offset();

        let viewport_top = top_border_height - mode_border_offset;
        for scanline in 0..viewport_top {
            for pixel in 0..256 {
                self.frame_buffer.set(scanline, pixel, backdrop_color);
            }
        }

        let viewport_bottom = height - bottom_border_height + mode_border_offset;
        for scanline in viewport_bottom..height {
            for pixel in 0..256 {
                self.frame_buffer.set(scanline, pixel, backdrop_color);
            }
        }
    }

    pub fn frame_buffer(&self) -> &FrameBuffer {
        &self.frame_buffer
    }

    pub fn read_control(&mut self) -> u8 {
        self.registers.read_control()
    }

    pub fn write_control(&mut self, value: u8) {
        self.registers.write_control(value, &self.vram);
    }

    pub fn read_data(&mut self) -> u8 {
        self.registers.read_data(&self.vram)
    }

    pub fn write_data(&mut self, value: u8) {
        self.registers
            .write_data(value, &mut self.vram, &mut self.color_ram);
    }

    pub fn v_counter(&self) -> u8 {
        match (self.registers.version, self.registers.mode) {
            (VdpVersion::NtscMasterSystem2 | VdpVersion::GameGear, Mode::Four) => {
                if self.scanline <= 0xDA {
                    self.scanline as u8
                } else {
                    (self.scanline - 6) as u8
                }
            }
            (VdpVersion::PalMasterSystem2, Mode::Four) => {
                if self.scanline <= 0xF2 {
                    self.scanline as u8
                } else {
                    (self.scanline - 57) as u8
                }
            }
            (VdpVersion::NtscMasterSystem2 | VdpVersion::GameGear, Mode::Four224Line) => {
                if self.scanline <= 0xEA {
                    self.scanline as u8
                } else {
                    (self.scanline - 6) as u8
                }
            }
            (VdpVersion::PalMasterSystem2, Mode::Four224Line) => {
                if self.scanline <= 0xFF {
                    self.scanline as u8
                } else if self.scanline <= 0x102 {
                    (self.scanline - 0x100) as u8
                } else {
                    (self.scanline - 57) as u8
                }
            }
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

fn get_color_id(tile: &[u8], tile_row: u16, tile_col: u16, horizontal_flip: bool) -> u8 {
    let shift = if horizontal_flip {
        tile_col
    } else {
        7 - tile_col
    };
    let mask = 1 << shift;
    ((tile[(4 * tile_row) as usize] & mask) >> shift)
        | (((tile[(4 * tile_row + 1) as usize] & mask) >> shift) << 1)
        | (((tile[(4 * tile_row + 2) as usize] & mask) >> shift) << 2)
        | (((tile[(4 * tile_row + 3) as usize] & mask) >> shift) << 3)
}
