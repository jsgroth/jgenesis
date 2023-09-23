use crate::cdrom;
use anyhow::anyhow;
use bincode::{Decode, Encode};
use regex::Regex;
use std::ops::{Add, AddAssign, Sub, SubAssign};
use std::path::Path;
use std::str::FromStr;
use std::sync::OnceLock;
use std::{fs, mem};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct CdTime {
    pub minutes: u8,
    pub seconds: u8,
    pub frames: u8,
}

impl CdTime {
    pub const ZERO: Self = Self { minutes: 0, seconds: 0, frames: 0 };

    pub const MAX_MINUTES: u8 = 61;
    pub const SECONDS_PER_MINUTE: u8 = 60;
    pub const FRAMES_PER_SECOND: u8 = 75;

    pub fn new(minutes: u8, seconds: u8, frames: u8) -> Self {
        assert!(minutes < Self::MAX_MINUTES, "Minutes must be less than {}", Self::MAX_MINUTES);
        assert!(
            seconds < Self::SECONDS_PER_MINUTE,
            "Seconds must be less than {}",
            Self::SECONDS_PER_MINUTE
        );
        assert!(
            frames < Self::FRAMES_PER_SECOND,
            "Frames must be less than {}",
            Self::FRAMES_PER_SECOND
        );

        Self { minutes, seconds, frames }
    }

    pub fn to_sector_number(self) -> u32 {
        (u32::from(Self::SECONDS_PER_MINUTE) * u32::from(self.minutes) + u32::from(self.seconds))
            * u32::from(Self::FRAMES_PER_SECOND)
            + u32::from(self.frames)
    }

    pub fn from_sector_number(sector_number: u32) -> Self {
        // All Sega CD sector numbers are less than 270,000
        assert!(sector_number < 270000, "Invalid sector number: {sector_number}");

        let frames = sector_number % u32::from(Self::FRAMES_PER_SECOND);
        let seconds = (sector_number / u32::from(Self::FRAMES_PER_SECOND))
            % u32::from(Self::SECONDS_PER_MINUTE);
        let minutes = sector_number
            / (u32::from(Self::FRAMES_PER_SECOND) * u32::from(Self::SECONDS_PER_MINUTE));

        Self::new(minutes as u8, seconds as u8, frames as u8)
    }
}

impl Add for CdTime {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        let (frames, carried) = add(self.frames, rhs.frames, false, Self::FRAMES_PER_SECOND);
        let (seconds, carried) = add(self.seconds, rhs.seconds, carried, Self::SECONDS_PER_MINUTE);
        let (minutes, _) = add(self.minutes, rhs.minutes, carried, Self::MAX_MINUTES);

        Self { minutes, seconds, frames }
    }
}

impl AddAssign for CdTime {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for CdTime {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        let (frames, borrowed) = sub(self.frames, rhs.frames, false, Self::FRAMES_PER_SECOND);
        let (seconds, borrowed) =
            sub(self.seconds, rhs.seconds, borrowed, Self::SECONDS_PER_MINUTE);
        let (minutes, _) = sub(self.minutes, rhs.minutes, borrowed, Self::MAX_MINUTES);

        Self { minutes, seconds, frames }
    }
}

impl SubAssign for CdTime {
    fn sub_assign(&mut self, rhs: Self) {
        *self = *self - rhs;
    }
}

fn add(a: u8, b: u8, overflow: bool, base: u8) -> (u8, bool) {
    let sum = a + b + u8::from(overflow);
    (sum % base, sum >= base)
}

fn sub(a: u8, b: u8, overflow: bool, base: u8) -> (u8, bool) {
    let operand_r = b + u8::from(overflow);
    if a < operand_r { (base - (operand_r - a), true) } else { (a - operand_r, false) }
}

impl FromStr for CdTime {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = s.as_bytes();
        if bytes.len() != 8 {
            return Err(format!("Invalid time length: {}", bytes.len()));
        }

        if bytes[2] != b':' || bytes[5] != b':' {
            return Err(format!("Unexpected time format: {s}"));
        }

        let err_fn = |_err| format!("Invalid time string: {s}");
        let minutes: u8 = s[0..2].parse().map_err(err_fn)?;
        let seconds: u8 = s[3..5].parse().map_err(err_fn)?;
        let frames: u8 = s[6..8].parse().map_err(err_fn)?;

