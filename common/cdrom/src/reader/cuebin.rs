//! Code for loading and reading CD-ROM images in CUE/BIN format

#[cfg(test)]
mod tests;

use crate::cdtime::CdTime;
use crate::cue::{CueSheet, Track, TrackMode, TrackType};
use crate::{CdRomError, CdRomResult, cue};
use bincode::{Decode, Encode};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::LazyLock;
use std::{fs, io, mem};

#[derive(Debug, Clone, Encode, Decode)]
pub struct TrackMetadata {
    pub file_name: String,
    pub time_in_file: CdTime,
}

#[derive(Debug)]
struct CdRomFile<F: Read + Seek> {
    file: BufReader<F>,
    position: u64,
}

impl<F: Read + Seek> CdRomFile<F> {
    fn new(file: F) -> Self {
        Self { file: BufReader::new(file), position: 0 }
    }
}

#[derive(Debug)]
pub struct CdBinFiles<F: Read + Seek> {
    files: HashMap<String, CdRomFile<F>>,
    track_metadata: Vec<TrackMetadata>,
}

impl<F: Read + Seek> CdBinFiles<F> {
    pub fn empty() -> Self {
        Self { files: HashMap::new(), track_metadata: Vec::new() }
    }

    pub fn create<OpenFn, P: AsRef<Path>>(
        cue_path: P,
        bin_open_fn: OpenFn,
    ) -> CdRomResult<(Self, CueSheet)>
    where
        OpenFn: for<'a> Fn(&'a Path) -> io::Result<F>,
    {
        let cue_path = cue_path.as_ref();

        let (cue_sheet, track_metadata) = parse_cue(cue_path)?;

        let file_names: HashSet<_> =
            track_metadata.iter().map(|metadata| metadata.file_name.clone()).collect();

        let parent_dir = cue_path
            .parent()
            .ok_or_else(|| CdRomError::CueParentDir(cue_path.display().to_string()))?;

        let mut files = HashMap::with_capacity(file_names.len());
        for file_name in file_names {
            let file_path = parent_dir.join(Path::new(&file_name));
            let file = bin_open_fn(&file_path).map_err(|source| CdRomError::BinOpen {
                path: file_path.display().to_string(),
                source,
            })?;
            files.insert(file_name, CdRomFile::new(file));
        }

        let bin_files = Self { files, track_metadata };
        Ok((bin_files, cue_sheet))
    }

    pub fn read_sector(
        &mut self,
        track_number: u8,
        relative_sector_number: u32,
        out: &mut [u8],
    ) -> CdRomResult<()> {
        let metadata = &self.track_metadata[(track_number - 1) as usize];
        let CdRomFile { file: track_file, position } = self
            .files
            .get_mut(&metadata.file_name)
            .expect("Track file was not opened on load; this is a bug");

        let sector_number = metadata.time_in_file.to_sector_number() + relative_sector_number;
        let sector_addr = u64::from(sector_number) * crate::BYTES_PER_SECTOR;

        // Only seek if the file descriptor is not already at the desired position
        if *position != sector_addr {
            track_file.seek(SeekFrom::Start(sector_addr)).map_err(CdRomError::DiscReadIo)?;
        }

        track_file
            .read_exact(&mut out[..crate::BYTES_PER_SECTOR as usize])
            .map_err(CdRomError::DiscReadIo)?;
        *position = sector_addr + crate::BYTES_PER_SECTOR;

        Ok(())
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq, Eq))]
struct ParsedTrack {
    number: u8,
    mode: TrackMode,
    pregap_len: Option<CdTime>,
    pause_start: Option<CdTime>,
    track_start: CdTime,
}

#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq, Eq))]
struct ParsedFile {
    file_name: String,
    tracks: Vec<ParsedTrack>,
}

#[derive(Debug, Clone)]
struct CueParser {
    files: Vec<ParsedFile>,
    tracks: Vec<ParsedTrack>,
    current_file: Option<String>,
    current_track: Option<(u8, TrackMode)>,
    last_track_number: Option<u8>,
    pregap_len: Option<CdTime>,
    pause_start: Option<CdTime>,
    track_start: Option<CdTime>,
}

