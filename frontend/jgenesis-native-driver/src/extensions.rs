use std::ffi::OsStr;
use std::path::Path;
use std::sync::LazyLock;

pub const MASTER_SYSTEM: &[&str] = &["sms"];
pub const GAME_GEAR: &[&str] = &["gg"];
pub const GENESIS: &[&str] = &["gen", "md", "bin", "smd"];
pub const SEGA_CD: &[&str] = &["cue", "chd"];
pub const SEGA_32X: &[&str] = &["32x"];
pub const NES: &[&str] = &["nes"];
pub const SNES: &[&str] = &["sfc", "smc"];
pub const GAME_BOY: &[&str] = &["gb"];
pub const GAME_BOY_COLOR: &[&str] = &["gbc"];

pub const SUPPORTED_ARCHIVES: &[&str] = &["zip", "7z"];

pub static SMSGG: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    [MASTER_SYSTEM, GAME_GEAR].into_iter().flat_map(|system| system.iter().copied()).collect()
});

pub static GB_GBC: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    [GAME_BOY, GAME_BOY_COLOR].into_iter().flat_map(|system| system.iter().copied()).collect()
});

pub static ALL_CARTRIDGE_BASED: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    [MASTER_SYSTEM, GAME_GEAR, GENESIS, SEGA_32X, NES, SNES, GAME_BOY, GAME_BOY_COLOR]
        .into_iter()
        .flat_map(|system| system.iter().copied())
        .collect()
});

pub static ALL: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    ALL_CARTRIDGE_BASED.clone().into_iter().chain(SEGA_CD.iter().copied()).collect()
});

pub static ALL_PLUS_ARCHIVES: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| ALL.clone().into_iter().chain(SUPPORTED_ARCHIVES.iter().copied()).collect());

#[must_use]
pub fn from_path<P: AsRef<Path>>(path: P) -> Option<String> {
    fn inner(path: &Path) -> Option<String> {
        path.extension().map(OsStr::to_ascii_lowercase).and_then(|s| s.to_str().map(String::from))
    }

    inner(path.as_ref())
}
