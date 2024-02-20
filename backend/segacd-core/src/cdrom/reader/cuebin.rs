//! Code for loading and reading CD-ROM images in CUE/BIN format

use crate::api::{DiscError, DiscResult};
use crate::cdrom;
use crate::cdrom::cue::TrackMetadata;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug)]
struct CdRomFile {
    file: File,
    position: u64,
}

impl CdRomFile {
    fn new(file: File) -> Self {
        Self { file, position: 0 }
    }
}

#[derive(Debug, Default)]
pub struct CdBinFiles {
    files: HashMap<String, CdRomFile>,
    track_metadata: Vec<TrackMetadata>,
}

impl CdBinFiles {
    pub fn create<P: AsRef<Path>>(
        track_metadata: Vec<TrackMetadata>,
        directory: P,
    ) -> DiscResult<Self> {
        let file_names: HashSet<_> =
            track_metadata.iter().map(|metadata| metadata.file_name.clone()).collect();

        let directory = directory.as_ref();
        let mut files = HashMap::with_capacity(file_names.len());
        for file_name in file_names {
            let file_path = directory.join(Path::new(&file_name));
            let file = File::open(&file_path).map_err(|source| DiscError::BinOpen {
                path: file_path.display().to_string(),
                source,
            })?;
            files.insert(file_name, CdRomFile::new(file));
        }

        Ok(Self { files, track_metadata })
    }

    pub fn read_sector(
        &mut self,
        track_number: u8,
        relative_sector_number: u32,
        out: &mut [u8],
    ) -> DiscResult<()> {
        let metadata = &self.track_metadata[(track_number - 1) as usize];
        let CdRomFile { file: track_file, position } = self
            .files
            .get_mut(&metadata.file_name)
            .expect("Track file was not opened on load; this is a bug");

        let sector_number = metadata.time_in_file.to_sector_number() + relative_sector_number;
        let sector_addr = u64::from(sector_number) * cdrom::BYTES_PER_SECTOR;

        // Only seek if the file descriptor is not already at the desired position
        if *position != sector_addr {
            track_file.seek(SeekFrom::Start(sector_addr)).map_err(DiscError::DiscReadIo)?;
        }

        track_file
            .read_exact(&mut out[..cdrom::BYTES_PER_SECTOR as usize])
            .map_err(DiscError::DiscReadIo)?;
        *position = sector_addr + cdrom::BYTES_PER_SECTOR;

        Ok(())
    }
}
