//! Code for loading and reading CD-ROM images in CHD format

use crate::api::{DiscError, DiscResult};
use crate::cdrom;
use crate::cdrom::cdtime::CdTime;
use crate::cdrom::cue;
use crate::cdrom::cue::{CueSheet, Track, TrackType};
use chd::iter::LendingIterator;
use chd::Chd;
use std::fmt::{Debug, Formatter};
use std::io::{Read, Seek};

#[derive(Debug, Clone, Copy)]
struct CdMetadata {
    track_number: u8,
    track_type: TrackType,
    frames: u32,
    pregap_frames: u32,
}

impl CdMetadata {
    fn parse_from(ascii_bytes: Vec<u8>) -> Option<Self> {
        let text = String::from_utf8(ascii_bytes).ok()?;

        let mut track_number: Option<u8> = None;
        let mut track_type: Option<TrackType> = None;
        let mut frames: Option<u32> = None;
        let mut pregap_frames: u32 = 0;
        for token in text.split(' ') {
            let Some((key, value)) = token.split_once(':') else { continue };

            match key {
                "TRACK" => track_number = Some(value.parse().ok()?),
                "TYPE" => match value {
                    "MODE1/2352" | "MODE1_RAW" => track_type = Some(TrackType::Data),
                    "AUDIO" => track_type = Some(TrackType::Audio),
                    _ => return None,
                },
                "FRAMES" => frames = Some(value.parse().ok()?),
                "PREGAP" => pregap_frames = value.parse().ok()?,
                _ => {}
            }
        }

        Some(Self {
            track_number: track_number?,
            track_type: track_type?,
            frames: frames?,
            pregap_frames,
        })
    }
}

pub struct ChdFile<F: Read + Seek> {
    chd: Chd<F>,
    cue: CueSheet,
    track_start_frames: Vec<u32>,
    compressed_buffer: Vec<u8>,
    decompressed_buffer: Vec<u8>,
    current_hunk_number: u32,
}

impl<F: Read + Seek> ChdFile<F> {
    pub fn open(f: F) -> DiscResult<(Self, CueSheet)> {
        let mut chd = Chd::open(f, None)?;

        // Parse TOC info from CHD metadata
        let mut metadata_iter = chd.metadata();
        let mut cd_metadata_list = Vec::new();
        while let Some(mut metadata_entry) = metadata_iter.next() {
            let metadata = metadata_entry.read()?;

            let Some(cd_metadata) = CdMetadata::parse_from(metadata.value.clone()) else {
                let value_str = String::from_utf8_lossy(&metadata.value).to_string();
                return Err(DiscError::ChdHeaderParseError { metadata_value: value_str });
            };

            cd_metadata_list.push(cd_metadata);
        }

        cd_metadata_list.sort_by_key(|metadata| metadata.track_number);
        validate_track_numbers(&cd_metadata_list)?;

        // Use parsed info to build the TOC
        let mut tracks = Vec::new();
        let mut track_start_frames = Vec::with_capacity(cd_metadata_list.len());
        let mut current_start_time = CdTime::ZERO;
        let mut current_frame = 0;
        for cd_metadata in cd_metadata_list {
            let pregap_len = match cd_metadata.track_type {
                TrackType::Data => {
                    // Data tracks always have a 2-second pregap
                    CdTime::new(0, 2, 0)
                }
                TrackType::Audio => CdTime::from_frames(cd_metadata.pregap_frames),
            };

            let postgap_len = cd_metadata.track_type.default_postgap_len();

            let track_len = CdTime::from_frames(cd_metadata.frames);
            let padded_track_len = pregap_len + track_len + postgap_len;

            tracks.push(Track {
                number: cd_metadata.track_number,
                track_type: cd_metadata.track_type,
                start_time: current_start_time,
                end_time: current_start_time + padded_track_len,
                pregap_len,
                pause_len: CdTime::ZERO,
                postgap_len,
            });
            track_start_frames.push(current_frame);

            current_start_time += padded_track_len;

            // CHD pads all tracks to a multiple of 4 frames
            current_frame += cd_metadata.frames;
            if current_frame % 4 != 0 {
                current_frame = (current_frame & !3) + 4;
            }
        }

        cue::finalize_track_list(&mut tracks);

        assert!(
            cue::tracks_are_continuous(&tracks),
            "CHD tracks are not continuous; this is a bug"
        );

        println!("start frames: {track_start_frames:?}");

        let compressed_buffer = Vec::with_capacity(chd.header().hunk_size() as usize);
        let decompressed_buffer = chd.get_hunksized_buffer();

        let cue_sheet = CueSheet::new(tracks);
        let chd_file = Self {
            chd,
            cue: cue_sheet.clone(),
            track_start_frames,
            compressed_buffer,
            decompressed_buffer,
            current_hunk_number: u32::MAX,
        };

        Ok((chd_file, cue_sheet))
    }

    pub fn read_sector(
        &mut self,
        track_number: u8,
        relative_sector_number: u32,
        out: &mut [u8],
    ) -> DiscResult<()> {
        let track_start_frame = self.track_start_frames[(track_number - 1) as usize];
        let sector_number = track_start_frame + relative_sector_number;

        let hunk_size = self.chd.header().hunk_size();
        let unit_bytes = self.chd.header().unit_bytes();
        let hunk_size_sectors = hunk_size / unit_bytes;

        let hunk_number = sector_number / hunk_size_sectors;
        let hunk_offset_sectors = sector_number % hunk_size_sectors;
        let hunk_offset_bytes = hunk_offset_sectors * unit_bytes;

        // Only load hunk if necessary
        if hunk_number != self.current_hunk_number {
            let mut hunk = self.chd.hunk(hunk_number)?;

            self.compressed_buffer.clear();
            hunk.read_hunk_in(&mut self.compressed_buffer, &mut self.decompressed_buffer)?;

            self.current_hunk_number = hunk_number;
        }

        out[..cdrom::BYTES_PER_SECTOR as usize].copy_from_slice(
            &self.decompressed_buffer[hunk_offset_bytes as usize
                ..(hunk_offset_bytes + cdrom::BYTES_PER_SECTOR as u32) as usize],
        );

        if self.cue.track(track_number).track_type == TrackType::Audio {
            // CHD audio tracks decompress into big-endian audio samples for some reason. Swap all
            // the bytes to make them little-endian to match the CD-DA format
            for chunk in out[..cdrom::BYTES_PER_SECTOR as usize].chunks_exact_mut(2) {
                chunk.swap(0, 1);
            }
        }

        Ok(())
    }
}

impl<F: Read + Seek> Debug for ChdFile<F> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "ChdFile {{ current_hunk_number: {} }}", self.current_hunk_number)
    }
}

fn validate_track_numbers(cd_metadata_list: &[CdMetadata]) -> DiscResult<()> {
    for (i, metadata) in cd_metadata_list.iter().enumerate() {
        if metadata.track_number != (i + 1) as u8 {
            let track_numbers =
                cd_metadata_list.iter().map(|metadata| metadata.track_number).collect();
            return Err(DiscError::ChdInvalidTrackList { track_numbers });
        }
    }

    Ok(())
}
