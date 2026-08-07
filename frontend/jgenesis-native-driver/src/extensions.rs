use crate::archive;
use crate::archive::{ArchiveEntry, ArchiveError};
use cdrom::reader::{CdRom, CdRomFileFormat};
use genesis_core::api::GenesisHardware;
use jgenesis_proc_macros::{CustomValueEnum, EnumAll, EnumDisplay, EnumFromStr};
use smsgg_core::SmsGgHardware;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::LazyLock;
use std::{cmp, fs, io};

pub const SG_1000: &[&str] = &["sg"];
pub const MASTER_SYSTEM: &[&str] = &["sms"];
pub const GAME_GEAR: &[&str] = &["gg"];
pub const GENESIS: &[&str] = &["gen", "md", "bin", "smd"];
pub const SEGA_CD: &[&str] = &["cue", "chd"];
pub const SEGA_32X: &[&str] = &["32x", "bin"];
pub const NES: &[&str] = &["nes"];
pub const SNES: &[&str] = &["sfc", "smc"];
pub const GAME_BOY: &[&str] = &["gb"];
pub const GAME_BOY_COLOR: &[&str] = &["gbc"];
pub const GAME_BOY_ADVANCE: &[&str] = &["gba", "bin"];
pub const PC_ENGINE: &[&str] = &["pce"];

pub const SUPPORTED_ARCHIVES: &[&str] = &["zip", "7z"];

const GENESIS_32X_HEADER_LEN: usize =
    s32x_core::SECURITY_PROGRAM_CARTRIDGE_ADDR + s32x_core::SECURITY_PROGRAM_LEN;

fn concat_extensions(iter: impl IntoIterator<Item = &'static [&'static str]>) -> Vec<&'static str> {
    iter.into_iter()
        .flat_map(|system| system.iter().copied())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}

pub static SMSGG: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| concat_extensions([SG_1000, MASTER_SYSTEM, GAME_GEAR]));

pub static GENESIS_32X: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| concat_extensions([GENESIS, SEGA_32X]));

pub static SEGA_CD_32X: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| concat_extensions([GENESIS, SEGA_CD, SEGA_32X]));

pub static GB_GBC: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| concat_extensions([GAME_BOY, GAME_BOY_COLOR]));

pub static ALL_CARTRIDGE_BASED: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    concat_extensions([
        SG_1000,
        MASTER_SYSTEM,
        GAME_GEAR,
        GENESIS,
        SEGA_32X,
        NES,
        SNES,
        GAME_BOY,
        GAME_BOY_COLOR,
        GAME_BOY_ADVANCE,
        PC_ENGINE,
    ])
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
        (SG_1000, Console::Sg1000),
        (MASTER_SYSTEM, Console::MasterSystem),
        (GAME_GEAR, Console::GameGear),
        // Exclude Genesis/32X because need to check the header for Sega CD support
        (SEGA_CD, Console::SegaCd),
        (NES, Console::Nes),
        (SNES, Console::Snes),
        (GAME_BOY, Console::GameBoy),
        (GAME_BOY_COLOR, Console::GameBoyColor),
        (GAME_BOY_ADVANCE, Console::GameBoyAdvance),
        (PC_ENGINE, Console::PcEngine),
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

#[derive(Debug, Clone, Copy)]
pub struct SupportedExtensions {
    // Should display the console name if label is None
    pub label: Option<&'static str>,
    extensions: &'static [&'static str],
    include_archives: bool,
}

impl SupportedExtensions {
    #[must_use]
    pub fn to_list(self) -> Cow<'static, [&'static str]> {
        if self.include_archives {
            self.extensions.iter().copied().chain(["zip", "7z"]).collect()
        } else {
            self.extensions.into()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumDisplay, EnumFromStr, EnumAll, CustomValueEnum)]
pub enum Console {
    MasterSystem,
    GameGear,
    Sg1000,
    Genesis,
    SegaCd,
    Sega32X,
    SegaCd32X,
    Nes,
    Snes,
    GameBoy,
    GameBoyColor,
    GameBoyAdvance,
    PcEngine,
}

