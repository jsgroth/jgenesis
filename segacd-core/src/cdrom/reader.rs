use crate::cdrom;
use crate::cdrom::cdtime::CdTime;
use crate::cdrom::cue::CueSheet;
use anyhow::anyhow;
use bincode::{Decode, Encode};
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io;
use std::io::{Read, Seek, SeekFrom};
use std::ops::{Deref, DerefMut};
use std::path::Path;

const SECTOR_HEADER_LEN: u64 = 16;
const SECTOR_DATA_LEN: u64 = 2048;
// 16 header bytes + 2048 data bytes + 288 error detection/correction bytes

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
    fn create<P: AsRef<Path>>(cue_sheet: &CueSheet, directory: P) -> anyhow::Result<Self> {
        let file_names: HashSet<_> =
            cue_sheet.tracks().map(|track| track.metadata.file_name.clone()).collect();

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
    cue_sheet: CueSheet,
    files: CdRomFiles,
}

impl CdRom {
    pub fn open<P: AsRef<Path>>(cue_sheet: CueSheet, directory: P) -> anyhow::Result<Self> {
        let files = CdRomFiles::create(&cue_sheet, directory)?;
        Ok(Self { cue_sheet, files })
    }

    pub fn cue(&self) -> &CueSheet {
        &self.cue_sheet
    }

    /// Read a 2352-byte sector from the given track into a buffer.
    ///
    /// # Errors
    ///
    /// This method will propagate any I/O error encountered while reading from disk.
    ///
    /// # Panics
    ///
    /// This method will panic if `out`'s length is less than 2352 or if `relative_time` is past the
    /// end of the track file.
    pub fn read_sector(
        &mut self,
        track_number: u8,
        relative_time: CdTime,
        out: &mut [u8],
    ) -> io::Result<()> {
        let track_metadata = &self.cue_sheet.track(track_number).metadata;
        let track_file = self
            .files
            .get_mut(&track_metadata.file_name)
            .expect("Track file was not opened on load; this is a bug");

        let sector_number = relative_time.to_sector_number();
        track_file.seek(SeekFrom::Start(u64::from(sector_number) * cdrom::BYTES_PER_SECTOR))?;
        track_file.read_exact(&mut out[..cdrom::BYTES_PER_SECTOR as usize])?;

        Ok(())
    }
}
