//! Code for emulating the CD-ROM² add-on's SCSI CD-ROM drive

mod seektime;

use crate::cd::{CdInterruptFlags, CdInterruptType};
use bincode::{Decode, Encode};
use cdrom::CdRomError;
use cdrom::cdtime::CdTime;
use cdrom::cue::{Track, TrackType};
use cdrom::reader::CdRom;
use jgenesis_proc_macros::PartialClone;
use std::cmp;
use std::collections::VecDeque;
use std::ops::ControlFlow;

const MCLK_FREQUENCY: u64 = crate::api::MASTER_CLOCK_FREQUENCY as u64;
const CD_FREQUENCY: u64 = 44100;
const SAMPLES_PER_SECTOR: u32 = 588;

const PREPARE_READ_CYCLES: u32 = 6 * SAMPLES_PER_SECTOR;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct ScsiBusSignals {
    pub bsy: bool, // Busy
    pub sel: bool, // Select
    pub req: bool, // Request
    pub ack: bool, // Acknowledge
    pub msg: bool, // Message
    pub c_d: bool, // Control (1) / Data (0)
    pub i_o: bool, // Input (1) / Output (0)
    pub rst: bool, // Reset
}

impl ScsiBusSignals {
    pub fn data_in_phase(self) -> bool {
        !self.msg && !self.c_d && self.i_o
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum ScsiBusPhase {
    BusFree,
    Command,
    ProcessingCommand, // STATUS bus phase but before drive sets REQ=1
    Status,
    DataIn,
    MessageIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScsiCommand {
    // Standard SCSI commands
    TestUnitReady,
    RequestSense,
    Read6,
    // PCE vendor-specific commands
    AudioStartPosition,
    AudioEndPosition,
    AudioPause,
    ReadSubchannelQ,
    ReadToc,
}

impl ScsiCommand {
    fn from_byte(command_byte: u8) -> Option<Self> {
        match command_byte {
            0x00 => Some(Self::TestUnitReady),
            0x03 => Some(Self::RequestSense),
            0x08 => Some(Self::Read6),
            0xD8 => Some(Self::AudioStartPosition),
            0xD9 => Some(Self::AudioEndPosition),
            0xDA => Some(Self::AudioPause),
            0xDD => Some(Self::ReadSubchannelQ),
            0xDE => Some(Self::ReadToc),
            _ => None,
        }
    }

    fn length(self) -> usize {
        match self {
            Self::TestUnitReady | Self::RequestSense | Self::Read6 => 6,
            Self::AudioStartPosition
            | Self::AudioEndPosition
            | Self::AudioPause
            | Self::ReadSubchannelQ
            | Self::ReadToc => 10,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScsiStatus {
    Good = 0,
    CheckCondition = 1,
}

impl ScsiStatus {
    const fn to_status_byte(self) -> u8 {
        (self as u8) << 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum SenseKey {
    NoSense = 0x0,
    NotReady = 0x2,
    MediumError = 0x3,
    IllegalRequest = 0x5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum SeekMode {
    Data { length: u32 },
    Audio,
    Pause,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum DriveState {
    // Cycle values are 44100 Hz cycles
    Paused(CdTime),
    AudioPaused(CdTime),
    Seeking { from: CdTime, to: CdTime, cycles_remaining: u32, mode: SeekMode },
    PreparingToRead { time: CdTime, cycles_remaining: u32, mode: SeekMode },
    Reading { time: CdTime, sectors_remaining: u32 },
    Playing { time: CdTime },
}

impl DriveState {
    fn current_time(self) -> CdTime {
        match self {
            Self::Paused(time)
            | Self::AudioPaused(time)
            | Self::Seeking { from: time, .. }
            | Self::PreparingToRead { time, .. }
            | Self::Reading { time, .. }
            | Self::Playing { time, .. } => time,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum AudioPlaybackMode {
    Off,
    PlayOnce { interrupt: bool },
    PlayLoop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
struct AudioPlaybackState {
    buffer_idx: u16,
    current_sample: (i16, i16),
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct ScsiCdDrive {
    #[partial_clone(default)]
    disc: Option<CdRom>,
    signals: ScsiBusSignals,
    phase: ScsiBusPhase,
    data_bus: u8,
    command_bytes: Vec<u8>,
    data_in_bytes: VecDeque<u8>,
    sense_key: SenseKey,
    drive_state: DriveState,
    audio_playback_mode: AudioPlaybackMode,
    audio_start_time: CdTime,
    audio_end_time: CdTime,
    sector_buffer: Box<[u8; cdrom::BYTES_PER_SECTOR as usize]>,
    audio_playback_state: Option<AudioPlaybackState>,
    cd_cycle_product: u64,
    divider_75hz: u32,
    audio_samples: VecDeque<(i16, i16)>,
}

impl ScsiCdDrive {
    pub fn new(disc: Option<CdRom>) -> Self {
        Self {
            disc,
            signals: ScsiBusSignals::default(),
            phase: ScsiBusPhase::BusFree,
            data_bus: 0,
            command_bytes: Vec::with_capacity(10),
            data_in_bytes: VecDeque::with_capacity(2048),
            sense_key: SenseKey::NoSense,
            drive_state: DriveState::Paused(CdTime::ZERO),
            audio_playback_mode: AudioPlaybackMode::Off,
            audio_start_time: CdTime::ZERO,
            audio_end_time: CdTime::ZERO,
            sector_buffer: vec![0; cdrom::BYTES_PER_SECTOR as usize]
                .into_boxed_slice()
                .try_into()
                .unwrap(),
            audio_playback_state: None,
            cd_cycle_product: 0,
            divider_75hz: SAMPLES_PER_SECTOR,
            audio_samples: VecDeque::with_capacity(44100 / 60),
        }
    }

    pub fn tick(
        &mut self,
        mclk_elapsed: u64,
        irqs_pending: &mut CdInterruptFlags,
    ) -> Result<(), CdRomError> {
        self.cd_cycle_product += mclk_elapsed * CD_FREQUENCY;
        while self.cd_cycle_product >= MCLK_FREQUENCY {
            self.cd_cycle_product -= MCLK_FREQUENCY;
            self.clock_44100hz(irqs_pending)?;
        }

        Ok(())
    }

    // Signed 16-bit samples, 44100 Hz sample rate
    pub fn drain_audio_samples(&mut self) -> impl Iterator<Item = (i16, i16)> {
        self.audio_samples.drain(..)
    }

    fn clock_44100hz(&mut self, irqs_pending: &mut CdInterruptFlags) -> Result<(), CdRomError> {
        self.divider_75hz -= 1;
        if self.divider_75hz == 0 {
            self.divider_75hz = SAMPLES_PER_SECTOR;
            self.clock_75hz()?;
        }

        match &mut self.drive_state {
            DriveState::Seeking { from, to, cycles_remaining, mode, .. } => {
                *cycles_remaining -= 1;
                if *cycles_remaining == 0 {
                    log::debug!("Seek to {to} finished");

                    match *mode {
                        SeekMode::Pause => {
                            self.drive_state = DriveState::AudioPaused(*to);

                            if self.phase == ScsiBusPhase::ProcessingCommand {
                                self.enter_status_phase(ScsiStatus::Good, SenseKey::NoSense);
                            }
                        }
                        SeekMode::Data { .. } | SeekMode::Audio => {
                            self.drive_state = DriveState::PreparingToRead {
                                time: *to,
                                cycles_remaining: PREPARE_READ_CYCLES,
                                mode: *mode,
                            };
                        }
                    }
                } else {
                    *from = estimate_mid_seek_time(*from, *to, *cycles_remaining);
                }
            }
            DriveState::PreparingToRead { time, cycles_remaining, mode } => {
                *cycles_remaining -= 1;
                if *cycles_remaining == 0 {
                    log::debug!("Beginning read at {time}");

                    self.drive_state = match *mode {
                        SeekMode::Data { length } => {
                            DriveState::Reading { time: *time, sectors_remaining: length }
                        }
                        SeekMode::Audio => DriveState::Playing { time: *time },
                        SeekMode::Pause => DriveState::AudioPaused(*time),
                    };

                    // In "play with interrupt" mode, don't set REQ=1 and trigger the status IRQ
                    // until playback is finished
                    if self.phase == ScsiBusPhase::ProcessingCommand
                        && self.audio_playback_mode
                            != (AudioPlaybackMode::PlayOnce { interrupt: true })
                    {
                        self.enter_status_phase(ScsiStatus::Good, SenseKey::NoSense);
                    }
                }
            }
            DriveState::Paused(_)
            | DriveState::AudioPaused(_)
            | DriveState::Reading { .. }
            | DriveState::Playing { .. } => {}
        }

        if let Some(state) = &mut self.audio_playback_state {
            let idx = 4 * state.buffer_idx as usize;

            state.current_sample = (
                i16::from_le_bytes([self.sector_buffer[idx], self.sector_buffer[idx + 1]]),
                i16::from_le_bytes([self.sector_buffer[idx + 2], self.sector_buffer[idx + 3]]),
            );
            state.buffer_idx += 1;
        }

        self.audio_samples.push_back(
            self.audio_playback_state.map(|state| state.current_sample).unwrap_or((0, 0)),
        );

        self.update_irq_flags(irqs_pending);

        Ok(())
    }

    fn clock_75hz(&mut self) -> Result<(), CdRomError> {
        fn read_sector(
            time: CdTime,
            drive: &mut ScsiCdDrive,
        ) -> Result<ControlFlow<()>, CdRomError> {
            let Some(disc) = &mut drive.disc else {
                drive.drive_state = DriveState::Paused(CdTime::ZERO);
                drive.enter_status_phase(ScsiStatus::CheckCondition, SenseKey::NotReady);
                return Ok(ControlFlow::Break(()));
            };

            let Some(track) = disc.cue().find_track_by_time(time) else {
                drive.drive_state = DriveState::Paused(drive.drive_state.current_time());
                drive.enter_status_phase(ScsiStatus::CheckCondition, SenseKey::IllegalRequest);
                return Ok(ControlFlow::Break(()));
            };

            log::debug!(
                "Reading sector at {time}, audio={}",
                drive.audio_playback_mode != AudioPlaybackMode::Off
            );

            let relative_time = time - track.start_time;
            disc.read_sector(track.number, relative_time, drive.sector_buffer.as_mut_slice())?;

            Ok(ControlFlow::Continue(()))
        }

        self.audio_playback_state = None;

        match self.drive_state {
            DriveState::Reading { time, sectors_remaining } => {
                if read_sector(time, self)? == ControlFlow::Break(()) {
                    return Ok(());
                }

                if !self.data_in_bytes.is_empty() {
                    log::debug!(
                        "DATA IN buffer not empty; {} bytes remaining, restarting read at {time}",
                        self.data_in_bytes.len()
                    );

                    // If the game has not yet drained the previous sector, restart the read at the
                    // current position.
                    // Timing is based on Sherlock Holmes which depends on this delay for
                    // video/audio sync in cutscenes
                    self.drive_state = DriveState::Seeking {
                        from: time + CdTime::new(0, 0, 1),
                        to: time,
                        cycles_remaining: 12000,
                        mode: SeekMode::Data { length: sectors_remaining },
                    };
                    return Ok(());
                }

                // TODO this assumes Mode 1, are any PCE games Mode 2?
                self.data_in_bytes.extend(&self.sector_buffer[16..16 + 2048]);

                // TODO what if bus is no longer in DATA IN phase? e.g. reset or SEL=1
                self.signals.req = true;
                self.data_bus = self.data_in_bytes.pop_front().unwrap();

                let new_time = time + CdTime::new(0, 0, 1);
                let new_sectors_remaining = sectors_remaining - 1;
                self.drive_state = if new_sectors_remaining == 0 {
                    log::debug!("Finished read, pausing drive at {new_time}");
                    DriveState::Paused(new_time)
                } else {
                    log::debug!("{new_sectors_remaining} sectors remaining");
                    DriveState::Reading { time: new_time, sectors_remaining: new_sectors_remaining }
                };
            }
            DriveState::Playing { time } => {
                if read_sector(time, self)? == ControlFlow::Break(()) {
                    return Ok(());
                }

                self.audio_playback_state =
                    Some(AudioPlaybackState { buffer_idx: 0, current_sample: (0, 0) });

                let new_time = time + CdTime::new(0, 0, 1);
                self.drive_state = if new_time >= self.audio_end_time {
                    match self.audio_playback_mode {
                        AudioPlaybackMode::PlayOnce { interrupt } => {
                            log::debug!("Audio playback finished, pausing drive");

                            if interrupt {
                                self.enter_status_phase(ScsiStatus::Good, SenseKey::NoSense);
                            }

                            DriveState::Paused(new_time)
                        }
                        AudioPlaybackMode::PlayLoop => {
                            log::debug!("Looping audio back to {}", self.audio_start_time);

                            let seek_cycles =
                                seektime::estimate_clocks(new_time, self.audio_start_time);
                            DriveState::Seeking {
                                from: new_time,
                                to: self.audio_start_time,
                                cycles_remaining: seek_cycles,
                                mode: SeekMode::Audio,
                            }
                        }
                        AudioPlaybackMode::Off => {
                            log::error!(
                                "Audio playback mode is off at audio end time; this is a bug"
                            );
                            DriveState::Paused(new_time)
                        }
                    }
                } else {
                    DriveState::Playing { time: new_time }
                };
            }
            DriveState::Paused(..)
            | DriveState::AudioPaused(_)
            | DriveState::Seeking { .. }
            | DriveState::PreparingToRead { .. } => {}
        }

        Ok(())
    }

    pub fn current_audio_sample(&self) -> (i16, i16) {
        self.audio_playback_state.map(|state| state.current_sample).unwrap_or((0, 0))
    }

    pub fn data_bus(&self) -> u8 {
        self.data_bus
    }

    pub fn set_data_bus(&mut self, value: u8) {
        self.data_bus = value;
    }

    pub fn signals(&self) -> ScsiBusSignals {
        self.signals
    }

    pub fn set_sel(&mut self, sel: bool, irqs_pending: &mut CdInterruptFlags) {
        self.signals.sel = sel;
        self.update_state(irqs_pending);
    }

    pub fn set_ack(&mut self, ack: bool, irqs_pending: &mut CdInterruptFlags) {
        self.signals.ack = ack;
        self.update_state(irqs_pending);
    }

    pub fn set_rst(&mut self, rst: bool, irqs_pending: &mut CdInterruptFlags) {
        self.signals.rst = rst;
        self.update_state(irqs_pending);
    }

    fn update_state(&mut self, irqs_pending: &mut CdInterruptFlags) {
        if self.signals.rst {
            self.set_phase(ScsiBusPhase::BusFree);
            irqs_pending.clear(CdInterruptType::ScsiDataIn);
            irqs_pending.clear(CdInterruptType::ScsiStatus);
            irqs_pending.clear(CdInterruptType::Subchannel);
            return;
        }

        if self.signals.sel && self.phase != ScsiBusPhase::BusFree {
            // Setting SEL=1 mid-command seems to cause the drive to stop driving all bus signals,
            // or at least that's what games seem to expect?
            self.set_phase(ScsiBusPhase::BusFree);
        } else {
            match self.phase {
                ScsiBusPhase::BusFree => self.update_state_bus_free(),
                ScsiBusPhase::Command => self.update_state_command(),
                ScsiBusPhase::ProcessingCommand => {}
                ScsiBusPhase::Status => self.update_state_status(),
                ScsiBusPhase::DataIn => self.update_state_data_in(),
                ScsiBusPhase::MessageIn => self.update_state_message_in(),
            }
        }

        self.update_irq_flags(irqs_pending);
    }

    fn update_irq_flags(&self, irqs_pending: &mut CdInterruptFlags) {
        irqs_pending.clear(CdInterruptType::ScsiDataIn);
        irqs_pending.clear(CdInterruptType::ScsiStatus);
        if self.signals.req && self.signals.i_o {
            // STATUS, DATA IN, or MESSAGE IN phase with data available to initiator
            irqs_pending.set(if self.signals.c_d {
                CdInterruptType::ScsiStatus
            } else {
                CdInterruptType::ScsiDataIn
            });
        }
    }

    fn update_state_bus_free(&mut self) {
        if self.signals.sel {
            self.set_phase(ScsiBusPhase::Command);
            self.command_bytes.clear();
        }
    }

    fn update_state_command(&mut self) {
        if self.signals.req && self.signals.ack {
            // New byte available from initiator
            self.command_bytes.push(self.data_bus);
            self.signals.req = false;
        } else if !self.signals.req && !self.signals.ack {
            // Byte handshake finished
            if !self.command_bytes.is_empty() {
                let Some(command) = ScsiCommand::from_byte(self.command_bytes[0]) else {
                    // Invalid/unsupported command
                    log::warn!("Unsupported SCSI command byte: {:02X}", self.command_bytes[0]);
                    return self
                        .enter_status_phase(ScsiStatus::CheckCondition, SenseKey::IllegalRequest);
                };

                if self.command_bytes.len() < command.length() {
                    // Need more bytes from initiator
                    self.signals.req = true;
                } else {
                    // Command fully received
                    self.process_command(command);
                }
            }
        }
    }

    fn update_state_status(&mut self) {
        if self.signals.req && self.signals.ack {
            self.signals.req = false;
        } else if !self.signals.req && !self.signals.ack {
            self.set_phase(ScsiBusPhase::MessageIn);
            self.data_bus = 0; // Command complete message code
        }
    }

    fn update_state_data_in(&mut self) {
        if self.signals.req && self.signals.ack {
            self.signals.req = false;
        } else if !self.signals.req && !self.signals.ack {
            match self.data_in_bytes.pop_front() {
                Some(byte) => {
                    self.data_bus = byte;
                    self.signals.req = true;

                    log::trace!("DATA IN phase, {} bytes remaining", self.data_in_bytes.len());
                }
                None => match self.drive_state {
                    DriveState::Reading { .. }
                    | DriveState::PreparingToRead { mode: SeekMode::Data { .. }, .. }
                    | DriveState::Seeking { mode: SeekMode::Data { .. }, .. } => {}
                    _ => {
                        self.enter_status_phase(ScsiStatus::Good, SenseKey::NoSense);
                    }
                },
            }
        }
    }

    fn update_state_message_in(&mut self) {
        if self.signals.req && self.signals.ack {
            self.signals.req = false;
        } else if !self.signals.req && !self.signals.ack {
            self.set_phase(ScsiBusPhase::BusFree);
        }
    }

    fn set_phase(&mut self, phase: ScsiBusPhase) {
        if phase == self.phase {
            return;
        }
        self.phase = phase;

        log::debug!("SCSI bus phase changed to {phase:?}");

        if phase != ScsiBusPhase::DataIn {
            self.abort_data_read();
        }

        match phase {
            ScsiBusPhase::BusFree => {
                self.signals.bsy = false;
                self.signals.req = false;
                self.signals.msg = false;
                self.signals.c_d = false;
                self.signals.i_o = false;
            }
            ScsiBusPhase::Command => {
                self.signals.bsy = true;
                self.signals.req = true;
                self.signals.msg = false;
                self.signals.c_d = true;
                self.signals.i_o = false;
            }
            ScsiBusPhase::ProcessingCommand => {
                self.signals.bsy = true;
                self.signals.req = false;
                self.signals.msg = false;
                self.signals.c_d = true;
                self.signals.i_o = true;
            }
            ScsiBusPhase::Status => {
                self.signals.bsy = true;
                self.signals.req = true;
                self.signals.msg = false;
                self.signals.c_d = true;
                self.signals.i_o = true;
            }
            ScsiBusPhase::DataIn => {
                self.signals.bsy = true;
                self.signals.req = true;
                self.signals.msg = false;
                self.signals.c_d = false;
                self.signals.i_o = true;
            }
            ScsiBusPhase::MessageIn => {
                self.signals.bsy = true;
                self.signals.req = true;
                self.signals.msg = true;
                self.signals.c_d = true;
                self.signals.i_o = true;
            }
        }
    }

    fn abort_data_read(&mut self) {
        match self.drive_state {
            DriveState::Reading { time, sectors_remaining }
            | DriveState::PreparingToRead {
                time,
                mode: SeekMode::Data { length: sectors_remaining },
                ..
            }
            | DriveState::Seeking {
                from: time,
                mode: SeekMode::Data { length: sectors_remaining },
                ..
            } => {
                log::debug!(
                    "Aborting in-progress read at {time}, had {sectors_remaining} sectors remaining"
                );
                self.drive_state = DriveState::Paused(time);
            }
            _ => {
                // Drive is not reading or preparing to read, leave it alone
            }
        }
    }

    fn enter_status_phase(&mut self, status: ScsiStatus, sense_key: SenseKey) {
        self.set_phase(ScsiBusPhase::Status);
        self.data_bus = status.to_status_byte();
        self.sense_key = sense_key;
    }

    fn enter_data_in_phase(&mut self) {
        assert!(!self.data_in_bytes.is_empty(), "enter_data_in_phase() called with no data ready");

        self.set_phase(ScsiBusPhase::DataIn);
        self.data_bus = self.data_in_bytes.pop_front().unwrap();
    }

    fn process_command(&mut self, command: ScsiCommand) {
        log::debug!("Processing command {command:?}, command bytes: {:02X?}", self.command_bytes);

        match command {
            ScsiCommand::TestUnitReady => self.handle_test_unit_ready(),
            ScsiCommand::RequestSense => self.handle_request_sense(),
            ScsiCommand::Read6 => self.handle_read(),
            ScsiCommand::AudioStartPosition => self.handle_set_audio_start_position(),
            ScsiCommand::AudioEndPosition => self.handle_set_audio_end_position(),
            ScsiCommand::AudioPause => self.handle_audio_pause(),
            ScsiCommand::ReadSubchannelQ => self.handle_read_subchannel_q(),
            ScsiCommand::ReadToc => self.handle_read_toc(),
        }
    }

    // Command 0x00 - TEST UNIT READY
    fn handle_test_unit_ready(&mut self) {
        match &self.disc {
            Some(_) => {
                self.enter_status_phase(ScsiStatus::Good, SenseKey::NoSense);
            }
            None => {
                self.enter_status_phase(ScsiStatus::CheckCondition, SenseKey::NotReady);
            }
        }
    }

    // Command 0x03 - REQUEST SENSE
    fn handle_request_sense(&mut self) {
        log::debug!("REQUEST SENSE command executed; sense key = {:?}", self.sense_key);

        let requested_len = self.command_bytes[4] as usize;
        if requested_len == 0 {
            // ???
            return self.enter_status_phase(ScsiStatus::CheckCondition, SenseKey::IllegalRequest);
        }

        self.data_in_bytes.clear();
        self.data_in_bytes.extend([
            0x70,                 // Error code
            0,                    // Segment number
            self.sense_key as u8, // Sense key and status bits, TODO EOM bit?
            0,                    // Information (4 bytes)
            0,
            0,
            0,
            10, // Additional sense length
            0,  // Command-specific information (4 bytes)
            0,
            0,
            0,
            0, // Additional sense code (ASC)
            0, // Additional sense code qualifier (ASCQ)
            0, // Field replaceable unit code
            0, // Sense key specific (3 bytes)
            0,
            0,
        ]);

        assert_eq!(self.data_in_bytes.len(), 18);

        if requested_len < self.data_in_bytes.len() {
            self.data_in_bytes.truncate(requested_len);
        }

        self.enter_data_in_phase();
    }

    // Command 0x08 - READ(6)
    fn handle_read(&mut self) {
        let Some(disc) = &self.disc else {
            return self.enter_status_phase(ScsiStatus::CheckCondition, SenseKey::NotReady);
        };

        // Add 150 (2 seconds of frames) because LBA 0 means MSF 00:02:00
        let raw_lba = u32::from_be_bytes([
            0,
            self.command_bytes[1] & 0x1F,
            self.command_bytes[2],
            self.command_bytes[3],
        ]);
        let sector_number = raw_lba + 150;

        let read_len = self.command_bytes[4];
        // TODO SCSI spec says length of 0 means 256, but unclear whether PCE follows that
        let read_len: u32 = if read_len == 0 { 256 } else { read_len.into() };

        log::debug!("READ command, LBA={sector_number} length={read_len}");

        if sector_number >= disc.cue().last_track().end_time.to_sector_number() {
            return self.enter_status_phase(ScsiStatus::CheckCondition, SenseKey::IllegalRequest);
        }

        let seek_time = CdTime::from_sector_number(sector_number);
        self.start_seek(seek_time, SeekMode::Data { length: read_len });

        self.audio_playback_mode = AudioPlaybackMode::Off;

        // Enter DATA IN phase immediately, but leave REQ=0 until first sector is read
        self.set_phase(ScsiBusPhase::DataIn);
        self.signals.req = false;

        self.data_in_bytes.clear();
    }

    fn start_seek(&mut self, seek_time: CdTime, mode: SeekMode) {
        let current_time = self.drive_state.current_time();
        self.drive_state = if seek_time == current_time {
            DriveState::PreparingToRead {
                time: seek_time,
                cycles_remaining: PREPARE_READ_CYCLES,
                mode,
            }
        } else {
            let seek_cycles = seektime::estimate_clocks(current_time, seek_time);
            DriveState::Seeking {
                from: current_time,
                to: seek_time,
                cycles_remaining: seek_cycles,
                mode,
            }
        };

        self.audio_playback_state = None;

        log::debug!(
            "Starting seek from {current_time} to {seek_time} in mode {mode:?}, cycles: {}",
            match self.drive_state {
                DriveState::Seeking { cycles_remaining, .. }
                | DriveState::PreparingToRead { cycles_remaining, .. } => cycles_remaining,
                _ => 0,
            }
        );
    }

    // Command 0xD8 - Set audio start position (PCE vendor-specific)
    fn handle_set_audio_start_position(&mut self) {
        let Some(disc) = &self.disc else {
            return self.enter_status_phase(ScsiStatus::CheckCondition, SenseKey::NotReady);
        };

        let Some(start_time) = Self::parse_audio_position_time(&self.command_bytes, disc) else {
            return self.enter_status_phase(ScsiStatus::CheckCondition, SenseKey::IllegalRequest);
        };

        self.audio_start_time = start_time;
        self.audio_end_time = disc.cue().last_track().end_time;

        log::debug!("Audio start position set to {start_time}");

        self.audio_playback_mode = if self.command_bytes[1] != 0 {
            AudioPlaybackMode::PlayOnce { interrupt: false }
        } else {
            AudioPlaybackMode::Off
        };

        log::debug!("Audio playback mode: {:?}", self.audio_playback_mode);

        self.start_seek(
            start_time,
            match self.audio_playback_mode {
                AudioPlaybackMode::Off => SeekMode::Pause,
                _ => SeekMode::Audio,
            },
        );

        // Wait to set REQ=1 until drive finishes seeking to start time; games seem to rely on this
        // for correct audio timing
        self.set_phase(ScsiBusPhase::ProcessingCommand);
    }

    // Command 0xD9 - Set audio end position (PCE vendor-specific)
    fn handle_set_audio_end_position(&mut self) {
        let Some(disc) = &self.disc else {
            return self.enter_status_phase(ScsiStatus::CheckCondition, SenseKey::NotReady);
        };

        let Some(end_time) = Self::parse_audio_position_time(&self.command_bytes, disc) else {
            return self.enter_status_phase(ScsiStatus::CheckCondition, SenseKey::IllegalRequest);
        };

        self.audio_end_time = end_time;

        log::debug!("Audio end time set to {end_time}");

        self.audio_playback_mode = match self.command_bytes[1] & 3 {
            0 => AudioPlaybackMode::Off,
            1 => AudioPlaybackMode::PlayLoop,
            2 => AudioPlaybackMode::PlayOnce { interrupt: true },
            3 => AudioPlaybackMode::PlayOnce { interrupt: false },
            _ => unreachable!("value & 3 is always <= 3"),
        };

        log::debug!("Audio playback mode: {:?}", self.audio_playback_mode);

        match self.audio_playback_mode {
            AudioPlaybackMode::PlayLoop | AudioPlaybackMode::PlayOnce { .. } => {
                self.start_seek(self.audio_start_time, SeekMode::Audio);
                self.set_phase(ScsiBusPhase::ProcessingCommand);
            }
            AudioPlaybackMode::Off => {
                self.pause_audio_if_playing();
                self.enter_status_phase(ScsiStatus::Good, SenseKey::NoSense);
            }
        }
    }

    fn parse_audio_position_time(command: &[u8], disc: &CdRom) -> Option<CdTime> {
        match command[9] >> 6 {
            0 => {
                // LBA specified in bytes 3-5 (big-endian)
                // (Probably actually a 32-bit value in bytes 2-5, but safe to assume highest byte is
                // always 0 if that's the case; CD-ROM LBAs always fit in 19 bits)
                let raw_lba = u32::from_be_bytes([0, command[3], command[4], command[5]]);

                // Add 150 because LBA 0 should match MSF 00:02:00
                let sector_number = raw_lba + 150;

                CdTime::from_sector_number_checked(sector_number)
            }
            1 => {
                // MSF time specified in bytes 2-4
                CdTime::new_checked(
                    bcd_to_binary(command[2]),
                    bcd_to_binary(command[3]),
                    bcd_to_binary(command[4]),
                )
            }
            2 => {
                // Beginning of track specified in byte 2
                let track_number = cmp::max(1, bcd_to_binary(command[2]));
                if track_number > disc.cue().last_track().number {
                    return None;
                }

                Some(disc.cue().track(track_number).effective_start_time())
            }
            3 => {
                log::warn!("Invalid audio position in start/end pos command: {command:02X?}");
                None
            }
            _ => unreachable!("value >> 6 is always <= 3"),
        }
    }

    fn pause_audio_if_playing(&mut self) {
        if matches!(
            self.drive_state,
            DriveState::Playing { .. }
                | DriveState::Seeking { mode: SeekMode::Audio, .. }
                | DriveState::PreparingToRead { mode: SeekMode::Audio, .. }
        ) {
            self.drive_state = DriveState::AudioPaused(self.drive_state.current_time());
        }
    }

    // Command 0xDA - Pause audio (PCE vendor-specific)
    fn handle_audio_pause(&mut self) {
        if self.disc.is_none() {
            return self.enter_status_phase(ScsiStatus::CheckCondition, SenseKey::NotReady);
        }

        self.pause_audio_if_playing();

        log::debug!(
            "Audio pause command executed, now paused at {}",
            self.drive_state.current_time()
        );

        self.enter_status_phase(ScsiStatus::Good, SenseKey::NoSense);
    }

    // Command 0xDD - Read subchannel Q (PCE vendor-specific)
    fn handle_read_subchannel_q(&mut self) {
        struct SubchannelQInfo {
            track_number_bcd: u8,
            index: u8,
            relative_time: CdTime,
            absolute_time: CdTime,
        }

        let Some(disc) = &self.disc else {
            return self.enter_status_phase(ScsiStatus::CheckCondition, SenseKey::NotReady);
        };

        let audio_status = match self.drive_state {
            DriveState::Playing { .. }
            | DriveState::Seeking { mode: SeekMode::Audio, .. }
            | DriveState::PreparingToRead { mode: SeekMode::Audio, .. } => 0,
            DriveState::AudioPaused(_) => 2,
            _ => 3, // Stopped
        };

        // TODO what if drive is not reading/playing? return subchannel from last read sector?

        let current_time = self.drive_state.current_time();
        let track = disc.cue().find_track_by_time(current_time);

        let data_track_bit = track.is_some_and(|track| {
            // Per ECMA-130, subchannel Q control bits can't change within the pause portion of a
            // track (generally first 2 seconds), except in the lead-in to the first track
            let track_for_subchannel =
                if track.number > 1 && current_time < track.effective_start_time() {
                    disc.cue().track(track.number - 1)
                } else {
                    track
                };
            track_for_subchannel.track_type == TrackType::Data
        });

        // Lowest 4 bits 0001 indicates q-Mode 1 (track number + index + rel time + abs time)
        let control = 0x01 | (u8::from(data_track_bit) << 6);

        let info = match track {
            Some(track) => {
                let (index, relative_time) = if current_time < track.effective_start_time() {
                    // In pause section, relative time should begin at pause_len and count down to 00:00:00
                    (0, track.effective_start_time() - current_time)
                } else {
                    (1, current_time - track.effective_start_time())
                };

                SubchannelQInfo {
                    track_number_bcd: binary_to_bcd(track.number),
                    index,
                    relative_time,
                    absolute_time: current_time,
                }
            }
            None => {
                // End of disc
                SubchannelQInfo {
                    track_number_bcd: 0xAA,
                    index: 0,
                    relative_time: CdTime::ZERO,
                    absolute_time: disc.cue().last_track().end_time,
                }
            }
        };

        self.data_in_bytes.clear();
        self.data_in_bytes.extend([
            audio_status,
            control,
            info.track_number_bcd,
            info.index,
            binary_to_bcd(info.relative_time.minutes),
            binary_to_bcd(info.relative_time.seconds),
            binary_to_bcd(info.relative_time.frames),
            binary_to_bcd(info.absolute_time.minutes),
            binary_to_bcd(info.absolute_time.seconds),
            binary_to_bcd(info.absolute_time.frames),
        ]);

        assert_eq!(self.data_in_bytes.len(), 10);

        log::debug!(
            "Read subchannel Q command executed at time {current_time}, bytes: {:02X?}",
            self.data_in_bytes
        );

        self.enter_data_in_phase();
    }

    // Command 0xDE - Read TOC (PCE vendor-specific)
    fn handle_read_toc(&mut self) {
        let Some(disc) = &self.disc else {
            return self.enter_status_phase(ScsiStatus::CheckCondition, SenseKey::NotReady);
        };

        // TODO this command shouldn't execute instantly

        let cue = disc.cue();

        self.data_in_bytes.clear();

        match self.command_bytes[1] {
            0 => {
                // First/last track numbers
                self.data_in_bytes.extend([1, binary_to_bcd(cue.last_track().number)]);
            }
            1 => {
                // Disc length
                let disc_end_time = cue.last_track().end_time;
                self.data_in_bytes.extend([
                    binary_to_bcd(disc_end_time.minutes),
                    binary_to_bcd(disc_end_time.seconds),
                    binary_to_bcd(disc_end_time.frames),
                ]);
            }
            2 => {
                // Track N start time
                let track_number = cmp::max(1, bcd_to_binary(self.command_bytes[2]));

                let valid_track_number = track_number <= cue.last_track().number;
                let track = valid_track_number.then(|| cue.track(track_number));

                let track_time =
                    track.map(Track::effective_start_time).unwrap_or(cue.last_track().end_time);
                let is_data_track = track.is_some_and(|track| track.track_type == TrackType::Data);

                self.data_in_bytes.extend([
                    binary_to_bcd(track_time.minutes),
                    binary_to_bcd(track_time.seconds),
                    binary_to_bcd(track_time.frames),
                    if is_data_track { 4 } else { 0 }, // Flags
                ]);
            }
            _ => {
                // Invalid
                return self
                    .enter_status_phase(ScsiStatus::CheckCondition, SenseKey::IllegalRequest);
            }
        }

        log::debug!("Read TOC data bytes: {:02X?}", self.data_in_bytes);

        self.enter_data_in_phase();
    }

    pub fn take_disc(&mut self) -> Option<CdRom> {
        self.disc.take()
    }

    pub fn take_disc_from(&mut self, other: &mut Self) {
        self.disc = other.disc.take();
    }
}

fn binary_to_bcd(value: u8) -> u8 {
    (value % 10) + ((value / 10) << 4)
}

fn bcd_to_binary(value: u8) -> u8 {
    (value & 0x0F) + 10 * (value >> 4)
}

fn estimate_mid_seek_time(current: CdTime, to: CdTime, cycles_remaining: u32) -> CdTime {
    if cycles_remaining == 1 {
        return current;
    }

    // TODO this is not accurate, seeking is non-linear, but mid-seek current time is only used if
    // a game interrupts a seek with another command
    let diff = if current < to { to - current } else { current - to };
    let diff_frames = diff.to_sector_number();

    let elapsed_frames = (f64::from(diff_frames) / f64::from(cycles_remaining)).round() as u32;
    if current < to {
        current + CdTime::from_sector_number(elapsed_frames)
    } else {
        current.saturating_sub(CdTime::from_sector_number(elapsed_frames))
    }
}
