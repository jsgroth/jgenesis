//! Sega CD's physical drive, which documentation refers to as the CDD

use crate::api::{SegaCdEmulatorConfig, SegaCdLoadResult};
use crate::cddrive::cdc::{Rchip, RchipDmaArgs};
use bincode::{Decode, Encode};
use cdrom::cdtime::CdTime;
use cdrom::cue::{Track, TrackType};
use cdrom::reader::{CdRom, CdRomFileFormat};
use genesis_config::GenesisRegion;
use jgenesis_proc_macros::PartialClone;
use regex::Regex;
use std::cmp::Ordering;
use std::path::Path;
use std::sync::LazyLock;
use std::{array, cmp};

const INITIAL_STATUS: [u8; 10] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0F];

// The BIOS freezes if this delay is too short or too long
// Time Gal freezes at boot if this delay is too short
const PLAY_DELAY_CLOCKS: u8 = 6;

// 2x signed 16-bit PCM samples, one per stereo channel
const BYTES_PER_AUDIO_SAMPLE: u16 = 4;

const MAX_FADER_VOLUME: u16 = 1 << 10;

// Fast-forward / rewind should skip at roughly 100x playback speed
const FAST_FORWARD_SECONDS: u8 = 1;
const FAST_FORWARD_FRAMES: u8 = 25;

const DIVIDER_75HZ: u16 = 44100 / 75;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Stopped = 0x00,
    Playing = 0x01,
    Seeking = 0x02,
    Scanning = 0x03,
    Paused = 0x04,
    TrayOpen = 0x05,
    InvalidCommand = 0x07,
    ReadingToc = 0x09,
    TrackSkipping = 0x0A,
    NoDisc = 0x0B,
    DiscEnd = 0x0C,
    DiscStart = 0x0D,
    TrayMoving = 0x0E,
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
enum State {
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
    FastForwarding(CdTime),
    Rewinding(CdTime),
    DiscStart,
    DiscEnd(CdTime),
    TrayOpening {
        auto_close: bool,
    },
    TrayOpen {
        auto_close: bool,
    },
    TrayClosing,
    InvalidCommand(CdTime),
    ReadingToc,
}

impl State {
    fn current_time(self) -> CdTime {
        match self {
            Self::MotorStopped
            | Self::NoDisc
            | Self::DiscStart
            | Self::ReadingToc
            | Self::TrayOpening { .. }
            | Self::TrayOpen { .. }
            | Self::TrayClosing => CdTime::ZERO,
            Self::PreparingToPlay { time, .. }
            | Self::Playing(time)
            | Self::Paused(time)
            | Self::FastForwarding(time)
            | Self::Rewinding(time)
            | Self::DiscEnd(time)
            | Self::InvalidCommand(time) => time,
            Self::Seeking { current_time, .. } | Self::TrackSkipping { current_time, .. } => {
                current_time
            }
        }
    }
}

