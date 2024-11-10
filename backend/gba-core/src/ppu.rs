use bincode::{Decode, Encode};
use jgenesis_common::frontend::{Color, FrameSize};
use jgenesis_proc_macros::{FakeDecode, FakeEncode};

const SCREEN_WIDTH: u32 = 240;
const SCREEN_HEIGHT: u32 = 160;
const FRAME_BUFFER_LEN: usize = (SCREEN_WIDTH * SCREEN_HEIGHT) as usize;

pub const FRAME_SIZE: FrameSize = FrameSize { width: SCREEN_WIDTH, height: SCREEN_HEIGHT };

#[derive(Debug, FakeEncode, FakeDecode)]
struct FrameBuffer(Box<[Color; FRAME_BUFFER_LEN]>);

impl Default for FrameBuffer {
    fn default() -> Self {
        Self(vec![Color::default(); FRAME_BUFFER_LEN].into_boxed_slice().try_into().unwrap())
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Ppu {}

impl Ppu {
    pub fn new() -> Self {
        Self {}
    }

    pub fn tick(&mut self, ppu_cycles: u32) {
        todo!("tick PPU {ppu_cycles}")
    }
}
