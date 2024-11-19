use crate::app::Console;
use jgenesis_native_config::RecentOpen;
use regex::Regex;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, LazyLock, Mutex, mpsc};
use std::{fs, io, thread};

pub const ALL_EXTENSIONS: &[&str] =
    &["sms", "gg", "md", "bin", "cue", "chd", "32x", "nes", "sfc", "smc", "gb", "gbc", "zip", "7z"];

const SMSGG_EXTENSIONS: &[&str] = &["sms", "gg"];
const GENESIS_EXTENSIONS: &[&str] = &["md", "bin"];
const SCD_EXTENSIONS: &[&str] = &["cue", "chd"];
const S32X_EXTENSIONS: &[&str] = &["32x"];
const NES_EXTENSIONS: &[&str] = &["nes"];
const SNES_EXTENSIONS: &[&str] = &["sfc", "smc"];
const GB_EXTENSIONS: &[&str] = &["gb", "gbc"];

impl Console {
    fn from_extension(extension: &str) -> Option<Self> {
        match extension {
            "sms" => Some(Self::MasterSystem),
            "gg" => Some(Self::GameGear),
            "md" | "bin" => Some(Self::Genesis),
            "cue" | "chd" => Some(Self::SegaCd),
            "32x" => Some(Self::Sega32X),
            "nes" => Some(Self::Nes),
            "sfc" | "smc" => Some(Self::Snes),
            "gb" => Some(Self::GameBoy),
            "gbc" => Some(Self::GameBoyColor),
            _ => None,
        }
    }

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
        }
    }

    #[must_use]
    pub fn supported_extensions(self) -> &'static [&'static str] {
        match self {
            Self::MasterSystem | Self::GameGear => SMSGG_EXTENSIONS,
            Self::Genesis => GENESIS_EXTENSIONS,
            Self::SegaCd => SCD_EXTENSIONS,
            Self::Sega32X => S32X_EXTENSIONS,
            Self::Nes => NES_EXTENSIONS,
            Self::Snes => SNES_EXTENSIONS,
            Self::GameBoy | Self::GameBoyColor => GB_EXTENSIONS,
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

pub fn read_metadata(path: &Path) -> Option<RomMetadata> {
    let file_name = path.file_name().and_then(OsStr::to_str)?;
    let metadata = fs::metadata(path).ok()?;
    process_file(file_name, path, metadata)
}

fn process_file(file_name: &str, path: &Path, metadata: fs::Metadata) -> Option<RomMetadata> {
    let full_path = path.to_str().map(String::from)?;
    let file_name_no_ext = Path::new(&file_name).with_extension("").to_string_lossy().to_string();
    let extension = Path::new(&file_name).extension().and_then(OsStr::to_str)?;

    match extension {
        "zip" => {
            let zip_entry = jgenesis_native_driver::archive::first_supported_file_in_zip(
                path,
                jgenesis_native_driver::all_supported_extensions(),
            )
            .ok()
            .flatten()?;
            let console = Console::from_extension(&zip_entry.extension)?;
            Some(RomMetadata { full_path, file_name_no_ext, console, file_size: zip_entry.size })
        }
        "7z" => {
            let zip_entry = jgenesis_native_driver::archive::first_supported_file_in_7z(
                path,
                jgenesis_native_driver::all_supported_extensions(),
            )
            .ok()
            .flatten()?;
            let console = Console::from_extension(&zip_entry.extension)?;
            Some(RomMetadata { full_path, file_name_no_ext, console, file_size: zip_entry.size })
        }
        _ => {
            let console = Console::from_extension(extension)?;
            let file_size = match extension {
                "cue" => sega_cd_file_size(&full_path).ok()?,
                _ => metadata.len(),
            };

            Some(RomMetadata { full_path, file_name_no_ext, console, file_size })
        }
    }
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
    static LINE_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"FILE "(.*)" BINARY"#).unwrap());

    cue_contents
        .lines()
        .filter_map(|line| LINE_RE.captures(line).map(|captures| captures.get(1).unwrap().as_str()))
}

pub fn from_recent_opens(recent_opens: &[RecentOpen]) -> Vec<RomMetadata> {
    recent_opens
        .iter()
        .filter_map(|RecentOpen { console, path: path_str }| {
            let console = Console::from_str(console).ok()?;
            let path = Path::new(path_str);
            let file_name_no_ext =
                path.with_extension("").file_name()?.to_string_lossy().to_string();
            let metadata = fs::metadata(path).ok()?;

            let file_size = match path.extension().and_then(OsStr::to_str) {
                Some("cue") => sega_cd_file_size(path_str).ok()?,
                _ => metadata.len(),
            };

            Some(RomMetadata { full_path: path_str.into(), file_name_no_ext, console, file_size })
        })
        .collect()
}

#[derive(Debug)]
pub struct RomListThreadHandle {
    scan_requests_sender: Sender<Vec<String>>,
    scan_request_counter: Arc<AtomicU32>,
}

impl RomListThreadHandle {
    pub fn spawn(rom_list: Arc<Mutex<Vec<RomMetadata>>>) -> Self {
        let (scan_requests_sender, scan_requests_receiver) = mpsc::channel::<Vec<String>>();
        let scan_request_counter = Arc::new(AtomicU32::new(0));
        let scan_request_counter_handle = Arc::clone(&scan_request_counter);

        thread::spawn(move || {
            while let Ok(scan_request) = scan_requests_receiver.recv() {
                let new_rom_list = build(&scan_request);
                *rom_list.lock().unwrap() = new_rom_list;

                scan_request_counter.fetch_sub(1, Ordering::SeqCst);
            }
        });

        Self { scan_requests_sender, scan_request_counter: scan_request_counter_handle }
    }

    pub fn request_scan(&self, scan_request: Vec<String>) {
        self.scan_request_counter.fetch_add(1, Ordering::SeqCst);
        self.scan_requests_sender.send(scan_request).unwrap();
    }

    pub fn any_scans_in_progress(&self) -> bool {
        self.scan_request_counter.load(Ordering::SeqCst) != 0
    }
}
