use js_sys::{Promise, Uint8Array};
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

    pub fn showSmsGgConfig(input_names: Vec<String>, input_keys: Vec<String>);

    pub fn showGenesisConfig(input_names: Vec<String>, input_keys: Vec<String>);

    pub fn showSnesConfig(input_names: Vec<String>, input_keys: Vec<String>);

    pub fn showGbaConfig(input_names: Vec<String>, input_keys: Vec<String>);

    pub fn setCursorVisible(visible: bool);

    pub fn setRomTitle(rom_title: &str);

    pub fn setSaveUiEnabled(save_ui_enabled: bool);

    pub fn beforeInputConfigure();

    pub fn afterInputConfigure(name: &str, key: &str);

    pub fn localStorageGet(key: &str) -> Option<String>;

    pub fn localStorageSet(key: &str, value: &str);
}

#[wasm_bindgen(module = "/js/idb.js")]
extern "C" {
    // Promise<Object<String, Uint8Array>>
    pub fn loadSaveFiles(key: &str) -> Promise;

    // Promise<()>
    pub fn writeSaveFile(key: &str, extension: &str, bytes: Uint8Array) -> Promise;

    // Promise<Uint8Array | null>
    pub fn loadBios(key: &str) -> Promise;

    // Promise<()>
    pub fn writeBios(key: &str, bytes: Uint8Array) -> Promise;
}
