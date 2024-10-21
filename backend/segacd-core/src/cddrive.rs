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
const CD_DA_FREQUENCY: u64 = 44100;
const CD_75HZ_DIVIDER: u16 = 44100 / 75;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrescalerEvent {
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

    fn tick(
        &mut self,
        sega_cd_mclk_cycles: u64,
        mut callback: impl FnMut(PrescalerEvent) -> SegaCdLoadResult<()>,
    ) -> SegaCdLoadResult<()> {
        self.drive_cycle_product += sega_cd_mclk_cycles * CD_DA_FREQUENCY;

        while self.drive_cycle_product >= SEGA_CD_MCLK_FREQUENCY {
            self.drive_cycle_product -= SEGA_CD_MCLK_FREQUENCY;
            self.divider_75hz -= 1;
            if self.divider_75hz == 0 {
                self.divider_75hz = CD_75HZ_DIVIDER;
                callback(PrescalerEvent::SampleAudioAndClockCdd)?;
            } else {
                callback(PrescalerEvent::SampleAudio)?;
            }
        }

        Ok(())
    }
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
        prg_ram_accessible: bool,
        pcm: &mut Rf5c164,
        mut audio_callback: impl FnMut(f64, f64),
    ) -> SegaCdLoadResult<()> {
        self.prescaler.tick(mclk_cycles, |event| {
            let (sample_l, sample_r) = match event {
                PrescalerEvent::SampleAudio => {
                    let (sample_l, sample_r) = self.drive.update_audio_sample();
                    self.rchip.clock_44100hz(word_ram, prg_ram, prg_ram_accessible, pcm);

                    (sample_l, sample_r)
                }
                PrescalerEvent::SampleAudioAndClockCdd => {
                    let (sample_l, sample_r) = self.drive.update_audio_sample();
                    self.drive.clock(&mut self.rchip)?;
                    self.rchip.clock_75hz();

                    (sample_l, sample_r)
                }
            };

            audio_callback(sample_l, sample_r);

            Ok(())
        })
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
