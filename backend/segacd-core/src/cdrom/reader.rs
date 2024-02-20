//! Code for reading CD-ROM files

mod chd;
mod cuebin;
mod seekvec;

use crate::api::{DiscError, DiscResult};
use crate::cdrom;
use crate::cdrom::cdtime::CdTime;
use crate::cdrom::cue::{CueSheet, TrackType};
use crate::cdrom::reader::chd::ChdFile;
use crate::cdrom::reader::cuebin::CdBinFiles;
use crate::cdrom::reader::seekvec::SeekableVec;
use bincode::{Decode, Encode};
use crc::Crc;
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::ffi::OsStr;
use std::fs::File;
use std::io::BufReader;
use std::ops::Range;
use std::path::Path;

const SECTOR_HEADER_LEN: u64 = 16;

const CD_ROM_CRC: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_CD_ROM_EDC);
const CRC32_DIGEST_RANGE: Range<usize> = 0..2064;
const CRC32_CHECKSUM_LOCATION: Range<usize> = 2064..2068;

type ChdFsFile = ChdFile<BufReader<File>>;
type ChdMemoryFile = ChdFile<SeekableVec>;

#[derive(Debug, FakeEncode, FakeDecode)]
enum CdRomReader {
    CueBin(CdBinFiles),
    ChdFs(ChdFsFile),
    ChdMemory(ChdMemoryFile),
}

impl Default for CdRomReader {
    fn default() -> Self {
        Self::CueBin(CdBinFiles::default())
    }
}

impl CdRomReader {
    fn read_sector(
        &mut self,
        track_number: u8,
        relative_sector_number: u32,
        out: &mut [u8],
    ) -> DiscResult<()> {
        match self {
            Self::CueBin(bin_files) => {
                bin_files.read_sector(track_number, relative_sector_number, out)
            }
            Self::ChdFs(chd_file) => {
                chd_file.read_sector(track_number, relative_sector_number, out)
            }
            Self::ChdMemory(chd_file) => {
                chd_file.read_sector(track_number, relative_sector_number, out)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CdRomFileFormat {
    // CUE file + BIN files
    CueBin,
    // CHD files
    Chd,
}

impl CdRomFileFormat {
    pub fn from_file_path<P: AsRef<Path>>(path: P) -> Option<Self> {
        match path.as_ref().extension().and_then(OsStr::to_str) {
            Some("cue") => Some(Self::CueBin),
            Some("chd") => Some(Self::Chd),
            _ => None,
        }
    }
}

#[derive(Debug, Encode, Decode)]
pub struct CdRom {
    cue_sheet: CueSheet,
    reader: CdRomReader,
}

impl CdRom {
    pub fn open<P: AsRef<Path>>(path: P, format: CdRomFileFormat) -> DiscResult<Self> {
        match format {
            CdRomFileFormat::CueBin => Self::open_cue_bin(path),
            CdRomFileFormat::Chd => Self::open_chd(path),
        }
    }

    fn open_cue_bin<P: AsRef<Path>>(cue_path: P) -> DiscResult<Self> {
        let (bin_files, cue_sheet) = CdBinFiles::create(cue_path)?;

        Ok(Self { cue_sheet, reader: CdRomReader::CueBin(bin_files) })
    }

    fn open_chd<P: AsRef<Path>>(chd_path: P) -> DiscResult<Self> {
        let chd_path = chd_path.as_ref();

        let file = File::open(chd_path).map_err(|source| DiscError::ChdOpen {
            path: chd_path.display().to_string(),
            source,
        })?;
        let (chd_file, cue_sheet) = ChdFile::open(BufReader::new(file))?;

        Ok(Self { cue_sheet, reader: CdRomReader::ChdFs(chd_file) })
    }

    pub fn open_chd_in_memory(chd_bytes: Vec<u8>) -> DiscResult<Self> {
        let seekable_vec = SeekableVec::new(chd_bytes);
        let (chd_file, cue_sheet) = ChdFile::open(seekable_vec)?;

        Ok(Self { cue_sheet, reader: CdRomReader::ChdMemory(chd_file) })
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
        if relative_time < track.pregap_len
            || relative_time >= track.end_time - track.postgap_len - track.start_time
        {
            // Reading data in pregap or postgap that does not exist in the file
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

        let relative_sector_number = (relative_time - track.pregap_len).to_sector_number();
        self.reader.read_sector(track_number, relative_sector_number, out)?;

        if track.track_type == TrackType::Data {
            // Perform error detection check
            let checksum = CD_ROM_CRC.checksum(&out[CRC32_DIGEST_RANGE]);

            let edc_bytes: [u8; 4] = out[CRC32_CHECKSUM_LOCATION].try_into().unwrap();
            let edc = u32::from_le_bytes(edc_bytes);

            if checksum != edc {
                return Err(DiscError::DiscReadInvalidChecksum {
                    track_number,
                    sector_number: relative_sector_number,
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
