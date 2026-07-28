use jgenesis_common::frontend::Color;

#[derive(Debug, Clone, Copy, Default)]
pub struct CramEntry {
    // Raw 16-bit CRAM value
    pub value: u16,
    // RGB888 color displayed by emulator
    pub color: Color,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SpriteAttributeEntry {
    pub tile_number: u16,
    pub x: u16,
    pub y: u16,
    pub h_cells: u8,
    pub v_cells: u8,
    pub palette: u8,
    pub priority: bool,
    pub h_flip: bool,
    pub v_flip: bool,
    pub link: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct CopySpriteAttributesResult {
    pub sprite_table_len: u32,
    pub top_left_x: u16,
    pub top_left_y: u16,
}
