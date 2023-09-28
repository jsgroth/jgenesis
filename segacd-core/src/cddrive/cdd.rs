use crate::api::DiscResult;
use crate::cddrive::cdc::Rchip;
use crate::cdrom;
use crate::cdrom::cdtime::CdTime;
use crate::cdrom::cue::{Track, TrackType};
use crate::cdrom::reader::CdRom;
use bincode::{Decode, Encode};
use genesis_core::GenesisRegion;
use regex::Regex;
use std::sync::OnceLock;
use std::{array, cmp};

const INITIAL_STATUS: [u8; 10] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0F];

const PLAY_DELAY_CLOCKS: u8 = 10;

// 2x signed 16-bit PCM samples, one per stereo channel
const BYTES_PER_AUDIO_SAMPLE: u16 = 4;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CddStatus {
    Stopped = 0x00,
    Playing = 0x01,
    Seeking = 0x02,
    Paused = 0x04,
    InvalidCommand = 0x07,
    ReadingToc = 0x09,
    TrackSkipping = 0x0A,
    NoDisc = 0x0B,
    DiscEnd = 0x0C,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum ReportType {
    AbsoluteTime,
    RelativeTime,
    CurrentTrack,
    DiscLength,
    StartAndEndTracks,
    TrackNStartTime(u8),
}

impl ReportType {
    fn to_byte(self) -> u8 {
        match self {
            Self::AbsoluteTime => 0x00,
            Self::RelativeTime => 0x01,
            Self::CurrentTrack => 0x02,
            Self::DiscLength => 0x03,
            Self::StartAndEndTracks => 0x04,
            Self::TrackNStartTime(..) => 0x05,
        }
    }

    fn from_command(command: [u8; 10]) -> Self {
        // Report type is always stored in Command 3 for Read TOC commands
        match command[3] {
            0x00 => Self::AbsoluteTime,
            0x01 => Self::RelativeTime,
            0x02 => Self::CurrentTrack,
            0x03 => Self::DiscLength,
            0x04 => Self::StartAndEndTracks,
            0x05 => {
                // Track number (BCD) is at Command 4-5
                let track_number = 10 * command[4] + command[5];
                Self::TrackNStartTime(track_number)
            }
            _ => {
                log::warn!("Invalid CDD report type byte: {}; defaulting to absolute", command[3]);
                Self::AbsoluteTime
            }
        }
    }
}

impl Default for ReportType {
    fn default() -> Self {
        Self::AbsoluteTime
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum ReaderStatus {
    Playing,
    Paused,
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
enum CddState {
    MotorStopped,
    NoDisc,
    PreparingToPlay {
        time: CdTime,
        clocks_remaining: u8,
    },
    Playing(CdTime),
    Paused(CdTime),
    Seeking {
        current_time: CdTime,
        seek_time: CdTime,
        next_status: ReaderStatus,
        clocks_remaining: u8,
    },
    TrackSkipping {
        current_time: CdTime,
        skip_time: CdTime,
        clocks_remaining: u8,
    },
    DiscEnd,
    InvalidCommand(CdTime),
    ReadingToc,
}

impl CddState {
    fn current_time(self) -> CdTime {
        match self {
            Self::MotorStopped | Self::NoDisc | Self::DiscEnd | Self::ReadingToc => CdTime::ZERO,
            Self::PreparingToPlay { time, .. }
            | Self::Playing(time)
            | Self::Paused(time)
            | Self::InvalidCommand(time) => time,
            Self::Seeking { current_time, .. } | Self::TrackSkipping { current_time, .. } => {
                current_time
            }
        }
    }
}

impl Default for CddState {
    fn default() -> Self {
        Self::MotorStopped
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct CdDrive {
    disc: Option<CdRom>,
    sector_buffer: [u8; cdrom::BYTES_PER_SECTOR as usize],
    state: CddState,
    report_type: ReportType,
    interrupt_pending: bool,
    status: [u8; 10],
    audio_sample_idx: u16,
    loaded_audio_sector: bool,
    current_audio_sample: (f64, f64),
}

impl CdDrive {
    pub(super) fn new(disc: Option<CdRom>) -> Self {
        Self {
            disc,
            sector_buffer: array::from_fn(|_| 0),
            state: CddState::default(),
            report_type: ReportType::default(),
            interrupt_pending: false,
            status: INITIAL_STATUS,
            audio_sample_idx: 0,
            loaded_audio_sector: false,
            current_audio_sample: (0.0, 0.0),
        }
    }

    #[allow(clippy::match_same_arms)]
    pub fn send_command(&mut self, command: [u8; 10]) {
        log::trace!("CDD command: {command:02X?}");

        match command[0] {
            0x00 => {
                // No-op; return current status
                log::trace!("  Command: No-op");
            }
            0x01 => {
                // Stop motor
                log::trace!("  Command: Stop motor");

                self.state = CddState::MotorStopped;
                self.report_type = ReportType::AbsoluteTime;
            }
            0x02 => {
                // Read TOC
                log::trace!("  Command: Read TOC");

                self.execute_read_toc(command);
            }
            0x03 => {
                // Seek and play
                log::trace!("  Command: Seek and play");

                self.execute_seek(command, ReaderStatus::Playing);
            }
            0x04 => {
                // Seek
                log::trace!("  Command: Seek");

                self.execute_seek(command, ReaderStatus::Paused);
            }
            0x06 => {
                // Pause
                log::trace!("  Command: Pause");

                match &self.disc {
                    Some(_) => {
                        self.state = CddState::Paused(self.state.current_time());
                    }
                    None => {
                        self.state = CddState::NoDisc;
                    }
                }
            }
            0x07 => {
                // Play
                todo!("Play")
            }
            0x08 => {
                // Fast-forward
                todo!("Fast-forward")
            }
            0x09 => {
                // Rewind
                todo!("Rewind")
            }
            0x0A => {
                // Start track skipping
                log::trace!("  Command: Track Skip");

                self.execute_track_skip(command);
            }
            0x0B => {
                // Start track cueing
                todo!("Start track cueing")
            }
            0x0C => {
                // Close tray
                todo!("Close tray")
            }
            0x0D => {
                // Open tray
                todo!("Open tray")
            }
            _ => {}
        }

        // Executing any command other than No-op or Read TOC cancels the TOCN report; default to absolute time
        if command[0] != 0x00
            && command[0] != 0x02
            && matches!(self.report_type, ReportType::TrackNStartTime(..))
        {
            self.report_type = ReportType::AbsoluteTime;
        }

        self.update_status();

        log::trace!("CDD status: {:02X?}", self.status);
    }

    fn update_status(&mut self) {
        self.status.fill(0);

        // Status 0 is always drive status
        self.status[0] = self.current_cdd_status() as u8;

        // If seeking/skipping/stopped, return "not ready" status; not doing this causes Lunar to randomly freeze
        if matches!(
            self.state,
            CddState::Seeking { .. } | CddState::TrackSkipping { .. } | CddState::MotorStopped
        ) {
            self.status[1] = 0x0F;
            update_cdd_checksum(&mut self.status);
            return;
        }

        // Status 1 is always report type
        self.status[1] = self.report_type.to_byte();

        // Only update other status bytes if there is a disc
        if let Some(disc) = &self.disc {
            // Status 2-8 depend on report type
            match self.report_type {
                ReportType::AbsoluteTime => {
                    // Write current absolute time in minutes/seconds/frames (BCD) to Status 2-7
                    let current_time = self.state.current_time();
                    write_time_to_status(current_time, &mut self.status);
                }
                ReportType::RelativeTime => {
                    // Write current relative time in minutes/seconds/frames (BCD) to Status 2-7
                    let current_time = self.state.current_time();
                    let track_start_time = disc
                        .cue()
                        .find_track_by_time(current_time)
                        .map_or(CdTime::ZERO, Track::effective_start_time);
                    write_time_to_status(
                        current_time.saturating_sub(track_start_time),
                        &mut self.status,
                    );
                }
                ReportType::CurrentTrack => {
                    // Write current track number (BCD) to Status 2-3
                    let current_time = self.state.current_time();
                    let track_number =
                        disc.cue().find_track_by_time(current_time).map_or(0, |track| track.number);
                    self.status[2] = track_number / 10;
                    self.status[3] = track_number % 10;
                }
                ReportType::DiscLength => {
                    // Write disc length in minutes/seconds/frames (BCD) to Status 2-7
                    let disc_end_time = disc.cue().last_track().end_time;
                    write_time_to_status(disc_end_time, &mut self.status);
                }
                ReportType::StartAndEndTracks => {
                    // Write start track number to Status 2-3 and end track number to Status 4-5, both in BCD
                    // Assume start track number is always 1
                    self.status[2] = 0x00;
                    self.status[3] = 0x01;

                    let end_track_number = disc.cue().last_track().number;
                    self.status[4] = end_track_number / 10;
                    self.status[5] = end_track_number % 10;
                }
                ReportType::TrackNStartTime(track_number) => {
                    let track = if track_number <= disc.cue().last_track().number {
                        disc.cue().track(track_number)
                    } else {
                        // ??? Invalid track number
                        disc.cue().last_track()
                    };

                    let track_start_time = track.effective_start_time();
                    let track_type = track.track_type;

                    // Write track start time in minutes/seconds/frames (BCD) to Status 2-7
                    write_time_to_status(track_start_time, &mut self.status);

                    // If this is a data track, OR Status 6 with $08
                    if track_type == TrackType::Data {
                        self.status[6] |= 0x08;
                    }

                    // Write the lower digit of track number to Status 8
                    self.status[8] = track_number % 10;
                }
            }
        }

        // Update checksum in Status 9
        update_cdd_checksum(&mut self.status);
    }

    fn current_cdd_status(&self) -> CddStatus {
        match self.state {
            CddState::MotorStopped => CddStatus::Stopped,
            CddState::NoDisc => CddStatus::NoDisc,
            CddState::Paused(..) => CddStatus::Paused,
            CddState::PreparingToPlay { .. } | CddState::Playing(..) => CddStatus::Playing,
            CddState::Seeking { .. } => CddStatus::Seeking,
            CddState::TrackSkipping { .. } => CddStatus::TrackSkipping,
            CddState::DiscEnd => CddStatus::DiscEnd,
            CddState::InvalidCommand(..) => CddStatus::InvalidCommand,
            CddState::ReadingToc => CddStatus::ReadingToc,
        }
    }

    fn execute_read_toc(&mut self, command: [u8; 10]) {
        let report_type = ReportType::from_command(command);
        self.report_type = report_type;

        log::trace!("  Report type changed to {report_type:?}");

        self.state = match (self.state, &self.disc, report_type) {
            (CddState::MotorStopped, None, _) => CddState::NoDisc,
            (CddState::MotorStopped, Some(_), _) => CddState::Paused(CdTime::ZERO),
            (_, Some(_), ReportType::DiscLength | ReportType::TrackNStartTime(..)) => {
                // TOCT and TOCN reports require reading the TOC; move back to start of disc
                CddState::ReadingToc
            }
            _ => self.state,
        };
    }

    fn execute_seek(&mut self, command: [u8; 10], next_status: ReaderStatus) {
        if self.disc.is_none() {
            self.state = CddState::NoDisc;
            return;
        }

        let Some(seek_time) = read_time_from_command(command) else {
            self.state = CddState::InvalidCommand(self.state.current_time());
            return;
        };

        let current_time = self.state.current_time();

        if seek_time == current_time {
            log::trace!("Already at desired seek time {seek_time}; preparing to play");
            self.state =
                CddState::PreparingToPlay { time: seek_time, clocks_remaining: PLAY_DELAY_CLOCKS };
            return;
        }

        let seek_clocks = estimate_seek_clocks(current_time, seek_time);

        log::trace!(
            "Seeking from {current_time} to {seek_time}; estimated time {seek_clocks} 75Hz clocks"
        );

        self.state = CddState::Seeking {
            current_time,
            seek_time,
            next_status,
            clocks_remaining: seek_clocks,
        };
    }

    fn execute_track_skip(&mut self, command: [u8; 10]) {
        let Some(disc) = &self.disc else {
            self.state = CddState::NoDisc;
            return;
        };

        // Number of "tracks" to skip is in Command 4-7, as a 16-bit value stored across 4 nibbles
        let skip_tracks = (u32::from(command[4] & 0x0F) << 12)
            | (u32::from(command[5] & 0x0F) << 8)
            | (u32::from(command[6] & 0x0F) << 4)
            | u32::from(command[7] & 0x0F);

        // Treat a "track" as 15 blocks. This isn't completely accurate but it doesn't need to be.
        // The BIOS will often issue a Track Skip command before a Seek or Read command.
        let skip_blocks = 15 * skip_tracks;

        let current_time = self.state.current_time();

        // Command 3 holds direction; treat 0 as positive, non-0 as negative
        let skip_time = if command[3] == 0 {
            // Skip forwards
            let skip_sector = current_time.to_sector_number() + skip_blocks;
            let disc_end_time = disc.cue().last_track().end_time;

            if skip_sector >= disc_end_time.to_sector_number() {
                disc_end_time
            } else {
                CdTime::from_sector_number(skip_sector)
            }
        } else {
            // Skip backwards
            let skip_sector = current_time.to_sector_number().saturating_sub(skip_blocks);
            CdTime::from_sector_number(skip_sector)
        };

        let clocks_required = estimate_seek_clocks(current_time, skip_time);

        log::trace!(
            "Skipping from {current_time} to {skip_time}; estimated {clocks_required} 75Hz cycles"
        );

        self.state =
            CddState::TrackSkipping { current_time, skip_time, clocks_remaining: clocks_required };
    }

    pub fn status(&self) -> [u8; 10] {
        self.status
    }

    pub fn playing_audio(&self) -> bool {
        match self.state {
            CddState::Playing(current_time) => {
                let is_audio_track = self.disc.as_ref().is_some_and(|disc| {
                    disc.cue()
                        .find_track_by_time(current_time)
                        .is_some_and(|track| track.track_type == TrackType::Audio)
                });
                is_audio_track && self.loaded_audio_sector
            }
            _ => false,
        }
    }

    pub fn update_audio_sample(&mut self) -> (f64, f64) {
        self.current_audio_sample = if self.playing_audio() {
            let idx = self.audio_sample_idx as usize;

            let sample_l =
                i16::from_le_bytes([self.sector_buffer[idx], self.sector_buffer[idx + 1]]);
            let sample_r =
                i16::from_le_bytes([self.sector_buffer[idx + 2], self.sector_buffer[idx + 3]]);

            // TODO fader

            let sample_l = f64::from(sample_l) / -f64::from(i16::MIN);
            let sample_r = f64::from(sample_r) / -f64::from(i16::MIN);

            (sample_l, sample_r)
        } else {
            (0.0, 0.0)
        };

        self.audio_sample_idx =
            (self.audio_sample_idx + BYTES_PER_AUDIO_SAMPLE) % cdrom::BYTES_PER_SECTOR as u16;

        self.current_audio_sample
    }

    pub fn clock(&mut self, rchip: &mut Rchip) -> DiscResult<()> {
        // It is a bug if clock() is called when audio index is not 0; update_audio_sample() must
        // be called before clock() on the cycle when both are called
        assert_eq!(self.audio_sample_idx, 0);

        // CDD interrupt fires once every 1/75 of a second
        self.interrupt_pending = true;

        match self.state {
            CddState::Seeking { current_time, seek_time, next_status, clocks_remaining } => {
                if clocks_remaining == 1 {
                    log::trace!("Seek to {seek_time} complete");

                    self.state = match next_status {
                        ReaderStatus::Paused => CddState::Paused(seek_time),
                        ReaderStatus::Playing => CddState::PreparingToPlay {
                            time: seek_time,
                            clocks_remaining: PLAY_DELAY_CLOCKS,
                        },
                    };
                } else {
                    // 113 clocks to seek across the entire disc
                    let new_time = estimate_intermediate_seek_time(
                        current_time,
                        seek_time,
                        clocks_remaining - 1,
                    );

                    log::trace!(
                        "Current seek status: prev_time={current_time}, new_time={new_time}, seek_time={seek_time}, clocks_remaining={}",
                        clocks_remaining - 1
                    );

                    self.state = CddState::Seeking {
                        current_time: new_time,
                        seek_time,
                        next_status,
                        clocks_remaining: clocks_remaining - 1,
                    };
                }
            }
            CddState::TrackSkipping { current_time, skip_time, clocks_remaining } => {
                if clocks_remaining == 1 {
                    log::trace!("Skip to {skip_time} complete");

                    self.state = CddState::Paused(skip_time);
                } else {
                    // 56 clocks to skip across the entire desc
                    let new_time = estimate_intermediate_seek_time(
                        current_time,
                        skip_time,
                        clocks_remaining - 1,
                    );

                    log::trace!(
                        "Current skip status: prev_time={current_time}, new_time={new_time}, skip_time={skip_time}, clocks_remaining={}",
                        clocks_remaining - 1
                    );

                    self.state = CddState::TrackSkipping {
                        current_time: new_time,
                        skip_time,
                        clocks_remaining: clocks_remaining - 1,
                    };
                }
            }
            CddState::PreparingToPlay { time, clocks_remaining } => {
                if clocks_remaining == 1 {
                    log::trace!("Beginning to play at {time}");

                    self.state = CddState::Playing(time);
                } else {
                    self.state =
                        CddState::PreparingToPlay { time, clocks_remaining: clocks_remaining - 1 };
                }
            }
            CddState::Playing(time) => {
                log::trace!("Playing at {time}");

                let Some(disc) = &mut self.disc else {
                    self.state = CddState::NoDisc;
                    return Ok(());
                };

                let Some(track) = disc.cue().find_track_by_time(time) else {
                    self.state = CddState::DiscEnd;
                    return Ok(());
                };

                let relative_time = time - track.start_time;
                let track_type = track.track_type;
                disc.read_sector(track.number, relative_time, &mut self.sector_buffer)?;

                self.loaded_audio_sector = track_type == TrackType::Audio;

                rchip.decode_block(&self.sector_buffer);

                self.state = CddState::Playing(time + CdTime::new(0, 0, 1));
            }
            CddState::MotorStopped => {
                // Always transition to Reading TOC one clock after the motor is stopped; this fixes
                // the EU BIOS freezing after leaving the options menu
                self.state = CddState::ReadingToc;
            }
            _ => {}
        }

        Ok(())
    }

    pub fn interrupt_pending(&self) -> bool {
        self.interrupt_pending
    }

    pub fn acknowledge_interrupt(&mut self) {
        self.interrupt_pending = false;
    }

    pub fn disc_title(&mut self, region: GenesisRegion) -> DiscResult<Option<String>> {
        static WHITESPACE_RE: OnceLock<Regex> = OnceLock::new();

        let Some(disc) = &mut self.disc else { return Ok(None) };

        // Title information is always stored in the first sector of track 1, which is located at 00:02:00
        disc.read_sector(1, CdTime::new(0, 2, 0), &mut self.sector_buffer)?;

        let title_bytes = match region {
            GenesisRegion::Japan => &self.sector_buffer[0x130..0x160],
            GenesisRegion::Americas | GenesisRegion::Europe => &self.sector_buffer[0x160..0x190],
        };
        let title: String = title_bytes
            .iter()
            .copied()
            .filter_map(|b| {
                let c = b as char;
                (c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || c.is_ascii_punctuation())
                    .then_some(c)
            })
            .collect();

        let whitespace_re = WHITESPACE_RE.get_or_init(|| Regex::new(r" +").unwrap());

        Ok(Some(whitespace_re.replace_all(title.trim(), " ").to_string()))
    }

    pub fn take_disc(&mut self) -> Option<CdRom> {
        self.disc.take()
    }

    pub fn take_disc_from(&mut self, other: &mut Self) {
        self.disc = other.disc.take();
    }

    pub fn clone_without_disc(&self) -> Self {
        Self { disc: None, ..self.clone() }
    }

    pub fn reset(&mut self) {
        self.state = CddState::default();
        self.report_type = ReportType::default();
        self.status = INITIAL_STATUS;
        self.current_audio_sample = (0.0, 0.0);
        self.interrupt_pending = false;
    }
}

// Checksum is the first 9 nibbles summed and then inverted
fn update_cdd_checksum(cdd_status: &mut [u8; 10]) {
    let sum = cdd_status[0..9].iter().copied().sum::<u8>();
    cdd_status[9] = !sum & 0x0F;
}

fn read_time_from_command(command: [u8; 10]) -> Option<CdTime> {
    // Minutes stored in Command 2-3
    let minutes = 10 * command[2] + command[3];

    // Seconds stored in Command 4-5
    let seconds = 10 * command[4] + command[5];

    // Frames stored in Command 6-7
    let frames = 10 * command[6] + command[7];

    CdTime::new_checked(minutes, seconds, frames)
}

fn write_time_to_status(time: CdTime, status: &mut [u8; 10]) {
    // Minutes stored in Status 2-3
    status[2] = time.minutes / 10;
    status[3] = time.minutes % 10;

    // Seconds stored in Status 4-5
    status[4] = time.seconds / 10;
    status[5] = time.seconds % 10;

    // Frames stored in Status 6-7
    status[6] = time.frames / 10;
    status[7] = time.frames % 10;
}

fn estimate_seek_clocks(current_time: CdTime, seek_time: CdTime) -> u8 {
    let diff =
        if current_time >= seek_time { current_time - seek_time } else { seek_time - current_time };

    // It supposedly takes roughly 1.5 seconds / 113 frames to seek from one end of the disc to the
    // other, so scale based on that
    let seek_cycles = (113.0 * f64::from(diff.to_frames())
        / f64::from(CdTime::DISC_END.to_frames()))
    .round() as u8;

    // Require seek to always take at least 1 cycle
    cmp::max(1, seek_cycles)
}

fn estimate_intermediate_seek_time(
    current_time: CdTime,
    seek_time: CdTime,
    clocks_remaining: u8,
) -> CdTime {
    let diff_frames = f64::from(clocks_remaining) / 113.0 * f64::from(CdTime::DISC_END.to_frames());
    let diff = CdTime::from_frames(diff_frames.round() as u32);

    if current_time < seek_time { seek_time - diff } else { seek_time + diff }
}
