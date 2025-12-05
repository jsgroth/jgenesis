use crate::js;
use js_sys::{Array, Object, Uint8Array};
use std::collections::HashMap;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

pub async fn load_all(file_name: &str) -> HashMap<String, Vec<u8>> {
    try_load_all(file_name).await.unwrap_or_else(|err| {
        log::error!(
            "Error reading save files for file {file_name}: {}",
            err.as_string().unwrap_or_default()
        );
        HashMap::new()
    })
}

async fn try_load_all(file_name: &str) -> Result<HashMap<String, Vec<u8>>, JsValue> {
    let files = JsFuture::from(js::loadSaveFiles(file_name)).await?;
    let files = files.dyn_into::<Object>()?;

    let mut files_map: HashMap<String, Vec<u8>> = HashMap::new();
    for file in Object::entries(&files) {
        let file = file.dyn_into::<Array>()?;

        let Some(extension) = file.get(0).as_string() else {
            return Err(JsValue::from_str("Invalid data in IndexedDB"));
        };
        let bytes = file.get(1).dyn_into::<Uint8Array>()?;

        files_map.insert(extension, bytes.to_vec());
    }

    Ok(files_map)
}

pub async fn write(file_name: &str, extension: &str, bytes: &[u8]) {
    if let Err(err) = try_write(file_name, extension, bytes).await {
        log::error!(
            "Error persisting save file for file {file_name} extension {extension}: {}",
            err.as_string().unwrap_or_default()
        );
    }
}

async fn try_write(file_name: &str, extension: &str, bytes: &[u8]) -> Result<(), JsValue> {
    let array = Uint8Array::new_from_slice(bytes);
    JsFuture::from(js::writeSaveFile(file_name, extension, array)).await?;

    Ok(())
}
