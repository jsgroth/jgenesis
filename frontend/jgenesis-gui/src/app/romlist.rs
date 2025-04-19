use crate::app::Console;
use jgenesis_native_config::RecentOpen;
use jgenesis_native_driver::extensions;
use jgenesis_native_driver::extensions::ConsoleWithSize;
use regex::Regex;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, LazyLock, Mutex, mpsc};
use std::{fs, io, thread};

#[derive(Debug, Clone)]
pub struct RomMetadata {
    pub full_path: PathBuf,
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
                            process_file(&file_name, &dir_entry.path())
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .collect();

    // Remove any files that are referenced in .cue files
    let cd_bin_file_names = metadata
        .iter()
        .filter(|metadata| extensions::from_path(&metadata.full_path).as_deref() == Some("cue"))
        .filter_map(|metadata| {
            let path = Path::new(&metadata.full_path);

            let cue_directory = path.parent()?;
            let cue_contents = fs::read_to_string(path).ok()?;

            let file_paths = parse_bin_file_names(&cue_contents)
                .map(|file_name| cue_directory.join(file_name))
                .collect::<Vec<_>>();
            Some(file_paths)
        })
        .flatten()
        .collect::<HashSet<_>>();

    metadata.retain(|metadata| !cd_bin_file_names.contains(&metadata.full_path));

    metadata.sort_by(|a, b| a.file_name_no_ext.cmp(&b.file_name_no_ext));
    metadata
}

pub fn read_metadata(path: &Path) -> Option<RomMetadata> {
    let file_name = path.file_name().and_then(OsStr::to_str)?;
    process_file(file_name, path)
}

fn process_file(file_name: &str, path: &Path) -> Option<RomMetadata> {
    let ConsoleWithSize { console, file_size: raw_file_size } = Console::from_file(path)?;

    let file_name_no_ext = Path::new(file_name).with_extension("").to_string_lossy().to_string();
    let extension = extensions::from_path(file_name)?;

    let file_size = match extension.as_str() {
        "cue" => sega_cd_file_size(path).ok()?,
        _ => raw_file_size,
    };

    Some(RomMetadata { full_path: path.into(), file_name_no_ext, console, file_size })
}

fn sega_cd_file_size(cue_path: &Path) -> io::Result<u64> {
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

            let file_size = match extensions::from_path(path).as_deref() {
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
    pub fn spawn(rom_list: Arc<Mutex<Vec<RomMetadata>>>, egui_ctx: egui::Context) -> Self {
        let (scan_requests_sender, scan_requests_receiver) = mpsc::channel::<Vec<String>>();
        let scan_request_counter = Arc::new(AtomicU32::new(0));
        let scan_request_counter_handle = Arc::clone(&scan_request_counter);

        thread::spawn(move || {
            while let Ok(scan_request) = scan_requests_receiver.recv() {
                let new_rom_list = build(&scan_request);
                *rom_list.lock().unwrap() = new_rom_list;

                scan_request_counter.fetch_sub(1, Ordering::SeqCst);
                egui_ctx.request_repaint();
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
