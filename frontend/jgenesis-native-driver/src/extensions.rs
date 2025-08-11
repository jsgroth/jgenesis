use crate::archive;
use crate::archive::{ArchiveEntry, ArchiveError};
use jgenesis_proc_macros::{EnumAll, EnumDisplay, EnumFromStr};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::LazyLock;
use std::{fs, io};

pub const MASTER_SYSTEM: &[&str] = &["sms"];
pub const GAME_GEAR: &[&str] = &["gg"];
pub const GENESIS: &[&str] = &["gen", "md", "bin", "smd"];
pub const SEGA_CD: &[&str] = &["cue", "chd"];
pub const SEGA_32X: &[&str] = &["32x", "bin"];
pub const NES: &[&str] = &["nes"];
pub const SNES: &[&str] = &["sfc", "smc"];
pub const GAME_BOY: &[&str] = &["gb"];
pub const GAME_BOY_COLOR: &[&str] = &["gbc"];
pub const GAME_BOY_ADVANCE: &[&str] = &["gba"];

pub const SUPPORTED_ARCHIVES: &[&str] = &["zip", "7z"];

pub static SMSGG: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    [MASTER_SYSTEM, GAME_GEAR].into_iter().flat_map(|system| system.iter().copied()).collect()
});

pub static GB_GBC: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    [GAME_BOY, GAME_BOY_COLOR].into_iter().flat_map(|system| system.iter().copied()).collect()
});

pub static ALL_CARTRIDGE_BASED: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    [
        MASTER_SYSTEM,
        GAME_GEAR,
        GENESIS,
        SEGA_32X,
        NES,
        SNES,
        GAME_BOY,
        GAME_BOY_COLOR,
        GAME_BOY_ADVANCE,
    ]
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

struct FoundEntryMetadata {
    file_name: String,
    extension: String,
    size: u64,
}

struct ArchiveListCallback {
    contains_cue: bool,
    first_supported_file: Option<FoundEntryMetadata>,
}

impl ArchiveListCallback {
    fn new() -> Self {
        Self { contains_cue: false, first_supported_file: None }
    }

    fn as_fn_mut(&mut self) -> impl FnMut(ArchiveEntry<'_>) + use<'_> {
        |entry| {
            let Some(extension) = from_path(entry.file_name) else { return };

            self.contains_cue |= extension.as_str() == "cue";

            if self.first_supported_file.is_none() && ALL.contains(&extension.as_str()) {
                self.first_supported_file = Some(FoundEntryMetadata {
                    file_name: entry.file_name.into(),
                    extension,
                    size: entry.size,
                });
            }
        }
    }
}

static EXTENSION_LOOKUP: LazyLock<HashMap<&'static str, Console>> =
    LazyLock::new(build_extension_lookup);

fn build_extension_lookup() -> HashMap<&'static str, Console> {
    [
        (MASTER_SYSTEM, Console::MasterSystem),
        (GAME_GEAR, Console::GameGear),
        (GENESIS, Console::Genesis),
        (SEGA_CD, Console::SegaCd),
        (SEGA_32X, Console::Sega32X),
        (NES, Console::Nes),
        (SNES, Console::Snes),
        (GAME_BOY, Console::GameBoy),
        (GAME_BOY_COLOR, Console::GameBoyColor),
        (GAME_BOY_ADVANCE, Console::GameBoyAdvance),
    ]
    .into_iter()
    .flat_map(|(extensions, console)| extensions.iter().map(move |&extension| (extension, console)))
    .filter(|&(extension, _)| {
        // Exclude .bin files because it's ambiguous whether the ROM is Genesis or 32X without
        // checking the file contents
        extension != "bin"
    })
    .collect()
}

#[derive(Debug, Clone, Copy)]
pub struct ConsoleWithSize {
    pub console: Console,
    pub file_size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumDisplay, EnumFromStr, EnumAll)]
pub enum Console {
    MasterSystem,
    GameGear,
    Genesis,
    SegaCd,
    Sega32X,
    Nes,
    Snes,
    GameBoy,
    GameBoyColor,
    GameBoyAdvance,
}

