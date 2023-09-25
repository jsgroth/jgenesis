// TODO remove
#![allow(dead_code)]

use crate::cdrom;
use crate::cdrom::cdtime::CdTime;
use crate::cdrom::reader::CdRom;
use bincode::{Decode, Encode};
use genesis_core::GenesisRegion;
use regex::Regex;
use std::sync::OnceLock;
use std::{array, io};

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
struct InterruptPrescaler {
    mclk_cycles: u64,
    prescaler_cycle: u8,
}

impl InterruptPrescaler {
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

#[derive(Debug, Clone, Encode, Decode)]
pub struct CdDrive {
    disc: Option<CdRom>,
    sector_buffer: [u8; cdrom::BYTES_PER_SECTOR as usize],
    interrupt_pending: bool,
    status: [u8; 10],
}

impl CdDrive {
    fn new(disc: Option<CdRom>) -> Self {
        Self {
            disc,
            sector_buffer: array::from_fn(|_| 0),
            interrupt_pending: false,
            status: INITIAL_STATUS,
        }
    }

    pub fn send_command(&mut self, command: [u8; 10]) {
        // TODO remove
        #[allow(clippy::match_same_arms)]
        match command[0] {
            0x00 => {
                // No-op; return current status
                self.status = NO_DISC_STATUS;
            }
            0x01 => {
                // Stop motor
                self.status = STOP_MOTOR_STATUS;
            }
            0x02 => {
                // Read TOC
                // todo!("Read TOC command")
            }
            0x03 => {
                // Seek and play
                todo!("Seek and play")
            }
            0x04 => {
                // Seek
                todo!("Seek")
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

        compute_cdd_checksum(&mut self.status);
    }

    pub fn status(&self) -> [u8; 10] {
        self.status
    }

    pub fn interrupt_pending(&self) -> bool {
        self.interrupt_pending
    }

    pub fn acknowledge_interrupt(&mut self) {
        self.interrupt_pending = false;
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct CdController {
    drive: CdDrive,
    sector_buffer: [u8; cdrom::BYTES_PER_SECTOR as usize],
    interrupt_prescaler: InterruptPrescaler,
}

impl CdController {
    pub fn new(disc: Option<CdRom>) -> Self {
        Self {
            drive: CdDrive::new(disc),
            sector_buffer: array::from_fn(|_| 0),
            interrupt_prescaler: InterruptPrescaler::new(),
        }
    }

    pub fn tick(&mut self, mclk_cycles: u64) {
        if self.interrupt_prescaler.tick(mclk_cycles) == PrescalerTickResult::Clocked {
            self.drive.interrupt_pending = true;
        }

        // TODO CDC interrupts
    }

    pub fn cdd(&self) -> &CdDrive {
        &self.drive
    }

    pub fn cdd_mut(&mut self) -> &mut CdDrive {
        &mut self.drive
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

// Checksum is the first 8 nibbles summed and then inverted
// Status 8 is always set to 0
fn compute_cdd_checksum(cdd_status: &mut [u8; 10]) {
    let sum = cdd_status[0..8].iter().copied().sum::<u8>();
    cdd_status[8] = 0x00;
    cdd_status[9] = !sum & 0x0F;
}