impl Default for State {
    fn default() -> Self {
        Self::MotorStopped
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum CdModel {
    One,
    Two,
}

#[derive(Debug, Encode, Decode, PartialClone)]
pub struct CdDrive {
    #[partial_clone(default)]
    disc: Option<CdRom>,
    model: CdModel,
    sector_buffer: Box<[u8; cdrom::BYTES_PER_SECTOR as usize]>,
    state: State,
    report_type: ReportType,
    // If a command is sent while the drive is playing, it needs to read at least 1 more sector
    // before it responds to the new command. Radical Rex depends on this or it will crash shortly
    // into the intro
    next_clock_play: Option<CdTime>,
    interrupt_pending: bool,
    status: [u8; 10],
    audio_sample_idx: u16,
    loaded_audio_sector: bool,
    fader_volume: u16,
    current_volume: u16,
    divider_75hz: u16,
    data_speed: u16,
}

impl CdDrive {
    pub(super) fn new(disc: Option<CdRom>, model: CdModel, config: &SegaCdEmulatorConfig) -> Self {
        Self {
            disc,
            model,
            sector_buffer: Box::new(array::from_fn(|_| 0)),
            state: State::default(),
            report_type: ReportType::default(),
            interrupt_pending: false,
            next_clock_play: None,
            status: INITIAL_STATUS,
            audio_sample_idx: 0,
            loaded_audio_sector: false,
            fader_volume: 0,
            current_volume: 0,
            divider_75hz: DIVIDER_75HZ,
            data_speed: config.disc_drive_speed.get(),
        }
    }

    #[allow(clippy::match_same_arms)]
    pub fn send_command(&mut self, command: [u8; 10]) {
        log::trace!("CDD command: {command:02X?}");

        let prev_playing_time = match self.state {
            State::Playing(time) => Some(time),
            _ => None,
        };

        match command[0] {
            0x00 => {
                // No-op; return current status
                log::trace!("  Command: No-op");
            }
            0x01 => {
                // Stop motor
                log::trace!("  Command: Stop motor");

                self.state = State::MotorStopped;
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
                        self.state = State::Paused(self.state.current_time());
                    }
                    None => {
                        self.state = State::NoDisc;
                    }
                }
            }
            0x07 => {
                // Play
                log::trace!("  Command: Play");

                match self.state {
                    State::Paused(time) | State::FastForwarding(time) | State::Rewinding(time) => {
                        self.state =
                            State::PreparingToPlay { time, clocks_remaining: PLAY_DELAY_CLOCKS };
                    }
                    _ => {}
                }
            }
            0x08 => {
                // Fast-forward
                log::trace!("  Command: Fast-forward");

                if self.disc.is_some() {
                    self.state = State::FastForwarding(self.state.current_time());
                }
            }
            0x09 => {
                // Rewind
                log::trace!("  Command: Rewind");

                if self.disc.is_some() {
                    self.state = State::Rewinding(self.state.current_time());
                }
            }
            0x0A => {
                // Start track skipping
                log::trace!("  Command: Track Skip");

                self.execute_track_skip(command);
            }
            0x0B => {
                // Start track cueing
                // This command does not appear to be used by any official BIOS version
                log::error!("Ignoring unimplemented CDD command: Cue Track");
            }
            0x0C => {
                // Close tray
                log::trace!("  Command: Close Tray");
                if matches!(self.state, State::TrayOpening { .. } | State::TrayOpen { .. }) {
                    self.state = State::TrayClosing;
                }
            }
            0x0D => {
                // Open tray
                log::trace!("  Command: Open Tray");
                if !matches!(self.state, State::TrayOpening { .. }) {
                    self.state = State::TrayOpening { auto_close: false };
                }
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

        if prev_playing_time.is_some() && !matches!(self.state, State::Playing(..)) {
            // If the drive was playing and a command just changed the state, have it read one more
            // sector on the next clock before it handles the new state (e.g. seeking).
            // Per tests by krikzz, the CDD status changes immediately in response to a command
            // despite this behavior, so changing the state immediately is correct
            self.next_clock_play = prev_playing_time;
        }

        log::trace!("CDD status: {:02X?}", self.status);
    }

    fn update_status(&mut self) {
        self.status.fill(0);

        // Status 0 is always drive status
        self.status[0] = self.current_cdd_status() as u8;

        // If drive is stopped, leave status as all 0s
        if matches!(self.state, State::MotorStopped) {
            update_cdd_checksum(&mut self.status);
            return;
        }

        // If seeking/skipping, return "not ready" status; not doing this causes Lunar to randomly freeze
        if matches!(
            self.state,
            State::Seeking { .. }
                | State::TrackSkipping { .. }
                | State::TrayOpening { .. }
                | State::TrayOpen { .. }
                | State::TrayClosing
                | State::NoDisc
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
                    self.status[8] = self.status_flags();
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
                    self.status[8] = self.status_flags();
                }
                ReportType::CurrentTrack => {
                    // Write current track number (BCD) to Status 2-3
                    let current_time = self.state.current_time();
                    let track_number =
                        disc.cue().find_track_by_time(current_time).map_or(0, |track| track.number);
                    self.status[2] = track_number / 10;
                    self.status[3] = track_number % 10;

                    self.status[8] = self.status_flags();
                }
                ReportType::DiscLength => {
                    // Write disc length in minutes/seconds/frames (BCD) to Status 2-7
                    let disc_end_time = disc.cue().last_track().end_time;
                    write_time_to_status(disc_end_time, &mut self.status);
                    self.status[8] = self.status_flags();
                }
                ReportType::StartAndEndTracks => {
                    // Write start track number to Status 2-3 and end track number to Status 4-5, both in BCD
                    // Assume start track number is always 1
                    self.status[2] = 0x00;
                    self.status[3] = 0x01;

                    let end_track_number = disc.cue().last_track().number;
                    self.status[4] = end_track_number / 10;
                    self.status[5] = end_track_number % 10;

                    self.status[8] = self.status_flags();
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

    fn status_flags(&self) -> u8 {
        // $04 if playing a data track, $00 otherwise
        let playing_data_track = match self.state {
            State::Playing(time) | State::PreparingToPlay { time, .. } => {
                self.disc.as_ref().is_some_and(|disc| {
                    disc.cue()
                        .find_track_by_time(time)
                        .is_some_and(|track| track.track_type == TrackType::Data)
                })
            }
            _ => false,
        };

        if playing_data_track { 0x04 } else { 0x00 }
    }

    fn current_cdd_status(&self) -> Status {
        match self.state {
            State::MotorStopped => Status::Stopped,
            State::NoDisc => Status::NoDisc,
            State::Paused(..) => Status::Paused,
            State::PreparingToPlay { .. } | State::Playing(..) => Status::Playing,
            State::Seeking { .. } => Status::Seeking,
            State::TrackSkipping { .. } => Status::TrackSkipping,
            State::FastForwarding(..) | State::Rewinding(..) => Status::Scanning,
            State::DiscStart => Status::DiscStart,
            State::DiscEnd(..) => Status::DiscEnd,
            State::TrayOpening { .. } | State::TrayClosing => Status::TrayMoving,
            State::TrayOpen { .. } => Status::TrayOpen,
            State::InvalidCommand(..) => Status::InvalidCommand,
            State::ReadingToc => Status::ReadingToc,
        }
    }

    fn execute_read_toc(&mut self, command: [u8; 10]) {
        let report_type = ReportType::from_command(command);
        self.report_type = report_type;

        log::trace!("  Report type changed to {report_type:?}");

        self.state = match (self.state, &self.disc, report_type) {
            (State::MotorStopped, None, _) => State::NoDisc,
            (State::MotorStopped, Some(_), _) => State::Paused(CdTime::ZERO),
            (_, Some(_), ReportType::TrackNStartTime(..)) => {
                // TOCN reports require reading the TOC; move back to start of disc
                State::ReadingToc
            }
            _ => self.state,
        };
    }

    fn execute_seek(&mut self, command: [u8; 10], next_status: ReaderStatus) {
        if self.disc.is_none() {
            self.state = State::NoDisc;
            return;
        }

        let Some(seek_time) = read_time_from_command(command) else {
            self.state = State::InvalidCommand(self.state.current_time());
            return;
        };

        let current_time = self.state.current_time();

        if seek_time == current_time {
            log::trace!(
                "Already at desired seek time {seek_time}; changing status to {next_status:?}"
            );
            self.state = match next_status {
                ReaderStatus::Paused => State::Paused(seek_time),
                ReaderStatus::Playing => {
                    State::PreparingToPlay { time: seek_time, clocks_remaining: PLAY_DELAY_CLOCKS }
                }
            };
            return;
        }

        // Enforce a minimum seek time of 7 cycles at 75 Hz
        // At least one game depends on this, the Japanese version of Cobra Command / Thunder Storm FX
        let seek_clocks = cmp::max(7, estimate_seek_clocks(current_time, seek_time));

        log::trace!(
            "Seeking from {current_time} to {seek_time}; estimated time {seek_clocks} 75Hz clocks"
        );

        self.state =
            State::Seeking { current_time, seek_time, next_status, clocks_remaining: seek_clocks };
    }

    fn execute_track_skip(&mut self, command: [u8; 10]) {
        let Some(disc) = &self.disc else {
            self.state = State::NoDisc;
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
            State::TrackSkipping { current_time, skip_time, clocks_remaining: clocks_required };
    }

    pub fn status(&self) -> [u8; 10] {
        self.status
    }

    fn reading_data_track(&self) -> bool {
        match self.state {
            State::Playing(current_time) => self.disc.as_ref().is_some_and(|disc| {
                disc.cue()
                    .find_track_by_time(current_time)
                    .is_some_and(|track| track.track_type == TrackType::Data)
            }),
            _ => false,
        }
    }

    pub fn playing_audio(&self) -> bool {
        match self.state {
            State::Playing(current_time) => {
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

    pub fn set_fader_volume(&mut self, fader_volume: u16) {
        self.fader_volume = cmp::min(MAX_FADER_VOLUME, fader_volume);

        log::trace!("Fader volume set to {:03X}", self.fader_volume);
    }

    pub fn clock_44100hz(
        &mut self,
        rchip: &mut Rchip,
        mut rchip_dma_args: RchipDmaArgs<'_>,
    ) -> SegaCdLoadResult<(f64, f64)> {
        self.adjust_fader_volume();

        let audio_sample = self.sample_audio().unwrap_or((0.0, 0.0));

        let divider_decrement = if self.reading_data_track() { self.data_speed } else { 1 };
        for _ in 0..divider_decrement {
            self.divider_75hz -= 1;
            if self.divider_75hz == 0 {
                self.divider_75hz = DIVIDER_75HZ;
                self.clock_75hz(rchip)?;
                rchip.clock_75hz();
            } else {
                rchip.clock_44100hz(rchip_dma_args.reborrow());
            }
        }

        Ok(audio_sample)
    }

    fn adjust_fader_volume(&mut self) {
        match self.current_volume.cmp(&self.fader_volume) {
            Ordering::Less => self.current_volume += 1,
            Ordering::Greater => self.current_volume -= 1,
            Ordering::Equal => {}
        }
    }

    fn sample_audio(&mut self) -> Option<(f64, f64)> {
        if !self.playing_audio() {
            self.audio_sample_idx = 0;
            return None;
        }

        let idx = self.audio_sample_idx as usize;

        let sample_l = i16::from_le_bytes([self.sector_buffer[idx], self.sector_buffer[idx + 1]]);
        let sample_r =
            i16::from_le_bytes([self.sector_buffer[idx + 2], self.sector_buffer[idx + 3]]);

        let fader_multiplier = fader_volume_multiplier(self.current_volume);

        let sample_l = fader_multiplier * f64::from(sample_l) / -f64::from(i16::MIN);
        let sample_r = fader_multiplier * f64::from(sample_r) / -f64::from(i16::MIN);

        self.audio_sample_idx =
            (self.audio_sample_idx + BYTES_PER_AUDIO_SAMPLE) % cdrom::BYTES_PER_SECTOR as u16;

        Some((sample_l, sample_r))
    }

    fn clock_75hz(&mut self, rchip: &mut Rchip) -> SegaCdLoadResult<()> {
        // It is a bug if clock_75hz() is called when audio index is not 0; sample_audio() must
        // be called before clock_75hz() on the cycle when both are called
        assert_eq!(self.audio_sample_idx, 0);

        // CDD interrupt fires once every 1/75 of a second
        self.interrupt_pending = true;

        if let Some(time) = self.next_clock_play.take() {
            // If the state was just changed from playing, read one more sector before handling the
            // new state
            log::trace!("Reading sector at {time} before handling new state of {:?}", self.state);
            return self.handle_playing(time, false, rchip);
        }

        match self.state {
            State::Seeking { current_time, seek_time, next_status, clocks_remaining } => {
                if clocks_remaining == 1 {
                    log::trace!("Seek to {seek_time} complete");

                    self.state = match next_status {
                        ReaderStatus::Paused => State::Paused(seek_time),
                        ReaderStatus::Playing => State::PreparingToPlay {
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

                    self.state = State::Seeking {
                        current_time: new_time,
                        seek_time,
                        next_status,
                        clocks_remaining: clocks_remaining - 1,
                    };
                }
            }
            State::TrackSkipping { current_time, skip_time, clocks_remaining } => {
                if clocks_remaining == 1 {
                    log::trace!("Skip to {skip_time} complete");

                    self.state = State::Paused(skip_time);
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

                    self.state = State::TrackSkipping {
                        current_time: new_time,
                        skip_time,
                        clocks_remaining: clocks_remaining - 1,
                    };
                }
            }
            State::FastForwarding(time) => {
                let new_time = time + CdTime::new(0, FAST_FORWARD_SECONDS, FAST_FORWARD_FRAMES);
                self.state = if let Some(disc) = &self.disc {
                    let disc_end_time = disc.cue().last_track().end_time;
                    if new_time >= disc_end_time {
                        log::trace!("Fast-forwarded to end of disc");
                        State::DiscEnd(disc_end_time)
                    } else {
                        log::trace!("Fast-forwarded to {new_time}");
                        State::FastForwarding(new_time)
                    }
                } else {
                    log::trace!("Fast-forwarded to {new_time}");
                    State::FastForwarding(new_time)
                };
            }
            State::Rewinding(time) => {
                let new_time =
                    time.saturating_sub(CdTime::new(0, FAST_FORWARD_SECONDS, FAST_FORWARD_FRAMES));
                if new_time == CdTime::ZERO {
                    log::trace!("Rewound to beginning of disc");
                    self.state = State::DiscStart;
                } else {
                    log::trace!("Rewound to {new_time}");
                    self.state = State::Rewinding(new_time);
                }
            }
            State::PreparingToPlay { time, clocks_remaining } => {
                if clocks_remaining == 1 {
                    log::trace!("Beginning to play at {time}");

                    self.state = State::Playing(time);

                    // Ensure that leftover data in the buffer doesn't get played until the buffer is refilled
                    self.loaded_audio_sector = false;
                } else {
                    self.state =
                        State::PreparingToPlay { time, clocks_remaining: clocks_remaining - 1 };
                }
            }
            State::Playing(time) => {
                log::trace!("Playing at {time}");
                self.handle_playing(time, true, rchip)?;
            }
            State::MotorStopped => {
                match &self.disc {
                    Some(_) => {
                        // Always transition to Reading TOC one clock after the motor is stopped; this fixes
                        // the EU BIOS freezing after leaving the options menu
                        self.state = State::ReadingToc;
                    }
                    None => {
                        self.state = State::NoDisc;
                    }
                }
            }
            State::TrayOpening { auto_close } => {
                self.state = State::TrayOpen { auto_close };
            }
            State::TrayClosing => {
                self.state = State::MotorStopped;
                self.report_type = ReportType::AbsoluteTime;
            }
            State::TrayOpen { auto_close: true } => {
                self.state = State::TrayClosing;
            }
            _ => {}
        }

        Ok(())
    }

    fn handle_playing(
        &mut self,
        time: CdTime,
        change_state: bool,
        rchip: &mut Rchip,
    ) -> SegaCdLoadResult<()> {
        let Some(disc) = &mut self.disc else {
            if change_state {
                self.state = State::NoDisc;
            }
            return Ok(());
        };

        let Some(track) = disc.cue().find_track_by_time(time) else {
            if change_state {
                self.state = State::DiscEnd(disc.cue().last_track().end_time);
            }
            return Ok(());
        };

        let relative_time = time - track.start_time;
        let track_type = track.track_type;
        disc.read_sector(track.number, relative_time, self.sector_buffer.as_mut())?;

        self.loaded_audio_sector = track_type == TrackType::Audio;

        rchip.decode_block(&self.sector_buffer);

        if change_state {
            self.state = State::Playing(time + CdTime::new(0, 0, 1));
        }

        Ok(())
    }

    pub fn interrupt_pending(&self) -> bool {
        self.interrupt_pending
    }

    pub fn acknowledge_interrupt(&mut self) {
        self.interrupt_pending = false;
    }

    #[allow(clippy::items_after_statements)]
    pub fn disc_title(&mut self, region: GenesisRegion) -> SegaCdLoadResult<Option<String>> {
        let Some(disc) = &mut self.disc else { return Ok(None) };

        // Title information is always stored in the first sector of track 1
        disc.read_sector(1, CdTime::SECTOR_0_START, self.sector_buffer.as_mut())?;

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

        static WHITESPACE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" +").unwrap());

        Ok(Some(WHITESPACE_RE.replace_all(title.trim(), " ").to_string()))
    }

    pub fn take_disc(&mut self) -> Option<CdRom> {
        self.disc.take()
    }

    pub fn take_disc_from(&mut self, other: &mut Self) {
        self.disc = other.disc.take();
    }

    pub fn reset(&mut self) {
        self.state = State::default();
        self.report_type = ReportType::default();
        self.status = INITIAL_STATUS;
        self.interrupt_pending = false;
    }

    pub fn remove_disc(&mut self) {
        log::info!("Removing disc");

        self.disc = None;
        self.state = State::TrayOpening { auto_close: self.model == CdModel::Two };
    }

    pub fn change_disc<P: AsRef<Path>>(
        &mut self,
        rom_path: P,
        format: CdRomFileFormat,
        load_disc_into_ram: bool,
    ) -> SegaCdLoadResult<()> {
        let cue_path = rom_path.as_ref();

        log::info!("Changing disc to '{}'", cue_path.display());

        self.disc = Some(if load_disc_into_ram {
            CdRom::open_in_memory(cue_path, format)?
        } else {
            CdRom::open(cue_path, format)?
        });

        // Only open the tray if running a Model 2 BIOS version.
        // Model 1 BIOS versions will usually crash if the tray opens without the BIOS first sending
        // an "open tray" command
        if self.model == CdModel::Two {
            self.state = State::TrayOpening { auto_close: true };
        }

        Ok(())
    }

    pub fn reload_config(&mut self, config: &SegaCdEmulatorConfig) {
        self.data_speed = config.disc_drive_speed.get();
    }
}

fn fader_volume_multiplier(volume: u16) -> f64 {
    // Yes, 1025; fader volumes range from 0 to 1024 inclusive
    static LOOKUP_TABLE: LazyLock<[f64; 1025]> = LazyLock::new(|| {
        let mut lookup_table = [0.0; 1025];

        // The fader appears to use a linear scale, not logarithmic.
        //
        // Based on LC7883 documentation:
        //   For every volume >3, multiplier is (Bits 9-2)/256
        //   For volumes 1-3, multiplier is (Bit 1)/512 + (Bit 0)/1024. This matches the dB values in LC7883 docs
        //   For volume 0, multiplier is 0 (i.e. attenuation -inf)
        for (i, value) in lookup_table.iter_mut().enumerate() {
            *value = match i {
                0..=3 => i as f64 / 1024.0,
                4..=1024 => (i >> 2) as f64 / 256.0,
                _ => unreachable!("array len is 1025"),
            };
        }

        lookup_table
    });

    LOOKUP_TABLE[volume as usize]
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
