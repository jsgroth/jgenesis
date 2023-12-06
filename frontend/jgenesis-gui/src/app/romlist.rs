use regex::Regex;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::Path;
use std::sync::OnceLock;
use std::{fs, io};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Console {
    MasterSystem,
    GameGear,
    Genesis,
    SegaCd,
    Snes,
}

impl Console {
    fn from_extension(extension: &str) -> Option<Self> {
        match extension {
            "sms" => Some(Self::MasterSystem),
            "gg" => Some(Self::GameGear),
            "md" | "bin" => Some(Self::Genesis),
            "cue" => Some(Self::SegaCd),
            "sfc" | "smc" => Some(Self::Snes),
            _ => None,
        }
    }

    pub fn to_str(self) -> &'static str {
        match self {
            Self::MasterSystem => "Master System",
            Self::GameGear => "Game Gear",
            Self::Genesis => "Genesis",
            Self::SegaCd => "Sega CD",
            Self::Snes => "SNES",
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

pub fn build(rom_search_dirs: &[String]) -> Vec<RomMetadata> {
    let mut metadata: Vec<_> = rom_search_dirs
        .iter()
        .flat_map(|rom_search_dir| {
            fs::read_dir(Path::new(rom_search_dir))
                .map(|read_dir| {
                    read_dir
                        .filter_map(|dir_entry| {
                            let dir_entry = dir_entry.ok()?;
                            let metadata = dir_entry.metadata().ok()?;
                            if !metadata.is_file() {
                                return None;
                            }

                            let file_name = dir_entry.file_name().to_string_lossy().to_string();
                            process_file(&file_name, &dir_entry.path(), metadata)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .collect();

    // Remove any files that are referenced in .cue files
    let cd_bin_file_names = metadata
        .iter()
        .filter(|metadata| {
            Path::new(&metadata.full_path).extension().and_then(OsStr::to_str) == Some("cue")
        })
        .filter_map(|metadata| {
            let path = Path::new(&metadata.full_path);

            let cue_directory = path.parent()?;
            let cue_contents = fs::read_to_string(path).ok()?;

            let file_names = parse_bin_file_names(&cue_contents)
                .filter_map(|file_name| cue_directory.join(file_name).to_str().map(String::from))
                .collect::<Vec<_>>();
            Some(file_names)
        })
        .flatten()
        .collect::<HashSet<_>>();

    metadata.retain(|metadata| !cd_bin_file_names.contains(&metadata.full_path));

    metadata.sort_by(|a, b| a.file_name_no_ext.cmp(&b.file_name_no_ext));
    metadata
}

fn process_file(file_name: &str, path: &Path, metadata: fs::Metadata) -> Option<RomMetadata> {
    let extension = Path::new(&file_name).extension().and_then(OsStr::to_str)?;
    let console = Console::from_extension(extension)?;

    let Some(full_path) = path.to_str().map(String::from) else {
        return None;
    };
    let file_name_no_ext = Path::new(&file_name).with_extension("").to_string_lossy().to_string();

    let file_size = match console {
        Console::SegaCd => sega_cd_file_size(&full_path).ok()?,
        _ => metadata.len(),
    };

    Some(RomMetadata { full_path, file_name_no_ext, console, file_size })
}

fn sega_cd_file_size(cue_path: &str) -> io::Result<u64> {
    let cue_contents = fs::read_to_string(cue_path)?;
    let cue_directory =
        Path::new(cue_path).parent().expect("Valid file should always have a parent dir");

    let unique_file_names = parse_bin_file_names(&cue_contents).collect::<HashSet<_>>();

    unique_file_names
        .iter()
        .map(|file_name| {
            let full_path = cue_directory.join(file_name);
            fs::metadata(full_path).map(|metadata| metadata.len())
        })
        .sum()
}

fn parse_bin_file_names(cue_contents: &str) -> impl Iterator<Item = &str> {
    static LINE_RE: OnceLock<Regex> = OnceLock::new();

    cue_contents.lines().filter_map(|line| {
        let line_re = LINE_RE.get_or_init(|| Regex::new(r#"FILE "(.*)" BINARY"#).unwrap());

        line_re.captures(line).map(|captures| captures.get(1).unwrap().as_str())
    })
}

pub fn from_recent_opens(recent_opens: &[String]) -> Vec<RomMetadata> {
    recent_opens
        .iter()
        .filter_map(|path| {
            let path = Path::new(path);
            let file_name = path.file_name()?.to_string_lossy();
            let metadata = fs::metadata(path).ok()?;

            process_file(&file_name, path, metadata)
        })
        .collect()
}
