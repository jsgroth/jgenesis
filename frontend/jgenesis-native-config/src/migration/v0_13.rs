use toml_edit::DocumentMut;

#[must_use]
pub fn migrate_document(document: &mut DocumentMut) -> bool {
    let mut changed = false;

    // v0.13.0: NES allow_opposing_joypad_inputs -> allow_opposing_joypad_directions
    if let Some((_, nes_value)) = document.get_key_value_mut("nes")
        && let Some(nes_config) = nes_value.as_table_like_mut()
        && let Some((_, allow_opposing_inputs_value)) =
            nes_config.get_key_value_mut("allow_opposing_joypad_inputs")
        && let Some(allow_opposing_inputs) = allow_opposing_inputs_value.as_bool()
        && nes_config.get_key_value("allow_opposing_joypad_directions").is_none()
    {
        nes_config
            .insert("allow_opposing_joypad_directions", toml_edit::value(allow_opposing_inputs));
        changed = true;
    }

    changed
}
