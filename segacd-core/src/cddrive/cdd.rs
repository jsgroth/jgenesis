use crate::api::DiscResult;
use crate::cddrive::cdc;
use crate::cddrive::cdc::{Rchip, PLAY_DELAY_CLOCKS};
use crate::cdrom;
use crate::cdrom::cdtime::CdTime;
use crate::cdrom::cue::TrackType;
use crate::cdrom::reader::CdRom;
use bincode::{Decode, Encode};
use genesis_core::GenesisRegion;
use regex::Regex;
use std::array;
use std::sync::OnceLock;

const INITIAL_STATUS: [u8; 10] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0F];

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CddStatus {
    Stopped = 0x00,
    Playing = 0x01,
    Seeking = 0x02,
    Paused = 0x04,
    InvalidCommand = 0x07,
    NoDisc = 0x0B,
    DiscEnd = 0x0C,
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
    DiscEnd,
    InvalidCommand(CdTime),
}

impl CddState {
    fn current_time(self) -> CdTime {
        match self {
            Self::MotorStopped | Self::NoDisc | Self::DiscEnd => CdTime::ZERO,
            Self::PreparingToPlay { time, .. }
            | Self::Playing(time)
            | Self::Paused(time)
            | Self::InvalidCommand(time) => time,
            Self::Seeking { current_time, .. } => current_time,
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
    interrupt_pending: bool,
    status: [u8; 10],
}

impl CdDrive {
    pub(super) fn new(disc: Option<CdRom>) -> Self {
        Self {
            disc,
            sector_buffer: array::from_fn(|_| 0),
            state: CddState::default(),
            interrupt_pending: false,
            status: INITIAL_STATUS,
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

                // TODO should seek during playback continue playing after seek?
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

        self.status[0] = self.current_cdd_status() as u8;

        match command[0] {
            0x02 => {
                self.status[1] = command[3];
            }
            0x03 | 0x07 => {
                self.status[1] = 0x02;
            }
            0x00 => {}
            _ => {
                self.status[1] = 0x00;
            }
        }

        update_cdd_checksum(&mut self.status);

        log::trace!("CDD status: {:02X?}", self.status);
    }

    fn current_cdd_status(&self) -> CddStatus {
        match self.state {
            CddState::MotorStopped => CddStatus::Stopped,
            CddState::NoDisc => CddStatus::NoDisc,
            CddState::PreparingToPlay { .. } | CddState::Paused(..) => CddStatus::Paused,
            CddState::Playing(..) => CddStatus::Playing,
            CddState::Seeking { .. } => CddStatus::Seeking,
            CddState::DiscEnd => CddStatus::DiscEnd,
            CddState::InvalidCommand(..) => CddStatus::InvalidCommand,
        }
    }

    fn execute_read_toc(&mut self, command: [u8; 10]) {
        if let CddState::MotorStopped = self.state {
            self.state = match &self.disc {
                Some(_) => CddState::Paused(CdTime::ZERO),
                None => CddState::NoDisc,
            };
        }

        let Some(disc) = &self.disc else {
            write_time_to_status(CdTime::ZERO, &mut self.status);
            return;
        };
        let cue = disc.cue();

        // Command 3 contains the subcommand to execute
        match command[3] {
            0x00 => {
                // Get absolute position
                log::trace!("  Subcommand: Get absolute position");

                let current_time = self.state.current_time();
                write_time_to_status(current_time, &mut self.status);
            }
            0x01 => {
                // Get relative position
                log::trace!("  Subcommand: Get relative position");

                let current_time = self.state.current_time();
                let relative_time = match cue.find_track_by_time(current_time) {
                    Some(track) => current_time - track.start_time,
                    None => {
                        // Past end of disc
                        CdTime::ZERO
                    }
                };

                write_time_to_status(relative_time, &mut self.status);
            }
            0x02 => {
                // Get current track number
                log::trace!("  Subcommand: Get current track number");

                let current_time = self.state.current_time();
                let track_number = match cue.find_track_by_time(current_time) {
                    Some(track) => track.number,
                    None => cue.num_tracks(),
                };

                // Write track number to Status 2-3
                self.status[2] = track_number / 10;
                self.status[3] = track_number % 10;
            }
            0x03 => {
                // Get CD length
                log::trace!("  Subcommand: Get CD length");

                write_time_to_status(cue.last_track().end_time, &mut self.status);
            }
            0x04 => {
                // Get number of tracks
                log::trace!("  Subcommand: Get number of tracks");

                // Write start track number (always 1) to Status 2-3
                self.status[2] = 0x00;
                self.status[3] = 0x01;

                // Write end track number to Status 4-5
                let last_track_number = cue.num_tracks();
                self.status[4] = last_track_number / 10;
                self.status[5] = last_track_number % 10;
            }
            0x05 => {
                // Get track start time
                log::trace!("  Subcommand: Get track start time");

                // Track number is stored in commands 4-5
                let track_number = 10 * command[4] + command[5];
                let track = cue.track(track_number);

                // Write track time to Status 2-7
                write_time_to_status(track.effective_start_time(), &mut self.status);

                // Status 6 is always set to $08 for data tracks
                if track.track_type == TrackType::Data {
                    self.status[6] = 0x08;
                }

                // Status 8 is always set to the lowest 4 bits of the track number
                self.status[8] = track_number & 0x0F;
            }
            _ => {}
        }
    }

    fn execute_seek(&mut self, command: [u8; 10], next_status: ReaderStatus) {
        if self.disc.is_none() {
            self.state = CddState::NoDisc;
            return;
        }

        let Some(raw_seek_time) = read_time_from_command(command) else {
            self.state = CddState::InvalidCommand(self.state.current_time());
            return;
        };

        // Seek to 3 frames prior to the specified time; otherwise the BIOS might miss the starting
        // block
        let seek_time = raw_seek_time.saturating_sub(CdTime::new(0, 0, 3));

        let current_time = self.state.current_time();

        let seek_clocks = cdc::estimate_seek_clocks(current_time, seek_time);

        log::trace!(
            "Seeking from {current_time} to {seek_time}; estimated time {seek_clocks} 75Hz clocks"
        );

        // TODO preserve state when playing?
        self.state = CddState::Seeking {
            current_time,
            seek_time,
            next_status,
            clocks_remaining: seek_clocks,
        };
    }

    pub fn status(&self) -> [u8; 10] {
        self.status
    }

    pub fn clock(&mut self, rchip: &mut Rchip) -> DiscResult<()> {
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
                    // Estimate current time based on clocks remaining
                    let diff_frames = f64::from(clocks_remaining - 1) / 113.0
                        * f64::from(CdTime::DISC_END.to_frames());
                    let diff = CdTime::from_frames(diff_frames.round() as u32);
                    let new_time =
                        if current_time < seek_time { seek_time - diff } else { seek_time + diff };

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
                disc.read_sector(track.number, relative_time, &mut self.sector_buffer)?;

                rchip.decode_block(&self.sector_buffer);

                self.state = CddState::Playing(time + CdTime::new(0, 0, 1));
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
            GenesisRegion::Japan => &self.sector_buffer[0x120..0x150],
            GenesisRegion::Americas | GenesisRegion::Europe => &self.sector_buffer[0x150..0x180],
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

    pub fn take_disc_from(&mut self, other: &mut Self) {
        self.disc = other.disc.take();
    }

    pub fn clone_without_disc(&self) -> Self {
        Self { disc: None, ..self.clone() }
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
