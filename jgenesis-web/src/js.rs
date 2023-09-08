use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/js/ui.js")]
extern "C" {
    pub fn focusCanvas();
}