impl Console {
    #[must_use]
    pub fn from_file(file_path: &Path) -> Option<ConsoleWithSize> {
        let extension = from_path(file_path)?;
        if let Some(&console) = EXTENSION_LOOKUP.get(&extension.as_str()) {
            let console = match console {
                Console::SegaCd if is_disc_sega_cd_32x(file_path) => Console::SegaCd32X,
                _ => console,
            };

            let file_size = fs::metadata(file_path).ok()?.len();
            return Some(ConsoleWithSize { console, file_size });
        }

        if GENESIS_32X.contains(&extension.as_str()) {
            let console = guess_genesis_console_raw_file(file_path).ok()?;
            let file_size = fs::metadata(file_path).ok()?.len();
            return Some(ConsoleWithSize { console, file_size });
        }

        match extension.as_str() {
            "zip" => Self::from_zip(file_path),
            "7z" => Self::from_7z(file_path),
            _ => None,
        }
    }

    fn from_zip(zip_path: &Path) -> Option<ConsoleWithSize> {
        let mut callback = ArchiveListCallback::new();
        archive::list_files_zip(zip_path, callback.as_fn_mut()).ok()?;

        Self::from_list_callback(callback, |file_name, max_len| {
            archive::read_file_zip(zip_path, file_name, max_len)
        })
    }

    fn from_7z(sevenz_path: &Path) -> Option<ConsoleWithSize> {
        let mut callback = ArchiveListCallback::new();
        archive::list_files_7z(sevenz_path, callback.as_fn_mut()).ok()?;

        Self::from_list_callback(callback, |file_name, max_len| {
            archive::read_file_7z(sevenz_path, file_name, max_len)
        })
    }

    fn from_list_callback(
        callback: ArchiveListCallback,
        read_file_fn: impl FnOnce(&str, usize) -> Result<Vec<u8>, ArchiveError>,
    ) -> Option<ConsoleWithSize> {
        if callback.contains_cue {
            return None;
        }

        let first_supported_file = callback.first_supported_file?;
        if let Some(&console) = EXTENSION_LOOKUP.get(&first_supported_file.extension.as_str()) {
            return Some(ConsoleWithSize { console, file_size: first_supported_file.size });
        }

        if GENESIS_32X.contains(&first_supported_file.extension.as_str()) {
            let header =
                read_file_fn(&first_supported_file.file_name, GENESIS_32X_HEADER_LEN).ok()?;
            let console = guess_genesis_console(&header);
            return Some(ConsoleWithSize { console, file_size: first_supported_file.size });
        }

        None
    }

