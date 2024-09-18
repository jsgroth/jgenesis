//! Code for reading CD-ROM files

mod chd;
mod cuebin;
mod seekvec;

use crate::cdtime::CdTime;
use crate::cue::{CueSheet, TrackMode, TrackType};
use crate::reader::chd::ChdFile;
use crate::reader::cuebin::CdBinFiles;
use crate::reader::seekvec::SeekableVec;
use crate::{CdRomError, CdRomResult};
use bincode::{Decode, Encode};
use crc::Crc;
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use std::ffi::OsStr;
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::ops::Range;
use std::path::Path;

const SECTOR_HEADER_LEN: u64 = 16;

const CD_ROM_CRC: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_CD_ROM_EDC);

const MODE_1_DIGEST_RANGE: Range<usize> = 0..2064;
const MODE_1_CHECKSUM_LOCATION: Range<usize> = 2064..2068;

const MODE_2_SUBMODE_LOCATION: usize = 18;

const MODE_2_FORM_1_DIGEST_RANGE: Range<usize> = 16..2072;
const MODE_2_FORM_1_CHECKSUM_LOCATION: Range<usize> = 2072..2076;

const MODE_2_FORM_2_DIGEST_RANGE: Range<usize> = 16..2348;
const MODE_2_FORM_2_CHECKSUM_LOCATION: Range<usize> = 2348..2352;

type CdBinFsFiles = CdBinFiles<File>;
type CdBinMemoryFiles = CdBinFiles<SeekableVec>;

type ChdFsFile = ChdFile<BufReader<File>>;
type ChdMemoryFile = ChdFile<SeekableVec>;

#[derive(Debug, FakeEncode, FakeDecode)]
enum CdRomReader {
    CueBin(CdBinFsFiles),
    CueBinMemory(CdBinMemoryFiles),
    ChdFs(ChdFsFile),
    ChdMemory(ChdMemoryFile),
}

impl Default for CdRomReader {
    fn default() -> Self {
        Self::CueBin(CdBinFiles::empty())
    }
}

