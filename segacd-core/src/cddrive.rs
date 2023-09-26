pub mod cdc;
pub mod cdd;

use crate::api::DiscResult;
use crate::cdrom;
use crate::cdrom::reader::CdRom;
use bincode::{Decode, Encode};
use cdc::Rchip;
use cdd::CdDrive;
use genesis_core::GenesisRegion;
use std::array;

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

    pub fn tick(&mut self, mclk_cycles: u64) -> DiscResult<()> {
        if self.prescaler_75hz.tick(mclk_cycles) == PrescalerTickResult::Clocked {
            self.drive.clock(&mut self.rchip)?;
        }

        // TODO CDC interrupts

        Ok(())
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

    pub fn disc_title(&mut self, region: GenesisRegion) -> DiscResult<Option<String>> {
        self.drive.disc_title(region)
    }

    pub fn take_disc_from(&mut self, other: &mut Self) {
        self.drive.take_disc_from(&mut other.drive);
    }

    pub fn clone_without_disc(&self) -> Self {
        Self { drive: self.drive.clone_without_disc(), ..self.clone() }
    }
}
