use crate::ppu::State;
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::array;
use std::fmt::{Display, Formatter};
use std::ops::Index;

pub const TILE_MAP_AREA_0: u16 = 0x1800;
pub const TILE_MAP_AREA_1: u16 = 0x1C00;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum TileDataArea {
    // $8800-$97FF
    #[default]
    Zero,
    // $8000-$8FFF
    One,
}

impl TileDataArea {
    // Sprites always use $8000-$8FFF
    pub const SPRITES: Self = Self::One;

    pub fn tile_address(self, tile_number: u8) -> u16 {
        // 16 bytes per tile
        match self {
            Self::Zero => {
                // Treat tile number as a signed integer so that 128-255 map to $8800-$8FFF
                let relative_tile_addr = (tile_number as i8 as u16) << 4;
                0x1000_u16.wrapping_add(relative_tile_addr)
            }
            Self::One => u16::from(tile_number) << 4,
        }
    }

    fn from_bit(bit: bool) -> Self {
        if bit { Self::One } else { Self::Zero }
    }

    fn to_bit(self) -> bool {
        self == Self::One
    }
}

impl Display for TileDataArea {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Zero => write!(f, "$8800-$97FF"),
            Self::One => write!(f, "$8000-$8FFF"),
        }
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct Registers {
    // LCDC: LCD control
    pub ppu_enabled: bool,
    pub bg_enabled: bool,
    pub window_enabled: bool,
    pub sprites_enabled: bool,
    pub bg_tile_map_addr: u16,
    pub window_tile_map_addr: u16,
    pub bg_tile_data_area: TileDataArea,
    pub double_height_sprites: bool,
    // STAT: LCD status
    pub lyc_interrupt_enabled: bool,
    pub mode_2_interrupt_enabled: bool,
    pub mode_1_interrupt_enabled: bool,
    pub mode_0_interrupt_enabled: bool,
    // LYC: LY compare
    pub ly_compare: u8,
    // SCX/SCY: Background X/Y position
    pub bg_x_scroll: u8,
    pub bg_y_scroll: u8,
    // WX/WY: Window X/Y position
    pub window_x: u8,
    pub window_y: u8,
    // BGP: Background palette
    pub bg_palette: [u8; 4],
    // OBP0/OBP1: Sprite palettes
    pub sprite_palettes: [[u8; 4]; 2],
    // VBK: VRAM bank
    pub vram_bank: u8,
}

impl Registers {
    pub fn new() -> Self {
        Self {
            ppu_enabled: true,
            bg_enabled: true,
            window_enabled: false,
            sprites_enabled: false,
            bg_tile_map_addr: TILE_MAP_AREA_0,
            window_tile_map_addr: TILE_MAP_AREA_0,
            bg_tile_data_area: TileDataArea::One,
            double_height_sprites: false,
            lyc_interrupt_enabled: false,
            mode_2_interrupt_enabled: false,
            mode_1_interrupt_enabled: false,
            mode_0_interrupt_enabled: false,
            ly_compare: 0,
            bg_x_scroll: 0,
            bg_y_scroll: 0,
            window_x: 0,
            window_y: 0,
            // Power-on value is $FC / 0b11_11_11_00
            bg_palette: [0, 3, 3, 3],
            sprite_palettes: [[0; 4]; 2],
            vram_bank: 0,
        }
    }

    pub fn write_lcdc(&mut self, value: u8) {
        self.ppu_enabled = value.bit(7);
        self.window_tile_map_addr = if value.bit(6) { TILE_MAP_AREA_1 } else { TILE_MAP_AREA_0 };
        self.window_enabled = value.bit(5);
        self.bg_tile_data_area = TileDataArea::from_bit(value.bit(4));
        self.bg_tile_map_addr = if value.bit(3) { TILE_MAP_AREA_1 } else { TILE_MAP_AREA_0 };
        self.double_height_sprites = value.bit(2);
        self.sprites_enabled = value.bit(1);
        self.bg_enabled = value.bit(0);

        log::trace!("LCDC write: {value:02X}");
        log::trace!("  PPU enabled: {}", self.ppu_enabled);
        log::trace!("  BG/window enabled: {}", self.bg_enabled);
        log::trace!("  Window enabled: {}", self.window_enabled);
        log::trace!("  Sprites enabled: {}", self.sprites_enabled);
        log::trace!("  BG tile map address: ${:04X}", self.bg_tile_map_addr);
        log::trace!("  Window tile map address: ${:04X}", self.window_tile_map_addr);
        log::trace!("  BG tile data area: {}", self.bg_tile_data_area);
        log::trace!("  Double height sprites: {}", self.double_height_sprites);
    }

    pub fn read_lcdc(&self) -> u8 {
        (u8::from(self.ppu_enabled) << 7)
            | (u8::from(self.window_tile_map_addr == TILE_MAP_AREA_1) << 6)
            | (u8::from(self.window_enabled) << 5)
            | (u8::from(self.bg_tile_data_area.to_bit()) << 4)
            | (u8::from(self.bg_tile_map_addr == TILE_MAP_AREA_1) << 3)
            | (u8::from(self.double_height_sprites) << 2)
            | (u8::from(self.sprites_enabled) << 1)
            | u8::from(self.bg_enabled)
    }