impl Console {
    #[must_use]
    pub fn from_file(file_path: &Path) -> Option<ConsoleWithSize> {
        let extension = from_path(file_path)?;
        if let Some(&console) = EXTENSION_LOOKUP.get(&extension.as_str()) {
            let file_size = fs::metadata(file_path).ok()?.len();
            return Some(ConsoleWithSize { console, file_size });
        }

        match extension.as_str() {
            "bin" => {
                let console = guess_bin_console_raw_file(file_path).ok()?;
                let file_size = fs::metadata(file_path).ok()?.len();
                Some(ConsoleWithSize { console, file_size })
            }
            "zip" => Self::from_zip(file_path),
            "7z" => Self::from_7z(file_path),
            _ => None,
        }
    }

    fn from_zip(zip_path: &Path) -> Option<ConsoleWithSize> {
        let mut callback = ArchiveListCallback::new();
        archive::list_files_zip(zip_path, callback.as_fn_mut()).ok()?;

        Self::from_list_callback(callback, |file_name| archive::read_file_zip(zip_path, file_name))
    }

    fn from_7z(sevenz_path: &Path) -> Option<ConsoleWithSize> {
        let mut callback = ArchiveListCallback::new();
        archive::list_files_7z(sevenz_path, callback.as_fn_mut()).ok()?;

        Self::from_list_callback(callback, |file_name| {
            archive::read_file_7z(sevenz_path, file_name)
        })
    }

    fn from_list_callback(
        callback: ArchiveListCallback,
        read_file_fn: impl FnOnce(&str) -> Result<Vec<u8>, ArchiveError>,
    ) -> Option<ConsoleWithSize> {
        if callback.contains_cue {
            return None;
        }

        let first_supported_file = callback.first_supported_file?;
        if let Some(&console) = EXTENSION_LOOKUP.get(&first_supported_file.extension.as_str()) {
            return Some(ConsoleWithSize { console, file_size: first_supported_file.size });
        }

        if first_supported_file.extension.as_str() == "bin" {
            let contents = read_file_fn(&first_supported_file.file_name).ok()?;
            let console = guess_bin_console_archive(&contents);
            return Some(ConsoleWithSize { console, file_size: first_supported_file.size });
        }

        None
    }

    #[inline]
    #[must_use]
    pub fn display_str(self) -> &'static str {
        match self {
            Self::MasterSystem => "Master System",
            Self::GameGear => "Game Gear",
            Self::Genesis => "Genesis",
            Self::SegaCd => "Sega CD",
            Self::Sega32X => "32X",
            Self::Nes => "NES",
            Self::Snes => "SNES",
            Self::GameBoy => "Game Boy",
            Self::GameBoyColor => "Game Boy Color",
            Self::GameBoyAdvance => "Game Boy Advance",
        }
    }

    #[inline]
    #[must_use]
    pub fn supported_extensions(self) -> &'static [&'static str] {
        match self {
            Self::MasterSystem | Self::GameGear => &SMSGG,
            Self::Genesis => GENESIS,
            Self::SegaCd => SEGA_CD,
            Self::Sega32X => SEGA_32X,
            Self::Nes => NES,
            Self::Snes => SNES,
            Self::GameBoy | Self::GameBoyColor => &GB_GBC,
            Self::GameBoyAdvance => GAME_BOY_ADVANCE,
        }
    }
}

fn guess_bin_console_raw_file(path: &Path) -> io::Result<Console> {
    const SECURITY_PROGRAM_CARTRIDGE_ADDR: u64 = s32x_core::SECURITY_PROGRAM_CARTRIDGE_ADDR as u64;
    const SECURITY_PROGRAM_LEN: usize = s32x_core::SECURITY_PROGRAM_LEN;

    let file = File::open(path)?;
    if file.metadata()?.len() < SECURITY_PROGRAM_CARTRIDGE_ADDR + SECURITY_PROGRAM_LEN as u64 {
        return Ok(Console::Genesis);
    }

    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(SECURITY_PROGRAM_CARTRIDGE_ADDR))?;

    let mut buffer = [0; SECURITY_PROGRAM_LEN];
    reader.read_exact(&mut buffer)?;

    Ok(if buffer == s32x_core::security_program() { Console::Sega32X } else { Console::Genesis })
}

fn guess_bin_console_archive(file: &[u8]) -> Console {
    let start = s32x_core::SECURITY_PROGRAM_CARTRIDGE_ADDR;
    let end = start + s32x_core::SECURITY_PROGRAM_LEN;

    let contains_s32x_security_program =
        file.len() >= end && &file[start..end] == s32x_core::security_program();
    if contains_s32x_security_program { Console::Sega32X } else { Console::Genesis }
}
