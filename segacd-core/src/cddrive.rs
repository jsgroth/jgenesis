// TODO remove
#![allow(dead_code)]

use crate::cdrom;
use crate::cdrom::cdtime::CdTime;
use crate::cdrom::cue::TrackType;
use crate::cdrom::reader::CdRom;
use crate::memory::ScdCpu;
use bincode::{Decode, Encode};
use genesis_core::GenesisRegion;
use jgenesis_traits::num::GetBit;
use regex::Regex;
use std::sync::OnceLock;
use std::{array, cmp, io};

const BUFFER_RAM_LEN: usize = 16 * 1024;

const INITIAL_STATUS: [u8; 10] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0F];
const STOP_MOTOR_STATUS: [u8; 10] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0F];
const NO_DISC_STATUS: [u8; 10] = [0x0B, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrescalerTickResult {
    None,
    Clocked,
}

#[derive(Debug, Clone, Encode, Decode)]
struct FramePrescaler {
    mclk_cycles: u64,
    prescaler_cycle: u8,
}

impl FramePrescaler {
    fn new() -> Self {
        Self { mclk_cycles: 0, prescaler_cycle: 0 }
    }

    fn tick(&mut self, mclk_cycles: u64) -> PrescalerTickResult {
        let threshold = match self.prescaler_cycle {
            0 => 666_667,
            1 => 1_333_333,
            2 => 2_000_000,
            _ => panic!("invalid prescaler divider cycle: {}", self.prescaler_cycle),
        };

        let clocked = self.mclk_cycles < threshold && self.mclk_cycles + mclk_cycles >= threshold;
        self.mclk_cycles = (self.mclk_cycles + mclk_cycles) % 2_000_000;
        if clocked {
            self.prescaler_cycle = (self.prescaler_cycle + 1) % 3;
            PrescalerTickResult::Clocked
        } else {
            PrescalerTickResult::None
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CddStatus {
    Stopped = 0x00,
    Playing = 0x01,
    Seeking = 0x02,
    Scanning = 0x03,
    Paused = 0x04,
    TrayOpen = 0x05,
    InvalidCommand = 0x07,
    ReadingToc = 0x09,
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
    Paused(CdTime),
    Seeking {
        current_time: CdTime,
        seek_time: CdTime,
        next_status: ReaderStatus,
        clocks_remaining: u8,
    },
    InvalidCommand(CdTime),
}

impl CddState {
    fn current_time(self) -> CdTime {
        match self {
            Self::MotorStopped | Self::NoDisc => CdTime::ZERO,
            Self::Paused(time) | Self::InvalidCommand(time) => time,
            Self::Seeking { current_time, .. } => current_time,
        }
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
    fn new(disc: Option<CdRom>) -> Self {
        Self {
            disc,
            sector_buffer: array::from_fn(|_| 0),
            state: CddState::MotorStopped,
            interrupt_pending: false,
            status: INITIAL_STATUS,
        }
    }

    pub fn send_command(&mut self, command: [u8; 10]) {
        log::trace!("CDD command: {command:02X?}");

        // TODO remove
        #[allow(clippy::match_same_arms)]
        match command[0] {
            0x00 => {
                // No-op; return current status
            }
            0x01 => {
                // Stop motor
                self.state = CddState::MotorStopped;
            }
            0x02 => {
                // Read TOC
                self.execute_read_toc(command);
            }
            0x03 => {
                // Seek and play
                self.execute_seek(command, ReaderStatus::Playing);
            }
            0x04 => {
                // Seek
                // TODO should seek during playback continue playing after seek?
                self.execute_seek(command, ReaderStatus::Paused);
            }
            0x06 => {
                // Pause
                todo!("Pause")
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

        // Status 1 is set to Command 3 for Read TOC commands ($02) and $00 otherwise
        if command[0] == 0x02 {
            self.status[1] = command[3];
        } else if command[0] != 0x00 {
            self.status[1] = 0x00;
        }

        update_cdd_checksum(&mut self.status);

        log::trace!("CDD status: {:02X?}", self.status);
    }

    fn current_cdd_status(&self) -> CddStatus {
        match self.state {
            CddState::MotorStopped => CddStatus::Stopped,
            CddState::NoDisc => CddStatus::NoDisc,
            CddState::Paused(..) => CddStatus::Paused,
            CddState::Seeking { .. } => CddStatus::Seeking,
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
                let current_time = self.state.current_time();
                write_time_to_status(current_time, &mut self.status);
            }
            0x01 => {
                // Get relative position
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
                write_time_to_status(cue.last_track().end_time, &mut self.status);
            }
            0x04 => {
                // Get number of tracks

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
        let Some(seek_time) = read_time_from_command(command) else {
            self.state = CddState::InvalidCommand(self.state.current_time());
            return;
        };

        let current_time = self.state.current_time();

        let seek_clocks = estimate_seek_clocks(current_time, seek_time);

        log::trace!(
            "Seeking from {current_time} to {seek_time}; estimated time {seek_clocks} 75Hz clocks"
        );

        // TODO preserve state when playing
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

    pub fn clock(&mut self) {
        // CDD interrupt fires once every 1/75 of a second
        self.interrupt_pending = true;

        // TODO update state as needed
        if let CddState::Seeking { current_time, seek_time, next_status, clocks_remaining } =
            self.state
        {
            if clocks_remaining == 1 {
                log::trace!("Seek to {seek_time} complete");

                self.state = match next_status {
                    ReaderStatus::Paused => CddState::Paused(seek_time),
                    ReaderStatus::Playing => todo!("Play after seek"),
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
    }

    pub fn interrupt_pending(&self) -> bool {
        self.interrupt_pending
    }

    pub fn acknowledge_interrupt(&mut self) {
        self.interrupt_pending = false;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum DeviceDestination {
    #[default]
    MainCpuRegister,
    SubCpuRegister,
    Pcm,
    PrgRam,
    WordRam,
}

impl DeviceDestination {
    pub fn to_bits(self) -> u8 {
        match self {
            Self::MainCpuRegister => 0b010,
            Self::SubCpuRegister => 0b011,
            Self::Pcm => 0b100,
            Self::PrgRam => 0b101,
            Self::WordRam => 0b111,
        }
    }

    pub fn from_bits(bits: u8) -> Self {
        match bits & 0x07 {
            0b010 => Self::MainCpuRegister,
            0b011 => Self::SubCpuRegister,
            0b100 => Self::Pcm,
            0b101 => Self::PrgRam,
            0b111 => Self::WordRam,
            0b000 | 0b001 | 0b110 => {
                // Prohibited; just default to main CPU register
                log::warn!("Prohibited CDC device destination set: {:03b}", bits & 0x07);
                Self::MainCpuRegister
            }
            _ => unreachable!("value & 0x07 is always <= 0x07"),
        }
    }
}

// The LC8951, which the documentation describes as a "Real-Time Error Correction and Host Interface
// Processor".
//
// Sega CD documentation refers to this chip as the CDC.
#[derive(Debug, Clone, Encode, Decode)]
pub struct Rchip {
    buffer_ram: Box<[u8; BUFFER_RAM_LEN]>,
    device_destination: DeviceDestination,
    register_address: u8,
    dma_address: u32,
    transfer_end_interrupt_enabled: bool,
    decoder_interrupt_enabled: bool,
    data_out_enabled: bool,
    write_address: u16,
    block_pointer: u16,
}

impl Rchip {
    fn new() -> Self {
        Self {
            buffer_ram: vec![0; BUFFER_RAM_LEN].into_boxed_slice().try_into().unwrap(),
            device_destination: DeviceDestination::default(),
            register_address: 0,
            dma_address: 0,
            transfer_end_interrupt_enabled: true,
            decoder_interrupt_enabled: true,
            data_out_enabled: false,
            write_address: 0,
            block_pointer: 0,
        }
    }

    pub fn device_destination(&self) -> DeviceDestination {
        self.device_destination
    }

    pub fn set_device_destination(&mut self, device_destination: DeviceDestination) {
        // Changing device destination resets DMA controller
        if device_destination != self.device_destination {
            // TODO cancel any in-progress DMA
            self.dma_address = 0;
        }

        self.device_destination = device_destination;
    }

    pub fn read_host_data(&mut self, cpu: ScdCpu) -> u16 {
        if (cpu == ScdCpu::Main && self.device_destination != DeviceDestination::MainCpuRegister)
            || (cpu == ScdCpu::Sub && self.device_destination != DeviceDestination::SubCpuRegister)
        {
            // Invalid host data read
            return 0x0000;
        }

        todo!("read host data ({cpu:?})")
    }

    pub fn register_address(&self) -> u8 {
        self.register_address
    }

    pub fn set_register_address(&mut self, register_address: u8) {
        self.register_address = register_address;
    }

    pub fn read_register(&mut self) -> u8 {
        todo!("CDC read register")
    }

    pub fn write_register(&mut self, value: u8) {
        match self.register_address {
            1 => {
                // IFCTRL (Host Interface Control)
                // Intentionally ignoring CMDIEN, CMDBK, DTWAI, STWAI, SOUTEN bits

                log::trace!("IFCTRL write: {value:02X}");

                // Interrupt enable bits are active low
                self.transfer_end_interrupt_enabled = !value.bit(6);
                self.decoder_interrupt_enabled = !value.bit(5);

                self.data_out_enabled = value.bit(1);

                log::trace!("  DTEIEN: {}", self.transfer_end_interrupt_enabled);
                log::trace!("  DECIEN: {}", self.decoder_interrupt_enabled);
                log::trace!("  DOUTEN: {}", self.data_out_enabled);
            }
            8 => {
                // WAL (Write Address, Low Byte)

                log::trace!("WAL write: {value:02X}");

                self.write_address = (self.write_address & 0xFF00) | u16::from(value);

                log::trace!("  WA: {:04X}", self.write_address);
            }
            9 => {
                // WAH (Write Address, High Byte)

                log::trace!("WAH write: {value:02X}");

                self.write_address = (self.write_address & 0x00FF) | (u16::from(value) << 8);

                log::trace!("  WA: {:04X}", self.write_address);
            }
            12 => {
                // PTL (Block Pointer, Low Byte)

                log::trace!("PTL write: {value:02X}");

                self.block_pointer = (self.block_pointer & 0xFF00) | u16::from(value);

                log::trace!("  PT: {:04X}", self.block_pointer);
            }
            13 => {
                // PTH (Block Pointer, High Byte)

                log::trace!("PTH write: {value:02X}");

                self.block_pointer = (self.block_pointer & 0x00FF) | (u16::from(value) << 8);

                log::trace!("  PT: {:04X}", self.block_pointer);
            }
            14 => {
                // Unused, do nothing
            }
            15 => {
                // RESET

                self.transfer_end_interrupt_enabled = true;
                self.decoder_interrupt_enabled = true;
                self.data_out_enabled = false;
                // TODO reset chip; clear IFCTRL, CTRL0, and CTRL1
            }
            0 | 2 | 3 | 4 | 5 | 6 | 7 | 10 | 11 => {
                todo!("write CDC register {}", self.register_address)
            }
            _ => panic!("CDC register address should always be <= 15"),
        }
    }

    pub fn set_dma_address(&mut self, dma_address: u32) {
        self.dma_address = dma_address;
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct CdController {
    drive: CdDrive,
    rchip: Rchip,
    sector_buffer: [u8; cdrom::BYTES_PER_SECTOR as usize],
    prescaler_75hz: FramePrescaler,
}

impl CdController {
    pub fn new(disc: Option<CdRom>) -> Self {
        Self {
            drive: CdDrive::new(disc),
            rchip: Rchip::new(),
            sector_buffer: array::from_fn(|_| 0),
            prescaler_75hz: FramePrescaler::new(),
        }
    }

    pub fn tick(&mut self, mclk_cycles: u64) {
        if self.prescaler_75hz.tick(mclk_cycles) == PrescalerTickResult::Clocked {
            self.drive.clock();
        }

        // TODO CDC interrupts
    }

    pub fn cdd(&self) -> &CdDrive {
        &self.drive
    }

    pub fn cdd_mut(&mut self) -> &mut CdDrive {
        &mut self.drive
    }

    pub fn cdc(&self) -> &Rchip {
        &self.rchip
    }

    pub fn cdc_mut(&mut self) -> &mut Rchip {
        &mut self.rchip
    }

    pub fn disc_title(&mut self, region: GenesisRegion) -> io::Result<Option<String>> {
        static WHITESPACE_RE: OnceLock<Regex> = OnceLock::new();

        let Some(disc) = &mut self.drive.disc else { return Ok(None) };

        // Title information is always stored in the first sector of track 1
        disc.read_sector(1, CdTime::ZERO, &mut self.sector_buffer)?;

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
        self.drive.disc = other.drive.disc.take();
    }

    pub fn clone_without_disc(&self) -> Self {
        Self { drive: CdDrive { disc: None, ..self.drive.clone() }, ..self.clone() }
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