        Ok(CdTime { minutes, seconds, frames })
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
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
    pub relative_start_time: CdTime,
    pub relative_end_time: CdTime,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Track {
    pub number: u8,
    pub track_type: TrackType,
    pub start_time: CdTime,
    pub end_time: CdTime,
    pub metadata: TrackMetadata,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct CueSheet {
    tracks: Vec<Track>,
}

impl CueSheet {
    fn new(tracks: Vec<Track>) -> Self {
        assert!(!tracks.is_empty(), "track list must not be empty");
        Self { tracks }
    }

    pub fn track(&self, track_number: u8) -> &Track {
        &self.tracks[(track_number - 1) as usize]
    }

    pub fn tracks(&self) -> impl Iterator<Item = &Track> + '_ {
        self.tracks.iter()
    }

    pub fn first_track(&self) -> &Track {
        &self.tracks[0]
    }

    pub fn last_track(&self) -> &Track {
        self.tracks.last().unwrap()
    }

    pub fn num_tracks(&self) -> u8 {
        self.tracks.len() as u8
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

    fn parse(mut self, file: &str) -> anyhow::Result<Vec<ParsedFile>> {
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
            return Err(anyhow!("CUE file has no tracks"));
        }

        Ok(self.files)
    }

    fn parse_file_line(&mut self, line: &str) -> anyhow::Result<()> {
        static RE: OnceLock<Regex> = OnceLock::new();

        self.push_file()?;

        let re = RE.get_or_init(|| Regex::new(r#"FILE "(.*)" BINARY"#).unwrap());
        let captures = re.captures(line).ok_or_else(|| anyhow!("Invalid file line: '{line}'"))?;
        let file_name = captures.get(1).unwrap();
        self.current_file = Some(file_name.as_str().into());

        Ok(())
    }

    fn parse_track_line(&mut self, line: &str) -> anyhow::Result<()> {
        static RE: OnceLock<Regex> = OnceLock::new();

        self.push_track()?;

        let re = RE.get_or_init(|| Regex::new(r"TRACK ([^ ]*) ([^ ]*)").unwrap());
        let captures = re.captures(line).ok_or_else(|| anyhow!("Invalid track line: '{line}'"))?;
        let track_number = captures
            .get(1)
            .unwrap()
            .as_str()
            .parse::<u8>()
            .map_err(|err| anyhow!("Invalid track number in line: '{line}': {err}"))?;
        let mode = captures
            .get(2)
            .unwrap()
            .as_str()
            .parse::<TrackType>()
            .map_err(|err| anyhow!("Invalid track mode in line: '{line}': {err}"))?;

        self.current_track = Some((track_number, mode));

        Ok(())
    }

    fn parse_index_line(&mut self, line: &str) -> anyhow::Result<()> {
        static RE: OnceLock<Regex> = OnceLock::new();

        let re = RE.get_or_init(|| Regex::new(r"INDEX ([^ ]*) ([^ ]*)").unwrap());
        let captures = re.captures(line).ok_or_else(|| anyhow!("Invalid index line: '{line}'"))?;
        let index_number = captures.get(1).unwrap();
        let start_time = captures
            .get(2)
            .unwrap()
            .as_str()
            .parse::<CdTime>()
            .map_err(|err| anyhow!("Invalid start time in index line: '{line}': {err}"))?;

        match index_number.as_str() {
            "00" => {
                self.pause_start = Some(start_time);
            }
            "01" => {
                self.track_start = Some(start_time);
            }
            _ => {
                return Err(anyhow!("Invalid index number in line: '{line}'"));
            }
        }

        Ok(())
    }

    fn parse_pregap_line(&mut self, line: &str) -> anyhow::Result<()> {
        static RE: OnceLock<Regex> = OnceLock::new();

        let re = RE.get_or_init(|| Regex::new(r"PREGAP ([^ ]*)").unwrap());
        let captures = re.captures(line).ok_or_else(|| anyhow!("Invalid pregap line: '{line}'"))?;
        let pregap_len = captures
            .get(1)
            .unwrap()
            .as_str()
            .parse::<CdTime>()
            .map_err(|err| anyhow!("Invalid length in pregap line: '{line}': {err}"))?;

        self.pregap_len = Some(pregap_len);

        Ok(())
    }

    fn push_file(&mut self) -> anyhow::Result<()> {
        self.push_track()?;

        let Some(current_file) = self.current_file.take() else { return Ok(()) };

        if self.tracks.is_empty() {
            return Err(anyhow!("No tracks listed for file '{current_file}'"));
        }

        self.files
            .push(ParsedFile { file_name: current_file, tracks: mem::take(&mut self.tracks) });

        Ok(())
    }

    fn push_track(&mut self) -> anyhow::Result<()> {
        if let Some((track_number, track_type)) = self.current_track.take() {
            match self.last_track_number {
                None => {
                    if track_number != 1 {
                        return Err(anyhow!("Expected first track to be 01, was {track_number}"));
                    }
                }
                Some(last_track_number) => {
                    if track_number != last_track_number + 1 {
                        return Err(anyhow!(
                            "Tracks out of order; track {track_number} after {last_track_number}"
                        ));
                    }
                }
            }
            self.last_track_number = Some(track_number);

            let Some(track_start) = self.track_start.take() else {
                return Err(anyhow!("No start time found for track {track_number}"));
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

pub fn parse<P: AsRef<Path>>(cue_path: P) -> anyhow::Result<CueSheet> {
    let cue_path = cue_path.as_ref();

    let cue_file = fs::read_to_string(cue_path)?;
    let parsed_files = CueParser::new().parse(&cue_file)?;

    to_cue_sheet(parsed_files, cue_path)
}

fn to_cue_sheet(parsed_files: Vec<ParsedFile>, cue_path: &Path) -> anyhow::Result<CueSheet> {
    let cue_parent_dir = cue_path.parent().ok_or_else(|| {
        anyhow!("Unable to determine parent directory of CUE path: {}", cue_path.display())
    })?;

    let mut absolute_start_time = CdTime::ZERO;
    let mut tracks = Vec::new();

    for ParsedFile { file_name, tracks: parsed_tracks } in parsed_files {
        let bin_path = cue_parent_dir.join(&file_name);

        let file_metadata = fs::metadata(&bin_path).map_err(|err| {
            anyhow!("Unable to get file metadata for file: '{}': {err}", bin_path.display())
        })?;
        let file_len_bytes = file_metadata.len();
        let file_len_sectors = (file_len_bytes / cdrom::BYTES_PER_SECTOR) as u32;

        for i in 0..parsed_tracks.len() {
            let track = &parsed_tracks[i];
            let relative_start_time = track.track_start;
            let pregap_len = if let Some(pregap_len) = track.pregap_len {
                pregap_len
            } else if let Some(pause_start) = track.pause_start {
                track.track_start - pause_start
            } else {
                // Default to 2-second pregap if not specified
                CdTime::new(0, 2, 0)
            };
            let postgap_len = match track.track_type {
                // Data tracks always have a 2-second postgap
                TrackType::Data => CdTime::new(0, 2, 0),
                TrackType::Audio => CdTime::ZERO,
            };

            let is_last_track_in_file = i == parsed_tracks.len() - 1;
            let relative_end_time = if is_last_track_in_file {
                CdTime::from_sector_number(file_len_sectors)
            } else {
                let next_track = &parsed_tracks[i + 1];
                next_track.pause_start.unwrap_or(next_track.track_start)
            };

            absolute_start_time += pregap_len;
            tracks.push(Track {
                number: track.number,
                track_type: track.track_type,
                start_time: absolute_start_time,
                end_time: absolute_start_time + (relative_end_time - relative_start_time),
                metadata: TrackMetadata {
                    file_name: file_name.clone(),
                    relative_start_time,
                    relative_end_time,
                },
            });
            absolute_start_time += relative_end_time - relative_start_time + postgap_len;
        }
    }

    Ok(CueSheet { tracks })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cd_time_add() {
        // No carries
        assert_eq!(CdTime::new(10, 20, 30) + CdTime::new(15, 25, 35), CdTime::new(25, 45, 65));

        // Frames carry
        assert_eq!(CdTime::new(10, 20, 30) + CdTime::new(15, 25, 55), CdTime::new(25, 46, 10));

        // Seconds carry
        assert_eq!(CdTime::new(10, 20, 30) + CdTime::new(15, 55, 35), CdTime::new(26, 15, 65));
    }

    #[test]
    fn cd_time_sub() {
        // No borrows
        assert_eq!(CdTime::new(12, 13, 14) - CdTime::new(7, 7, 7), CdTime::new(5, 6, 7));

        // Frames borrow
        assert_eq!(CdTime::new(5, 4, 3) - CdTime::new(1, 1, 10), CdTime::new(4, 2, 68));

        // Seconds borrow
        assert_eq!(CdTime::new(15, 5, 39) - CdTime::new(13, 16, 25), CdTime::new(1, 49, 14));
    }
}
