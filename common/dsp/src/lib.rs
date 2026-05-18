pub mod design;
pub mod iir;
pub mod sinc;

#[cfg(target_arch = "x86_64")]
macro_rules! option_env_is_none_or_empty {
    ($var:literal) => {
        match option_env!($var) {
            Some(var) => var.is_empty(),
            None => true,
        }
    };
}

// Compile-time env vars to disable AVX2 and AVX512 code paths for testing
const AVX2_ENABLED: bool = cfg_select! {
    target_arch = "x86_64" => option_env_is_none_or_empty!("JGENESIS_NO_AVX2"),
    _ => false,
};
const AVX512_ENABLED: bool = cfg_select! {
    target_arch = "x86_64" => option_env_is_none_or_empty!("JGENESIS_NO_AVX512"),
    _ => false,
};
