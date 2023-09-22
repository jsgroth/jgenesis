use crate::cdrom::cue::{AudioTrack, DataTrack, Track};
use anyhow::anyhow;
use bincode::{Decode, Encode};
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::{cmp, iter};

const SECTOR_HEADER_LEN: u64 = 16;
const SECTOR_DATA_LEN: u64 = 2048;
// 16 header bytes + 2048 data bytes + 288 error detection/correction bytes
const BYTES_PER_SECTOR: u64 = 2352;

#[derive(Debug, Default, FakeEncode, FakeDecode)]
struct CdRomFiles(HashMap<String, File>);

impl Deref for CdRomFiles {
    type Target = HashMap<String, File>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for CdRomFiles {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Clone for CdRomFiles {
    fn clone(&self) -> Self {
        let mut files = HashMap::with_capacity(self.0.len());

        for (file_name, file) in &self.0 {
            let file = file.try_clone().expect("Unable to clone file descriptor");
            files.insert(file_name.clone(), file);
        }

        Self(files)
    }
}

impl CdRomFiles {
    fn create<P: AsRef<Path>>(
        data_track: &DataTrack,
        audio_tracks: &[Option<AudioTrack>],
        directory: P,
    ) -> anyhow::Result<Self> {
        let file_names: HashSet<_> = iter::once(data_track.file.clone())
            .chain(
                audio_tracks
                    .iter()
                    .filter_map(|track| track.as_ref().map(|track| track.file.clone())),
            )
            .collect();

        let directory = directory.as_ref();
        let mut files = HashMap::with_capacity(file_names.len());
        for file_name in file_names {
            let file_path = directory.join(Path::new(&file_name));
            let file = File::open(&file_path)
                .map_err(|err| anyhow!("Error opening file '{file_name}': {err}"))?;
            files.insert(file_name, file);
        }

        Ok(Self(files))
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct CdRom {
    data_track: DataTrack,
    audio_tracks: Vec<Option<AudioTrack>>,
    files: CdRomFiles,
}

impl CdRom {
    pub fn open<P: AsRef<Path>>(tracks: Vec<Track>, directory: P) -> anyhow::Result<Self> {
        let (data_tracks, audio_tracks) = partition_tracks(tracks)?;

        if data_tracks.len() != 1 {
            return Err(anyhow!("Expected exactly 1 data track, found {}", data_tracks.len()));
        }

        let data_track = data_tracks.into_iter().next().unwrap();
        if data_track.number != 1 {
            return Err(anyhow!(
                "Expected data track to be track 1, was track {}",
                data_track.number
            ));
        }

        let files = CdRomFiles::create(&data_track, &audio_tracks, directory)?;

        Ok(Self { data_track, audio_tracks, files })
    }

    // Read a 2352-byte sector from the data track
    pub fn read_data_sector(&mut self, sector_number: u32, out: &mut [u8]) -> anyhow::Result<()> {
        // TODO cache sectors
        let data_track_file = self
            .files
            .get_mut(&self.data_track.file)
            .expect("Invalid state: data track file not loaded");

        let file_addr = BYTES_PER_SECTOR * u64::from(sector_number);
        data_track_file.seek(SeekFrom::Start(file_addr))?;
        data_track_file.read_exact(&mut out[..BYTES_PER_SECTOR as usize])?;

        Ok(())
    }
}

fn partition_tracks(
    tracks: Vec<Track>,
) -> anyhow::Result<(Vec<DataTrack>, Vec<Option<AudioTrack>>)> {
    let mut data_tracks = Vec::new();
    let mut raw_audio_tracks = Vec::new();
    let mut max_track_number = 0;

    for track in tracks {
        match track {
            Track::Data(data_track) => {
                data_tracks.push(data_track);
            }
            Track::Audio(audio_track) => {
                max_track_number = cmp::max(max_track_number, audio_track.number);
                raw_audio_tracks.push(audio_track);
            }
        }
    }

    if raw_audio_tracks.is_empty() {
        return Ok((data_tracks, vec![]));
    }

    let mut audio_tracks = vec![None; (max_track_number + 1) as usize];
    for audio_track in raw_audio_tracks {
        let track_number = audio_track.number as usize;

        if audio_tracks[track_number].is_some() {
            return Err(anyhow!("Duplicate audio tracks for track number {track_number}"));
        }

        audio_tracks[track_number] = Some(audio_track);
    }

    Ok((data_tracks, audio_tracks))
}
