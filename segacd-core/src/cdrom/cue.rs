use anyhow::anyhow;
use bincode::{Decode, Encode};
use regex::Regex;
use std::ops::{Add, Sub};
use std::str::FromStr;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, Encode, Decode)]
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

    pub fn to_sector_number(self) -> u32 {
        (u32::from(Self::SECONDS_PER_MINUTE) * u32::from(self.minutes) + u32::from(self.seconds))
            * u32::from(Self::FRAMES_PER_SECOND)
            + u32::from(self.frames)
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

fn add(a: u8, b: u8, overflow: bool, base: u8) -> (u8, bool) {
    let sum = a + b + u8::from(overflow);
    (sum % base, sum >= base)
}

fn sub(a: u8, b: u8, overflow: bool, base: u8) -> (u8, bool) {
    let operand_r = b + u8::from(overflow);
    if a < operand_r { (base - a + operand_r, true) } else { (a - operand_r, false) }
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

#[derive(Debug, Clone, Copy)]
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
pub struct DataTrack {
    pub number: u8,
    pub file: String,
    pub time: CdTime,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct AudioTrack {
    pub number: u8,
    pub file: String,
    pub pregap: Pregap,
    pub time: CdTime,
}

#[derive(Debug, Clone)]
pub enum Track {
    Data(DataTrack),
    Audio(AudioTrack),
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub enum Pregap {
    None,
    StartTime(CdTime),
    Silence(CdTime),
}

#[derive(Default)]
pub struct CueParser {
    tracks: Vec<Track>,
    current_file: Option<String>,
    current_track: Option<(u8, TrackType)>,
    current_pregap: Option<Pregap>,
    current_track_time: Option<CdTime>,
}

impl CueParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn parse(mut self, file: &str) -> anyhow::Result<Vec<Track>> {
        static FILE_RE: OnceLock<Regex> = OnceLock::new();
        static TRACK_RE: OnceLock<Regex> = OnceLock::new();
        static INDEX_RE: OnceLock<Regex> = OnceLock::new();

        for line in file.lines() {
            if line.starts_with("FILE ") {
                self.push_track()?;

                let file_re = FILE_RE.get_or_init(|| Regex::new(r#"FILE "(.*)" BINARY"#).unwrap());

                let file_captures =
                    file_re.captures(line).ok_or_else(|| anyhow!("Invalid line: {line}"))?;
                self.current_file = Some(file_captures.get(1).unwrap().as_str().into());
                self.current_track = None;
                self.current_pregap = None;
                self.current_track_time = None;
            } else if line.starts_with("  TRACK ") {
                self.push_track()?;

                let track_re =
                    TRACK_RE.get_or_init(|| Regex::new(r"TRACK ([^ ]*) ([^ ]*)").unwrap());

                let track_captures =
                    track_re.captures(line).ok_or_else(|| anyhow!("Invalid line: {line}"))?;
                let track_number = track_captures.get(1).unwrap();
                let track_type = track_captures.get(2).unwrap();

                let track_number = track_number.as_str().parse()?;
                let track_type = track_type.as_str().parse().map_err(anyhow::Error::msg)?;
                self.current_track = Some((track_number, track_type));
                self.current_pregap = None;
                self.current_track_time = None;
            } else if line.starts_with("    INDEX ") {
                let index_re =
                    INDEX_RE.get_or_init(|| Regex::new(r"INDEX ([^ ]*) ([^ ]*)").unwrap());

                let index_captures =
                    index_re.captures(line).ok_or_else(|| anyhow!("Invalid line: {line}"))?;
                let index_number = index_captures.get(1).unwrap();
                let index_time = index_captures.get(2).unwrap();

                let index_time: CdTime = index_time.as_str().parse().map_err(anyhow::Error::msg)?;
                match index_number.as_str() {
                    "00" => {
                        self.current_pregap = Some(Pregap::StartTime(index_time));
                    }
                    "01" => {
                        self.current_track_time = Some(index_time);
                    }
                    _ => panic!("Unexpected index number: {}", index_number.as_str()),
                }
            } else if line.starts_with("    PREGAP ") {
                static PREGAP_RE: OnceLock<Regex> = OnceLock::new();
                let pregap_re = PREGAP_RE.get_or_init(|| Regex::new(r"PREGAP ([^ ]*)").unwrap());

                let pregap_captures =
                    pregap_re.captures(line).ok_or_else(|| anyhow!("Invalid line: {line}"))?;
                let pregap_interval = pregap_captures.get(1).unwrap();
                let pregap_interval =
                    pregap_interval.as_str().parse().map_err(anyhow::Error::msg)?;
                self.current_pregap = Some(Pregap::Silence(pregap_interval));
            }
        }

        self.finish()
    }

    fn push_track(&mut self) -> anyhow::Result<()> {
        let Some((track_number, track_type)) = self.current_track else { return Ok(()) };

        let file = self
            .current_file
            .clone()
            .ok_or_else(|| anyhow!("Track with no file: {track_number}"))?;
        let track_time = self
            .current_track_time
            .ok_or_else(|| anyhow!("Track with no track time: {track_number}"))?;

        match track_type {
            TrackType::Data => {
                self.tracks.push(Track::Data(DataTrack {
                    number: track_number,
                    file,
                    time: track_time,
                }));
            }
            TrackType::Audio => {
                let pregap = self.current_pregap.unwrap_or(Pregap::None);
                self.tracks.push(Track::Audio(AudioTrack {
                    number: track_number,
                    file,
                    pregap,
                    time: track_time,
                }));
            }
        }

        Ok(())
    }

    fn finish(mut self) -> anyhow::Result<Vec<Track>> {
        self.push_track()?;
        Ok(self.tracks)
    }
}