    #[inline]
    #[must_use]
    pub const fn display_str(self) -> &'static str {
        match self {
            Self::Sg1000 => "SG-1000",
            Self::MasterSystem => "Master System",
            Self::GameGear => "Game Gear",
            Self::Genesis => "Genesis",
            Self::SegaCd => "Sega CD",
            Self::Sega32X => "32X",
            Self::SegaCd32X => "Sega CD 32X",
            Self::Nes => "NES",
            Self::Snes => "SNES",
            Self::GameBoy => "Game Boy",
            Self::GameBoyColor => "Game Boy Color",
            Self::GameBoyAdvance => "Game Boy Advance",
            Self::PcEngine => "PC Engine",
        }
    }

    #[inline]
    #[must_use]
    pub fn supported_extensions(self) -> Vec<SupportedExtensions> {
        fn single(extensions: &'static [&'static str]) -> Vec<SupportedExtensions> {
            vec![SupportedExtensions { label: None, extensions, include_archives: true }]
        }

        match self {
            Self::Sg1000 | Self::MasterSystem | Self::GameGear => single(&SMSGG),
            Self::Genesis => single(GENESIS),
            Self::SegaCd => vec![
                SupportedExtensions { label: None, extensions: SEGA_CD, include_archives: false },
                SupportedExtensions {
                    label: Some("Genesis"),
                    extensions: GENESIS,
                    include_archives: true,
                },
            ],
            Self::Sega32X => single(SEGA_32X),
            Self::SegaCd32X => vec![
                SupportedExtensions {
                    label: None,
                    extensions: &SEGA_CD_32X,
                    include_archives: true,
                },
                SupportedExtensions {
                    label: Some("Sega CD"),
                    extensions: SEGA_CD,
                    include_archives: false,
                },
                SupportedExtensions {
                    label: Some("Genesis/32X"),
                    extensions: &GENESIS_32X,
                    include_archives: true,
                },
            ],
            Self::Nes => single(NES),
            Self::Snes => single(SNES),
            Self::GameBoy | Self::GameBoyColor => single(&GB_GBC),
            Self::GameBoyAdvance => single(GAME_BOY_ADVANCE),
            Self::PcEngine => single(PC_ENGINE),
        }
    }

    #[inline]
    #[must_use]
    pub const fn standard_extension(self) -> &'static str {
        match self {
            Self::MasterSystem => "sms",
            Self::GameGear => "gg",
            Self::Sg1000 => "sg",
            Self::Genesis => "md",
            Self::SegaCd | Self::SegaCd32X => "scd", // Intentionally not CUE or CHD, too ambiguous
            Self::Sega32X => "32x",
            Self::Nes => "nes",
            Self::Snes => "sfc",
            Self::GameBoy => "gb",
            Self::GameBoyColor => "gbc",
            Self::GameBoyAdvance => "gba",
            Self::PcEngine => "pce",
        }
    }

    #[inline]
    #[must_use]
    pub fn to_smsgg_hardware(self) -> Option<SmsGgHardware> {
        match self {
            Self::MasterSystem => Some(SmsGgHardware::MasterSystem),
            Self::GameGear => Some(SmsGgHardware::GameGear),
            Self::Sg1000 => Some(SmsGgHardware::Sg1000),
            _ => None,
        }
    }

    #[inline]
    #[must_use]
    pub fn to_genesis_hardware(self) -> Option<GenesisHardware> {
        match self {
            Self::Genesis => Some(GenesisHardware::Standalone),
            Self::SegaCd => Some(GenesisHardware::SegaCd),
            Self::Sega32X => Some(GenesisHardware::Sega32X),
            Self::SegaCd32X => Some(GenesisHardware::SegaCd32X),
            _ => None,
        }
    }
}

// Assuming this is a path to a Genesis or 32X image, determine what hardware is supported/required
fn guess_genesis_console_raw_file(path: &Path) -> io::Result<Console> {
    let file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let buffer_len = cmp::min(file_len as usize, GENESIS_32X_HEADER_LEN);

    let mut reader = BufReader::new(file);
    let mut buffer = vec![0; buffer_len];
    reader.read_exact(&mut buffer)?;

    Ok(guess_genesis_console(&buffer))
}

fn guess_genesis_console(header: &[u8]) -> Console {
    let start = s32x_core::SECURITY_PROGRAM_CARTRIDGE_ADDR;
    let end = start + s32x_core::SECURITY_PROGRAM_LEN;
    let contains_s32x_security_program =
        header.len() >= end && &header[start..end] == s32x_core::security_program();

    // 'C' in the devices section indicates Sega CD support: https://plutiedev.com/rom-header#devices
    // Exclude J-Cart games (have device string 'OJKRPTBVFCA')
    let mut supports_sega_cd = header.len() >= 0x1A0
        && header[0x190..0x1A0].contains(&b'C')
        && &header[0x190..0x19B] != b"OJKRPTBVFCA";

    // Special case Flux (audio CD visualizer), header doesn't indicate Sega CD support
    supports_sega_cd |= header.len() >= 0x18B && &header[0x180..0x18B] == b"GM T-70416-";

    if supports_sega_cd && contains_s32x_security_program {
        Console::SegaCd32X
    } else if contains_s32x_security_program {
        Console::Sega32X
    } else if supports_sega_cd {
        Console::SegaCd
    } else {
        Console::Genesis
    }
}

// Assuming this is a path to a Sega CD disc image, check whether the game supports/requires 32X
fn is_disc_sega_cd_32x(path: &Path) -> bool {
    let Some(disc_format) = CdRomFileFormat::from_file_path(path) else { return false };
    let Ok(mut disc) = CdRom::open(path, disc_format) else { return false };

    segacd_core::is_cd_32x_disc(&mut disc)
}
