use crate::ppu;
use crate::ppu::{ColorEmphasis, FrameBuffer};
use jgenesis_common::frontend::{Color, TimingMode};

pub trait TimingModeGraphicsExt {
    fn visible_screen_height(self) -> u16;

    fn starting_row(self) -> u16;
}

impl TimingModeGraphicsExt for TimingMode {
    fn visible_screen_height(self) -> u16 {
        match self {
            Self::Ntsc => 224,
            Self::Pal => 240,
        }
    }

    fn starting_row(self) -> u16 {
        match self {
            Self::Ntsc => 8,
            Self::Pal => 0,
        }
    }
}

const PALETTE: &[u8] = include_bytes!("nespalette.pal");

pub fn ppu_frame_buffer_to_rgba(
    ppu_frame_buffer: &FrameBuffer,
    rgba_frame_buffer: &mut [Color],
    timing_mode: TimingMode,
) {
    let row_offset = timing_mode.starting_row();
    let visible_screen_height = timing_mode.visible_screen_height();

    for (row, scanline) in ppu_frame_buffer
        .iter()
        .skip(row_offset as usize)
        .take(visible_screen_height as usize)
        .enumerate()
    {
        for (col, &(nes_color, color_emphasis)) in scanline.iter().enumerate() {
            let color_emphasis_offset = get_color_emphasis_offset(color_emphasis);
            let palette_idx = (color_emphasis_offset + 3 * u16::from(nes_color)) as usize;

            let r = PALETTE[palette_idx];
            let g = PALETTE[palette_idx + 1];
            let b = PALETTE[palette_idx + 2];
            let rgba_color = Color::rgb(r, g, b);

            rgba_frame_buffer[row * ppu::SCREEN_WIDTH as usize + col] = rgba_color;
        }
    }
}

fn get_color_emphasis_offset(color_emphasis: ColorEmphasis) -> u16 {
    3 * 64 * u16::from(color_emphasis.red())
        + 3 * 128 * u16::from(color_emphasis.green())
        + 3 * 256 * u16::from(color_emphasis.blue())
}
