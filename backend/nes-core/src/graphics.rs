mod debug;

pub use debug::{PatternTable, copy_nametables, copy_oam, copy_palette_ram};

use crate::ppu;
use crate::ppu::{ColorEmphasis, FrameBuffer};
use jgenesis_common::frontend::{Color, TimingMode};
use nes_config::{NesPalette, Overscan};

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

pub fn ppu_frame_buffer_to_rgba(
    ppu_frame_buffer: &FrameBuffer,
    rgba_frame_buffer: &mut [Color],
    overscan: Overscan,
    display_mode: TimingMode,
    palette: &NesPalette,
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
            let rgba_color = nes_color_to_rgba(nes_color, color_emphasis, palette);
            rgba_frame_buffer[row * num_cols_rendered + col] = rgba_color;
        }
    }
}

pub fn nes_color_to_rgba(
    nes_color: u8,
    color_emphasis: ColorEmphasis,
    palette: &NesPalette,
) -> Color {
    let palette_idx = usize::from(nes_color)
        + 64 * usize::from(color_emphasis.red())
        + 128 * usize::from(color_emphasis.green())
        + 256 * usize::from(color_emphasis.blue());

    let (r, g, b) = palette[palette_idx];
    Color::rgb(r, g, b)
}