impl CueParser {
    fn new() -> Self {
        Self {
            files: vec![],
            tracks: vec![],
            current_file: None,
            current_track: None,
            last_track_number: None,
            pregap_len: None,
            pause_start: None,
            track_start: None,
        }
    }

    fn parse(mut self, file: &str) -> CdRomResult<Vec<ParsedFile>> {
        for line in file.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("FILE ") {
                self.parse_file_line(line)?;
            } else if trimmed.starts_with("TRACK ") {
                self.parse_track_line(line)?;
            } else if trimmed.starts_with("INDEX ") {
                self.parse_index_line(line)?;
            } else if trimmed.starts_with("PREGAP ") {
                self.parse_pregap_line(line)?;
            }
        }

        self.push_file()?;

        if self.files.is_empty() {
            return Err(CdRomError::CueParse("CUE file has no tracks".into()));
        }

        Ok(self.files)
    }

    #[allow(clippy::items_after_statements)]
    fn parse_file_line(&mut self, line: &str) -> CdRomResult<()> {
        self.push_file()?;

        static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"FILE "(.*)" BINARY"#).unwrap());

        let captures =
            RE.captures(line).ok_or_else(|| CdRomError::CueInvalidFileLine(line.into()))?;
        let file_name = captures.get(1).unwrap();
        self.current_file = Some(file_name.as_str().into());

        Ok(())
    }

    #[allow(clippy::items_after_statements)]
    fn parse_track_line(&mut self, line: &str) -> CdRomResult<()> {
        self.push_track()?;

        static RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"TRACK ([^ ]*) ([^ ]*)").unwrap());

        let captures =
            RE.captures(line).ok_or_else(|| CdRomError::CueInvalidTrackLine(line.into()))?;
        let track_number = captures
            .get(1)
            .unwrap()
            .as_str()
            .parse::<u8>()
            .map_err(|_| CdRomError::CueInvalidTrackLine(line.into()))?;
        let mode = captures
            .get(2)
            .unwrap()
            .as_str()
            .parse::<TrackMode>()
            .map_err(|_| CdRomError::CueInvalidTrackLine(line.into()))?;

        self.current_track = Some((track_number, mode));

        Ok(())
    }

    fn parse_index_line(&mut self, line: &str) -> CdRomResult<()> {
        static RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"INDEX ([^ ]*) ([^ ]*)").unwrap());

        let captures =
            RE.captures(line).ok_or_else(|| CdRomError::CueInvalidIndexLine(line.into()))?;
        let index_number = captures.get(1).unwrap();
        let start_time = captures
            .get(2)
            .unwrap()
            .as_str()
            .parse::<CdTime>()
            .map_err(|_| CdRomError::CueInvalidIndexLine(line.into()))?;

        match index_number.as_str() {
            "00" => {
                self.pause_start = Some(start_time);
            }
            "01" => {
                self.track_start = Some(start_time);
            }
            _ => {
                return Err(CdRomError::CueInvalidIndexLine(line.into()));
            }
        }

        Ok(())
    }

    fn parse_pregap_line(&mut self, line: &str) -> CdRomResult<()> {
        static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"PREGAP ([^ ]*)").unwrap());

        let captures =
            RE.captures(line).ok_or_else(|| CdRomError::CueInvalidPregapLine(line.into()))?;
        let pregap_len = captures
            .get(1)
            .unwrap()
            .as_str()
            .parse::<CdTime>()
            .map_err(|_| CdRomError::CueInvalidPregapLine(line.into()))?;

        self.pregap_len = Some(pregap_len);

        Ok(())
    }

    fn push_file(&mut self) -> CdRomResult<()> {
        self.push_track()?;

        let Some(current_file) = self.current_file.take() else { return Ok(()) };

        if self.tracks.is_empty() {
            return Err(CdRomError::CueParse(format!(
                "No tracks listed for file '{current_file}'"
            )));
        }

        self.files
            .push(ParsedFile { file_name: current_file, tracks: mem::take(&mut self.tracks) });

        Ok(())
    }

    fn push_track(&mut self) -> CdRomResult<()> {
        let Some((track_number, track_mode)) = self.current_track.take() else {
            return Ok(());
        };

        match self.last_track_number {
            None => {
                if track_number != 1 {
                    return Err(CdRomError::CueParse(format!(
                        "Expected first track to be 01, was {track_number}"
                    )));
                }
            }
            Some(last_track_number) => {
                if track_number != last_track_number + 1 {
                    return Err(CdRomError::CueParse(format!(
                        "Tracks out of order; track {track_number} after {last_track_number}"
                    )));
                }
            }
        }
        self.last_track_number = Some(track_number);

        let Some(track_start) = self.track_start.take() else {
            return Err(CdRomError::CueParse(format!(
                "No start time found for track {track_number}"
            )));
        };

        self.tracks.push(ParsedTrack {
            number: track_number,
            mode: track_mode,
            pregap_len: self.pregap_len.take(),
            pause_start: self.pause_start.take(),
            track_start,
        });

        Ok(())
    }
}

