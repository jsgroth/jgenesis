use crate::bus::PpuBus;

pub const SCREEN_WIDTH: u16 = 256;
pub const SCREEN_HEIGHT: u16 = 240;
pub const VISIBLE_SCREEN_HEIGHT: u16 = 224;

const FIRST_VBLANK_SCANLINE: u16 = 241;
const PRE_RENDER_SCANLINE: u16 = 261;
const DOTS_PER_SCANLINE: u16 = 341;

pub type FrameBuffer = [[u8; SCREEN_WIDTH as usize]; SCREEN_HEIGHT as usize];

pub struct PpuState {
    scanline: u16,
    dot: u16,
    frame_buffer: FrameBuffer,
}

impl PpuState {
    pub fn new() -> Self {
        Self {
            scanline: PRE_RENDER_SCANLINE,
            dot: 0,
            frame_buffer: [[0; SCREEN_WIDTH as usize]; SCREEN_HEIGHT as usize],
        }
    }
}

struct SpriteData {
    y_pos: u8,
    tile_index: u8,
    attributes: u8,
    x_pos: u8,
}

pub fn tick(state: &mut PpuState, bus: &mut PpuBus<'_>) {
    match state.scanline {
        scanline @ 0..=239 => {
            // Visible scanlines
        }
        240 => {
            // Post-render scanline
        }
        241 => {
            // First VBlank scanline
            if state.dot == 1 {
                bus.get_ppu_registers_mut().set_vblank_flag(true);
            }
        }
        242..=260 => {
            // Remaining VBlank scanlines
        }
        261 => {
            // Pre-render scanline
            if state.dot == 1 {
                let ppu_registers = bus.get_ppu_registers_mut();
                ppu_registers.set_vblank_flag(false);
                ppu_registers.set_sprite_0_hit(false);
                ppu_registers.set_sprite_overflow(false);
            }
        }
        _ => panic!("invalid scanline: {}", state.scanline),
    }

    state.dot += 1;
    if state.dot == DOTS_PER_SCANLINE {
        state.scanline += 1;
        state.dot = 0;

        if state.scanline == PRE_RENDER_SCANLINE + 1 {
            state.scanline = 0;
        }
    }
}
