use std::ffi::OsStr;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Console {
    MasterSystem,
    GameGear,
    Genesis,
}

impl Console {
    fn from_extension(extension: &str) -> Option<Self> {
        match extension {
            "sms" => Some(Self::MasterSystem),
            "gg" => Some(Self::GameGear),
            "md" | "bin" => Some(Self::Genesis),
            _ => None,
        }
    }

    pub fn to_str(self) -> &'static str {
        match self {
            Self::MasterSystem => "Master System",
            Self::GameGear => "Game Gear",
            Self::Genesis => "Genesis",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RomMetadata {
    pub full_path: String,
    pub file_name_no_ext: String,
    pub console: Console,
    pub file_size: u64,
}

pub fn build(rom_search_dir: Option<&String>) -> Vec<RomMetadata> {
    let Some(rom_search_dir) = rom_search_dir else { return vec![] };

    let mut list: Vec<_> = fs::read_dir(Path::new(rom_search_dir))
        .map(|read_dir| {
            read_dir
                .filter_map(|dir_entry| {
                    let dir_entry = dir_entry.ok()?;
                    let metadata = dir_entry.metadata().ok()?;
                    if !metadata.is_file() {
                        return None;
                    }

                    let file_name = dir_entry.file_name().to_string_lossy().to_string();
                    let extension = Path::new(&file_name).extension().and_then(OsStr::to_str)?;
                    let console = Console::from_extension(extension)?;

                    let Some(full_path) = dir_entry.path().to_str().map(String::from) else {
                        return None;
                    };
                    let file_name_no_ext =
                        Path::new(&file_name).with_extension("").to_string_lossy().to_string();

                    let file_size = metadata.len();

                    Some(RomMetadata { full_path, file_name_no_ext, console, file_size })
                })
                .collect()
        })
        .unwrap_or_default();

    list.sort_by(|a, b| a.file_name_no_ext.cmp(&b.file_name_no_ext));
    list
}
