//! Code for parsing and representing CUE files

use crate::api::{DiscError, DiscResult};
use crate::cdrom;
use crate::cdrom::cdtime::CdTime;
use bincode::{Decode, Encode};
use regex::Regex;
use std::path::Path;
use std::str::FromStr;
use std::sync::OnceLock;
use std::{fs, mem};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum TrackType {
    Data,
    Audio,
}

impl FromStr for TrackType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "MODE1/2352" => Ok(Self::Data),
            "AUDIO" => Ok(Self::Audio),
            _ => Err(format!("unsupported CD track type: {s}")),
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct TrackMetadata {
    pub file_name: String,
    pub time_in_file: CdTime,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Track {
    pub number: u8,
    pub track_type: TrackType,
    pub start_time: CdTime,
    pub end_time: CdTime,
    pub pregap_len: CdTime,
    pub pause_len: CdTime,
    pub postgap_len: CdTime,
    pub metadata: TrackMetadata,
}

impl Track {
    pub fn effective_start_time(&self) -> CdTime {
        self.start_time + self.pregap_len + self.pause_len
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct CueSheet {
    tracks: Vec<Track>,
    track_start_times: Vec<CdTime>,
}

impl CueSheet {
    fn new(tracks: Vec<Track>) -> Self {
        assert!(!tracks.is_empty(), "track list must not be empty");

        let track_start_times = tracks.iter().map(|track| track.start_time).collect();

        Self { tracks, track_start_times }
    }

    pub fn track(&self, track_number: u8) -> &Track {
        &self.tracks[(track_number - 1) as usize]
    }

    pub fn tracks(&self) -> impl Iterator<Item = &Track> + '_ {
        self.tracks.iter()
    }

    pub fn last_track(&self) -> &Track {
        self.tracks.last().unwrap()
    }

    // Returns None if `time` is past the end of the disc
    pub fn find_track_by_time(&self, time: CdTime) -> Option<&Track> {
        match self.track_start_times.binary_search(&time) {
            Ok(i) => Some(&self.tracks[i]),
            Err(i) => {
                if i < self.tracks.len() {
                    Some(&self.tracks[i - 1])
                } else {
                    let last_track = self.last_track();
                    (time <= last_track.end_time).then_some(last_track)
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
struct ParsedTrack {
    number: u8,
    track_type: TrackType,
    pregap_len: Option<CdTime>,
    pause_start: Option<CdTime>,
    track_start: CdTime,
}

#[derive(Debug, Clone)]
struct ParsedFile {
    file_name: String,
    tracks: Vec<ParsedTrack>,
}

#[derive(Debug, Clone)]
struct CueParser {
    files: Vec<ParsedFile>,
    tracks: Vec<ParsedTrack>,
    current_file: Option<String>,
    current_track: Option<(u8, TrackType)>,
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

    fn parse(mut self, file: &str) -> DiscResult<Vec<ParsedFile>> {
        for line in file.lines() {
            if line.starts_with("FILE ") {
                self.parse_file_line(line)?;
            } else if line.starts_with("  TRACK ") {
                self.parse_track_line(line)?;
            } else if line.starts_with("    INDEX ") {
                self.parse_index_line(line)?;
            } else if line.starts_with("    PREGAP ") {
                self.parse_pregap_line(line)?;
            }
        }

        self.push_file()?;

        if self.files.is_empty() {
            return Err(DiscError::CueParse("CUE file has no tracks".into()));
        }

        Ok(self.files)
    }

    fn parse_file_line(&mut self, line: &str) -> DiscResult<()> {
        static RE: OnceLock<Regex> = OnceLock::new();

        self.push_file()?;

        let re = RE.get_or_init(|| Regex::new(r#"FILE "(.*)" BINARY"#).unwrap());
        let captures =
            re.captures(line).ok_or_else(|| DiscError::CueInvalidFileLine(line.into()))?;
        let file_name = captures.get(1).unwrap();
        self.current_file = Some(file_name.as_str().into());

        Ok(())
    }

    fn parse_track_line(&mut self, line: &str) -> DiscResult<()> {
        static RE: OnceLock<Regex> = OnceLock::new();

        self.push_track()?;

        let re = RE.get_or_init(|| Regex::new(r"TRACK ([^ ]*) ([^ ]*)").unwrap());
        let captures =
            re.captures(line).ok_or_else(|| DiscError::CueInvalidTrackLine(line.into()))?;
        let track_number = captures
            .get(1)
            .unwrap()
            .as_str()
            .parse::<u8>()
            .map_err(|_| DiscError::CueInvalidTrackLine(line.into()))?;
        let mode = captures
            .get(2)
            .unwrap()
            .as_str()
            .parse::<TrackType>()
            .map_err(|_| DiscError::CueInvalidTrackLine(line.into()))?;

        self.current_track = Some((track_number, mode));

        Ok(())
    }

    fn parse_index_line(&mut self, line: &str) -> DiscResult<()> {
        static RE: OnceLock<Regex> = OnceLock::new();

        let re = RE.get_or_init(|| Regex::new(r"INDEX ([^ ]*) ([^ ]*)").unwrap());
        let captures =
            re.captures(line).ok_or_else(|| DiscError::CueInvalidIndexLine(line.into()))?;
        let index_number = captures.get(1).unwrap();
        let start_time = captures
            .get(2)
            .unwrap()
            .as_str()
            .parse::<CdTime>()
            .map_err(|_| DiscError::CueInvalidIndexLine(line.into()))?;

        match index_number.as_str() {
            "00" => {
                self.pause_start = Some(start_time);
            }
            "01" => {
                self.track_start = Some(start_time);
            }
            _ => {
                return Err(DiscError::CueInvalidIndexLine(line.into()));
            }
        }

        Ok(())
    }

    fn parse_pregap_line(&mut self, line: &str) -> DiscResult<()> {
        static RE: OnceLock<Regex> = OnceLock::new();

        let re = RE.get_or_init(|| Regex::new(r"PREGAP ([^ ]*)").unwrap());
        let captures =
            re.captures(line).ok_or_else(|| DiscError::CueInvalidPregapLine(line.into()))?;
        let pregap_len = captures
            .get(1)
            .unwrap()
            .as_str()
            .parse::<CdTime>()
            .map_err(|_| DiscError::CueInvalidPregapLine(line.into()))?;

        self.pregap_len = Some(pregap_len);

        Ok(())
    }

    fn push_file(&mut self) -> DiscResult<()> {
        self.push_track()?;

        let Some(current_file) = self.current_file.take() else { return Ok(()) };

        if self.tracks.is_empty() {
            return Err(DiscError::CueParse(format!("No tracks listed for file '{current_file}'")));
        }

        self.files
            .push(ParsedFile { file_name: current_file, tracks: mem::take(&mut self.tracks) });

        Ok(())
    }

    fn push_track(&mut self) -> DiscResult<()> {
        if let Some((track_number, track_type)) = self.current_track.take() {
            match self.last_track_number {
                None => {
                    if track_number != 1 {
                        return Err(DiscError::CueParse(format!(
                            "Expected first track to be 01, was {track_number}"
                        )));
                    }
                }
                Some(last_track_number) => {
                    if track_number != last_track_number + 1 {
                        return Err(DiscError::CueParse(format!(
                            "Tracks out of order; track {track_number} after {last_track_number}"
                        )));
                    }
                }
            }
            self.last_track_number = Some(track_number);

            let Some(track_start) = self.track_start.take() else {
                return Err(DiscError::CueParse(format!(
                    "No start time found for track {track_number}"
                )));
            };

            self.tracks.push(ParsedTrack {
                number: track_number,
                track_type,
                pregap_len: self.pregap_len.take(),
                pause_start: self.pause_start.take(),
                track_start,
            });
        }

        Ok(())
    }
}

pub fn parse<P: AsRef<Path>>(cue_path: P) -> DiscResult<CueSheet> {
    let cue_path = cue_path.as_ref();

    let cue_file = fs::read_to_string(cue_path)
        .map_err(|source| DiscError::CueOpen { path: cue_path.display().to_string(), source })?;
    let parsed_files = CueParser::new().parse(&cue_file)?;

    to_cue_sheet(parsed_files, cue_path)
}

fn to_cue_sheet(parsed_files: Vec<ParsedFile>, cue_path: &Path) -> DiscResult<CueSheet> {
    let cue_parent_dir =
        cue_path.parent().ok_or_else(|| DiscError::CueParentDir(cue_path.display().to_string()))?;

    let mut absolute_start_time = CdTime::ZERO;
    let mut tracks = Vec::new();

    for ParsedFile { file_name, tracks: parsed_tracks } in parsed_files {
        let bin_path = cue_parent_dir.join(&file_name);

        let file_metadata = fs::metadata(&bin_path).map_err(|source| DiscError::FsMetadata {
            path: bin_path.display().to_string(),
            source,
        })?;
        let file_len_bytes = file_metadata.len();
        let file_len_sectors = (file_len_bytes / cdrom::BYTES_PER_SECTOR) as u32;

        for i in 0..parsed_tracks.len() {
            let track = &parsed_tracks[i];

            let pregap_len = match track.track_type {
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

            let postgap_len = match track.track_type {
                // Data tracks always have a 2-second postgap
                TrackType::Data => CdTime::new(0, 2, 0),
                TrackType::Audio => CdTime::ZERO,
            };

            let padded_track_len =
                pregap_len + pause_len + (data_end_time - track.track_start) + postgap_len;
            tracks.push(Track {
                number: track.number,
                track_type: track.track_type,
                start_time: absolute_start_time,
                end_time: absolute_start_time + padded_track_len,
                pregap_len,
                pause_len,
                postgap_len,
                metadata: TrackMetadata {
                    file_name: file_name.clone(),
                    time_in_file: track.pause_start.unwrap_or(track.track_start),
                },
            });
            absolute_start_time += padded_track_len;
        }
    }

    // The final track always has a 2-second postgap
    let last_track = tracks.last_mut().unwrap();
    if last_track.postgap_len == CdTime::ZERO {
        last_track.postgap_len = CdTime::new(0, 2, 0);
        last_track.end_time += CdTime::new(0, 2, 0);
    }

    log::trace!("Parsed cue sheet:\n{tracks:#?}");

    assert!(
        tracks_are_continuous(&tracks),
        "Tracks in parsed CUE sheet are not continuous; this is a bug"
    );

    Ok(CueSheet::new(tracks))
}

fn tracks_are_continuous(tracks: &[Track]) -> bool {
    if tracks[0].start_time != CdTime::ZERO {
        return false;
    }

    for window in tracks.windows(2) {
        let [track, next] = window else { unreachable!("windows(2)") };
        if next.start_time != track.end_time {
            return false;
        }
    }

    true
}
