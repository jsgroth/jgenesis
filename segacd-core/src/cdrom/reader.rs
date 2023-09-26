use crate::api::{DiscError, DiscResult};
use crate::cdrom;
use crate::cdrom::cdtime::CdTime;
use crate::cdrom::cue::{CueSheet, TrackType};
use bincode::{Decode, Encode};
use crc::Crc;
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::ops::{Deref, DerefMut, Range};
use std::path::Path;

const SECTOR_HEADER_LEN: u64 = 16;

const CD_ROM_CRC: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_CD_ROM_EDC);
const CRC32_DIGEST_RANGE: Range<usize> = 0..2064;
const CRC32_CHECKSUM_LOCATION: Range<usize> = 2064..2068;

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
    fn create<P: AsRef<Path>>(cue_sheet: &CueSheet, directory: P) -> DiscResult<Self> {
        let file_names: HashSet<_> =
            cue_sheet.tracks().map(|track| track.metadata.file_name.clone()).collect();

        let directory = directory.as_ref();
        let mut files = HashMap::with_capacity(file_names.len());
        for file_name in file_names {
            let file_path = directory.join(Path::new(&file_name));
            let file = File::open(&file_path).map_err(|source| DiscError::BinOpen {
                path: file_path.display().to_string(),
                source,
            })?;
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
    pub fn open<P: AsRef<Path>>(cue_sheet: CueSheet, directory: P) -> DiscResult<Self> {
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
    ) -> DiscResult<()> {
        let track = self.cue_sheet.track(track_number);
        if relative_time < track.pregap_len {
            // Reading data that does not exist in the file
            match track.track_type {
                TrackType::Data => {
                    write_fake_data_pregap(relative_time, out);
                }
                TrackType::Audio => {
                    // Fill with all 0s
                    out[..cdrom::BYTES_PER_SECTOR as usize].fill(0);
                }
            }
            return Ok(());
        }

        // TODO correctly handle postgap

        let track_file = self
            .files
            .get_mut(&track.metadata.file_name)
            .expect("Track file was not opened on load; this is a bug");

        let sector_number = (relative_time - track.pregap_len).to_sector_number();
        track_file
            .seek(SeekFrom::Start(u64::from(sector_number) * cdrom::BYTES_PER_SECTOR))
            .map_err(DiscError::DiscReadIo)?;
        track_file
            .read_exact(&mut out[..cdrom::BYTES_PER_SECTOR as usize])
            .map_err(DiscError::DiscReadIo)?;

        if track.track_type == TrackType::Data {
            // Perform error detection check
            let mut digest = CD_ROM_CRC.digest();
            digest.update(&out[CRC32_DIGEST_RANGE]);
            let checksum = digest.finalize();

            let edc_bytes: [u8; 4] = out[CRC32_CHECKSUM_LOCATION].try_into().unwrap();
            let edc = u32::from_le_bytes(edc_bytes);

            if checksum != edc {
                return Err(DiscError::DiscReadInvalidChecksum {
                    track_number,
                    sector_number,
                    expected: edc,
                    actual: checksum,
                });
            }
        }

        // TODO check P/Q ECC?

        Ok(())
    }
}

fn write_fake_data_pregap(time: CdTime, out: &mut [u8]) {
    // Make up a header; 12 sync bytes, then minutes, then seconds, then frames, then mode (always 1)
    let bcd_minutes = time_component_to_bcd(time.minutes);
    let bcd_seconds = time_component_to_bcd(time.seconds);
    let bcd_frames = time_component_to_bcd(time.frames);
    out[..SECTOR_HEADER_LEN as usize].copy_from_slice(&[
        0x00,
        0x11,
        0x11,
        0x11,
        0x11,
        0x11,
        0x11,
        0x11,
        0x11,
        0x11,
        0x11,
        0x00,
        bcd_minutes,
        bcd_seconds,
        bcd_frames,
        0x01,
    ]);
    out[SECTOR_HEADER_LEN as usize..cdrom::BYTES_PER_SECTOR as usize].fill(0);
}

fn time_component_to_bcd(component: u8) -> u8 {
    let msb = component / 10;
    let lsb = component % 10;
    (msb << 4) | lsb
}