fn parse_cue<P: AsRef<Path>>(cue_path: P) -> CdRomResult<(CueSheet, Vec<TrackMetadata>)> {
    let cue_path = cue_path.as_ref();

    let cue_file = fs::read_to_string(cue_path)
        .map_err(|source| CdRomError::CueOpen { path: cue_path.display().to_string(), source })?;
    let parsed_files = CueParser::new().parse(&cue_file)?;

    to_cue_sheet(parsed_files, cue_path)
}

fn to_cue_sheet(
    parsed_files: Vec<ParsedFile>,
    cue_path: &Path,
) -> CdRomResult<(CueSheet, Vec<TrackMetadata>)> {
    let cue_parent_dir = cue_path
        .parent()
        .ok_or_else(|| CdRomError::CueParentDir(cue_path.display().to_string()))?;

    let mut absolute_start_time = CdTime::ZERO;
    let mut tracks = Vec::new();
    let mut track_metadata = Vec::new();

    for ParsedFile { file_name, tracks: parsed_tracks } in parsed_files {
        let bin_path = cue_parent_dir.join(&file_name);

        let file_metadata = fs::metadata(&bin_path).map_err(|source| CdRomError::FsMetadata {
            path: bin_path.display().to_string(),
            source,
        })?;
        let file_len_bytes = file_metadata.len();
        let file_len_sectors = (file_len_bytes / crate::BYTES_PER_SECTOR) as u32;

        for i in 0..parsed_tracks.len() {
            let track = &parsed_tracks[i];

            let track_type = track.mode.to_type();
            let pregap_len = match track_type {
                TrackType::Data => {
                    // Data tracks always have a 2-second pregap
                    CdTime::new(0, 2, 0)
                }
                TrackType::Audio => track.pregap_len.unwrap_or(CdTime::ZERO),
            };
            let pause_len = track
                .pause_start
                .map_or(CdTime::ZERO, |pause_start| track.track_start - pause_start);

            let is_last_track_in_file = i == parsed_tracks.len() - 1;
            let data_end_time = if is_last_track_in_file {
                CdTime::from_sector_number(file_len_sectors)
            } else {
                let next_track = &parsed_tracks[i + 1];
                next_track.pause_start.unwrap_or(next_track.track_start)
            };

            let postgap_len = track_type.default_postgap_len();

            let padded_track_len =
                pregap_len + pause_len + (data_end_time - track.track_start) + postgap_len;
            tracks.push(Track {
                number: track.number,
                mode: track.mode,
                track_type,
                start_time: absolute_start_time,
                end_time: absolute_start_time + padded_track_len,
                pregap_len,
                pause_len,
                postgap_len,
            });
            track_metadata.push(TrackMetadata {
                file_name: file_name.clone(),
                time_in_file: track.pause_start.unwrap_or(track.track_start),
            });

            absolute_start_time += padded_track_len;
        }
    }

    cue::finalize_track_list(&mut tracks);

    log::trace!("Parsed cue sheet:\n{tracks:#?}");

    assert!(
        cue::tracks_are_continuous(&tracks),
        "Tracks in parsed CUE sheet are not continuous; this is a bug"
    );

    Ok((CueSheet::new(tracks), track_metadata))
}
