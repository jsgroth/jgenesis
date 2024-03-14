//! Code for representing CUE files / TOC info

use crate::cdtime::CdTime;
use bincode::{Decode, Encode};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum TrackType {
    Data,
    Audio,
}

impl TrackType {
    #[must_use]
    pub fn default_postgap_len(self) -> CdTime {
        match self {
            // Data tracks always have a 2-second postgap
            Self::Data => CdTime::new(0, 2, 0),
            Self::Audio => CdTime::ZERO,
        }
    }
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
pub struct Track {
    pub number: u8,
    pub track_type: TrackType,
    pub start_time: CdTime,
    pub end_time: CdTime,
    pub pregap_len: CdTime,
    pub pause_len: CdTime,
    pub postgap_len: CdTime,
}

impl Track {
    #[must_use]
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
    /// Create a new `CueSheet` from the given track list.
    ///
    /// # Panics
    ///
    /// This function will panic if the track list is empty.
    #[must_use]
    pub(crate) fn new(tracks: Vec<Track>) -> Self {
        assert!(!tracks.is_empty(), "track list must not be empty");

        let track_start_times = tracks.iter().map(|track| track.start_time).collect();

        Self { tracks, track_start_times }
    }

    #[must_use]
    pub fn track(&self, track_number: u8) -> &Track {
        &self.tracks[(track_number - 1) as usize]
    }

    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn last_track(&self) -> &Track {
        self.tracks.last().unwrap()
    }

    /// Find the track containing the specified time. Returns `None` if the time is past the end of
    /// the disc.
    #[must_use]
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

#[must_use]
pub(crate) fn tracks_are_continuous(tracks: &[Track]) -> bool {
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

pub(crate) fn finalize_track_list(tracks: &mut [Track]) {
    // The final track always has a 2-second postgap
    let last_track = tracks.last_mut().unwrap();
    if last_track.postgap_len == CdTime::ZERO {
        last_track.postgap_len = CdTime::new(0, 2, 0);
        last_track.end_time += CdTime::new(0, 2, 0);
    }
}
