mod debug;

pub use debug::{PatternTable, copy_nametables, copy_oam, copy_palette_ram};

use crate::api::Overscan;
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

const PALETTE: &[u8; 3 * 64 * 8] = include_bytes!("nespalette.pal");

pub fn ppu_frame_buffer_to_rgba(
    ppu_frame_buffer: &FrameBuffer,
    rgba_frame_buffer: &mut [Color],
    overscan: Overscan,
    display_mode: TimingMode,
) {
    rgba_frame_buffer.fill(Color::BLACK);

    let row_offset = display_mode.starting_row();
    let visible_screen_height = display_mode.visible_screen_height();

    let num_rows_rendered =
        visible_screen_height.saturating_sub(overscan.top).saturating_sub(overscan.bottom) as usize;
    let num_cols_rendered =
        ppu::SCREEN_WIDTH.saturating_sub(overscan.left).saturating_sub(overscan.right) as usize;

    for (row, scanline) in ppu_frame_buffer
        .iter()
        .skip(row_offset as usize + overscan.top as usize)
        .take(num_rows_rendered)
        .enumerate()
    {
        for (col, &(nes_color, color_emphasis)) in
            scanline.iter().skip(overscan.left as usize).take(num_cols_rendered).enumerate()
        {
            let rgba_color = nes_color_to_rgba(nes_color, color_emphasis);
            rgba_frame_buffer[row * num_cols_rendered + col] = rgba_color;
        }
    }
}

pub fn nes_color_to_rgba(nes_color: u8, color_emphasis: ColorEmphasis) -> Color {
    let color_emphasis_offset = get_color_emphasis_offset(color_emphasis);
    let palette_idx = (color_emphasis_offset + 3 * u16::from(nes_color)) as usize;

    let r = PALETTE[palette_idx];
    let g = PALETTE[palette_idx + 1];
    let b = PALETTE[palette_idx + 2];
    Color::rgb(r, g, b)
}

fn get_color_emphasis_offset(color_emphasis: ColorEmphasis) -> u16 {
    3 * 64 * u16::from(color_emphasis.red())
        + 3 * 128 * u16::from(color_emphasis.green())
        + 3 * 256 * u16::from(color_emphasis.blue())
}
