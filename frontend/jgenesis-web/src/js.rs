use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/js/ui.js")]
extern "C" {
    pub fn showUi();

    pub fn focusCanvas();

    pub fn showSmsGgConfig();

    pub fn showGenesisConfig();

    pub fn showSnesConfig();

    pub fn setCursorVisible(visible: bool);

    pub fn setRomTitle(rom_title: &str);
}
