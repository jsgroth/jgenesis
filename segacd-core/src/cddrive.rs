pub mod cdc;
pub mod cdd;

use crate::api::DiscResult;
use crate::cdrom::reader::CdRom;
use crate::memory::wordram::WordRam;
use crate::rf5c164::Rf5c164;
use crate::{cdrom, memory};
use bincode::{Decode, Encode};
use cdc::Rchip;
use cdd::CdDrive;
use genesis_core::GenesisRegion;
use jgenesis_proc_macros::PartialClone;
use std::array;

const SEGA_CD_MCLK_FREQUENCY: f64 = 50_000_000.0;
const CD_DA_FREQUENCY: f64 = 44_100.0;
const CD_75HZ_DIVIDER: u16 = 44100 / 75;

// Arbitrary value to keep mclk cycle counts low-ish for better floating-point precision
const CYCLE_COUNT_MODULO: f64 = 100_000_000.0;

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
    drive_cycles: u64,
    drive_cycles_float: f64,
    prescale_ratio: f64,
    divider_75hz: u16,
}

impl CdPrescaler {
    fn new() -> Self {
        let prescale_ratio = SEGA_CD_MCLK_FREQUENCY / CD_DA_FREQUENCY;
        Self {
            sega_cd_mclk_cycles: 0,
            drive_cycles: 0,
            drive_cycles_float: 0.0,
            prescale_ratio,
            divider_75hz: CD_75HZ_DIVIDER,
        }
    }

    #[must_use]
    fn tick(&mut self, sega_cd_mclk_cycles: u64) -> PrescalerTickEffect {
        assert!(
            sega_cd_mclk_cycles < 1100,
            "sega CD mclk cycles was {sega_cd_mclk_cycles}, expected <1100"
        );

        let mut tick_effect = PrescalerTickEffect::None;

        let elapsed_drive_cycles = sega_cd_mclk_cycles as f64 / self.prescale_ratio;
        self.drive_cycles_float += elapsed_drive_cycles;
        let prev_drive_cycles = self.drive_cycles;
        self.drive_cycles = self.drive_cycles_float.round() as u64;

        if prev_drive_cycles != self.drive_cycles {
            tick_effect = PrescalerTickEffect::SampleAudio;

            self.divider_75hz -= 1;
            if self.divider_75hz == 0 {
                self.divider_75hz = CD_75HZ_DIVIDER;
                tick_effect = PrescalerTickEffect::SampleAudioAndClockCdd;
            }
        }

        self.drive_cycles_float %= CYCLE_COUNT_MODULO;
        self.drive_cycles = self.drive_cycles_float.round() as u64;

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
    ) -> DiscResult<CdTickEffect> {
        match self.prescaler.tick(mclk_cycles) {
            PrescalerTickEffect::None => Ok(CdTickEffect::None),
            PrescalerTickEffect::SampleAudio => {
                let (sample_l, sample_r) = self.drive.update_audio_sample();

                self.rchip.clock(word_ram, prg_ram, pcm);

                Ok(CdTickEffect::OutputAudioSample(sample_l, sample_r))
            }
            PrescalerTickEffect::SampleAudioAndClockCdd => {
                let (sample_l, sample_r) = self.drive.update_audio_sample();
                self.drive.clock(&mut self.rchip)?;

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

    pub fn disc_title(&mut self, region: GenesisRegion) -> DiscResult<Option<String>> {
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
