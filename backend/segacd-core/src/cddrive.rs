//! Sega CD CD-ROM drive

pub mod cdc;
pub mod cdd;

use crate::api::SegaCdLoadResult;
use crate::memory::wordram::WordRam;
use crate::rf5c164::Rf5c164;
use crate::{api, memory};
use bincode::{Decode, Encode};
use cdc::Rchip;
use cdd::CdDrive;
use cdrom::reader::CdRom;
use genesis_core::GenesisRegion;
use jgenesis_proc_macros::PartialClone;
use std::array;

const SEGA_CD_MCLK_FREQUENCY: u64 = api::SEGA_CD_MASTER_CLOCK_RATE;
const CD_DA_FREQUENCY: u64 = 44_100;
const CD_75HZ_DIVIDER: u16 = 44100 / 75;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrescalerTickEffect {
    None,
    SampleAudio,
    SampleAudioAndClockCdd,
}

// The CD drive's master clock is not actually derived from the Sega CD's master clock, but it's
// much easier to emulate by pretending that it is
#[derive(Debug, Clone, Encode, Decode)]
struct CdPrescaler {
    sega_cd_mclk_cycles: u64,
    drive_cycle_product: u64,
    divider_75hz: u16,
}

impl CdPrescaler {
    fn new() -> Self {
        Self { sega_cd_mclk_cycles: 0, drive_cycle_product: 0, divider_75hz: CD_75HZ_DIVIDER }
    }

    #[must_use]
    fn tick(&mut self, sega_cd_mclk_cycles: u64) -> PrescalerTickEffect {
        assert!(
            sega_cd_mclk_cycles < 1100,
            "sega CD mclk cycles was {sega_cd_mclk_cycles}, expected <1100"
        );

        self.drive_cycle_product += sega_cd_mclk_cycles * CD_DA_FREQUENCY;

        let mut tick_effect = PrescalerTickEffect::None;
        if self.drive_cycle_product >= SEGA_CD_MCLK_FREQUENCY {
            self.drive_cycle_product -= SEGA_CD_MCLK_FREQUENCY;
            tick_effect = PrescalerTickEffect::SampleAudio;

            self.divider_75hz -= 1;
            if self.divider_75hz == 0 {
                self.divider_75hz = CD_75HZ_DIVIDER;
                tick_effect = PrescalerTickEffect::SampleAudioAndClockCdd;
            }
        }

        tick_effect
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CdTickEffect {
    None,
    OutputAudioSample(f64, f64),
}

#[derive(Debug, Encode, Decode, PartialClone)]
pub struct CdController {
    #[partial_clone(partial)]
    drive: CdDrive,
    rchip: Rchip,
    sector_buffer: [u8; cdrom::BYTES_PER_SECTOR as usize],
    prescaler: CdPrescaler,
}

impl CdController {
    pub fn new(disc: Option<CdRom>) -> Self {
        Self {
            drive: CdDrive::new(disc),
            rchip: Rchip::new(),
            sector_buffer: array::from_fn(|_| 0),
            prescaler: CdPrescaler::new(),
        }
    }

    pub fn tick(
        &mut self,
        mclk_cycles: u64,
        word_ram: &mut WordRam,
        prg_ram: &mut [u8; memory::PRG_RAM_LEN],
        pcm: &mut Rf5c164,
    ) -> SegaCdLoadResult<CdTickEffect> {
        match self.prescaler.tick(mclk_cycles) {
            PrescalerTickEffect::None => Ok(CdTickEffect::None),
            PrescalerTickEffect::SampleAudio => {
                let (sample_l, sample_r) = self.drive.update_audio_sample();

                self.rchip.clock_44100hz(word_ram, prg_ram, pcm);

                Ok(CdTickEffect::OutputAudioSample(sample_l, sample_r))
            }
            PrescalerTickEffect::SampleAudioAndClockCdd => {
                let (sample_l, sample_r) = self.drive.update_audio_sample();
                self.drive.clock(&mut self.rchip)?;
                self.rchip.clock_75hz();

                Ok(CdTickEffect::OutputAudioSample(sample_l, sample_r))
            }
        }
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

    pub fn disc_title(&mut self, region: GenesisRegion) -> SegaCdLoadResult<Option<String>> {
        self.drive.disc_title(region)
    }

    pub fn take_disc(&mut self) -> Option<CdRom> {
        self.drive.take_disc()
    }

    pub fn take_disc_from(&mut self, other: &mut Self) {
        self.drive.take_disc_from(&mut other.drive);
    }

    pub fn reset(&mut self) {
        self.drive.reset();
        self.rchip.reset();
    }
}