    pub fn write_stat(&mut self, value: u8) {
        self.lyc_interrupt_enabled = value.bit(6);
        self.mode_2_interrupt_enabled = value.bit(5);
        self.mode_1_interrupt_enabled = value.bit(4);
        self.mode_0_interrupt_enabled = value.bit(3);

        log::trace!("STAT write: {value:02X}");
        log::trace!("  LY=LYC interrupt enabled: {}", self.lyc_interrupt_enabled);
        log::trace!("  Mode 2 (OAM scan) interrupt enabled: {}", self.mode_2_interrupt_enabled);
        log::trace!("  Mode 1 (VBlank) interrupt enabled: {}", self.mode_1_interrupt_enabled);
        log::trace!("  Mode 0 (HBlank) interrupt enabled: {}", self.mode_0_interrupt_enabled);
    }

    pub fn read_stat(&self, state: &State) -> u8 {
        let ly_lyc_bit = if self.ppu_enabled {
            state.scanline == self.ly_compare
        } else {
            state.frozen_ly_lyc_bit
        };

        0x80 | (u8::from(self.lyc_interrupt_enabled) << 6)
            | (u8::from(self.mode_2_interrupt_enabled) << 5)
            | (u8::from(self.mode_1_interrupt_enabled) << 4)
            | (u8::from(self.mode_0_interrupt_enabled) << 3)
            | (u8::from(ly_lyc_bit) << 2)
            | state.mode.to_bits()
    }

    pub fn write_lyc(&mut self, value: u8) {
        self.ly_compare = value;

        log::trace!("LYC write: {value:02X}");
    }

    pub fn write_scx(&mut self, value: u8) {
        self.bg_x_scroll = value;

        log::trace!("SCX write: {value:02X}");
    }

    pub fn write_scy(&mut self, value: u8) {
        self.bg_y_scroll = value;

        log::trace!("SCY write: {value:02X}");
    }

    pub fn write_wx(&mut self, value: u8) {
        self.window_x = value;

        log::trace!("WX write: {value:02X}");
    }

    pub fn write_wy(&mut self, value: u8) {
        self.window_y = value;

        log::trace!("WY write: {value:02X}");
    }

    pub fn write_bgp(&mut self, value: u8) {
        self.bg_palette = parse_dmg_palette(value);

        log::trace!("BGP write: {value:02X}");
    }

    pub fn read_bgp(&self) -> u8 {
        read_dmg_palette(self.bg_palette)
    }

    pub fn write_obp0(&mut self, value: u8) {
        self.sprite_palettes[0] = parse_dmg_palette(value);

        log::trace!("OBP0 write: {value:02X}");
    }

    pub fn write_obp1(&mut self, value: u8) {
        self.sprite_palettes[1] = parse_dmg_palette(value);

        log::trace!("OBP1 write: {value:02X}");
    }

    pub fn read_obp0(&self) -> u8 {
        read_dmg_palette(self.sprite_palettes[0])
    }

    pub fn read_obp1(&self) -> u8 {
        read_dmg_palette(self.sprite_palettes[1])
    }

    pub fn write_vbk(&mut self, value: u8) {
        self.vram_bank = value & 0x01;

        log::trace!("VBK write: VRAM bank = {}", self.vram_bank);
    }

    pub fn read_vbk(&self) -> u8 {
        0xFE | self.vram_bank
    }
}

fn parse_dmg_palette(value: u8) -> [u8; 4] {
    array::from_fn(|palette| (value >> (2 * palette)) & 0x3)
}

fn read_dmg_palette(palette: [u8; 4]) -> u8 {
    palette.into_iter().enumerate().map(|(i, color)| color << (2 * i)).reduce(|a, b| a | b).unwrap()
}

const PALETTE_RAM_LEN: usize = 64;

#[derive(Debug, Clone, Encode, Decode)]
pub struct CgbPaletteRam {
    ram: Box<[u8; PALETTE_RAM_LEN]>,
    data_port_address: u8,
    data_port_auto_increment: bool,
}

impl Index<usize> for CgbPaletteRam {
    type Output = u8;

    fn index(&self, index: usize) -> &Self::Output {
        &self.ram[index]
    }
}

impl CgbPaletteRam {
    pub fn new() -> Self {
        Self {
            ram: vec![0; PALETTE_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            data_port_address: 0,
            data_port_auto_increment: false,
        }
    }

    pub fn read_data_port_address(&self) -> u8 {
        0x40 | (u8::from(self.data_port_auto_increment) << 7) | self.data_port_address
    }

    pub fn write_data_port_address(&mut self, value: u8) {
        self.data_port_address = value & 0x3F;
        self.data_port_auto_increment = value.bit(7);
    }

    pub fn read_data_port(&self, cpu_can_access_vram: bool) -> u8 {
        if cpu_can_access_vram { self.ram[self.data_port_address as usize] } else { 0xFF }
    }

    pub fn write_data_port(&mut self, value: u8, cpu_can_access_vram: bool) {
        if cpu_can_access_vram {
            self.ram[self.data_port_address as usize] = value;
            log::trace!("CGB palette RAM write: {:02X}, {value:02X}", self.data_port_address);
        }

        // Auto-increment is always applied, even if the CPU can't access VRAM
        if self.data_port_auto_increment {
            self.data_port_address = (self.data_port_address + 1) & 0x3F;
        }
    }

    pub fn read_color(&self, palette: u8, color: u8) -> u16 {
        let addr = 2 * ((palette << 2) | color);
        let lsb = self.ram[addr as usize];
        let msb = self.ram[(addr + 1) as usize];
        u16::from_le_bytes([lsb, msb]) & 0x7FFF
    }
}