impl CdRomReader {
    fn read_sector(
        &mut self,
        track_number: u8,
        relative_sector_number: u32,
        out: &mut [u8],
    ) -> CdRomResult<()> {
        match self {
            Self::CueBin(bin_files) => {
                bin_files.read_sector(track_number, relative_sector_number, out)
            }
            Self::CueBinMemory(bin_files) => {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode2Form {
    // 2048-byte sector with ECC bytes
    One,
    // 2324-byte sector with no ECC bytes, only EDC
    Two,
}

impl Mode2Form {
    fn parse(sector_buffer: &[u8]) -> Self {
        // Submode bit 5 specifies Form 1 vs. Form 2
        if sector_buffer[MODE_2_SUBMODE_LOCATION] & (1 << 5) != 0 { Self::Two } else { Self::One }
    }
}

#[derive(Debug, Encode, Decode)]
pub struct CdRom {
    cue_sheet: CueSheet,
    reader: CdRomReader,
}

impl CdRom {
    /// Open a CD-ROM reader that will read from the filesystem as needed.
    ///
    /// # Errors
    ///
    /// Will propagate any I/O errors, and will return an error if the CD-ROM metadata appears
    /// invalid.
    pub fn open<P: AsRef<Path>>(path: P, format: CdRomFileFormat) -> CdRomResult<Self> {
        match format {
            CdRomFileFormat::CueBin => Self::open_cue_bin(path),
            CdRomFileFormat::Chd => Self::open_chd(path),
        }
    }

    fn open_cue_bin<P: AsRef<Path>>(cue_path: P) -> CdRomResult<Self> {
        let (bin_files, cue_sheet) = CdBinFiles::create(cue_path, |path| File::open(path))?;

        Ok(Self { cue_sheet, reader: CdRomReader::CueBin(bin_files) })
    }

    fn open_chd<P: AsRef<Path>>(chd_path: P) -> CdRomResult<Self> {
        let chd_path = chd_path.as_ref();

        let file = File::open(chd_path).map_err(|source| CdRomError::ChdOpen {
            path: chd_path.display().to_string(),
            source,
        })?;
        let (chd_file, cue_sheet) = ChdFile::open(BufReader::new(file))?;

        Ok(Self { cue_sheet, reader: CdRomReader::ChdFs(chd_file) })
    }

    /// Open a CD-ROM reader that will load the entire disc image into memory.
    ///
    /// # Errors
    ///
    /// Will return any error encountered while reading from disk, or if the CD-ROM metadata appears
    /// invalid.
    pub fn open_in_memory<P: AsRef<Path>>(path: P, format: CdRomFileFormat) -> CdRomResult<Self> {
        let path = path.as_ref();

        match format {
            CdRomFileFormat::CueBin => Self::open_cue_bin_in_memory(path),
            CdRomFileFormat::Chd => {
                let chd_bytes = fs::read(path).map_err(|source| CdRomError::ChdOpen {
                    path: path.display().to_string(),
                    source,
                })?;
                Self::open_chd_in_memory(chd_bytes)
            }
        }
    }

    /// Open a CD-ROM reader that will read from CUE/BIN files that will be read into memory.
    ///
    /// # Errors
    ///
    /// Will return any error encountered while reading from disk, or if the CUE file appears to be
    /// invalid.
    pub fn open_cue_bin_in_memory<P: AsRef<Path>>(cue_path: P) -> CdRomResult<Self> {
        let (bin_files, cue_sheet) = CdBinFiles::create(cue_path, |path| {
            let bin_bytes = fs::read(path)?;
            Ok(SeekableVec::new(bin_bytes))
        })?;

        Ok(Self { reader: CdRomReader::CueBinMemory(bin_files), cue_sheet })
    }

    /// Open a CD-ROM reader that will read from a CHD file that has been read into memory.
    ///
    /// # Errors
    ///
    /// Will return an error if the CHD or CD-ROM metadata appears invalid.
    pub fn open_chd_in_memory(chd_bytes: Vec<u8>) -> CdRomResult<Self> {
        let seekable_vec = SeekableVec::new(chd_bytes);
        let (chd_file, cue_sheet) = ChdFile::open(seekable_vec)?;

        Ok(Self { cue_sheet, reader: CdRomReader::ChdMemory(chd_file) })
    }

    #[must_use]
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
    ) -> CdRomResult<()> {
        let track = self.cue_sheet.track(track_number);
        if relative_time < track.pregap_len
            || relative_time >= track.end_time - track.postgap_len - track.start_time
        {
            // Reading data in pregap or postgap that does not exist in the file
            match track.track_type {
                TrackType::Data => {
                    write_fake_data_pregap(track.mode, relative_time, out);
                }
                TrackType::Audio => {
                    // Fill with all 0s
                    out[..crate::BYTES_PER_SECTOR as usize].fill(0);
                }
            }
            return Ok(());
        }

        let relative_sector_number = (relative_time - track.pregap_len).to_sector_number();
        self.reader.read_sector(track_number, relative_sector_number, out)?;

        validate_edc(track.mode, track_number, relative_sector_number, out)?;

        // TODO check P/Q ECC?

        Ok(())
    }
}

fn validate_edc(
    mode: TrackMode,
    track_number: u8,
    relative_sector_number: u32,
    sector: &[u8],
) -> CdRomResult<()> {
    let (digest_range, edc_location) = match mode {
        TrackMode::Mode1 => (MODE_1_DIGEST_RANGE, MODE_1_CHECKSUM_LOCATION),
        TrackMode::Mode2 => match Mode2Form::parse(sector) {
            Mode2Form::One => (MODE_2_FORM_1_DIGEST_RANGE, MODE_2_FORM_1_CHECKSUM_LOCATION),
            Mode2Form::Two => {
                // In Form 2, an EDC of 0 indicates no EDC
                if sector[MODE_2_FORM_2_CHECKSUM_LOCATION] == [0, 0, 0, 0] {
                    return Ok(());
                }

                (MODE_2_FORM_2_DIGEST_RANGE, MODE_2_FORM_2_CHECKSUM_LOCATION)
            }
        },
        TrackMode::Audio => return Ok(()),
    };

    let checksum = CD_ROM_CRC.checksum(&sector[digest_range]);

    let edc_bytes: [u8; 4] = sector[edc_location].try_into().unwrap();
    let edc = u32::from_le_bytes(edc_bytes);

    if checksum != edc {
        return Err(CdRomError::DiscReadInvalidChecksum {
            track_number,
            sector_number: relative_sector_number,
            expected: edc,
            actual: checksum,
        });
    }

    Ok(())
}

impl TrackMode {
    fn header_byte(self) -> u8 {
        match self {
            Self::Mode1 => 0x01,
            Self::Mode2 => 0x02,
            Self::Audio => 0x00,
        }
    }
}

fn write_fake_data_pregap(mode: TrackMode, time: CdTime, out: &mut [u8]) {
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
        mode.header_byte(),
    ]);
    out[SECTOR_HEADER_LEN as usize..crate::BYTES_PER_SECTOR as usize].fill(0);
}

fn time_component_to_bcd(component: u8) -> u8 {
    let msb = component / 10;
    let lsb = component % 10;
    (msb << 4) | lsb
}
