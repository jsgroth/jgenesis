use crate::ppu::registers::Registers;
use crate::ppu::{PpuFrameBuffer, Vram, SCREEN_WIDTH};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::array;
use std::collections::VecDeque;

const MAX_FIFO_X: u8 = SCREEN_WIDTH as u8 + 8;

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct BgPixel {
    color: u8,
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
struct SpritePixel {
    color: u8,
    palette: u8,
    low_priority: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
struct FifoStateFields {
    dots_remaining: u8,
    screen_x: u8,
    fetcher_x: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum FifoState {
    // Fetching the offscreen first tile
    InitialBgFetch { dots_remaining: u8 },
    // Rendering a background tile
    RenderingBgTile(FifoStateFields),
    // Fetching the first window tile
    InitialWindowFetch(FifoStateFields),
    // Rendering a window tile
    RenderingWindowTile(FifoStateFields),
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct PixelFifo {
    bg: VecDeque<BgPixel>,
    sprites: VecDeque<SpritePixel>,
    y: u8,
    state: FifoState,
}

impl PixelFifo {
    pub fn new() -> Self {
        Self {
            bg: VecDeque::with_capacity(16),
            sprites: VecDeque::with_capacity(16),
            y: 0,
            state: FifoState::InitialBgFetch { dots_remaining: 0 },
        }
    }

    pub fn start_new_line(&mut self, scanline: u8) {
        self.bg.clear();
        self.sprites.clear();
        self.y = scanline;

        // Initial BG tile fetch always takes 6 cycles
        self.state = FifoState::InitialBgFetch { dots_remaining: 6 };

        // TODO check WY here
    }

    pub fn tick(&mut self, vram: &Vram, registers: &Registers, frame_buffer: &mut PpuFrameBuffer) {
        match self.state {
            FifoState::InitialBgFetch { dots_remaining } => {
                self.handle_initial_bg_fetch(dots_remaining, vram, registers)
            }
            FifoState::RenderingBgTile(fields) => {
                self.handle_rendering_bg_tile(fields, vram, registers, frame_buffer)
            }
            FifoState::InitialWindowFetch { .. } | FifoState::RenderingWindowTile { .. } => {
                todo!("window")
            }
        }
    }

    fn handle_initial_bg_fetch(&mut self, dots_remaining: u8, vram: &Vram, registers: &Registers) {
        if dots_remaining == 6 {
            // Do the initial tile fetch
            for color in fetch_bg_tile(0, self.y, vram, registers) {
                self.bg.push_back(BgPixel { color });
            }
        }

        if dots_remaining != 1 {
            self.state = FifoState::InitialBgFetch { dots_remaining: dots_remaining - 1 };
            return;
        }

        // TODO check if WX=0 here

        // The first 8 pixels are always discarded, and (SCX % 8) additional pixels are discarded to handle fine X scrolling
        // Simulate this by using fine X scroll to move the screen position backwards
        let fine_x_scroll = registers.bg_x_scroll % 8;
        self.state = FifoState::RenderingBgTile(FifoStateFields {
            dots_remaining: 8,
            screen_x: 0_u8.wrapping_sub(fine_x_scroll),
            fetcher_x: 0,
        });
    }

    fn handle_rendering_bg_tile(
        &mut self,
        mut fields: FifoStateFields,
        vram: &Vram,
        registers: &Registers,
        frame_buffer: &mut PpuFrameBuffer,
    ) {
        if fields.dots_remaining == 8 {
            // Do fetch for the next tile
            for color in fetch_bg_tile(fields.fetcher_x, self.y, vram, registers) {
                self.bg.push_back(BgPixel { color });
            }
            fields.fetcher_x = fields.fetcher_x.wrapping_add(1);
        }

        // TODO check for sprites and WX overlap here

        let bg_pixel = self.bg.pop_front().expect("BG FIFO should never be empty while rendering");
        if (8..MAX_FIFO_X).contains(&fields.screen_x) {
            let color = registers.bg_palettes[bg_pixel.color as usize];
            frame_buffer.set(self.y, fields.screen_x - 8, color);
        }
        fields.screen_x = fields.screen_x.wrapping_add(1);

        if fields.dots_remaining == 1 {
            fields.dots_remaining = 8;
        } else {
            fields.dots_remaining -= 1;
        }

        self.state = FifoState::RenderingBgTile(fields);
    }

    pub fn done_with_line(&self) -> bool {
        match self.state {
            FifoState::InitialBgFetch { .. } | FifoState::InitialWindowFetch(..) => false,
            FifoState::RenderingBgTile(fields) | FifoState::RenderingWindowTile(fields) => {
                fields.screen_x == MAX_FIFO_X
            }
        }
    }
}

fn fetch_bg_tile(fetcher_x: u8, y: u8, vram: &Vram, registers: &Registers) -> [u8; 8] {
    let coarse_x_scroll = registers.bg_x_scroll / 8;
    let tile_map_x: u16 = (fetcher_x.wrapping_add(coarse_x_scroll) % 32).into();

    let bg_y: u16 = y.wrapping_add(registers.bg_y_scroll).into();
    let tile_map_y = bg_y / 8;

    let tile_map_addr = registers.bg_tile_map_addr | (tile_map_y << 5) | tile_map_x;
    let tile_number = vram[tile_map_addr as usize];

    let tile_row = bg_y % 8;
    let tile_addr = registers.bg_tile_data_area.tile_address(tile_number) | (tile_row << 1);
    let tile_data_lsb = vram[tile_addr as usize];
    let tile_data_msb = vram[(tile_addr + 1) as usize];

    array::from_fn(|i| {
        let pixel_idx = 7 - i as u8;
        u8::from(tile_data_lsb.bit(pixel_idx)) | (u8::from(tile_data_msb.bit(pixel_idx)) << 1)
    })
}
