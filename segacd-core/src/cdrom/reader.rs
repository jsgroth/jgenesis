use crate::cdrom::cue::CueSheet;
use anyhow::anyhow;
use bincode::{Decode, Encode};
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::collections::{HashMap, HashSet};
use std::fs::File;
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

    // Read a 2352-byte sector from the data track
    pub fn read_data_sector(&mut self, sector_number: u32, out: &mut [u8]) -> anyhow::Result<()> {
        todo!()
    }
}
