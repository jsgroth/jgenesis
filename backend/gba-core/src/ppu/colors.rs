use gba_config::GbaColorCorrection;
use jgenesis_common::frontend::Color;
use std::array;
use std::sync::LazyLock;

const RGB_5_TO_8: &[u8; 32] = &[
    0, 8, 16, 25, 33, 41, 49, 58, 66, 74, 82, 90, 99, 107, 115, 123, 132, 140, 148, 156, 165, 173,
    181, 189, 197, 206, 214, 222, 230, 239, 247, 255,
];

pub type ColorTable = [Color; 32768];

static NONE_TABLE: LazyLock<Box<ColorTable>> = LazyLock::new(|| {
    Box::new(array::from_fn(|color| {
        let r = color & 0x1F;
        let g = (color >> 5) & 0x1F;
        let b = (color >> 10) & 0x1F;
        Color::rgb(RGB_5_TO_8[r], RGB_5_TO_8[g], RGB_5_TO_8[b])
    }))
});

static GBA_LCD_TABLE: LazyLock<Box<ColorTable>> =
    LazyLock::new(|| Box::new(array::from_fn(gb_core::graphics::gba_lcd_correction)));

pub fn table(color_correction: GbaColorCorrection) -> &'static ColorTable {
    match color_correction {
        GbaColorCorrection::None => &NONE_TABLE,
        GbaColorCorrection::GbaLcd => &GBA_LCD_TABLE,
    }
}
