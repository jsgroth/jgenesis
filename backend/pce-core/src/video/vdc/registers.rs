use crate::video::WordByte;
use crate::video::vdc::{CgMode, DmaStep, Vdc};
use bincode::{Decode, Encode};
use jgenesis_common::define_bit_enum;
use jgenesis_common::num::{GetBit, U16Ext};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum VramAccessWidth {
    #[default]
    One, // 1 CPU access every 2 dots during BG fetching
    Two,  // 1 CPU access every 8 dots during BG fetching
    Four, // No CPU access during BG fetching, and BG fetches only fetch half of the bitplanes
}

impl VramAccessWidth {
    fn from_bits(bits: u8) -> Self {
        match bits & 3 {
            0 => Self::One,
            1 | 2 => Self::Two,
            3 => Self::Four,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }

    fn display(self) -> &'static str {
        match self {
            Self::One => "1",
            Self::Two => "2",
            Self::Four => "4",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
#[rustfmt::skip]
pub enum SpriteAccessWidth {
    #[default]
    One,       // Fetch 1 sprite per 4 dots
    TwoDouble, // Fetch 2 sprites per 16 dots
    TwoSingle, // Fetch 1 sprite per 8 dots
    Four,      // Fetch 1 sprite per 8 dots but only half of the bitplanes
}

impl SpriteAccessWidth {
    fn from_bits(bits: u8) -> Self {
        match bits & 3 {
            0 => Self::One,
            1 => Self::TwoDouble,
            2 => Self::TwoSingle,
            3 => Self::Four,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }

    fn display(self) -> &'static str {
        match self {
            Self::One => "1",
            Self::TwoDouble => "2 (2 sprites per 16 dots)",
            Self::TwoSingle => "2 (1 sprite per 8 dots)",
            Self::Four => "4",
        }
    }
}

define_bit_enum!(VirtualScreenHeight, [Single, Double]);

impl VirtualScreenHeight {
    fn display(self) -> &'static str {
        match self {
            Self::Single => "32 tiles",
            Self::Double => "64 tiles",
        }
    }

    pub fn to_tiles(self) -> u16 {
        match self {
            Self::Single => 32,
            Self::Double => 64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum VirtualScreenWidth {
    #[default]
    Single,
    Double,
    Quad,
}

impl VirtualScreenWidth {
    fn from_bits(bits: u8) -> Self {
        match bits & 3 {
            0 => Self::Single,
            1 => Self::Double,
            2 | 3 => Self::Quad,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }

    fn display(self) -> &'static str {
        match self {
            Self::Single => "32 tiles",
            Self::Double => "64 tiles",
            Self::Quad => "128 tiles",
        }
    }

    pub fn to_tiles(self) -> u16 {
        match self {
            Self::Single => 32,
            Self::Double => 64,
            Self::Quad => 128,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct VdcRegisters {
    // $00: MAWR (Memory address write register)
    pub vram_write_address: u16,
    // $01: MARR (Memory address read register)
    pub vram_read_address: u16,
    // $02: VRR (VRAM read register)
    pub vram_read_buffer: u16,
    // $03: VWR (VRAM write register)
    pub vram_write_latch: u8,
    // $05: CR (Control register)
    pub vblank_irq_enabled: bool,
    pub raster_compare_irq_enabled: bool,
    pub sprite_overflow_irq_enabled: bool,
    pub sprite_collision_irq_enabled: bool,
    pub bg_enabled: bool,
    pub sprites_enabled: bool,
    pub vram_address_increment: u16,
    // $06: RCR (Raster compare register)
    pub raster_compare: u16,
    // $07: BXR (BG X scroll register)
    pub bg_x_scroll: u16,
    // $08: BYR (BG Y scroll register)
    pub bg_y_scroll: u16,
    // $09: MWR (Memory width register)
    pub vram_access_width: VramAccessWidth,
    pub sprite_access_width: SpriteAccessWidth,
    pub virtual_screen_width: VirtualScreenWidth,
    pub virtual_screen_height: VirtualScreenHeight,
    pub bg_cg_mode: CgMode,
    // $0A: HSR (Horizontal sync register)
    pub h_sync_width: u16,    // HSW (+1 * 8)
    pub h_display_start: u16, // HDS (+1 * 8)
    // $0B: HDR (Horizontal display register)
    pub h_display_width: u16, // HDW (+1 * 8)
    pub h_display_end: u16,   // HDE (+1 * 8)
    // $0C: VSR (Vertical sync register)
    pub v_sync_width: u16,    // VSW (+1)
    pub v_display_start: u16, // VDS (+2)
    // $0D: VDR (Vertical display register)
    pub v_display_width_raw: u16,
    pub v_display_width: u16, // VDW (+1)
    // $0E: VCR (Vertical display end position register)
    pub v_display_end: u16, // VCR
    // $0F: DCR (DMA control register)
    pub vram_dma_irq_enabled: bool,
    pub sat_dma_irq_enabled: bool,
    pub vram_dma_source_step: DmaStep,
    pub vram_dma_destination_step: DmaStep,
    pub sat_dma_repeat: bool,
    // $10: SOUR (DMA source address register)
    pub vram_dma_source_address: u16,
    // $11: DESR (DMA destination address register)
    pub vram_dma_destination_address: u16,
    // $12: LENR (DMA length register)
    pub vram_dma_length: u16,
    // $13: DVSSR (VRAM-SAT DMA source address register)
    pub sat_dma_source_address: u16,
}

impl VdcRegisters {
    pub fn new() -> Self {
        Self {
            vram_write_address: !0,
            vram_read_address: !0,
            vram_read_buffer: !0,
            vram_write_latch: !0,
            vblank_irq_enabled: false,
            raster_compare_irq_enabled: false,
            sprite_overflow_irq_enabled: false,
            sprite_collision_irq_enabled: false,
            bg_enabled: false,
            sprites_enabled: false,
            vram_address_increment: 1,
            raster_compare: !0,
            bg_x_scroll: 0,
            bg_y_scroll: 0,
            vram_access_width: VramAccessWidth::default(),
            sprite_access_width: SpriteAccessWidth::default(),
            virtual_screen_width: VirtualScreenWidth::default(),
            virtual_screen_height: VirtualScreenHeight::default(),
            bg_cg_mode: CgMode::default(),
            // Arbitrarily default H/V display registers to the settings used by Bonk's Adventure
            h_sync_width: 24,
            h_display_start: 24,
            h_display_width: 256,
            h_display_end: 32,
            v_sync_width: 3,
            v_display_start: 17,
            v_display_width_raw: 239,
            v_display_width: 240,
            v_display_end: 3,
            vram_dma_irq_enabled: false,
            sat_dma_irq_enabled: false,
            vram_dma_source_step: DmaStep::default(),
            vram_dma_destination_step: DmaStep::default(),
            sat_dma_repeat: false,
            vram_dma_source_address: !0,
            vram_dma_destination_address: !0,
            vram_dma_length: !0,
            sat_dma_source_address: !0,
        }
    }
}

impl Vdc {
    pub fn read_status(&mut self) -> u8 {
        // TODO busy flag
        let status = (u8::from(self.state.vblank_irq_pending) << 5)
            | (u8::from(self.state.vram_dma_irq_pending) << 4)
            | (u8::from(self.state.sat_dma_irq_pending) << 3)
            | (u8::from(self.state.raster_compare_irq_pending) << 2)
            | (u8::from(self.state.sprite_overflow_irq_pending) << 1)
            | u8::from(self.state.sprite_collision_irq_pending);

        // All VDC IRQ flags are cleared on status register read
        self.state.vblank_irq_pending = false;
        self.state.raster_compare_irq_pending = false;
        self.state.sprite_collision_irq_pending = false;
        self.state.sprite_overflow_irq_pending = false;
        self.state.vram_dma_irq_pending = false;
        self.state.sat_dma_irq_pending = false;

        self.state.any_irq_pending = false;

        status
    }

    pub fn write_register_select(&mut self, value: u8) {
        self.selected_register = value & 0x1F;

        log::trace!("Selected register ${:02X}", self.selected_register);
    }

    pub fn read_data(&mut self, byte: WordByte) -> u8 {
        let value = byte.get(self.registers.vram_read_buffer);

        // MARR only increments when selected register is VRR/VWR
        if self.selected_register == 0x02 && byte == WordByte::High {
            // TODO timing
            self.registers.vram_read_buffer = self.read_vram(self.registers.vram_read_address);
            self.increment_vram_read_address();
        }

        value
    }

    pub fn write_data(&mut self, value: u8, byte: WordByte) {
        match self.selected_register {
            0x00 => {
                // MAWR (Memory address write register)
                byte.set(&mut self.registers.vram_write_address, value);

                log::trace!(
                    "MAWR {byte:?} write: {value:02X} (address {:04X})",
                    self.registers.vram_write_address
                );
            }
            0x01 => {
                // MARR (Memory address read register);
                byte.set(&mut self.registers.vram_read_address, value);

                log::trace!(
                    "MAAR {byte:?} write: {value:02X} (address {:04X})",
                    self.registers.vram_read_address
                );

                // Writing to MSB initiates VRAM read
                if byte == WordByte::High {
                    // TODO timing
                    self.registers.vram_read_buffer =
                        self.read_vram(self.registers.vram_read_address);
                    self.increment_vram_read_address();
                }
            }
            0x02 => {
                // VWR (VRAM write register)
                match byte {
                    WordByte::Low => {
                        // LSB writes latch the byte
                        self.registers.vram_write_latch = value;
                    }
                    WordByte::High => {
                        // MSB writes persist to VRAM along with latched byte
                        let word = u16::from_le_bytes([self.registers.vram_write_latch, value]);
                        self.write_vram(self.registers.vram_write_address, word);
                        self.increment_vram_write_address();
                    }
                }
            }
            0x05 => {
                // CR (Control register)
                match byte {
                    WordByte::Low => {
                        self.registers.sprite_collision_irq_enabled = value.bit(0);
                        self.registers.sprite_overflow_irq_enabled = value.bit(1);
                        self.registers.raster_compare_irq_enabled = value.bit(2);
                        self.registers.vblank_irq_enabled = value.bit(3);
                        self.registers.sprites_enabled = value.bit(6);
                        self.registers.bg_enabled = value.bit(7);

                        log::trace!("CR Low write: {value:02X}");
                        log::trace!("  BG enabled: {}", self.registers.bg_enabled);
                        log::trace!("  Sprites enabled: {}", self.registers.sprites_enabled);
                        log::trace!("  VBlank IRQ enabled: {}", self.registers.vblank_irq_enabled);
                        log::trace!(
                            "  Raster compare IRQ enabled: {}",
                            self.registers.raster_compare_irq_enabled
                        );
                        log::trace!(
                            "  Sprite overflow IRQ enabled: {}",
                            self.registers.sprite_overflow_irq_enabled
                        );
                        log::trace!(
                            "  Sprite collision IRQ enabled: {}",
                            self.registers.sprite_collision_irq_enabled
                        );
                    }
                    WordByte::High => {
                        let increment_idx = ((value >> 3) & 3) as usize;
                        self.registers.vram_address_increment =
                            [0x01, 0x20, 0x40, 0x80][increment_idx];

                        log::trace!("CR High write: {value:02X}");
                        log::trace!(
                            "  VRAM address increment: 0x{:02X}",
                            self.registers.vram_address_increment
                        );
                    }
                }
            }
            0x06 => {
                // RCR (Raster compare register)
                match byte {
                    WordByte::Low => self.registers.raster_compare.set_lsb(value),
                    WordByte::High => self.registers.raster_compare.set_msb(value & 3),
                }

                log::trace!(
                    "RCR {byte:?} write: {value:02X} (raster compare {})",
                    self.registers.raster_compare
                );
            }
            0x07 => {
                // BXR (BG X scroll register)
                match byte {
                    WordByte::Low => self.registers.bg_x_scroll.set_lsb(value),
                    WordByte::High => self.registers.bg_x_scroll.set_msb(value & 3),
                }

                log::trace!(
                    "BXR {byte:?} write: {value:02X} (BG X scroll {})",
                    self.registers.bg_x_scroll
                );
            }
            0x08 => {
                // BYR (BG Y scroll register)
                match byte {
                    WordByte::Low => self.registers.bg_y_scroll.set_lsb(value),
                    WordByte::High => self.registers.bg_y_scroll.set_msb(value & 1),
                }

                self.state.bg_y_scroll_written = true;

                log::trace!(
                    "BYR {byte:?} write: {value:02X} (BG Y scroll {})",
                    self.registers.bg_y_scroll
                );
            }
            0x09 => {
                // MWR (Memory width register)
                if byte == WordByte::Low {
                    self.registers.vram_access_width = VramAccessWidth::from_bits(value);
                    self.registers.sprite_access_width = SpriteAccessWidth::from_bits(value >> 2);
                    self.registers.virtual_screen_width = VirtualScreenWidth::from_bits(value >> 4);
                    self.registers.virtual_screen_height =
                        VirtualScreenHeight::from_bit(value.bit(6));
                    self.registers.bg_cg_mode = CgMode::from_bit(value.bit(7));
                }

                log::trace!("MWR {byte:?} write: {value:02X}");
                log::trace!("  VRAM access width: {}", self.registers.vram_access_width.display());
                log::trace!(
                    "  Sprite access width: {}",
                    self.registers.sprite_access_width.display()
                );
                log::trace!(
                    "  Virtual screen width: {}",
                    self.registers.virtual_screen_width.display()
                );
                log::trace!(
                    "  Virtual screen height: {}",
                    self.registers.virtual_screen_height.display()
                );
                log::trace!("  BG CG mode: {:?}", self.registers.bg_cg_mode);
            }
            0x0A => {
                // HSR (Horizontal sync register)
                log::trace!("HSR {byte:?} write: {value:02X}");

                match byte {
                    WordByte::Low => {
                        self.registers.h_sync_width = 8 * u16::from((value & 0x1F) + 1);
                        log::trace!("  H sync pulse width: {}", self.registers.h_sync_width);
                    }
                    WordByte::High => {
                        self.registers.h_display_start = 8 * u16::from((value & 0x7F) + 1);
                        log::trace!(
                            "  H display start position: {}",
                            self.registers.h_display_start
                        );
                    }
                }
            }
            0x0B => {
                // HDR (Horizontal display register)
                log::trace!("HDR {byte:?} write: {value:02X}");

                match byte {
                    WordByte::Low => {
                        self.registers.h_display_width = 8 * u16::from((value & 0x7F) + 1);
                        log::trace!("  H display width: {}", self.registers.h_display_width);
                    }
                    WordByte::High => {
                        self.registers.h_display_end = 8 * u16::from((value & 0x7F) + 1);
                        log::trace!("  H display end position: {}", self.registers.h_display_end);
                    }
                }
            }
            0x0C => {
                // VSR (Vertical sync register)
                log::trace!("VSR {byte:?} write: {value:02X}");

                match byte {
                    WordByte::Low => {
                        self.registers.v_sync_width = ((value & 0x1F) + 1).into();
                        log::trace!("  V sync pulse width: {}", self.registers.v_sync_width);
                    }
                    WordByte::High => {
                        self.registers.v_display_start = u16::from(value) + 2;
                        log::trace!(
                            "  V display start position: {}",
                            self.registers.v_display_start
                        );
                    }
                }
            }
            0x0D => {
                // VDR (Vertical display register)
                match byte {
                    WordByte::Low => self.registers.v_display_width_raw.set_lsb(value),
                    WordByte::High => self.registers.v_display_width_raw.set_msb(value & 1),
                }
                self.registers.v_display_width = self.registers.v_display_width_raw + 1;

                log::trace!("VDR {byte:?} write: {value:02X}");
                log::trace!("  V display width: {}", self.registers.v_display_width);
            }
            0x0E => {
                // VCR (Vertical display end position register)
                if byte == WordByte::Low {
                    self.registers.v_display_end = value.into();
                }

                log::trace!("VCR {byte:?} write: {value:02X}");
                log::trace!("  V display end position: {}", self.registers.v_display_end);
            }
            0x0F => {
                // DCR (DMA control register)
                if byte == WordByte::Low {
                    self.registers.sat_dma_irq_enabled = value.bit(0);
                    self.registers.vram_dma_irq_enabled = value.bit(1);
                    self.registers.vram_dma_source_step = DmaStep::from_bit(value.bit(2));
                    self.registers.vram_dma_destination_step = DmaStep::from_bit(value.bit(3));
                    self.registers.sat_dma_repeat = value.bit(4);
                }

                log::trace!("DCR {byte:?} write: {value:02X}");
                log::trace!(
                    "  VRAM-to-VRAM DMA IRQ enabled: {}",
                    self.registers.vram_dma_irq_enabled
                );
                log::trace!(
                    "  VRAM-to-SAT DMA IRQ enabled: {}",
                    self.registers.sat_dma_irq_enabled
                );
                log::trace!(
                    "  VRAM-to-VRAM DMA source step: {:?}",
                    self.registers.vram_dma_source_step
                );
                log::trace!(
                    "  VRAM-to-VRAM DMA destination step: {:?}",
                    self.registers.vram_dma_destination_step
                );
                log::trace!("  VRAM_to-SAT DMA repeat: {}", self.registers.sat_dma_repeat);
            }
            0x10 => {
                // SOUR (DMA source address register)
                byte.set(&mut self.registers.vram_dma_source_address, value);

                log::trace!(
                    "SOUR {byte:?} write: {value:02X} (address {:04X})",
                    self.registers.vram_dma_source_address
                );
            }
            0x11 => {
                // DESR (DMA destination address register)
                byte.set(&mut self.registers.vram_dma_destination_address, value);

                log::trace!(
                    "DESR {byte:?} write: {value:02X} (address {:04X})",
                    self.registers.vram_dma_destination_address
                );
            }
            0x12 => {
                // LENR (DMA length register)
                byte.set(&mut self.registers.vram_dma_length, value);

                if byte == WordByte::High {
                    todo!("start VRAM-to-VRAM DMA");
                }

                log::trace!(
                    "LENR {byte:?} write: {value:02X} (length {:04X})",
                    self.registers.vram_dma_length
                );
            }
            0x13 => {
                // DVSSR (VRAM-to-SAT DMA source address register)
                byte.set(&mut self.registers.sat_dma_source_address, value);

                if byte == WordByte::High {
                    // TODO start VRAM-to-SAT DMA
                }

                log::trace!(
                    "DVSSR {byte:?} write: {value:02X} (address {:04X})",
                    self.registers.sat_dma_source_address
                );
            }
            _ => {
                log::warn!(
                    "Invalid VDC register write {:02X} {byte:?} {value:02X}",
                    self.selected_register
                );
            }
        }
    }
}
