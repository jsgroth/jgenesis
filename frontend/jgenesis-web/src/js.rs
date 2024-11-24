use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    pub fn alert(message: &str);
}

#[wasm_bindgen(module = "/js/ui.js")]
extern "C" {
    pub fn showUi();

    pub fn setFullscreen(fullscreen: bool);

    pub fn focusCanvas();

    pub fn showSmsGgConfig();

    pub fn showGenesisConfig();

    pub fn showSnesConfig();

    pub fn setCursorVisible(visible: bool);

    pub fn setRomTitle(rom_title: &str);

    pub fn setSaveUiEnabled(save_ui_enabled: bool);

    pub fn localStorageGet(key: &str) -> Option<String>;

    pub fn localStorageSet(key: &str, value: &str);
}
