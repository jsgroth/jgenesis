use cdrom::cdtime::CdTime;
use cdrom::cue::TrackType;
use cdrom::reader::{CdRom, CdRomFileFormat};
use std::env;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args();
    let exe_name = args.next().expect("Executable name arg not present");

    let Some(path) = args.next() else { panic!("USAGE: {exe_name} file_path") };

    let Some(cdrom_format) = CdRomFileFormat::from_file_path(&path) else {
        panic!("Unable to determine CD-ROM image format for path '{path}'");
    };

    validate(CdRom::open(&path, cdrom_format)?)?;
    validate(CdRom::open_in_memory(&path, cdrom_format)?)?;

    Ok(())
}

fn validate(mut disc: CdRom) -> Result<(), Box<dyn Error>> {
    let mut sector_buffer = vec![0; cdrom::BYTES_PER_SECTOR as usize];

    let last_track_number = disc.cue().last_track().number;

    for track_number in 1..=last_track_number {
        let track = disc.cue().track(track_number);
        let track_type = track.track_type;
        let start_time = track.start_time;
        let end_time = track.end_time;

        let mut time = start_time;
        while time < end_time {
            let relative_time = time - start_time;
            disc.read_sector(track_number, relative_time, &mut sector_buffer)?;

            if track_type == TrackType::Data {
                // First 12 sector bytes are sync, next 3 bytes are MSF time
                let header_msf = CdTime::new(
                    bcd_to_binary(sector_buffer[12]),
                    bcd_to_binary(sector_buffer[13]),
                    bcd_to_binary(sector_buffer[14]),
                );

                assert_eq!(
                    header_msf, time,
                    "Time mismatch in data track {track_number} at relative time {relative_time}! Expected {time}, got {header_msf}"
                );
            }

            time += CdTime::new(0, 0, 1);
        }
    }

    Ok(())
}

fn bcd_to_binary(bcd_value: u8) -> u8 {
    (bcd_value & 0x0F) + 10 * (bcd_value >> 4)
}
